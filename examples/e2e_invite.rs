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

    let ev = inv::fetch_bundle_event(
        &p.relays,
        &p.link_signer,
        None,
        armada_tui::nostr::never_cancel(),
    )?;
    println!(
        "bundle evt: kind={} created_at={} id={}",
        ev["kind"], ev["created_at"], ev["id"]
    );
    let now_ms = chrono::Utc::now().timestamp_millis();
    let b = inv::open_bundle(&ev, &p.link_signer, &p.token, now_ms)?;
    println!("frota: '{}' canais={}", b.name, b.channels.len());
    // Control plane: canais públicos derivam do root.
    if let (Ok(root), Ok(cid)) = (hex::decode(&b.community_root), hex::decode(&b.community_id)) {
        let root: [u8; 32] = root.try_into().expect("root32");
        let _cid: [u8; 32] = cid.try_into().expect("cid32");
        match armada_tui::concord::control::fetch_control_channels(
            &b.relays,
            &root,
            &b.community_id,
            b.root_epoch,
            None,
            armada_tui::nostr::never_cancel(),
        ) {
            Ok(cc) => {
                println!("control: {} canais", cc.len());
                for c in &cc {
                    println!("  #{} priv={}", c.name, c.is_private);
                }
                // Lê um canal público (2º arg, default general): deriva do root.
                let want = std::env::args()
                    .nth(2)
                    .unwrap_or_else(|| "general".to_string());
                if let Some(ch) = cc.iter().find(|c| c.name == want && !c.is_private) {
                    use armada_tui::concord::{derive, stream};
                    use armada_tui::nostr;
                    let id: [u8; 32] = h32(&ch.id)?;
                    let g =
                        derive::group_key(derive::label::CHANNEL, &root, &id, Some(b.root_epoch));
                    // --send "texto": posta wrap selado com identidade throwaway.
                    if std::env::args().nth(3).as_deref() == Some("--send") {
                        let text = std::env::args().nth(4).expect("texto após --send");
                        let author = nostr::generate()?;
                        let wrap = stream::build_chat_wrap(
                            &text,
                            &ch.id,
                            b.root_epoch,
                            &author.secret,
                            &g.sk,
                        )?;
                        let n = nostr::publish_concord(&b.relays, wrap, Some(&author))?;
                        println!(
                            "ENVIADO a {n} relays como {}: {text}",
                            &author.pubkey_hex[..8]
                        );
                    }
                    println!(
                        "lendo #{} (stream {}…)…",
                        want,
                        armada_tui::models::short(&g.pk, 8).as_str()
                    );
                    match inv::fetch_wraps(
                        &b.relays,
                        &g.pk,
                        50,
                        None,
                        armada_tui::nostr::never_cancel(),
                    ) {
                        Ok(wraps) => {
                            println!("wraps: {}", wraps.len());
                            let mut msgs = vec![];
                            for w in &wraps {
                                match stream::open_wrap(w, &g.sk, &g.pk, &ch.id, b.root_epoch) {
                                    Ok(r) => {
                                        let t = chrono::DateTime::from_timestamp_millis(r.ms)
                                            .map(|d| d.format("%d/%m %H:%M").to_string())
                                            .unwrap_or_default();
                                        msgs.push((
                                            r.ms,
                                            format!(
                                                "[{t}] kind={} {}: {}",
                                                r.kind,
                                                armada_tui::models::short(&r.author, 8).as_str(),
                                                r.content
                                            ),
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
                        Err(e) => println!("wraps: {e:#}"),
                    }
                }
            }
            Err(e) => println!("control: {e:#}"),
        }
    }

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
        let wraps = inv::fetch_wraps(&b.relays, &pk, 50, None, armada_tui::nostr::never_cancel())?;
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
                        format!(
                            "[{t}] kind={} {}: {}",
                            r.kind,
                            armada_tui::models::short(&r.author, 8).as_str(),
                            r.content
                        ),
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
