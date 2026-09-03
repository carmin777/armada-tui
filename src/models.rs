use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommunityKind {
    /// Serverless E2EE (Concord / CORD) — gift-wrapped Nostr events.
    Concord,
    /// Relay-backed (NIP-29).
    Nip29,
}

impl CommunityKind {
    pub fn label(self) -> &'static str {
        match self {
            CommunityKind::Concord => "concord",
            CommunityKind::Nip29 => "nip-29",
        }
    }
}

/// Prefixo seguro por caracteres (sem pânico em fronteira UTF-8).
pub fn short(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub topic: String,
    pub is_voice: bool,
    pub messages: Vec<Message>,
    /// Grupo NIP-29 real quando `Some` (id do grupo); `None` = mock ou Concord.
    pub live_group: Option<String>,
    /// Stream Concord (canal com chave): segredo/id/epoch quando `Some`.
    pub stream_sk: Option<String>,
    pub stream_id: Option<String>,
    pub stream_epoch: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Community {
    pub id: String,
    pub name: String,
    pub kind: CommunityKind,
    pub channels: Vec<Channel>,
    pub unread: usize,
    /// Relay de origem quando buscado ao vivo (`r`); `None` = mock local.
    pub relay: Option<String>,
    /// Todos os relays da comunidade (bundle Concord ou live NIP-29).
    pub relays: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub author: String,
    pub content: String,
    pub time: String,
    pub mine: bool,
}

#[derive(Debug, Clone)]
pub struct DmThread {
    pub peer: String,
    pub preview: String,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone)]
pub struct ProjectItem {
    pub title: String,
    pub status: String,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RelayConfig {
    pub app_relays: Vec<String>,
    pub search_relays: Vec<String>,
    pub blossom_servers: Vec<String>,
    pub voice_brokers: Vec<String>,
}
