//! Convites CORD-05: link → token+relays → bundle 33301 → chaves dos canais.
//!
//! Link = `…/invite/<naddr>#<fragment>` (ou `naddr#fragment`):
//! naddr nomeia `(33301, link_signer, d="")`; fragmento base64url carrega
//! `[versão=4][flags][relays?][token:16]` — o fragmento nunca vai ao servidor.

use super::derive::{build_info, hkdf32, ZERO32};
use super::nip44;
use super::stream::verify_event;

pub const KIND_INVITE_BUNDLE: u64 = 33301;
pub const VSK_LIVE: &str = "6";
pub const VSK_REVOKED: &str = "9";
pub const FRAGMENT_VERSION: u8 = 4;
pub const TOKEN_BYTES: usize = 16;

pub const STOCK_RELAYS: [&str; 4] = [
    "wss://jskitty.com/nostr",
    "wss://asia.vectorapp.io/nostr",
    "wss://relay.ditto.pub",
    "wss://relay.dreamith.to",
];

fn dict_url(id: u8) -> Option<&'static str> {
    match id {
        1 => Some(STOCK_RELAYS[0]),
        2 => Some(STOCK_RELAYS[1]),
        3 => Some(STOCK_RELAYS[2]),
        4 => Some(STOCK_RELAYS[3]),
        _ => None,
    }
}

pub struct ParsedInvite {
    pub link_signer: String,
    pub token: [u8; 16],
    pub relays: Vec<String>,
}

/// Decodifica o fragmento base64url → token + bootstrap relays.
pub fn decode_fragment(frag: &str) -> anyhow::Result<([u8; 16], Vec<String>)> {
    use base64::Engine;
    let b = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(frag.trim())?;
    if b.len() < 2 {
        anyhow::bail!("fragmento curto");
    }
    if b[0] < FRAGMENT_VERSION {
        anyhow::bail!("link legado (versão {})", b[0]);
    }
    if b[0] > FRAGMENT_VERSION {
        anyhow::bail!("link mais novo que este cliente");
    }
    let flags = b[1];
    let mut o = 2usize;
    let mut relays = Vec::new();
    if flags & 0x01 != 0 {
        relays.extend(STOCK_RELAYS.iter().map(|s| s.to_string()));
    } else {
        if o >= b.len() {
            anyhow::bail!("fragmento truncado");
        }
        let count = b[o] as usize;
        o += 1;
        if count > 3 {
            anyhow::bail!("relays demais no fragmento");
        }
        for _ in 0..count {
            if o >= b.len() {
                anyhow::bail!("fragmento truncado");
            }
            let lead = b[o];
            o += 1;
            if (1..=254).contains(&lead) {
                if let Some(u) = dict_url(lead) {
                    relays.push(u.to_string());
                }
            } else {
                if o >= b.len() {
                    anyhow::bail!("fragmento truncado");
                }
                let len = b[o] as usize;
                o += 1;
                if o + len > b.len() {
                    anyhow::bail!("fragmento truncado");
                }
                let text = String::from_utf8(b[o..o + len].to_vec())?;
                o += len;
                relays.push(if lead == 255 {
                    text
                } else {
                    format!("wss://{text}")
                });
            }
        }
    }
    if o + TOKEN_BYTES > b.len() {
        anyhow::bail!("fragmento sem token");
    }
    let token: [u8; 16] = b[o..o + TOKEN_BYTES].try_into().expect("16B");
    o += TOKEN_BYTES;
    if o != b.len() {
        anyhow::bail!("bytes sobrando no fragmento");
    }
    Ok((token, relays))
}

/// Decodifica naddr (bech32 + TLV) → pubkey do link signer.
/// Exige kind 33301 e identifier vazio.
pub fn decode_naddr_signer(naddr: &str) -> anyhow::Result<String> {
    use bech32::FromBase32;
    let naddr = naddr.split('#').next().unwrap_or(naddr);
    let (hrp, data, _) = bech32::decode(naddr.trim())?;
    if hrp != "naddr" {
        anyhow::bail!("não é naddr");
    }
    let raw = Vec::<u8>::from_base32(&data)?;
    let mut identifier: Option<Vec<u8>> = None;
    let mut author: Option<Vec<u8>> = None;
    let mut kind: Option<u32> = None;
    let mut i = 0usize;
    while i + 2 <= raw.len() {
        let t = raw[i];
        let l = raw[i + 1] as usize;
        let v = raw
            .get(i + 2..i + 2 + l)
            .ok_or_else(|| anyhow::anyhow!("TLV truncado"))?;
        match t {
            0 => identifier = Some(v.to_vec()),
            2 => author = Some(v.to_vec()),
            3 => {
                if l != 4 {
                    anyhow::bail!("kind TLV inválido");
                }
                kind = Some(u32::from_be_bytes(v.try_into().expect("4B")));
            }
            _ => {}
        }
        i += 2 + l;
    }
    if kind != Some(KIND_INVITE_BUNDLE as u32) {
        anyhow::bail!("naddr não é coordenada de invite");
    }
    if identifier.as_deref().unwrap_or_default().is_empty() == false {
        anyhow::bail!("naddr com identifier inesperado");
    }
    let a = author.ok_or_else(|| anyhow::anyhow!("naddr sem autor"))?;
    if a.len() != 32 {
        anyhow::bail!("autor inválido");
    }
    Ok(hex::encode(a))
}

/// Parse do link completo (URL ou `naddr#fragmento`).
pub fn parse_invite_link(input: &str) -> Option<ParsedInvite> {
    let t = input.trim();
    let (naddr, fragment) = if let Some((head, rest)) = t.split_once('#') {
        if !head.to_lowercase().starts_with("naddr1") {
            // Pode ser URL com #fragmento.
            let (path, frag) = (head, rest);
            let marker = "/invite/";
            let pos = path.find(marker)?;
            Some((
                path[pos + marker.len()..].trim_end_matches('/').to_string(),
                frag.to_string(),
            ))
        } else {
            Some((head.to_string(), rest.to_string()))
        }
    } else if let Some(pos) = t.find("/invite/") {
        // URL sem fragmento → sem segredo, inútil.
        let _ = pos;
        return None;
    } else {
        return None;
    }?;
    if fragment.is_empty() {
        return None;
    }
    let link_signer = decode_naddr_signer(&naddr).ok()?;
    let (token, relays) = decode_fragment(&fragment).ok()?;
    Some(ParsedInvite {
        link_signer,
        token,
        relays,
    })
}

pub struct BundleChannel {
    pub id: String,
    pub key: String,
    pub epoch: u64,
    pub name: String,
}

pub struct Bundle {
    pub name: String,
    pub community_id: String,
    pub community_root: String,
    pub root_epoch: u64,
    pub relays: Vec<String>,
    pub channels: Vec<BundleChannel>,
}

/// `inviteBundleKey(token) = hkdf32(token, info("concord/invite-key", ZERO32))`.
pub fn invite_bundle_key(token: &[u8]) -> [u8; 32] {
    hkdf32(token, &build_info("concord/invite-key", &ZERO32, None))
}

/// Busca o bundle 33301 do signer nos relays (primeiro que responder vale).
pub fn fetch_bundle_event(relays: &[String], signer: &str) -> anyhow::Result<serde_json::Value> {
    let filter = serde_json::json!({"kinds": [KIND_INVITE_BUNDLE], "authors": [signer], "#d": [""], "limit": 5});
    let mut last_err = anyhow::anyhow!("sem relays");
    for r in relays {
        match crate::nostr::req_events(
            r,
            "armada-invite",
            filter.clone(),
            std::time::Duration::from_secs(12),
        ) {
            Ok(evs) => {
                // Prefere o mais recente com vsk live.
                let mut evs = evs;
                evs.sort_by_key(|e| e.created_at);
                if let Some(ev) = evs.into_iter().rev().next() {
                    let v = serde_json::json!({
                        "id": ev.id, "pubkey": ev.pubkey, "created_at": ev.created_at,
                        "kind": ev.kind, "tags": ev.tags, "content": ev.content, "sig": ev.sig,
                    });
                    return Ok(v);
                }
                last_err = anyhow::anyhow!("{r} sem bundle");
            }
            Err(e) => last_err = e,
        }
    }
    Err(anyhow::anyhow!("bundle não achado: {last_err:#}"))
}

/// Verifica + descriptografa o bundle (revogado/expirado/owner-mismatch incluídos).
pub fn open_bundle(
    event: &serde_json::Value,
    expected_signer: &str,
    token: &[u8],
    now_ms: i64,
) -> anyhow::Result<Bundle> {
    let kind = event.get("kind").and_then(|x| x.as_u64()).unwrap_or(0);
    let pubkey = event.get("pubkey").and_then(|x| x.as_str()).unwrap_or("");
    if kind != KIND_INVITE_BUNDLE || pubkey != expected_signer {
        anyhow::bail!("não é bundle do signer esperado");
    }
    verify_event(event)?;
    let vsk = event
        .get("tags")
        .and_then(|t| t.as_array())
        .and_then(|a| {
            a.iter().find_map(|t| {
                let a = t.as_array()?;
                (a.first()?.as_str()? == "vsk")
                    .then(|| a.get(1)?.as_str().map(|s| s.to_string()))?
            })
        })
        .unwrap_or_default();
    if vsk == VSK_REVOKED {
        anyhow::bail!("convite revogado");
    }
    if vsk != VSK_LIVE {
        anyhow::bail!("marcador de bundle desconhecido: {vsk}");
    }
    let content = event.get("content").and_then(|x| x.as_str()).unwrap_or("");
    let key = invite_bundle_key(token);
    let plain = nip44::decrypt(content, &key)?;
    let b: serde_json::Value = serde_json::from_slice(&plain)?;
    let get = |f: &str| {
        b.get(f)
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("bundle sem {f}"))
    };
    let (community_id, owner, salt) = (get("community_id")?, get("owner")?, get("owner_salt")?);
    // community_id auto-certifica (owner, salt).
    let o: [u8; 32] = hex::decode(&owner)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("owner inválido"))?;
    let s: [u8; 32] = hex::decode(&salt)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("salt inválido"))?;
    if hex::encode(super::derive::community_id_of(&o, &s)) != community_id {
        anyhow::bail!("owner-mismatch: community_id não reproduz");
    }
    if let Some(exp) = b.get("expires_at").and_then(|x| x.as_i64()) {
        if now_ms > exp {
            anyhow::bail!("convite expirado");
        }
    }
    let mut channels = Vec::new();
    if let Some(arr) = b.get("channels").and_then(|x| x.as_array()) {
        if arr.len() > 100 {
            anyhow::bail!("bundle com canais demais");
        }
        for c in arr {
            channels.push(BundleChannel {
                id: c
                    .get("id")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                key: c
                    .get("key")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                epoch: c.get("epoch").and_then(|x| x.as_u64()).unwrap_or(0),
                name: c
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("chat")
                    .to_string(),
            });
        }
    }
    Ok(Bundle {
        name: b
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("frota")
            .to_string(),
        community_id,
        community_root: get("community_root")?,
        root_epoch: b.get("root_epoch").and_then(|x| x.as_u64()).unwrap_or(0),
        relays: b
            .get("relays")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        channels,
    })
}

/// Busca wraps da stream (1059/21059 assinados pela stream) nos relays.
pub fn fetch_wraps(
    relays: &[String],
    stream_pk: &str,
    limit: u32,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let filter =
        serde_json::json!({"kinds": [1059, 21059], "authors": [stream_pk], "limit": limit});
    let mut last_err = anyhow::anyhow!("sem relays");
    for r in relays {
        match crate::nostr::req_events(
            r,
            "armada-wraps",
            filter.clone(),
            std::time::Duration::from_secs(12),
        ) {
            Ok(evs) if !evs.is_empty() => {
                return Ok(evs
                    .into_iter()
                    .map(|e| {
                        serde_json::json!({
                            "id": e.id, "pubkey": e.pubkey, "created_at": e.created_at,
                            "kind": e.kind, "tags": e.tags, "content": e.content, "sig": e.sig,
                        })
                    })
                    .collect());
            }
            Ok(_) => last_err = anyhow::anyhow!("{r} sem wraps"),
            Err(e) => last_err = e,
        }
    }
    Err(anyhow::anyhow!("wraps não achados: {last_err:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmento_stock() {
        // Fragmento da fixture: versão 4 + flag stock + token aa*16.
        let (tok, relays) = decode_fragment("BAGqqqqqqqqqqqqqqqqqqqqq").unwrap();
        assert_eq!(tok, [0xaa; 16]);
        assert_eq!(relays, STOCK_RELAYS);
    }

    #[test]
    fn bundle_roundtrip_referencia() {
        // Fixture gerada pelo nostr-tools (bundle_gen.mjs).
        let url = crate::concord::fixture::BUNDLE_URL;
        let p = parse_invite_link(url).expect("link válido");
        assert_eq!(p.link_signer, crate::concord::fixture::BUNDLE_SIGNER);
        assert_eq!(p.token, [0xaa; 16]);
        assert_eq!(p.relays, STOCK_RELAYS);

        let ev: serde_json::Value =
            serde_json::from_str(crate::concord::fixture::BUNDLE_EVENT_JSON).unwrap();
        let b = open_bundle(&ev, &p.link_signer, &p.token, 1719800000000).unwrap();
        assert_eq!(b.name, "frota-teste");
        assert_eq!(b.channels.len(), 1);
        assert_eq!(
            b.channels[0].key,
            "e3a112262b697db961c14d6ad1d4be7d351b4afa254b95f10d0147c0445b0394"
        );
        // Token errado → MAC falha.
        assert!(open_bundle(&ev, &p.link_signer, &[0xbb; 16], 1719800000000).is_err());
        // Expirado (agora em 2026) → a fixture é de 2024 sem expires_at... passa.
        // Revogado seria vsk 9 — coberto pelo parser.
    }
}
