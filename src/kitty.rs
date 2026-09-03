//! Imagens via Kitty graphics protocol + download de mídia (Blossom etc).
//!
//! MVP honesto: só PNG direto (f=100). JPEG/gif precisariam transcodificar
//! (crate `image` no roadmap) ou fallback sixel/iTerm2.
//! Detecção: Kitty define KITTY_WINDOW_ID / TERM=xterm-kitty; WezTerm e
//! Ghostty também falam o protocolo.

use std::io::Write;
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

/// Baixa URL e valida que é PNG (magic bytes). Limite 8 MiB.
pub fn fetch_png(url: &str) -> anyhow::Result<Vec<u8>> {
    let res = ureq::get(url).timeout(Duration::from_secs(15)).call()?;
    let ct = res.header("content-type").unwrap_or("").to_string();
    if !(ct.is_empty() || ct.contains("image/png") || ct.contains("octet-stream")) {
        anyhow::bail!("content-type não é PNG: {ct}");
    }
    let bytes = res.into_bytes()?;
    if bytes.len() > 8_000_000 {
        anyhow::bail!("imagem > 8 MiB, recusada no MVP");
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
