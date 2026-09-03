//! Imagens via Kitty graphics protocol + download de mídia (Blossom etc).
//!
//! MVP honesto: só PNG direto (f=100). JPEG/gif precisariam transcodificar
//! (crate `image` no roadmap) ou fallback sixel/iTerm2.
//! Detecção: Kitty define KITTY_WINDOW_ID / TERM=xterm-kitty; WezTerm e
//! Ghostty também falam o protocolo.

use std::io::{Read, Write};
use std::time::Duration;

pub fn supported() -> bool {
    if std::env::var("KITTY_WINDOW_ID").is_ok() {
        return true;
    }
    if matches!(std::env::var("TERM").as_deref(), Ok("xterm-kitty")) {
        return true;
    }
    matches!(
        std::env::var("TERM_PROGRAM").as_deref(),
        Ok("WezTerm") | Ok("ghostty")
    )
}

/// Hosts que `v` nunca busca (SSRF básico: tecla `v` abre URL de mensagem
/// alheia — sem isso um rumor malicioso sondaria a rede local/nuvem).
fn host_bloqueado(host: &str) -> bool {
    let h = host.to_lowercase();
    // IPv6 entre colchetes, com ou sem porta: [::1], [::1]:8080.
    if let Some(rest) = h.strip_prefix('[') {
        let inner = rest.split(']').next().unwrap_or("");
        return inner == "::1" || inner.is_empty();
    }
    // Tira :porta quando há um único ':' e o resto é numérico.
    let h = match h.rsplit_once(':') {
        Some((left, port)) if !left.contains(':') && port.parse::<u16>().is_ok() => left,
        _ => h.as_str(),
    };
    if ["localhost", ""].contains(&h) {
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
    ] {
        if h.ends_with(suf) {
            return true;
        }
    }
    // IPv4 privadas/link-local + metadata cloud.
    let parts: Vec<&str> = h.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        let n: Vec<u8> = parts.iter().map(|p| p.parse().unwrap()).collect();
        if n[0] == 127 || n[0] == 10 || n[0] == 169 && n[1] == 254 {
            return true;
        }
        if n[0] == 192 && n[1] == 168 {
            return true;
        }
        if n[0] == 172 && (16..=31).contains(&n[1]) {
            return true;
        }
    }
    // IPv6 loopback/ULA literais (só quando é literal, com ':').
    if h == "::1" || (h.contains(':') && (h.starts_with("fc") || h.starts_with("fd"))) {
        return true;
    }
    false
}

fn check_url(url: &str) -> anyhow::Result<()> {
    let lower = url.to_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        anyhow::bail!("só http(s)");
    }
    let after = url.split("://").nth(1).unwrap_or("");
    let host = after.split('/').next().unwrap_or("");
    if host_bloqueado(host) {
        anyhow::bail!("host bloqueado p/ viewer ({host})");
    }
    Ok(())
}

/// Baixa URL e valida que é PNG (magic bytes). Teto real de 8 MiB: lê no
/// máximo 8 MiB+1 (servidor malicioso não enche a RAM).
pub fn fetch_png(url: &str) -> anyhow::Result<Vec<u8>> {
    check_url(url)?;
    const TETO: u64 = 8_000_001;
    let res = ureq::get(url).timeout(Duration::from_secs(15)).call()?;
    let ct = res.header("content-type").unwrap_or("").to_string();
    if !(ct.is_empty() || ct.contains("image/png") || ct.contains("octet-stream")) {
        anyhow::bail!("content-type não é PNG: {ct}");
    }
    let mut bytes = Vec::new();
    res.into_reader().take(TETO).read_to_end(&mut bytes)?;
    if bytes.len() as u64 >= TETO {
        anyhow::bail!("imagem > 8 MiB, recusada");
    }
    if bytes.len() < 8 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        anyhow::bail!("só PNG direto no MVP (f=100 do protocolo kitty)");
    }
    Ok(bytes)
}

/// Transmite PNG via `ESC _G ... ESC \`, em chunks base64 de 4096.
pub fn display_png(png: &[u8], cols: u32) -> anyhow::Result<()> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    let bytes = b64.as_bytes();
    let total = bytes.chunks(4096).len();
    let out = std::io::stdout();
    let mut h = out.lock();
    for (i, chunk) in bytes.chunks(4096).enumerate() {
        let m = if i + 1 == total { 0 } else { 1 };
        if i == 0 {
            write!(h, "\x1b_Gf=100,a=T,c={cols},m={m};")?;
        } else {
            write!(h, "\x1b_Gm={m};")?;
        }
        h.write_all(chunk)?;
        write!(h, "\x1b\\")?;
    }
    h.write_all(b"\n")?;
    h.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocklist_ssrf() {
        for h in [
            "localhost",
            "127.0.0.1",
            "10.0.0.5",
            "192.168.1.1",
            "172.16.0.9",
            "172.31.255.1",
            "169.254.169.254",
            "::1",
            "x.local",
            "y.internal",
            "z.lan",
        ] {
            assert!(host_bloqueado(h), "{h} deveria bloquear");
        }
        for h in [
            "relay.ditto.pub",
            "blossom.primal.net",
            "8.8.8.8",
            "172.32.0.1",
            "11.0.0.1",
        ] {
            assert!(!host_bloqueado(h), "{h} não deveria bloquear");
        }
        assert!(check_url("https://blossom.primal.net/x.png").is_ok());
        assert!(check_url("http://169.254.169.254/meta").is_err());
        assert!(check_url("file:///etc/passwd").is_err());
    }
}
