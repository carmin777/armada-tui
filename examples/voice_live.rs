//! Voz V2 live (headless): suporte AV + token LiveKit via NIP-98.
//! Uso: cargo run --example voice_live -- <relay-wss>
//! Gera throwaway, acha grupo com tag `livekit`, pede token.
//! `403` = controle de acesso funcionando (não-membro); `200` = token real.

use armada_tui::{nostr, voice};

fn main() -> anyhow::Result<()> {
    let relay = std::env::args().nth(1).expect("relay-wss");
    let sup = voice::support_probe(&relay)?;
    println!("suporte AV: {sup}");
    let groups = nostr::fetch_groups(&relay, None, nostr::never_cancel())?;
    let com_voz: Vec<_> = groups.iter().filter(|g| g.has_voice).collect();
    println!("grupos: {}, com livekit: {}", groups.len(), com_voz.len());
    for g in com_voz.iter().take(5) {
        println!("- {} ({})", g.name, g.id);
    }
    let me = nostr::generate()?;
    println!("eu: {}", me.pubkey_hex);
    for g in com_voz.iter().take(5) {
        match voice::fetch_token(&relay, &g.id, &me) {
            Ok(t) => {
                println!(
                    "TOKEN OK p/ {}: server={} id={}",
                    g.id,
                    t.server_url,
                    voice::short(&t)
                );
                println!("VOICE SIGNAL LIVE OK");
                return Ok(());
            }
            Err(e) => println!("{}: {e}", g.id),
        }
    }
    println!("sem token (esperado p/ throwaway sem membresia)");
    Ok(())
}
