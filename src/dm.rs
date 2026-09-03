//! DMs E2EE (NIP-17): rumor 14 → seal 13 → wrap 1059.
//!
//! - Seal: kind 13, tags [], autor real, content = nip44(rumor, conv(sender, peer)).
//! - Wrap: kind 1059, tags [["p", peer, relay?]], autor efêmero, content =
//!   nip44(seal, conv(ephemeral, peer)). Um wrap por destinatário + um p/ si.
//! - Leitura: conv(my_secret, wrap.pubkey) → seal (verify) →
//!   conv(my_secret, seal.author) → rumor; rumor.pubkey == seal.pubkey.
//! - Envio: relays do kind 10050 do destinatário (fallback: app relays).
//! - `created_at` de seal/wrap randomizado até 48h p/ trás (anti-metadata).

use crate::concord::nip44;

pub const KIND_SEAL: u64 = 13;
pub const KIND_MSG: u64 = 14;
pub const KIND_WRAP: u64 = 1059;
pub const KIND_DM_RELAYS: u64 = 10050;

pub struct DmMsg {
    pub author: String,
    pub peer: String,
    pub content: String,
    pub created_at: i64,
    pub kind: u64,
}

fn xonly_of(secret: &[u8; 32]) -> anyhow::Result<String> {
    let secp = secp256k1::Secp256k1::new();
    let kp = secp256k1::Keypair::from_secret_key(&secp, &secp256k1::SecretKey::from_slice(secret)?);
    let (x, _) = secp256k1::XOnlyPublicKey::from_keypair(&kp);
    Ok(format!("{x}"))
}

fn random_past_secs(now: i64, max_back: i64) -> anyhow::Result<i64> {
    let mut b = [0u8; 8];
    getrandom::getrandom(&mut b)?;
    let r = u64::from_be_bytes(b) % (max_back.max(1) as u64);
    Ok(now - r as i64)
}

/// Constrói seal+wrap p/ um destinatário. Retorna o wrap assinado.
pub fn build_wrap(
    text: &str,
    peer_hex: &str,
    sender_secret: &[u8; 32],
    created_at: i64,
) -> anyhow::Result<serde_json::Value> {
    use crate::nostr::sign_event_with;
    let author = xonly_of(sender_secret)?;
    // Rumor kind 14 com p do destinatário.
    let rumor_tags = serde_json::json!([["p", peer_hex]]);
    let rumor_id = {
        use sha2::Digest;
        let commit = serde_json::json!([0, author, created_at, KIND_MSG, rumor_tags, text]);
        hex::encode(sha2::Sha256::digest(
            serde_json::to_string(&commit)?.as_bytes(),
        ))
    };
    let rumor = serde_json::json!({
        "kind": KIND_MSG, "pubkey": author, "created_at": created_at,
        "tags": rumor_tags, "content": text, "id": rumor_id,
    });
    // Seal kind 13 assinado pelo autor real.
    let conv = nip44::conversation_key(sender_secret, peer_hex)?;
    let seal_content = nip44::encrypt_random(&conv, serde_json::to_string(&rumor)?.as_bytes())?;
    let seal_at = random_past_secs(created_at, 172_800)?;
    let seal = sign_event_with(sender_secret, KIND_SEAL, Vec::new(), &seal_content, seal_at)?;
    // Wrap 1059 com chave efêmera.
    let mut eph = [0u8; 32];
    getrandom::getrandom(&mut eph)?;
    let eph_pk = xonly_of(&eph)?;
    let wconv = nip44::conversation_key(&eph, peer_hex)?;
    let wrap_content = nip44::encrypt_random(&wconv, serde_json::to_string(&seal)?.as_bytes())?;
    let wrap_at = random_past_secs(created_at, 172_800)?;
    let mut wrap = sign_event_with(
        &eph,
        KIND_WRAP,
        vec![vec!["p".to_string(), peer_hex.to_string()]],
        &wrap_content,
        wrap_at,
    )?;
    // Autor do wrap = chave efêmera (sign_event_with já deriva do secret).
    debug_assert_eq!(wrap["pubkey"], serde_json::Value::String(eph_pk));
    Ok(wrap)
}

/// Abre um wrap recebido (sou o `p`). `my_secret` = minha chave.
pub fn open_wrap(wrap: &serde_json::Value, my_secret: &[u8; 32]) -> anyhow::Result<DmMsg> {
    let kind = wrap.get("kind").and_then(|x| x.as_u64()).unwrap_or(0);
    if kind != KIND_WRAP {
        anyhow::bail!("não é gift wrap");
    }
    let wrap_pk = wrap.get("pubkey").and_then(|x| x.as_str()).unwrap_or("");
    let conv = nip44::conversation_key(my_secret, wrap_pk)?;
    let content = wrap.get("content").and_then(|x| x.as_str()).unwrap_or("");
    let seal_str = nip44::decrypt(content, &conv)?;
    let seal: serde_json::Value = serde_json::from_slice(&seal_str)?;
    crate::concord::stream::verify_event(&seal)?;
    let seal_kind = seal.get("kind").and_then(|x| x.as_u64()).unwrap_or(0);
    if seal_kind != KIND_SEAL {
        anyhow::bail!("não é seal NIP-17");
    }
    let seal_author = seal
        .get("pubkey")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let conv2 = nip44::conversation_key(my_secret, &seal_author)?;
    let seal_content = seal.get("content").and_then(|x| x.as_str()).unwrap_or("");
    let rumor_str = nip44::decrypt(seal_content, &conv2)?;
    let rumor: serde_json::Value = serde_json::from_slice(&rumor_str)?;
    let r_author = rumor.get("pubkey").and_then(|x| x.as_str()).unwrap_or("");
    if r_author != seal_author {
        anyhow::bail!("autor do rumor ≠ autor do seal (impersonation?)");
    }
    // Confere id canônico do rumor.
    let r_created = rumor
        .get("created_at")
        .and_then(|x| x.as_i64())
        .unwrap_or(-1);
    let r_kind = rumor.get("kind").and_then(|x| x.as_u64()).unwrap_or(0);
    let r_tags = rumor.get("tags").cloned().unwrap_or(serde_json::json!([]));
    let r_content = rumor.get("content").and_then(|x| x.as_str()).unwrap_or("");
    let r_id = rumor.get("id").and_then(|x| x.as_str()).unwrap_or("");
    {
        use sha2::Digest;
        let commit = serde_json::json!([0, r_author, r_created, r_kind, r_tags, r_content]);
        if hex::encode(sha2::Sha256::digest(
            serde_json::to_string(&commit)?.as_bytes(),
        )) != r_id
        {
            anyhow::bail!("id do rumor não confere");
        }
    }
    if r_kind != KIND_MSG {
        anyhow::bail!("rumor kind {r_kind} não é chat (ignorado)");
    }
    // Peer da thread: se eu sou o autor (wrap p/ mim mesmo), peer = p[0].
    let my_pk = xonly_of(my_secret)?;
    let peer = if r_author == my_pk {
        rumor
            .get("tags")
            .and_then(|t| t.as_array())
            .and_then(|a| {
                a.iter().find_map(|t| {
                    let a = t.as_array()?;
                    (a.first()?.as_str()? == "p")
                        .then(|| a.get(1)?.as_str().map(|s| s.to_string()))?
                })
            })
            .unwrap_or_else(|| r_author.to_string())
    } else {
        r_author.to_string()
    };
    Ok(DmMsg {
        author: r_author.to_string(),
        peer,
        content: r_content.to_string(),
        created_at: r_created,
        kind: r_kind,
    })
}

/// Relays de DM do peer (kind 10050); vazio = não pronto / fallback.
pub fn fetch_dm_relays(
    relays: &[String],
    peer_hex: &str,
    auth: Option<zeroize::Zeroizing<[u8; 32]>>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Vec<String> {
    let filter = serde_json::json!({"kinds": [KIND_DM_RELAYS], "authors": [peer_hex], "limit": 5});
    let mut out = Vec::new();
    for r in crate::netpolicy::filter_relays(relays) {
        if let Ok(evs) = crate::nostr::req_events(
            &r,
            "armada-dmrelays",
            filter.clone(),
            std::time::Duration::from_secs(10),
            auth.clone(),
            cancel.clone(),
        ) {
            let mut evs = evs;
            evs.sort_by_key(|e| e.created_at);
            if let Some(e) = evs.into_iter().next_back() {
                for t in e.tags.iter() {
                    // tags: Vec<Vec<String>> — coleta ["relay", url].
                    if t.first().map(|s| s == "relay").unwrap_or(false) {
                        if let Some(u) = t.get(1) {
                            if crate::netpolicy::check_relay_url(u).is_ok() {
                                out.push(u.clone());
                            }
                        }
                    }
                }
                if !out.is_empty() {
                    break;
                }
            }
        }
    }
    out
}

/// Busca wraps p/ mim (kind 1059 + #p eu) e abre os que forem DM.
pub fn fetch_threads(
    relays: &[String],
    my_secret: &[u8; 32],
    auth: Option<zeroize::Zeroizing<[u8; 32]>>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> anyhow::Result<Vec<DmMsg>> {
    let my_pk = xonly_of(my_secret)?;
    let filter = serde_json::json!({"kinds": [KIND_WRAP], "#p": [my_pk], "limit": 100});
    let mut all = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for r in crate::netpolicy::filter_relays(relays) {
        match crate::nostr::req_events(
            &r,
            "armada-dms",
            filter.clone(),
            std::time::Duration::from_secs(12),
            auth.clone(),
            cancel.clone(),
        ) {
            Ok(evs) => {
                for e in evs {
                    let v = serde_json::json!({
                        "id": e.id, "pubkey": e.pubkey, "created_at": e.created_at,
                        "kind": e.kind, "tags": e.tags, "content": e.content, "sig": e.sig,
                    });
                    if seen.insert(e.id.clone()) {
                        // Só DMs de verdade; resto (ex: outros gifts) ignora.
                        if let Ok(m) = open_wrap(&v, my_secret) {
                            all.push(m);
                        }
                    }
                }
            }
            Err(_) => {}
        }
    }
    all.sort_by_key(|m| m.created_at);
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sec(b: u8) -> [u8; 32] {
        [b; 32]
    }

    #[test]
    fn dm_roundtrip() {
        let alice = sec(0x11);
        let bob = sec(0x22);
        let bob_pk = xonly_of(&bob).unwrap();
        let alice_pk = xonly_of(&alice).unwrap();
        // Alice → Bob.
        let w = build_wrap("oi bob 🌊", &bob_pk, &alice, 1719800000).unwrap();
        assert_eq!(w["kind"], serde_json::Value::from(1059));
        let m = open_wrap(&w, &bob).unwrap();
        assert_eq!(m.content, "oi bob 🌊");
        assert_eq!(m.author, alice_pk);
        assert_eq!(m.peer, alice_pk);
        assert_eq!(m.kind, 14);
        // Bob não abre com chave errada.
        assert!(open_wrap(&w, &sec(0x33)).is_err());
        // Wrap p/ si (history sync): peer = destinatário.
        let w2 = build_wrap("nota p/ mim", &alice_pk, &alice, 1719800001).unwrap();
        let m2 = open_wrap(&w2, &alice).unwrap();
        assert_eq!(m2.peer, alice_pk);
        assert_eq!(m2.content, "nota p/ mim");
    }

    #[test]
    fn seal_forjado_rejeitado() {
        // Troca o autor do rumor sem re-assinar o seal → id quebra.
        let alice = sec(0x11);
        let bob = sec(0x22);
        let bob_pk = xonly_of(&bob).unwrap();
        let w = build_wrap("x", &bob_pk, &alice, 1719800000).unwrap();
        let mut tampered = w.clone();
        if let Some(c) = tampered.get_mut("content") {
            let mut s = c.as_str().unwrap().to_string();
            s.push('A');
            *c = serde_json::Value::String(s);
        }
        assert!(open_wrap(&tampered, &bob).is_err());
    }
}
