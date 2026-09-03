//! NIP-29 live (headless): groups | msgs <g> | voice <g> | join <g> | send <g> <texto>
//! Uso: cargo run --example nip29_live -- <relay> <cmd> [args...]
//! join/send usam identidade throwaway gerada na hora.

use armada_tui::nostr;

fn main() -> anyhow::Result<()> {
    let relay = std::env::args().nth(1).expect("relay");
    let cmd = std::env::args().nth(2).expect("cmd");
    match cmd.as_str() {
        "groups" => {
            for g in nostr::fetch_groups(&relay)? {
                let about: String = g.about.chars().take(60).collect();
                println!("{} [{}] {about}", g.id, g.name);
            }
        }
        "msgs" => {
            let group = std::env::args().nth(3).expect("grupo");
            for m in nostr::fetch_messages(&relay, &group, 20)? {
                println!(
                    "[{}] {}: {}",
                    m.time,
                    m.author,
                    m.content.chars().take(160).collect::<String>()
                );
            }
        }
        "voice" => {
            let group = std::env::args().nth(3).expect("grupo");
            let ps = nostr::fetch_participants(&relay, &group)?;
            println!("🔊 {} na chamada em {group}", ps.len());
            for p in ps {
                println!("  {p}");
            }
        }
        "join" => {
            let group = std::env::args().nth(3).expect("grupo");
            let k = nostr::generate()?;
            println!("identidade: {}", k.pubkey_hex);
            println!("join: {}", nostr::send_join(&relay, &k, &group)?);
        }
        "send" => {
            let group = std::env::args().nth(3).expect("grupo");
            let text: String = std::env::args().skip(4).collect::<Vec<_>>().join(" ");
            let k = nostr::generate()?;
            println!("identidade: {}", k.pubkey_hex);
            println!("send: {}", nostr::send_chat(&relay, &k, &group, &text)?);
        }
        _ => anyhow::bail!("cmd: groups|msgs|voice|join|send"),
    }
    Ok(())
}
