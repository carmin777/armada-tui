//! DM E2EE live (headless): send-self + readback.
//! Uso: cargo run --example dm_live -- <relay> <texto>
//! Gera identidade throwaway, manda DM p/ si, lê de volta e compara.

use armada_tui::{dm, nostr};

fn main() -> anyhow::Result<()> {
    let relay = std::env::args().nth(1).expect("relay");
    let text: String = std::env::args().skip(2).collect::<Vec<_>>().join(" ");
    let relays = vec![relay.clone()];
    let me = nostr::generate()?;
    println!("eu: {}", me.pubkey_hex);
    let now = chrono::Utc::now().timestamp();
    let w_peer = dm::build_wrap(&text, &me.pubkey_hex, &me.secret, now)?;
    let n = nostr::publish_concord(&relays, w_peer, Some(&me), nostr::never_cancel())?;
    println!("ENVIADO a {n} relay(s)");
    std::thread::sleep(std::time::Duration::from_secs(8));
    // Leitura COM auth (relays protegem 1059 via NIP-42, NIP-59).
    let raw: [u8; 32] = *me.secret;
    let msgs = dm::fetch_threads(
        &relays,
        &raw,
        Some(me.secret.clone()),
        nostr::never_cancel(),
    )?;
    let mine: Vec<_> = msgs.iter().filter(|m| m.content == text).collect();
    println!("lidas: {} threads?, {} com o texto", msgs.len(), mine.len());
    if mine.is_empty() {
        anyhow::bail!("read-back falhou: texto não voltou");
    }
    println!("ROUNDTRIP LIVE OK: {}", mine[0].content);
    Ok(())
}
