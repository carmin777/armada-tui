//! Nostr mínimo p/ fase 1: leitura pública NIP-29 via WebSocket.
//!
//! Sem dependência de SDK pesado: `tungstenite` sync + thread com timeout.
//!
//! - Grupos:  `["REQ", sub, {"kinds":[39000]}]` → metadados (d/name/about/picture)
//! - Mensagens: `["REQ", sub, {"kinds":[1,7,9,11,1111], "#h":[group-id], "limit":N}]`
//!
//! Grupos privados (NIP-42 auth) e escrita/E2EE ficam p/ fases seguintes.

use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// Flag que nunca cancela (exemplos/testes sem sessão).
pub fn never_cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

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

    pub fn has_tag(&self, name: &str) -> bool {
        self.tags
            .iter()
            .any(|t| t.first().map(|s| s == name).unwrap_or(false))
    }

    pub fn tag_all(&self, name: &str) -> Vec<&str> {
        self.tags
            .iter()
            .filter(|t| t.first().map(|s| s == name).unwrap_or(false))
            .filter_map(|t| t.get(1).map(|s| s.as_str()))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct Nip29Group {
    pub id: String,
    pub name: String,
    pub about: String,
    #[allow(dead_code)]
    pub picture: String,
    /// Tag `livekit` no anúncio 39000 → suporta sala de voz/vídeo.
    pub has_voice: bool,
}

#[derive(Debug, Clone)]
pub struct ChatMsg {
    pub kind: u64,
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

/// REQ genérico: conecta (teto 10s), manda filtro, coleta EVENTs até EOSE.
/// Deadline = timeout do chamador; sem EOSE = erro (sem sucesso parcial).
/// Roda em worker: socket com read-timeout morre sozinho.
pub(crate) fn req_events(
    relay_url: &str,
    sub_id: &str,
    filter: serde_json::Value,
    timeout: Duration,
    auth: Option<zeroize::Zeroizing<[u8; 32]>>,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<Vec<NostrEvent>> {
    let (tx, rx) = mpsc::channel();
    let url = relay_url.to_string();
    let req = serde_json::json!(["REQ", sub_id, filter]).to_string();
    let bye = serde_json::json!(["CLOSE", sub_id]).to_string();

    std::thread::spawn(move || {
        let out = (|| -> anyhow::Result<Vec<NostrEvent>> {
            let (mut socket, _) = {
                let (tx2, rx2) = mpsc::channel();
                let url2 = url.clone();
                std::thread::spawn(move || {
                    let _ = tx2.send(tungstenite::connect(&url2));
                });
                rx2.recv_timeout(Duration::from_secs(10))
                    .map_err(|_| anyhow::anyhow!("connect timeout (10s)"))??
            };
            arm_timeouts(&mut socket);
            let send_req = |socket: &mut Ws| -> anyhow::Result<()> {
                socket.send(tungstenite::Message::Text(req.clone()))?;
                Ok(())
            };
            send_req(&mut socket)?;
            let mut flow = ReadFlow::new();
            let mut events = Vec::new();
            let mut eose = false;
            let deadline = Instant::now() + timeout;
            loop {
                // Logout cancela: worker não assina nem envia mais nada.
                if cancel.load(Ordering::Relaxed) {
                    anyhow::bail!("cancelado (logout)");
                }
                if Instant::now() > deadline {
                    break;
                }
                // Teto anti-DoS: relay que ignora limit não inunda a RAM.
                if events.len() >= 2000 {
                    anyhow::bail!("relay excedeu 2000 eventos");
                }
                match socket.read() {
                    Ok(tungstenite::Message::Text(txt)) => {
                        let v: serde_json::Value = serde_json::from_str(&txt)?;
                        // NIP-42 no read via máquina (reenvia REQ só após OK do auth).
                        if matches!(v.get(0).and_then(|x| x.as_str()), Some("AUTH") | Some("OK")) {
                            let mut acted = false;
                            for act in flow.on_relay_msg(&v) {
                                acted = true;
                                match act {
                                    ReadAction::SendReq => send_req(&mut socket)?,
                                    ReadAction::SendAuth(ch) => {
                                        if let Some(secret) = &auth {
                                            let ev = sign_event(
                                                secret,
                                                22242,
                                                vec![
                                                    vec!["relay".to_string(), url.clone()],
                                                    vec!["challenge".to_string(), ch],
                                                ],
                                                "",
                                            )?;
                                            let aid = ev
                                                .get("id")
                                                .and_then(|x| x.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            socket.send(tungstenite::Message::Text(
                                                serde_json::json!(["AUTH", ev]).to_string(),
                                            ))?;
                                            flow.note_auth_sent(aid);
                                        }
                                    }
                                    ReadAction::Fail(e) => anyhow::bail!("{e}"),
                                    ReadAction::Ignore => {}
                                }
                            }
                            if acted {
                                continue;
                            }
                        }
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
                            Some("EOSE") => {
                                eose = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            let _ = socket.send(tungstenite::Message::Text(bye));
            let _ = socket.close(None);
            if !eose {
                anyhow::bail!("relay encerrou sem EOSE (resposta incompleta)");
            }
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
    // Estrito: id canônico é hex minúsculo (sem to_lowercase p/ fugir).
    if hex::encode(digest) != id {
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

/// Lista grupos públicos (kind 39000) do relay. Sem tag `d` = descarta;
/// duplicados por `d` ficam com o mais recente (não confia no relay).
pub fn fetch_groups(
    relay_url: &str,
    auth: Option<zeroize::Zeroizing<[u8; 32]>>,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<Vec<Nip29Group>> {
    crate::netpolicy::check_relay_url(relay_url)?;
    let filter = serde_json::json!({"kinds": [39000]});
    let evs = req_events(
        relay_url,
        "armada-groups",
        filter,
        Duration::from_secs(15),
        auth,
        cancel,
    )?;
    let mut by_id: std::collections::HashMap<String, NostrEvent> = std::collections::HashMap::new();
    for e in evs.into_iter().filter(|e| e.kind == 39000) {
        let Some(id) = e.tag("d").map(|s| s.to_string()) else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        match by_id.get(&id) {
            Some(old) if old.created_at >= e.created_at => {}
            _ => {
                by_id.insert(id, e);
            }
        }
    }
    Ok(by_id
        .into_values()
        .map(|e| {
            let id = e.tag("d").unwrap_or("?").to_string();
            let name = e.tag("name").unwrap_or(&id).to_string();
            Nip29Group {
                id,
                name,
                about: e.tag("about").unwrap_or("").to_string(),
                picture: e.tag("picture").unwrap_or("").to_string(),
                has_voice: e.has_tag("livekit"),
            }
        })
        .collect())
}

/// Últimas mensagens com tag `h = group_id` (kinds 1/7/9/11/1111), ordenadas.
/// Valida `#h` e deduplica por id localmente (relay pode ignorar o filtro).
pub fn fetch_messages(
    relay_url: &str,
    group_id: &str,
    limit: u32,
    auth: Option<zeroize::Zeroizing<[u8; 32]>>,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<Vec<ChatMsg>> {
    crate::netpolicy::check_relay_url(relay_url)?;
    let filter =
        serde_json::json!({"kinds": [1, 7, 9, 11, 1111], "#h": [group_id], "limit": limit});
    let mut evs = req_events(
        relay_url,
        "armada-msgs",
        filter,
        Duration::from_secs(15),
        auth,
        cancel,
    )?;
    evs.sort_by_key(|e| e.created_at);
    evs.retain(|e| matches!(e.kind, 1 | 7 | 9 | 11 | 1111));
    // #h local + dedupe por id.
    let mut seen = std::collections::HashSet::new();
    evs.retain(|e| {
        e.tags.iter().any(|t| {
            t.first().map(|s| s == "h").unwrap_or(false)
                && t.get(1).map(|s| s == group_id).unwrap_or(false)
        }) && seen.insert(e.id.clone())
    });
    Ok(evs
        .into_iter()
        .map(|e| ChatMsg {
            kind: e.kind,
            author: short_pk(&e.pubkey),
            content: e.content,
            time: fmt_time(e.created_at),
        })
        .collect())
}

/// Participantes ao vivo (kind 39004, tags `participant`); pega o evento
/// mais recente. Retorna pubkeys hex.
pub fn parse_participants(ev: &NostrEvent) -> Vec<String> {
    ev.tag_all("participant")
        .into_iter()
        .filter(|p| p.len() == 64 && hex::decode(p).is_ok())
        .map(|p| p.to_string())
        .collect()
}

/// Lê presença 39004 do grupo no relay (kind + tag d conferidos).
pub fn fetch_participants(
    relay_url: &str,
    group_id: &str,
    auth: Option<zeroize::Zeroizing<[u8; 32]>>,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<Vec<String>> {
    crate::netpolicy::check_relay_url(relay_url)?;
    let filter = serde_json::json!({"kinds": [39004], "#d": [group_id], "limit": 5});
    let mut evs = req_events(
        relay_url,
        "armada-voice",
        filter,
        Duration::from_secs(12),
        auth,
        cancel,
    )?;
    evs.retain(|e| e.kind == 39004 && e.tag("d").map(|d| d == group_id).unwrap_or(false));
    evs.sort_by_key(|e| e.created_at);
    Ok(evs
        .into_iter()
        .next_back()
        .map(|e| parse_participants(&e))
        .unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Fase 2: chaves + NIP-42 auth + escrita
// ---------------------------------------------------------------------------

/// Chave do usuário em memória (nunca logada, nunca serializada).
/// Drop zeroíza o segredo: clones em workers se limpam sozinhos.
#[derive(Debug, Clone)]
pub struct Keys {
    pub secret: [u8; 32],
    pub pubkey_hex: String,
    pub npub: String,
}

impl Drop for Keys {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret.zeroize();
    }
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
    sign_event_with(secret, kind, tags, content, chrono::Utc::now().timestamp())
}

/// Variante com timestamp explícito (testes, rumors com ms).
pub fn sign_event_with(
    secret: &[u8; 32],
    kind: u64,
    tags: Vec<Vec<String>>,
    content: &str,
    created_at: i64,
) -> anyhow::Result<serde_json::Value> {
    use sha2::Digest;
    let secp = secp256k1::Secp256k1::new();
    let sk = secp256k1::SecretKey::from_slice(secret)?;
    let kp = secp256k1::Keypair::from_secret_key(&secp, &sk);
    let (xonly, _) = secp256k1::XOnlyPublicKey::from_keypair(&kp);
    let pubkey = format!("{xonly}");
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

/// Máquina de estados NIP-42 p/ publish (pura, testável sem rede).
/// Regras: EVENT primeiro; AUTH → responde e SÓ reenvia EVENT após OK:true
/// do auth; OK:false com auth-required antes de autenticar = espera, não aborta.
pub struct PublishFlow {
    id: String,
    authed: bool,
    pending_auth: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FlowAction {
    SendEvent,
    SendAuth(String),
    Done(String),
    Fail(String),
    Ignore,
}

impl PublishFlow {
    pub fn new(id: String) -> Self {
        Self {
            id,
            authed: false,
            pending_auth: None,
        }
    }

    pub fn start(&mut self) -> FlowAction {
        FlowAction::SendEvent
    }

    pub fn note_auth_sent(&mut self, auth_id: String) {
        self.pending_auth = Some(auth_id);
    }

    fn is_auth_error(msg: &str) -> bool {
        let m = msg.to_lowercase();
        m.contains("auth-required") || m.contains("restricted:") || m.contains("authentication")
    }

    pub fn on_relay_msg(&mut self, v: &serde_json::Value) -> Vec<FlowAction> {
        match v.get(0).and_then(|x| x.as_str()) {
            Some("AUTH") => {
                if self.authed {
                    return vec![FlowAction::Ignore];
                }
                let ch = v.get(1).and_then(|x| x.as_str()).unwrap_or("").to_string();
                vec![FlowAction::SendAuth(ch)]
            }
            Some("OK") => {
                let oid = v.get(1).and_then(|x| x.as_str()).unwrap_or("");
                let ok = v.get(2).and_then(|x| x.as_bool()).unwrap_or(false);
                let msg = v.get(3).and_then(|x| x.as_str()).unwrap_or("").to_string();
                if Some(oid) == self.pending_auth.as_deref() {
                    if ok {
                        self.authed = true;
                        self.pending_auth = None;
                        return vec![FlowAction::SendEvent];
                    }
                    return vec![FlowAction::Fail(format!("auth rejeitado: {msg}"))];
                }
                if oid == self.id {
                    if ok {
                        return vec![FlowAction::Done(if msg.is_empty() {
                            self.id.clone()
                        } else {
                            msg
                        })];
                    }
                    if !self.authed && Self::is_auth_error(&msg) {
                        return vec![FlowAction::Ignore];
                    }
                    return vec![FlowAction::Fail(format!("relay rejeitou: {msg}"))];
                }
                vec![FlowAction::Ignore]
            }
            _ => vec![FlowAction::Ignore],
        }
    }
}

/// Máquina NIP-42 p/ LEITURA (pura, testável): AUTH → responde e SÓ
/// reenvia o REQ após OK:true do auth (não imediatamente).
pub struct ReadFlow {
    authed: bool,
    pending_auth: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReadAction {
    SendReq,
    SendAuth(String),
    Fail(String),
    Ignore,
}

impl ReadFlow {
    pub fn new() -> Self {
        Self {
            authed: false,
            pending_auth: None,
        }
    }

    pub fn note_auth_sent(&mut self, auth_id: String) {
        self.pending_auth = Some(auth_id);
    }

    pub fn on_relay_msg(&mut self, v: &serde_json::Value) -> Vec<ReadAction> {
        match v.get(0).and_then(|x| x.as_str()) {
            Some("AUTH") => {
                if self.authed {
                    return vec![ReadAction::Ignore];
                }
                let ch = v.get(1).and_then(|x| x.as_str()).unwrap_or("").to_string();
                vec![ReadAction::SendAuth(ch)]
            }
            Some("OK") => {
                let oid = v.get(1).and_then(|x| x.as_str()).unwrap_or("");
                if Some(oid) == self.pending_auth.as_deref() {
                    let ok = v.get(2).and_then(|x| x.as_bool()).unwrap_or(false);
                    if ok {
                        self.authed = true;
                        self.pending_auth = None;
                        return vec![ReadAction::SendReq];
                    }
                    let msg = v.get(3).and_then(|x| x.as_str()).unwrap_or("");
                    return vec![ReadAction::Fail(format!("auth rejeitado: {msg}"))];
                }
                vec![ReadAction::Ignore]
            }
            _ => vec![ReadAction::Ignore],
        }
    }
}

impl Default for ReadFlow {
    fn default() -> Self {
        Self::new()
    }
}

pub fn publish(
    relay_url: &str,
    keys: Option<&Keys>,
    event: serde_json::Value,
    timeout: Duration,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<String> {
    crate::netpolicy::check_relay_url(relay_url)?;
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
            // Connect com teto próprio (handshake sem timeout = thread órfã).
            let (mut socket, _) = {
                let (tx2, rx2) = mpsc::channel();
                let url2 = url.clone();
                std::thread::spawn(move || {
                    let _ = tx2.send(tungstenite::connect(&url2));
                });
                rx2.recv_timeout(Duration::from_secs(10))
                    .map_err(|_| anyhow::anyhow!("connect timeout (10s)"))??
            };
            arm_timeouts(&mut socket);
            let mut flow = PublishFlow::new(id.clone());
            debug_assert!(matches!(flow.start(), FlowAction::SendEvent));
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
            send_event(&mut socket)?;
            let deadline = Instant::now() + timeout;
            loop {
                if cancel.load(Ordering::Relaxed) {
                    anyhow::bail!("cancelado (logout)");
                }
                if Instant::now() > deadline {
                    anyhow::bail!("sem resposta OK do relay");
                }
                match socket.read() {
                    Ok(tungstenite::Message::Text(txt)) => {
                        let v: serde_json::Value = serde_json::from_str(&txt)?;
                        for act in flow.on_relay_msg(&v) {
                            match act {
                                FlowAction::SendEvent => send_event(&mut socket)?,
                                FlowAction::SendAuth(ch) => {
                                    let k = keys.as_ref().ok_or_else(|| {
                                        anyhow::anyhow!("relay pediu NIP-42 mas não há chave")
                                    })?;
                                    let ev = sign_event(
                                        &k.secret,
                                        22242,
                                        vec![
                                            vec!["relay".to_string(), url.clone()],
                                            vec!["challenge".to_string(), ch],
                                        ],
                                        "",
                                    )?;
                                    let aid = ev
                                        .get("id")
                                        .and_then(|x| x.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    socket.send(tungstenite::Message::Text(
                                        serde_json::json!(["AUTH", ev]).to_string(),
                                    ))?;
                                    flow.note_auth_sent(aid);
                                }
                                FlowAction::Done(out) => {
                                    let _ = socket.close(None);
                                    return Ok(out);
                                }
                                FlowAction::Fail(e) => anyhow::bail!("{e}"),
                                FlowAction::Ignore => {}
                            }
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

/// Publica wrap Concord em todos os relays EM PARALELO; basta 1 OK.
/// Teto global 25s p/ não somar timeouts em sequência.
pub fn publish_concord(
    relays: &[String],
    wrap: serde_json::Value,
    keys: Option<&Keys>,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<usize> {
    let relays = crate::netpolicy::filter_relays(relays);
    if relays.is_empty() {
        anyhow::bail!("nenhum relay permitido na política");
    }
    let (tx, rx) = mpsc::channel();
    for r in relays {
        let (tx, wrap, keys) = (tx.clone(), wrap.clone(), keys.cloned());
        std::thread::spawn(move || {
            let out = publish(&r, keys.as_ref(), wrap, Duration::from_secs(20));
            let _ = tx.send(out.is_ok());
        });
    }
    drop(tx);
    let deadline = Instant::now() + Duration::from_secs(25);
    let mut oks = 0usize;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match rx.recv_timeout(left) {
            Ok(true) => oks += 1,
            Ok(false) => {}
            Err(_) => break,
        }
    }
    if oks == 0 {
        anyhow::bail!("nenhum relay aceitou o wrap");
    }
    Ok(oks)
}

/// Chat no grupo live (kind 9 + tag h).
pub fn send_chat(
    relay_url: &str,
    keys: &Keys,
    group_id: &str,
    content: &str,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<String> {
    let ev = sign_event(
        &keys.secret,
        9,
        vec![vec!["h".to_string(), group_id.to_string()]],
        content,
    )?;
    publish(relay_url, Some(keys), ev, Duration::from_secs(20), cancel)
}

/// Pedido de entrada no grupo (kind 9021).
pub fn send_join(
    relay_url: &str,
    keys: &Keys,
    group_id: &str,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<String> {
    let ev = sign_event(
        &keys.secret,
        9021,
        vec![vec!["h".to_string(), group_id.to_string()]],
        "armada-tui",
    )?;
    publish(relay_url, Some(keys), ev, Duration::from_secs(20), cancel)
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

    fn msg(v: serde_json::Value) -> serde_json::Value {
        v
    }

    #[test]
    fn readflow_espera_ok_do_auth() {
        let mut f = ReadFlow::new();
        // Desafio → pede auth, SEM reenviar REQ ainda.
        let acts = f.on_relay_msg(&msg(serde_json::json!(["AUTH", "ch1"])));
        assert_eq!(acts, vec![ReadAction::SendAuth("ch1".to_string())]);
        f.note_auth_sent("a1".to_string());
        // OK do auth → agora sim reenvia REQ.
        let acts = f.on_relay_msg(&msg(serde_json::json!(["OK", "a1", true, ""])));
        assert_eq!(acts, vec![ReadAction::SendReq]);
        // Segundo desafio com sessão autenticada → ignora.
        let acts = f.on_relay_msg(&msg(serde_json::json!(["AUTH", "ch2"])));
        assert_eq!(acts, vec![ReadAction::Ignore]);
    }

    #[test]
    fn readflow_auth_negado_falha() {
        let mut f = ReadFlow::new();
        f.on_relay_msg(&msg(serde_json::json!(["AUTH", "ch1"])));
        f.note_auth_sent("a1".to_string());
        let acts = f.on_relay_msg(&msg(serde_json::json!(["OK", "a1", false, "banned"])));
        assert!(matches!(acts[0], ReadAction::Fail(_)));
    }

    #[test]
    fn flow_relay_aberto() {
        let mut f = PublishFlow::new("ev1".to_string());
        assert_eq!(f.start(), FlowAction::SendEvent);
        let acts = f.on_relay_msg(&msg(serde_json::json!(["OK", "ev1", true, ""])));
        assert_eq!(acts, vec![FlowAction::Done("ev1".to_string())]);
    }

    #[test]
    fn flow_nip42_completo() {
        let mut f = PublishFlow::new("ev1".to_string());
        assert_eq!(f.start(), FlowAction::SendEvent);
        // Relay pede auth antes: OK:false auth-required NÃO aborta.
        let acts = f.on_relay_msg(&msg(serde_json::json!([
            "OK",
            "ev1",
            false,
            "auth-required: login"
        ])));
        assert_eq!(acts, vec![FlowAction::Ignore]);
        // Desafio → responde auth (sem reenviar EVENT ainda).
        let acts = f.on_relay_msg(&msg(serde_json::json!(["AUTH", "ch rover"])));
        assert_eq!(acts, vec![FlowAction::SendAuth("ch rover".to_string())]);
        f.note_auth_sent("auth9".to_string());
        // OK do auth → SÓ agora reenvia EVENT.
        let acts = f.on_relay_msg(&msg(serde_json::json!(["OK", "auth9", true, ""])));
        assert_eq!(acts, vec![FlowAction::SendEvent]);
        // OK final.
        let acts = f.on_relay_msg(&msg(serde_json::json!(["OK", "ev1", true, "ok"])));
        assert_eq!(acts, vec![FlowAction::Done("ok".to_string())]);
    }

    #[test]
    fn presenca_39004_parse() {
        // Fixture no formato da spec (kind 39004 do relay).
        let ev = NostrEvent {
            id: String::new(),
            pubkey: String::new(),
            created_at: 0,
            kind: 39004,
            tags: vec![
                vec!["d".to_string(), "sala1".to_string()],
                vec![
                    "participant".to_string(),
                    "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798".to_string(),
                ],
                vec!["participant".to_string(), "lixo".to_string()],
                vec!["livekit".to_string(), "wss://voz.exemplo".to_string()],
            ],
            content: String::new(),
            sig: String::new(),
        };
        assert!(ev.has_tag("livekit"));
        assert!(!ev.has_tag("nada"));
        let ps = parse_participants(&ev);
        assert_eq!(ps.len(), 1);
        assert!(ps[0].starts_with("79be667e"));
    }

    #[test]
    fn flow_auth_rejeitado_e_erro_real() {
        let mut f = PublishFlow::new("ev1".to_string());
        f.on_relay_msg(&msg(serde_json::json!(["AUTH", "ch"])));
        f.note_auth_sent("auth9".to_string());
        let acts = f.on_relay_msg(&msg(serde_json::json!(["OK", "auth9", false, "banned"])));
        assert!(matches!(acts[0], FlowAction::Fail(_)));
        // Erro não-auth aborta na hora.
        let mut g = PublishFlow::new("ev2".to_string());
        let acts = g.on_relay_msg(&msg(serde_json::json!([
            "OK",
            "ev2",
            false,
            "blocked: spam"
        ])));
        assert!(matches!(acts[0], FlowAction::Fail(_)));
    }
}
