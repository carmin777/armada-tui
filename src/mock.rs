use crate::models::*;

/// Defaults espelhando o cliente Electron/web (ver README do armada):
/// app/search relays, Blossom media, voice broker CORD-07.
pub fn default_relays() -> RelayConfig {
    RelayConfig {
        app_relays: vec![
            "wss://relay.ditto.pub".to_string(),
            "wss://relay.dreamith.to".to_string(),
        ],
        search_relays: vec![
            "wss://relay.ditto.pub".to_string(),
            "wss://relay.dreamith.to".to_string(),
        ],
        blossom_servers: vec![
            "https://blossom.ditto.pub/".to_string(),
            "https://blossom.dreamith.to/".to_string(),
            "https://blossom.primal.net/".to_string(),
        ],
        voice_brokers: vec!["https://armada.buzz".to_string()],
    }
}

fn msg(author: &str, content: &str, time: &str, mine: bool) -> Message {
    Message {
        author: author.to_string(),
        content: content.to_string(),
        time: time.to_string(),
        mine,
    }
}

pub fn mock_communities() -> Vec<Community> {
    vec![
        Community {
            id: "fleet-1".to_string(),
            name: "soapbox-fleet".to_string(),
            kind: CommunityKind::Concord,
            unread: 3,
            relay: None,
            relays: Vec::new(),
            voice: false,
            channels: vec![
                Channel {
                    id: "general".to_string(),
                    name: "general".to_string(),
                    topic: "chat geral da frota (E2EE, sem servidor)".to_string(),
                    is_voice: false,
                    live_group: None,
                    stream_sk: None,
                    stream_id: None,
                    stream_epoch: None,
                    messages: vec![
                        msg(
                            "atlas",
                            "bem-vindos a bordo. chaves de vocês, frota de vocês.",
                            "10:01",
                            false,
                        ),
                        msg(
                            "vega",
                            "alguém testou voz via broker cego hoje?",
                            "10:04",
                            false,
                        ),
                        msg("você", "testando a TUI em ratatui 🐀", "10:05", true),
                    ],
                },
                Channel {
                    id: "dev".to_string(),
                    name: "dev".to_string(),
                    topic: "concord-v2 client-side, relays genéricos".to_string(),
                    is_voice: false,
                    live_group: None,
                    stream_sk: None,
                    stream_id: None,
                    stream_epoch: None,
                    messages: vec![
                        msg(
                            "atlas",
                            "gift-wrap (NIP-59) ok no relay local",
                            "09:12",
                            false,
                        ),
                        msg("vega", "rekey rodou sem cair o canal", "09:40", false),
                    ],
                },
                Channel {
                    id: "ponte".to_string(),
                    name: "ponte-voz".to_string(),
                    topic: "voz WebRTC via LiveKit (terminal: só presença)".to_string(),
                    is_voice: true,
                    live_group: None,
                    stream_sk: None,
                    stream_id: None,
                    stream_epoch: None,
                    messages: vec![msg(
                        "sistema",
                        "3 na chamada — áudio E2E só no app gráfico",
                        "10:00",
                        false,
                    )],
                },
            ],
        },
        Community {
            id: "relay-ops".to_string(),
            name: "relay-ops".to_string(),
            kind: CommunityKind::Nip29,
            unread: 0,
            relay: None,
            relays: Vec::new(),
            voice: false,
            channels: vec![
                Channel {
                    id: "ann".to_string(),
                    name: "announcements".to_string(),
                    topic: "grupo NIP-29 num relay externo".to_string(),
                    is_voice: false,
                    live_group: None,
                    stream_sk: None,
                    stream_id: None,
                    stream_epoch: None,
                    messages: vec![msg(
                        "relay-admin",
                        "relay wss://exemplo.br atualizado",
                        "08:00",
                        false,
                    )],
                },
                Channel {
                    id: "mod".to_string(),
                    name: "moderação".to_string(),
                    topic: "membership/moderation no relay".to_string(),
                    is_voice: false,
                    live_group: None,
                    stream_sk: None,
                    stream_id: None,
                    stream_epoch: None,
                    messages: vec![],
                },
            ],
        },
    ]
}

pub fn mock_dms() -> Vec<DmThread> {
    vec![
        DmThread {
            peer: "vega".to_string(),
            preview: "me manda teu npub quando puder".to_string(),
            messages: vec![
                msg("vega", "e aí, curtiu a armada?", "09:00", false),
                msg(
                    "você",
                    "curti — testando pelo terminal agora",
                    "09:01",
                    true,
                ),
                msg("vega", "me manda teu npub quando puder", "09:02", false),
            ],
        },
        DmThread {
            peer: "atlas".to_string(),
            preview: "invite: frota nova sexta".to_string(),
            messages: vec![msg("atlas", "invite: frota nova sexta", "07:30", false)],
        },
    ]
}

pub fn mock_projects() -> Vec<ProjectItem> {
    vec![
        ProjectItem {
            title: "paridade TUI: chat + canais".to_string(),
            status: "doing".to_string(),
            labels: vec!["tui".to_string(), "chat".to_string()],
        },
        ProjectItem {
            title: "voz no terminal (presença)".to_string(),
            status: "todo".to_string(),
            labels: vec!["voz".to_string(), "livekit".to_string()],
        },
        ProjectItem {
            title: "NIP-46 bunker login real".to_string(),
            status: "todo".to_string(),
            labels: vec!["auth".to_string(), "nostr".to_string()],
        },
    ]
}

pub fn mock_discover() -> Vec<(String, String)> {
    vec![
        ("buzz".to_string(), "team chat com agentes IA".to_string()),
        (
            "flotilla".to_string(),
            "comunidades nostr, rooms e moderação".to_string(),
        ),
        ("vector".to_string(), "DMs E2EE privadas".to_string()),
    ]
}
