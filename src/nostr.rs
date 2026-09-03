//! Nostr mínimo p/ fase 1: leitura pública NIP-29 via WebSocket.
//!
//! Sem dependência de SDK pesado: `tungstenite` sync + thread com timeout.
//! - Grupos:  `["REQ", sub, {"kinds":[39000]}]` → metadados (d/name/about/picture)
//! - Mensagens: `["REQ", sub, {"kinds":[1,7,9,11,1111], "#h":[group-id], "limit":N}]`
//! Grupos privados (NIP-42 auth) e escrita/E2EE ficam p/ fases seguintes.

use serde::Deserialize;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize)]
pub struct NostrEvent {
    pub id: String,
    pub pubkey: String,
    pub created_at: i64,
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    #[allow(dead_code)]
    pub content: String,
    #[allow(dead_code)]
    pub sig: String,
}

impl NostrEvent {
    pub fn tag(&self, name: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|t| t.first().map(|s| s == name).unwrap_or(false))
            .and_then(|t| t.get(1).map(|s| s.as_str()))
    }
}

#[derive(Debug, Clone)]
pub struct Nip29Group {
    pub id: String,
    pub name: String,
    pub about: String,
    #[allow(dead_code)]
    pub picture: String,
}

#[derive(Debug, Clone)]
pub struct ChatMsg {
    pub author: String,
    pub content: String,
    pub time: String,
}

fn short_pk(pk: &str) -> String {
    pk.chars().take(8).collect()
}

fn fmt_time(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.format("%d/%m %H:%M").to_string())
        .unwrap_or_else(|| "?".to_string())
}

/// REQ genérico: conecta, manda filtro, coleta EVENTs até EOSE ou deadline.
/// Roda numa thread filha; o socket tem read-timeout então a thread morre
/// sozinha se o relay travar (sem órfãs eternas).
pub(crate) fn req_events(
    relay_url: &str,
    sub_id: &str,
    filter: serde_json::Value,
    timeout: Duration,
) -> anyhow::Result<Vec<NostrEvent>> {
    let (tx, rx) = mpsc::channel();
    let url = relay_url.to_string();
    let req = serde_json::json!(["REQ", sub_id, filter]).to_string();
    let bye = serde_json::json!(["CLOSE", sub_id]).to_string();

    std::thread::spawn(move || {
        let out = (|| -> anyhow::Result<Vec<NostrEvent>> {
            let (mut socket, _) = tungstenite::connect(url)?;
            arm_timeouts(&mut socket);
            socket.send(tungstenite::Message::Text(req))?;
            let mut events = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(25);
            loop {
                if Instant::now() > deadline {
                    break;
                }
                match socket.read() {
                    Ok(tungstenite::Message::Text(txt)) => {
                        let v: serde_json::Value = serde_json::from_str(&txt)?;
                        match v.get(0).and_then(|x| x.as_str()) {
                            Some("EVENT") => {
                                if let Some(ev) = v.get(2) {
                                    if let Ok(e) = serde_json::from_value::<NostrEvent>(ev.clone())
                                    {
                                        // Relay malicioso não forja: valida antes de aceitar.
                                        if validate_nostr_event(&e).is_ok() {
                                            events.push(e);
                                        }
                                    }
                                }
                            }
                            Some("EOSE") => break,
                            _ => {}
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            let _ = socket.send(tungstenite::Message::Text(bye));
            let _ = socket.close(None);
            Ok(events)
        })();
        let _ = tx.send(out);
    });

    match rx.recv_timeout(timeout) {
        Ok(r) => r,
        Err(_) => anyhow::bail!("timeout falando com {relay_url} (15s)"),
    }
}

type Ws = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

/// Read-timeout no TCP interno (Plain ou Rustls) para threads não pendurarem.
fn arm_timeouts(socket: &mut Ws) {
    let d = Some(Duration::from_secs(30));
    #[allow(unreachable_patterns)]
    match socket.get_mut() {
        tungstenite::stream::MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(d);
        }
        tungstenite::stream::MaybeTlsStream::Rustls(s) => {
            let _ = s.get_mut().set_read_timeout(d);
        }
        _ => {}
    }
}

/// Validação NIP-01 central: id canônico + schnorr. Tudo que entra pela rede
/// passa aqui ANTES de ordenar/deduplicar/exibir.
pub(crate) fn validate_fields(
    pubkey: &str,
    created_at: i64,
    kind: u64,
    tags: &Vec<Vec<String>>,
    content: &str,
    id: &str,
    sig: &str,
) -> anyhow::Result<()> {
    use sha2::Digest;
    let commit = serde_json::json!([0, pubkey, created_at, kind, tags, content]);
    let digest = sha2::Sha256::digest(serde_json::to_string(&commit)?.as_bytes());
    if hex::encode(digest) != id.to_lowercase() {
        anyhow::bail!("id não confere");
    }
    let secp = secp256k1::Secp256k1::new();
    let s = secp256k1::schnorr::Signature::from_slice(&hex::decode(sig)?)?;
    let m = secp256k1::Message::from_digest(digest.into());
    let x = secp256k1::XOnlyPublicKey::from_slice(&hex::decode(pubkey)?)?;
    secp.verify_schnorr(&s, &m, &x)?;
    Ok(())
}

pub(crate) fn validate_nostr_event(e: &NostrEvent) -> anyhow::Result<()> {
    validate_fields(
        &e.pubkey,
        e.created_at,
        e.kind,
        &e.tags,
        &e.content,
        &e.id,
        &e.sig,
    )
}

/// Valida evento em JSON (bundle, wraps, o que vier de fora).
pub(crate) fn validate_value(ev: &serde_json::Value) -> anyhow::Result<()> {
    let s = |f: &str| {
        ev.get(f)
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow::anyhow!("sem campo {f}"))
    };
    let tags: Vec<Vec<String>> = ev
        .get("tags")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .map(|t| {
                    t.as_array()
                        .map(|inner| {
                            inner
                                .iter()
                                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
        .ok_or_else(|| anyhow::anyhow!("sem tags"))?;
    validate_fields(
        s("pubkey")?,
        ev.get("created_at").and_then(|x| x.as_i64()).unwrap_or(-1),
        ev.get("kind").and_then(|x| x.as_u64()).unwrap_or(u64::MAX),
        &tags,
        s("content")?,
        s("id")?,
        s("sig")?,
    )
}

/// Lista grupos públicos (kind 39000) do relay.
pub fn fetch_groups(relay_url: &str) -> anyhow::Result<Vec<Nip29Group>> {
    let filter = serde_json::json!({"kinds": [39000]});
    let evs = req_events(relay_url, "armada-groups", filter, Duration::from_secs(15))?;
    Ok(evs
        .into_iter()
        .map(|e| {
            let id = e.tag("d").unwrap_or("?").to_string();
            let name = e.tag("name").unwrap_or(&id).to_string();
            Nip29Group {
                id,
                name,
                about: e.tag("about").unwrap_or("").to_string(),
                picture: e.tag("picture").unwrap_or("").to_string(),
            }
        })
        .collect())
}

/// Últimas mensagens com tag `h = group_id` (kinds 1/7/9/11/1111), ordenadas.
pub fn fetch_messages(relay_url: &str, group_id: &str, limit: u32) -> anyhow::Result<Vec<ChatMsg>> {
    let filter =
        serde_json::json!({"kinds": [1, 7, 9, 11, 1111], "#h": [group_id], "limit": limit});
    let mut evs = req_events(relay_url, "armada-msgs", filter, Duration::from_secs(15))?;
    evs.sort_by_key(|e| e.created_at);
    Ok(evs
        .into_iter()
        .map(|e| ChatMsg {
            author: short_pk(&e.pubkey),
            content: e.content,
            time: fmt_time(e.created_at),
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Fase 2: chaves + NIP-42 auth + escrita
// ---------------------------------------------------------------------------

/// Chave do usuário em memória (nunca logada, nunca serializada).
#[derive(Debug, Clone)]
pub struct Keys {
    pub secret: [u8; 32],
    pub pubkey_hex: String,
    pub npub: String,
}

/// Aceita `nsec1…` (bech32) ou hex de 64 chars.
pub fn parse_secret(input: &str) -> anyhow::Result<Keys> {
    use bech32::{FromBase32, ToBase32};
    let v = input.trim();
    let raw: [u8; 32] = if v.starts_with("nsec1") {
        let (hrp, data, _variant) = bech32::decode(v)?;
        if hrp != "nsec" {
            anyhow::bail!("bech32 não é nsec");
        }
        let bytes = Vec::<u8>::from_base32(&data)?;
        bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("nsec com tamanho inválido"))?
    } else {
        let h = v.strip_prefix("0x").unwrap_or(v);
        let bytes = hex::decode(h)?;
        bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("esperava nsec1 ou hex de 64 chars"))?
    };
    let secp = secp256k1::Secp256k1::new();
    let sk = secp256k1::SecretKey::from_slice(&raw)?;
    let kp = secp256k1::Keypair::from_secret_key(&secp, &sk);
    let (xonly, _parity) = secp256k1::XOnlyPublicKey::from_keypair(&kp);
    let pubkey_hex = format!("{xonly}");
    let npub = bech32::encode(
        "npub",
        xonly.serialize().to_vec().to_base32(),
        bech32::Variant::Bech32,
    )?;
    Ok(Keys {
        secret: raw,
        pubkey_hex,
        npub,
    })
}

/// Conta de brincadeira: gera segredo aleatório (CSPRNG) e deriva tudo.
pub fn generate() -> anyhow::Result<Keys> {
    loop {
        let mut raw = [0u8; 32];
        getrandom::getrandom(&mut raw)?;
        if secp256k1::SecretKey::from_slice(&raw).is_ok() {
            return keys_from_secret(raw);
        }
    }
}

fn keys_from_secret(raw: [u8; 32]) -> anyhow::Result<Keys> {
    use bech32::ToBase32;
    let secp = secp256k1::Secp256k1::new();
    let sk = secp256k1::SecretKey::from_slice(&raw)?;
    let kp = secp256k1::Keypair::from_secret_key(&secp, &sk);
    let (xonly, _parity) = secp256k1::XOnlyPublicKey::from_keypair(&kp);
    let pubkey_hex = format!("{xonly}");
    let npub = bech32::encode(
        "npub",
        xonly.serialize().to_vec().to_base32(),
        bech32::Variant::Bech32,
    )?;
    Ok(Keys {
        secret: raw,
        pubkey_hex,
        npub,
    })
}

/// Assina evento (NIP-01: id = sha256 do serialize canônico, schnorr sign).
pub fn sign_event(
    secret: &[u8; 32],
    kind: u64,
    tags: Vec<Vec<String>>,
    content: &str,
) -> anyhow::Result<serde_json::Value> {
    use sha2::Digest;
    let secp = secp256k1::Secp256k1::new();
    let sk = secp256k1::SecretKey::from_slice(secret)?;
    let kp = secp256k1::Keypair::from_secret_key(&secp, &sk);
    let (xonly, _) = secp256k1::XOnlyPublicKey::from_keypair(&kp);
    let pubkey = format!("{xonly}");
    let created_at = chrono::Utc::now().timestamp();
    let commit = serde_json::json!([0, pubkey, created_at, kind, tags, content]);
    let serialized = serde_json::to_string(&commit)?;
    let digest = sha2::Sha256::digest(serialized.as_bytes());
    let msg = secp256k1::Message::from_digest(digest.into());
    let sig = secp.sign_schnorr_no_aux_rand(&msg, &kp);
    Ok(serde_json::json!({
        "id": hex::encode(digest),
        "pubkey": pubkey,
        "created_at": created_at,
        "kind": kind,
        "tags": tags,
        "content": content,
        "sig": sig.to_string(),
    }))
}

/// Publica evento respondendo NIP-42 (`["AUTH", challenge]` → kind 22242).
/// Retorna o `id` em caso de `OK:true`, ou erro com a mensagem do relay.
pub fn publish(
    relay_url: &str,
    keys: Option<&Keys>,
    event: serde_json::Value,
    timeout: Duration,
) -> anyhow::Result<String> {
    let (tx, rx) = mpsc::channel();
    let url = relay_url.to_string();
    let id = event
        .get("id")
        .and_then(|x| x.as_str())
        .unwrap_or("?")
        .to_string();
    let evt_s = event.to_string();
    let keys = keys.cloned();

    std::thread::spawn(move || {
        let out = (|| -> anyhow::Result<String> {
            let (mut socket, _) = tungstenite::connect(url.clone())?;
            arm_timeouts(&mut socket);
            let send_event = |socket: &mut Ws| -> anyhow::Result<()> {
                socket.send(tungstenite::Message::Text(
                    serde_json::json!([
                        "EVENT",
                        serde_json::from_str::<serde_json::Value>(&evt_s)?
                    ])
                    .to_string(),
                ))?;
                Ok(())
            };
            let send_auth = |socket: &mut Ws, challenge: &str| -> anyhow::Result<()> {
                let k = keys.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("relay pediu NIP-42 mas não há chave (faça login com nsec)")
                })?;
                let ev = sign_event(
                    &k.secret,
                    22242,
                    vec![
                        vec!["relay".to_string(), url.clone()],
                        vec!["challenge".to_string(), challenge.to_string()],
                    ],
                    "",
                )?;
                socket.send(tungstenite::Message::Text(
                    serde_json::json!(["AUTH", ev]).to_string(),
                ))?;
                Ok(())
            };
            send_event(&mut socket)?;
            let deadline = Instant::now() + Duration::from_secs(25);
            loop {
                if Instant::now() > deadline {
                    anyhow::bail!("sem resposta OK do relay");
                }
                match socket.read() {
                    Ok(tungstenite::Message::Text(txt)) => {
                        let v: serde_json::Value = serde_json::from_str(&txt)?;
                        match v.get(0).and_then(|x| x.as_str()) {
                            Some("AUTH") => {
                                let ch = v.get(1).and_then(|x| x.as_str()).unwrap_or("");
                                send_auth(&mut socket, ch)?;
                                // NIP-42: depois de autenticar, reenvia o EVENT.
                                send_event(&mut socket)?;
                            }
                            Some("OK") => {
                                if v.get(1).and_then(|x| x.as_str()) == Some(id.as_str()) {
                                    let ok = v.get(2).and_then(|x| x.as_bool()).unwrap_or(false);
                                    let msg =
                                        v.get(3).and_then(|x| x.as_str()).unwrap_or("").to_string();
                                    if ok {
                                        let _ = socket.close(None);
                                        return Ok(if msg.is_empty() { id } else { msg });
                                    }
                                    anyhow::bail!("relay rejeitou: {msg}");
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(_) => {}
                    Err(_) => anyhow::bail!("conexão caiu antes do OK"),
                }
            }
        })();
        let _ = tx.send(out);
    });

    match rx.recv_timeout(timeout) {
        Ok(r) => r,
        Err(_) => anyhow::bail!("timeout publicando em {relay_url}"),
    }
}

/// Chat no grupo live (kind 9 + tag h).
pub fn send_chat(
    relay_url: &str,
    keys: &Keys,
    group_id: &str,
    content: &str,
) -> anyhow::Result<String> {
    let ev = sign_event(
        &keys.secret,
        9,
        vec![vec!["h".to_string(), group_id.to_string()]],
        content,
    )?;
    publish(relay_url, Some(keys), ev, Duration::from_secs(20))
}

/// Pedido de entrada no grupo (kind 9021).
pub fn send_join(relay_url: &str, keys: &Keys, group_id: &str) -> anyhow::Result<String> {
    let ev = sign_event(
        &keys.secret,
        9021,
        vec![vec!["h".to_string(), group_id.to_string()]],
        "armada-tui",
    )?;
    publish(relay_url, Some(keys), ev, Duration::from_secs(20))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_vetor_nsec() {
        // Vetor gerado pelo nostr-tools (secret 0x01).
        let k = parse_secret("nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqsmhltgl")
            .unwrap();
        assert_eq!(
            k.pubkey_hex,
            "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
        );
        assert_eq!(
            k.npub,
            "npub10xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqpkge6d"
        );
        // Hex equivalente dá o mesmo.
        let k2 = parse_secret("0000000000000000000000000000000000000000000000000000000000000001")
            .unwrap();
        assert_eq!(k2.pubkey_hex, k.pubkey_hex);
        // Lixo rejeitado.
        assert!(parse_secret("nsec1xyz").is_err());
        assert!(parse_secret("00").is_err());
    }

    #[test]
    fn generate_valido() {
        let k = generate().unwrap();
        assert_eq!(k.pubkey_hex.len(), 64);
        assert!(k.npub.starts_with("npub1"));
    }
}
