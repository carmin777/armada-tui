//! Política central de rede: o que pode sair para onde.
//!
//! Parsing via crate `url` (WHATWG — userinfo, query, IPv6 e portas tratados
//! certo, sem parser manual). Dois contextos:
//! - URLs de conteúdo (imagens de mensagens): entrada NÃO confiável → bloqueio
//!   estrito + redirects revalidados (ver kitty.rs).
//! - Relays wss (invites, NIP-29): só `wss://` + mesmos hosts bloqueados.
//!   (Relay local p/ dev exige mudar o código de propósito.)
//!
//! Limitação honesta: sem pinning de IP no socket, DNS rebinding entre a
//! checagem e o connect continua teoricamente possível; a janela é mínima
//! (checagem imediatamente antes do uso, sem cache).

use std::net::{IpAddr, ToSocketAddrs};

fn is_public_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            !(v.is_loopback()
                || v.is_private()
                || v.is_link_local()
                || v.is_multicast()
                || v.is_unspecified())
        }
        IpAddr::V6(v) => {
            // IPv4-mapped (::ffff:127.0.0.1) valida como IPv4.
            if let Some(mapped) = v.to_ipv4_mapped() {
                return is_public_ip(&IpAddr::V4(mapped));
            }
            !(v.is_loopback()
                || v.is_multicast()
                || v.is_unspecified()
                || v.is_unicast_link_local()
                || (v.segments()[0] & 0xfe00) == 0xfc00)
        }
    }
}

fn domain_blocked(d: &str) -> bool {
    let h = d.to_lowercase();
    if h.is_empty() || h == "localhost" {
        return true;
    }
    [
        ".local",
        ".internal",
        ".lan",
        ".localhost",
        ".invalid",
        ".test",
        ".example",
        ".home",
        ".corp",
        ".arpa",
    ]
    .iter()
    .any(|s| h.ends_with(s))
}

/// Checa host já extraído (domínio ou IP literal).
fn check_host(host: &url::Host<&str>, port: u16) -> anyhow::Result<String> {
    match host {
        url::Host::Domain(d) => {
            if domain_blocked(d) {
                anyhow::bail!("domínio bloqueado ({d})");
            }
            // Fail-closed: sem DNS ou qualquer IP não-público = recusa.
            let mut saw = false;
            let mut all_public = true;
            match (d.to_string(), port).to_socket_addrs() {
                Ok(addrs) => {
                    for a in addrs {
                        saw = true;
                        all_public &= is_public_ip(&a.ip());
                    }
                }
                Err(_) => anyhow::bail!("DNS falhou p/ {d}"),
            }
            if !saw || !all_public {
                anyhow::bail!("host sem IP público ({d})");
            }
            Ok(d.to_string())
        }
        url::Host::Ipv4(v) => {
            if !is_public_ip(&IpAddr::V4(*v)) {
                anyhow::bail!("IP bloqueado ({v})");
            }
            Ok(v.to_string())
        }
        url::Host::Ipv6(v) => {
            if !is_public_ip(&IpAddr::V6(*v)) {
                anyhow::bail!("IP bloqueado ({v})");
            }
            Ok(v.to_string())
        }
    }
}

/// Valida URL de conteúdo. Retorna o host.
pub fn check_http_url(url: &str) -> anyhow::Result<String> {
    let u = url::Url::parse(url).map_err(|_| anyhow::anyhow!("URL inválida"))?;
    match u.scheme() {
        "http" | "https" => {}
        _ => anyhow::bail!("só http(s)"),
    }
    let host = u.host().ok_or_else(|| anyhow::anyhow!("sem host"))?;
    let port = u.port_or_known_default().unwrap_or(443);
    check_host(&host, port)
}

/// Valida relay: `wss://` + host liberado (+ DNS).
pub fn check_relay_url(url: &str) -> anyhow::Result<String> {
    let u = url::Url::parse(url).map_err(|_| anyhow::anyhow!("relay inválido"))?;
    if u.scheme() != "wss" {
        anyhow::bail!("relay precisa ser wss://");
    }
    let host = u.host().ok_or_else(|| anyhow::anyhow!("sem host"))?;
    let port = u.port_or_known_default().unwrap_or(443);
    check_host(&host, port)
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
        // Parser WHATWG resolve userinfo/query/fragment sozinho.
        for u in [
            "http://x@127.0.0.1/a.png",
            "http://user:pass@10.0.0.1/x",
            "https://127.0.0.1?x",
            "https://localhost:8080/x",
            "https://[::1]/x",
            "https://[fc00::1]/x",
            "https://[::ffff:127.0.0.1]/x",
            "https://[::ffff:10.0.0.1]/x",
            "https://[fe80::1]/x",
            "http://169.254.169.254/meta",
            "https://printer.local/x",
            "https://x.internal/y",
            "file:///etc/passwd",
            "wss://relay/x",
        ] {
            assert!(check_http_url(u).is_err(), "{u} deveria bloquear");
        }
        assert!(check_relay_url("wss://127.0.0.1:7000").is_err());
        assert!(check_relay_url("ws://relay.ditto.pub").is_err());
        assert!(check_relay_url("http://x@evil.com").is_err());
    }

    #[test]
    fn legitimos_passam() {
        // Exige DNS funcionando (CI tem); fail-closed de propósito.
        assert!(check_http_url("https://blossom.primal.net/x.png").is_ok());
        assert!(check_relay_url("wss://relay.ditto.pub").is_ok());
    }
}
