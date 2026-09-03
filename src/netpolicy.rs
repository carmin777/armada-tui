//! Política central de rede: o que pode sair para onde.
//!
//! Dois contextos:
//! - URLs de conteúdo (imagens de mensagens): entrada NÃO confiável → bloqueio
//!   estrito (userinfo, hosts locais, IPv6 especial, redirects revalidados).
//! - Relays wss (invites, NIP-29): só `wss://` + mesmos hosts bloqueados.
//!   (Relay local p/ dev exige mudar o código de propósito.)

/// Separa host de URL http(s) (sem userinfo, sem porta).
fn http_host(url: &str) -> anyhow::Result<String> {
    let lower = url.to_lowercase();
    let after = if let Some(a) = lower.strip_prefix("https://") {
        a
    } else if let Some(a) = lower.strip_prefix("http://") {
        a
    } else {
        anyhow::bail!("só http(s)");
    };
    let authority = after.split('/').next().unwrap_or("");
    // userinfo "user@host" → pega o host real (anti `http://x@127.0.0.1`).
    let hostport = authority.rsplit('@').next().unwrap_or("");
    Ok(strip_port(hostport))
}

/// Tira :porta (cuidando de IPv6 com/sem colchetes).
fn strip_port(hostport: &str) -> String {
    if let Some(rest) = hostport.strip_prefix('[') {
        return rest.split(']').next().unwrap_or("").to_string();
    }
    match hostport.rsplit_once(':') {
        Some((left, port)) if !left.contains(':') && port.parse::<u16>().is_ok() => {
            left.to_string()
        }
        _ => hostport.to_string(),
    }
}

/// True = proibido (localhost, redes privadas, link-local, metadata cloud,
/// pseudo-TLDs locais). Recebe host já sem porta/userinfo.
pub fn host_blocked(host: &str) -> bool {
    let h = host.to_lowercase();
    if h.is_empty() || h == "localhost" {
        return true;
    }
    for suf in [
        ".local",
        ".internal",
        ".lan",
        ".localhost",
        ".invalid",
        ".test",
        ".example",
        ".home",
        ".corp",
    ] {
        if h.ends_with(suf) {
            return true;
        }
    }
    // IPv6 literal: loopback, ULA, link-local.
    if h.contains(':') {
        return h == "::1" || h.starts_with("fc") || h.starts_with("fd") || h.starts_with("fe80:");
    }
    let parts: Vec<&str> = h.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        let n: Vec<u8> = parts.iter().map(|p| p.parse().unwrap()).collect();
        return n[0] == 127
            || n[0] == 10
            || (n[0] == 172 && (16..=31).contains(&n[1]))
            || (n[0] == 192 && n[1] == 168)
            || (n[0] == 169 && n[1] == 254);
    }
    false
}

/// DNS: true se resolve E todos os IPs são não-públicos. Falha de DNS =
/// false (fail-open p/ não quebrar rede instável; literais já cobertos acima).
pub fn host_resolves_private(host: &str) -> bool {
    use std::net::ToSocketAddrs;
    let probe = format!("{host}:443");
    let mut saw = false;
    let mut all_private = true;
    if let Ok(addrs) = probe.to_socket_addrs() {
        for a in addrs {
            saw = true;
            let ip = a.ip();
            let public = match ip {
                std::net::IpAddr::V4(v) => {
                    !(v.is_loopback()
                        || v.is_private()
                        || v.is_link_local()
                        || v.is_multicast()
                        || v.is_unspecified()
                        || v.octets()[0] == 169 && v.octets()[1] == 254)
                }
                std::net::IpAddr::V6(v) => {
                    !(v.is_loopback()
                        || v.is_multicast()
                        || v.is_unspecified()
                        || (v.segments()[0] & 0xfe00) == 0xfc00
                        || (v.segments()[0] & 0xffc0) == 0xfe80)
                }
            };
            if public {
                all_private = false;
            }
        }
    }
    saw && all_private
}

/// Valida URL de conteúdo. Retorna o host.
pub fn check_http_url(url: &str) -> anyhow::Result<String> {
    let host = http_host(url)?;
    if host_blocked(&host) {
        anyhow::bail!("host bloqueado ({host})");
    }
    if host_resolves_private(&host) {
        anyhow::bail!("host resolve p/ IP privado ({host})");
    }
    Ok(host)
}

/// Valida relay: `wss://` + host liberado (+ DNS).
pub fn check_relay_url(url: &str) -> anyhow::Result<String> {
    if !url.to_lowercase().starts_with("wss://") {
        anyhow::bail!("relay precisa ser wss:// ({url})");
    }
    let after = url.split("://").nth(1).unwrap_or("");
    let authority = after.split('/').next().unwrap_or("");
    let host = strip_port(authority.rsplit('@').next().unwrap_or(""));
    if host_blocked(&host) {
        anyhow::bail!("relay bloqueado ({host})");
    }
    if host_resolves_private(&host) {
        anyhow::bail!("relay resolve p/ IP privado ({host})");
    }
    Ok(host)
}

/// Filtra lista de relays pela política (bundle/invite: pula bloqueados).
pub fn filter_relays(relays: &[String]) -> Vec<String> {
    relays
        .iter()
        .filter(|r| check_relay_url(r).is_ok())
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloqueios() {
        for u in [
            "http://x@127.0.0.1/a.png",
            "http://user:pass@10.0.0.1/x",
            "https://localhost:8080/x",
            "https://[::1]/x",
            "https://[fc00::1]/x",
            "https://[fe80::1]/x",
            "http://169.254.169.254/meta",
            "https://printer.local/x",
            "https://x.internal/y",
            "file:///etc/passwd",
            "wss://relay/x",
        ] {
            assert!(check_http_url(u).is_err(), "{u} deveria bloquear");
        }
        assert!(check_http_url("https://blossom.primal.net/x.png").is_ok());
        assert!(check_relay_url("wss://relay.ditto.pub").is_ok());
        assert!(check_relay_url("ws://relay.ditto.pub").is_err());
        assert!(check_relay_url("wss://127.0.0.1:7000").is_err());
        assert!(check_relay_url("http://x@evil.com").is_err());
    }
}
