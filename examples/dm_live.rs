//! DM E2EE live (headless).
//! - Self: cargo run --example dm_live -- <relay> <texto>
//! - A→B: cargo run --example dm_live -- <relay> <texto> --peer
//!   (gera A e B, A manda p/ B, lê como B e compara).
//! Gera identidade throwaway, manda DM p/ si, lê de volta e compara.

use armada_tui::{dm, nostr};

fn main() -> anyhow::Result<()> {
    let relay = std::env::args().nth(1).expect("relay");
    let ab = std::env::args().any(|a| a == "--peer");
    let text: String = std::env::args()
        .skip(2)
        .filter(|a| a != "--peer")
        .collect::<Vec<_>>()
        .join(" ");
    let relays = vec![relay.clone()];
    // Conta A (remetente) e conta B (destinatário; B=A no modo self).
    let a = nostr::generate()?;
    let b = if ab {
        nostr::generate()?
    } else {
        nostr::Keys {
            secret: a.secret.clone(),
            pubkey_hex: a.pubkey_hex.clone(),
            npub: a.npub.clone(),
        }
    };
    println!("A: {}", a.pubkey_hex);
    println!("B: {}", b.pubkey_hex);
    let now = chrono::Utc::now().timestamp();
    let w = dm::build_wrap(&text, &b.pubkey_hex, &a.secret, now)?;
    let n = nostr::publish_concord(&relays, w, Some(&a), nostr::never_cancel())?;
    println!("ENVIADO A→B a {n} relay(s)");
    std::thread::sleep(std::time::Duration::from_secs(8));
    // Leitura COM auth como B (relays protegem 1059 via NIP-42, NIP-59).
    let braw: [u8; 32] = *b.secret;
    let msgs = dm::fetch_threads(
        &relays,
        &braw,
        Some(b.secret.clone()),
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
