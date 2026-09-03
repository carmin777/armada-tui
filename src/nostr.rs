//! Nostr mínimo p/ fase 1: leitura pública NIP-29 via WebSocket.
//!
//! Sem dependência de SDK pesado: `tungstenite` sync + thread com timeout.
//! - Grupos:  `["REQ", sub, {"kinds":[39000]}]` → metadados (d/name/about/picture)
//! - Mensagens: `["REQ", sub, {"kinds":[1,9,11], "#h":[group-id], "limit":N}]`
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
/// Roda numa thread filha; a principal espera com `timeout` (thread órfã
/// bloqueada em read é abandonada — aceitável no MVP).
fn req_events(
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
                                        events.push(e);
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

/// Últimas mensagens com tag `h = group_id` (kinds 1/9/11), ordenadas.
pub fn fetch_messages(relay_url: &str, group_id: &str, limit: u32) -> anyhow::Result<Vec<ChatMsg>> {
    let filter = serde_json::json!({"kinds": [1, 9, 11], "#h": [group_id], "limit": limit});
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
