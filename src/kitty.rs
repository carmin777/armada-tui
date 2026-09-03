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

/// Baixa URL e valida que é PNG (magic bytes). Teto real de 8 MiB: lê no
/// máximo 8 MiB+1 (servidor malicioso não enche a RAM). Redirects seguidos
/// manualmente com revalidação a cada salto (máx 3).
pub fn fetch_png(url: &str) -> anyhow::Result<Vec<u8>> {
    const TETO: u64 = 8_000_001;
    let mut current = url.to_string();
    let mut body: Option<Vec<u8>> = None;
    let mut content_type = String::new();
    for _ in 0..=3 {
        crate::netpolicy::check_http_url(&current)?;
        let agent: ureq::Agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(15))
            .redirects(0)
            .build();
        let res = agent.get(&current).call()?;
        let status = res.status();
        if [301, 302, 303, 307, 308].contains(&status) {
            let loc = res.header("location").unwrap_or("").to_string();
            if loc.is_empty() {
                anyhow::bail!("redirect sem location");
            }
            current = if loc.starts_with("http://") || loc.starts_with("https://") {
                loc
            } else if let Some(base) = current.split("://").next() {
                let host = current
                    .split("://")
                    .nth(1)
                    .unwrap_or("")
                    .split('/')
                    .next()
                    .unwrap_or("");
                format!(
                    "{base}://{host}{}",
                    if loc.starts_with('/') {
                        loc
                    } else {
                        format!("/{loc}")
                    }
                )
            } else {
                anyhow::bail!("redirect estranho");
            };
            continue;
        }
        content_type = res.header("content-type").unwrap_or("").to_string();
        let mut bytes = Vec::new();
        res.into_reader().take(TETO).read_to_end(&mut bytes)?;
        body = Some(bytes);
        break;
    }
    let bytes = body.ok_or_else(|| anyhow::anyhow!("redirects demais"))?;
    if !(content_type.is_empty()
        || content_type.contains("image/png")
        || content_type.contains("octet-stream"))
    {
        anyhow::bail!("content-type não é PNG: {content_type}");
    }
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
    fn politica_central_cobre_viewer() {
        use crate::netpolicy;
        assert!(netpolicy::check_http_url("http://x@127.0.0.1/a.png").is_err());
        assert!(netpolicy::check_http_url("https://blossom.primal.net/x.png").is_ok());
    }
}
