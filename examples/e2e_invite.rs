//! Runner headless do read-path E2EE (e2e real, fora do TUI interativo).
//! Uso: cargo run --example e2e_invite -- '<invite-url>'

use armada_tui::concord::{invite as inv, stream};

fn h32(s: &str) -> anyhow::Result<[u8; 32]> {
    let b = hex::decode(s)?;
    b.try_into().map_err(|_| anyhow::anyhow!("hex32 inválido"))
}

fn main() -> anyhow::Result<()> {
    let url = std::env::args().nth(1).expect("passe o invite como arg");
    let p = inv::parse_invite_link(&url).expect("link parse");
    println!("signer: {}", p.link_signer);
    println!("relays: {:?}", p.relays);
    println!("token: {}", hex::encode(p.token));

    let ev = inv::fetch_bundle_event(&p.relays, &p.link_signer)?;
    println!(
        "bundle evt: kind={} created_at={} id={}",
        ev["kind"], ev["created_at"], ev["id"]
    );
    let now_ms = chrono::Utc::now().timestamp_millis();
    let b = inv::open_bundle(&ev, &p.link_signer, &p.token, now_ms)?;
    println!("frota: '{}' canais={}", b.name, b.channels.len());
    println!("bundle debug: {b:#?}");

    for c in &b.channels {
        println!(
            "--- canal '{}' epoch={} relays={:?}",
            c.name, c.epoch, b.relays
        );
        let sk = h32(&c.key)?;
        let secp = secp256k1::Secp256k1::new();
        let kp =
            secp256k1::Keypair::from_secret_key(&secp, &secp256k1::SecretKey::from_slice(&sk)?);
        let (xonly, _) = secp256k1::XOnlyPublicKey::from_keypair(&kp);
        let pk = format!("{xonly}");
        let wraps = inv::fetch_wraps(&b.relays, &pk, 50)?;
        println!("wraps: {}", wraps.len());
        let mut msgs = vec![];
        for w in &wraps {
            match stream::open_wrap(w, &sk, &pk, &c.id, c.epoch) {
                Ok(r) => {
                    let t = chrono::DateTime::from_timestamp_millis(r.ms)
                        .map(|d| d.format("%d/%m %H:%M").to_string())
                        .unwrap_or_default();
                    msgs.push((
                        r.ms,
                        format!("[{t}] kind={} {}: {}", r.kind, &r.author[..8], r.content),
                    ));
                }
                Err(e) => println!("(wrap ignorado: {e:#})"),
            }
        }
        msgs.sort();
        for (_, m) in msgs {
            println!("{m}");
        }
    }
    Ok(())
}
