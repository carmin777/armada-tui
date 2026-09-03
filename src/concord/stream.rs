//! Abertura de planos selados: wrap(1059|21059) → seal(20013|20014) → rumor.
//!
//! Espelha `openWrap` + `checkChannelBinding` + `resolveMs` do `stream.ts`:
//! 1. wrap.pubkey == stream.pk (autor fixo da stream)
//! 2. nip44Decrypt(wrap.content, convKey) → seal + `verifyEvent(seal)`
//! 3. seal 20013 → nip44Decrypt → rumor (20014 → content verbatim);
//!    rumor.pubkey == seal.pubkey e rumor.id == getEventHash(rumor)
//! 4. binding: ["channel", id] + ["epoch", dec] strict-equal; ms válido.

use super::nip44;

pub const KIND_WRAP: u64 = 1059;
pub const KIND_WRAP_EPHEMERAL: u64 = 21059;
pub const KIND_SEAL_ENCRYPTED: u64 = 20013;
pub const KIND_SEAL_PLAINTEXT: u64 = 20014;

pub struct OpenedRumor {
    pub kind: u64,
    pub author: String,
    pub content: String,
    /// created_at*1000 + tag ms (igual `resolveMs`).
    pub ms: i64,
    pub channel: String,
    pub epoch: String,
}

fn tag(ev: &serde_json::Value, name: &str) -> Option<String> {
    ev.get("tags")?.as_array()?.iter().find_map(|t| {
        let a = t.as_array()?;
        if a.first()?.as_str()? == name {
            a.get(1)?.as_str().map(|s| s.to_string())
        } else {
            None
        }
    })
}

fn str_field(ev: &serde_json::Value, name: &str) -> anyhow::Result<String> {
    ev.get(name)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("evento sem campo {name}"))
}

/// id NIP-01 canônico (mesma serialização do sign_event).
fn event_id(
    pubkey: &str,
    created_at: i64,
    kind: u64,
    tags: &serde_json::Value,
    content: &str,
) -> anyhow::Result<[u8; 32]> {
    use sha2::Digest;
    let commit = serde_json::json!([0, pubkey, created_at, kind, tags, content]);
    let digest = sha2::Sha256::digest(serde_json::to_string(&commit)?.as_bytes());
    Ok(digest.into())
}

/// `verifyEvent`: id confere + schnorr válida (BIP-340).
pub(crate) fn verify_event(ev: &serde_json::Value) -> anyhow::Result<()> {
    let pubkey = str_field(ev, "pubkey")?;
    let created_at = ev
        .get("created_at")
        .and_then(|x| x.as_i64())
        .ok_or_else(|| anyhow::anyhow!("sem created_at"))?;
    let kind = ev
        .get("kind")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| anyhow::anyhow!("sem kind"))?;
    let tags = ev.get("tags").ok_or_else(|| anyhow::anyhow!("sem tags"))?;
    let content = str_field(ev, "content")?;
    let id = event_id(&pubkey, created_at, kind, tags, &content)?;
    if hex::encode(id) != str_field(ev, "id")? {
        anyhow::bail!("id não confere (evento adulterado?)");
    }
    let secp = secp256k1::Secp256k1::new();
    let sig = secp256k1::schnorr::Signature::from_slice(&hex::decode(str_field(ev, "sig")?)?)?;
    let msg = secp256k1::Message::from_digest(id);
    let xonly = secp256k1::XOnlyPublicKey::from_slice(&hex::decode(&pubkey)?)?;
    secp.verify_schnorr(&sig, &msg, &xonly)?;
    Ok(())
}

/// `resolveMs`: created_at(s)*1000 + tag ms (decimal estrito 0..999, sem "05").
fn resolve_ms(created_at: i64, ev: &serde_json::Value) -> anyhow::Result<i64> {
    match tag(ev, "ms") {
        None => Ok(created_at * 1000),
        Some(v) => {
            let ok = !v.is_empty()
                && v.len() <= 3
                && v.bytes().all(|b| b.is_ascii_digit())
                && (v == "0" || !v.starts_with('0'));
            if !ok {
                anyhow::bail!("tag ms inválida: {v}");
            }
            let ms: i64 = v.parse()?;
            if ms > 999 {
                anyhow::bail!("tag ms fora do limite: {v}");
            }
            Ok(created_at * 1000 + ms)
        }
    }
}

/// Abre um wrap da stream, validando tudo. `stream_sk` = segredo da stream
/// (ex: channelGroupKey().sk); `stream_pk` = hex x-only do autor fixo.
pub fn open_wrap(
    wrap: &serde_json::Value,
    stream_sk: &[u8; 32],
    stream_pk: &str,
    channel_id: &str,
    epoch: u64,
) -> anyhow::Result<OpenedRumor> {
    let kind = wrap
        .get("kind")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| anyhow::anyhow!("wrap sem kind"))?;
    if kind != KIND_WRAP && kind != KIND_WRAP_EPHEMERAL {
        anyhow::bail!("kind {kind} não é wrap");
    }
    if str_field(wrap, "pubkey")? != stream_pk {
        anyhow::bail!("wrap não é da stream esperada");
    }
    let conv = nip44::conversation_key(stream_sk, stream_pk)?;
    let seal_str = nip44::decrypt(&str_field(wrap, "content")?, &conv)?;
    let seal: serde_json::Value = serde_json::from_slice(&seal_str)?;
    verify_event(&seal)?;
    let seal_kind = seal
        .get("kind")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| anyhow::anyhow!("seal sem kind"))?;
    let rumor_str = if seal_kind == KIND_SEAL_ENCRYPTED {
        nip44::decrypt(&str_field(&seal, "content")?, &conv)?
    } else if seal_kind == KIND_SEAL_PLAINTEXT {
        str_field(&seal, "content")?.into_bytes()
    } else {
        anyhow::bail!("kind {seal_kind} não é seal");
    };
    let rumor: serde_json::Value = serde_json::from_slice(&rumor_str)?;
    if str_field(&rumor, "pubkey")? != str_field(&seal, "pubkey")? {
        anyhow::bail!("autor do rumor ≠ autor do seal");
    }
    // id do rumor precisa ser o hash canônico (pega splice/tamper).
    let r_created = rumor
        .get("created_at")
        .and_then(|x| x.as_i64())
        .ok_or_else(|| anyhow::anyhow!("rumor sem created_at"))?;
    let r_kind = rumor
        .get("kind")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| anyhow::anyhow!("rumor sem kind"))?;
    let r_tags = rumor
        .get("tags")
        .ok_or_else(|| anyhow::anyhow!("rumor sem tags"))?;
    let r_content = str_field(&rumor, "content")?;
    let r_pubkey = str_field(&rumor, "pubkey")?;
    if hex::encode(event_id(&r_pubkey, r_created, r_kind, r_tags, &r_content)?)
        != str_field(&rumor, "id")?
    {
        anyhow::bail!("id do rumor não confere");
    }
    // Binding anti-splice: channel + epoch strict-equal.
    let ch = tag(&rumor, "channel").ok_or_else(|| anyhow::anyhow!("rumor sem channel"))?;
    if ch != channel_id {
        anyhow::bail!("splice: channel {ch} ≠ esperado");
    }
    let ep = tag(&rumor, "epoch").ok_or_else(|| anyhow::anyhow!("rumor sem epoch"))?;
    if ep != epoch.to_string() {
        anyhow::bail!("epoch {ep} ≠ esperada");
    }
    Ok(OpenedRumor {
        kind: r_kind,
        author: r_pubkey,
        content: r_content,
        ms: resolve_ms(r_created, &rumor)?,
        channel: ch,
        epoch: ep,
    })
}

#[cfg(test)]
mod tests {
    use super::super::derive;
    use super::*;
    use crate::concord::fixture;

    fn h32(s: &str) -> [u8; 32] {
        hex::decode(s).unwrap().try_into().expect("32B")
    }

    #[test]
    fn abre_fixture_da_referencia() {
        // Chave da stream derivada por NÓS, wrap gerado pelo nostr-tools.
        let g = derive::group_key(
            derive::label::CHANNEL,
            &h32("0909090909090909090909090909090909090909090909090909090909090909"),
            &h32(fixture::CHANNEL_ID),
            Some(fixture::EPOCH),
        );
        assert_eq!(hex::encode(g.sk), fixture::STREAM_SK);
        assert_eq!(g.pk, fixture::STREAM_PK);

        let wrap: serde_json::Value = serde_json::from_str(fixture::WRAP_JSON).unwrap();
        let o = open_wrap(&wrap, &g.sk, &g.pk, fixture::CHANNEL_ID, fixture::EPOCH).unwrap();
        assert_eq!(o.kind, 9);
        assert_eq!(o.author, fixture::AUTHOR_PK);
        assert_eq!(o.content, "hello armada-tui 🌊");
        assert_eq!(o.ms, 1719800000417);
    }

    #[test]
    fn splice_e_epoch_errada_rejeitados() {
        let g = derive::group_key(
            derive::label::CHANNEL,
            &h32("0909090909090909090909090909090909090909090909090909090909090909"),
            &h32(fixture::CHANNEL_ID),
            Some(fixture::EPOCH),
        );
        let wrap: serde_json::Value = serde_json::from_str(fixture::WRAP_JSON).unwrap();
        let ff = "ff".repeat(32);
        assert!(open_wrap(&wrap, &g.sk, &g.pk, &ff, fixture::EPOCH).is_err());
        assert!(open_wrap(&wrap, &g.sk, &g.pk, fixture::CHANNEL_ID, 1).is_err());
        // Chave errada também falha (MAC do nip44).
        assert!(open_wrap(
            &wrap,
            &[7u8; 32],
            &g.pk,
            fixture::CHANNEL_ID,
            fixture::EPOCH
        )
        .is_err());
    }
}
