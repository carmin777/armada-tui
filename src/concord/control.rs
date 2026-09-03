//! Control plane (CORD): edições 3308 (vsk/eid/ev) → fold de canais.
//!
//! - Stream: `controlGroupKey(root, communityId, epoch)` (label concord/control).
//! - Edição de canal: vsk=2, eid=channelId, ev=versão, content={name, private, deleted?}.
//! - Fold: por eid, vence maior versão (desempate: maior rumor id).
//! - Canal PÚBLICO deriva do root (`channelGroupKey`); PRIVADO exige grant do bundle.

use super::derive;
use super::invite::fetch_wraps;
use super::stream::open_stream_event;
use std::collections::HashMap;

pub const KIND_CONTROL: u64 = 3308;
pub const VSK_CHANNEL: &str = "2";

#[derive(Debug, Clone)]
pub struct ControlChannel {
    pub id: String,
    pub name: String,
    pub is_private: bool,
}

fn tag(tags: &[Vec<String>], name: &str) -> Option<String> {
    tags.iter().find_map(|t| {
        (t.first().map(|s| s.as_str()) == Some(name))
            .then(|| t.get(1).cloned())
            .flatten()
    })
}

/// Dobra wraps do control em canais (puro, sem rede — testável).
pub fn fold_channel_wraps(
    wraps: &[serde_json::Value],
    sk: &[u8; 32],
    pk: &str,
) -> Vec<ControlChannel> {
    // eid → (versão, rumor_id, nome, privado, deletado)
    let mut fold: HashMap<String, (u64, String, String, bool, bool)> = HashMap::new();
    for w in wraps {
        let o = match open_stream_event(w, sk, pk) {
            Ok(o) => o,
            Err(_) => continue,
        };
        if o.kind != KIND_CONTROL || o.seal_kind != super::stream::KIND_SEAL_PLAINTEXT {
            continue;
        }
        if tag(&o.tags, "vsk").as_deref() != Some(VSK_CHANNEL) {
            continue;
        }
        let eid = match tag(&o.tags, "eid") {
            Some(e) if e.len() == 64 && hex::decode(&e).is_ok() => e,
            _ => continue,
        };
        let ver: u64 = tag(&o.tags, "ev").and_then(|v| v.parse().ok()).unwrap_or(0);
        let meta: serde_json::Value = match serde_json::from_str(&o.content) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let entry = (
            ver,
            o.rumor_id.clone(),
            meta.get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            meta.get("private")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
            meta.get("deleted")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
        );
        let replace = match fold.get(&eid) {
            Some((v, r, _, _, _)) => (ver, &o.rumor_id) > (*v, r),
            None => true,
        };
        if replace {
            fold.insert(eid, entry);
        }
    }
    let mut out: Vec<ControlChannel> = fold
        .into_iter()
        .filter_map(|(id, (_, _, name, is_private, deleted))| {
            if deleted || name.is_empty() {
                return None;
            }
            Some(ControlChannel {
                id,
                name,
                is_private,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Lê e dobra as edições de canal do control plane via relays.
pub fn fetch_control_channels(
    relays: &[String],
    root: &[u8; 32],
    community_id: &str,
    epoch: u64,
) -> anyhow::Result<Vec<ControlChannel>> {
    let cid: [u8; 32] = hex::decode(community_id)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("community_id inválido"))?;
    let g = derive::group_key(derive::label::CONTROL, root, &cid, Some(epoch));
    let wraps = fetch_wraps(relays, &g.pk, 200)?;
    Ok(fold_channel_wraps(&wraps, &g.sk, &g.pk))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concord::fixture;

    fn h32(s: &str) -> [u8; 32] {
        hex::decode(s).unwrap().try_into().expect("32B")
    }

    #[test]
    fn fold_das_edicoes_referencia() {
        let root = h32(fixture::CONTROL_ROOT);
        let g = derive::group_key(
            derive::label::CONTROL,
            &root,
            &h32(fixture::CONTROL_CID),
            Some(0),
        );
        assert_eq!(g.pk, fixture::CONTROL_PK);
        let wraps: Vec<serde_json::Value> = fixture::CONTROL_WRAPS
            .iter()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        let chs = fold_channel_wraps(&wraps, &g.sk, &g.pk);
        // general v0 → v1 (renamed vence); secret privado; metadata ignorado.
        assert_eq!(chs.len(), 2);
        assert_eq!(chs[0].name, "general-renamed");
        assert!(!chs[0].is_private);
        assert_eq!(chs[0].id, fixture::CONTROL_GENERAL);
        assert_eq!(chs[1].name, "secret");
        assert!(chs[1].is_private);
    }
}
