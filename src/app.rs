use crate::mock;
use crate::models::*;
use crate::nostr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Welcome,
    Server,
    Dms,
    Discover,
    Projects,
    Inbox,
    Settings,
    Help,
}

impl Screen {
    pub fn all() -> [Screen; 8] {
        [
            Screen::Welcome,
            Screen::Server,
            Screen::Dms,
            Screen::Discover,
            Screen::Projects,
            Screen::Inbox,
            Screen::Settings,
            Screen::Help,
        ]
    }

    pub fn title(self) -> &'static str {
        match self {
            Screen::Welcome => "1 Boas-vindas",
            Screen::Server => "2 Servidor",
            Screen::Dms => "3 DMs",
            Screen::Discover => "4 Descobrir",
            Screen::Projects => "5 Projetos",
            Screen::Inbox => "6 Inbox",
            Screen::Settings => "7 Settings",
            Screen::Help => "? Ajuda",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Channels,
    Messages,
}

/// Operação de rede pendente: executa após o próximo draw (p/ mostrar
/// "buscando…" antes do bloqueio de ~15s no MVP single-thread).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingOp {
    Groups,
    Messages,
    Send,
    Join,
}

pub struct App {
    pub screen: Screen,
    pub authed: bool,
    pub npub: String,
    pub login_input: String,
    pub login_error: Option<String>,
    pub communities: Vec<Community>,
    pub sel_community: usize,
    pub sel_channel: usize,
    pub sel_dm: usize,
    pub focus: Focus,
    pub msg_scroll: usize,
    pub input_mode: bool,
    pub input: String,
    pub status: String,
    pub relays: RelayConfig,
    pub dms: Vec<DmThread>,
    pub projects: Vec<ProjectItem>,
    pub discover: Vec<(String, String)>,
    pub pending: Option<PendingOp>,
    pub view_url: Option<String>,
    pub secret: Option<[u8; 32]>,
    pub live_write: bool,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::Welcome,
            authed: false,
            npub: "npub1…não logado".to_string(),
            login_input: String::new(),
            login_error: None,
            communities: mock::mock_communities(),
            sel_community: 0,
            sel_channel: 0,
            sel_dm: 0,
            focus: Focus::Sidebar,
            msg_scroll: 0,
            input_mode: false,
            input: String::new(),
            status: "desconectado — faça login com nsec (mock)".to_string(),
            relays: mock::default_relays(),
            dms: mock::mock_dms(),
            projects: mock::mock_projects(),
            discover: mock::mock_discover(),
            pending: None,
            view_url: None,
            secret: None,
            live_write: false,
            should_quit: false,
        }
    }

    pub fn current_community(&self) -> Option<&Community> {
        self.communities.get(self.sel_community)
    }

    pub fn current_channel(&self) -> Option<&Channel> {
        self.current_community()?.channels.get(self.sel_channel)
    }

    pub fn current_messages(&self) -> Vec<Message> {
        if self.screen == Screen::Dms {
            self.dms
                .get(self.sel_dm)
                .map(|d| d.messages.clone())
                .unwrap_or_default()
        } else {
            self.current_channel()
                .map(|c| c.messages.clone())
                .unwrap_or_default()
        }
    }

    pub fn login(&mut self) {
        let v = self.login_input.trim().to_string();
        if v.is_empty() {
            self.login_error = Some("cole seu nsec (qualquer valor vale no mock)".to_string());
            return;
        }
        // Fase 2: nsec/hex válido → escrita live; senão, modo leitura mock.
        match nostr::parse_secret(&v) {
            Ok(k) => {
                let suffix: String = k
                    .npub
                    .chars()
                    .rev()
                    .take(8)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                self.npub = format!("{}…{}", &k.npub[..9], suffix);
                self.secret = Some(k.secret);
                self.live_write = true;
                self.status = format!(
                    "chave ok — escrita live habilitada via {}",
                    self.relays
                        .app_relays
                        .first()
                        .map(|s| s.as_str())
                        .unwrap_or("?")
                );
            }
            Err(_) => {
                let suffix: String = v
                    .chars()
                    .rev()
                    .take(8)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                self.npub = format!("npub1…{suffix} (leitura)");
                self.secret = None;
                self.live_write = false;
                self.status =
                    "modo leitura (sem chave válida) — escrita live desligada".to_string();
            }
        }
        self.authed = true;
        self.screen = Screen::Server;
        self.login_error = None;
    }

    pub fn logout(&mut self) {
        self.authed = false;
        self.screen = Screen::Welcome;
        self.login_input.clear();
        self.secret = None;
        self.live_write = false;
        self.npub = "npub1…não logado".to_string();
        self.status = "desconectado".to_string();
    }

    pub fn goto(&mut self, s: Screen) {
        if !self.authed && s != Screen::Welcome && s != Screen::Help {
            self.status = "faça login primeiro (tela 1)".to_string();
            self.screen = Screen::Welcome;
            return;
        }
        self.screen = s;
        self.input_mode = false;
        self.msg_scroll = 0;
    }

    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Sidebar => Focus::Channels,
            Focus::Channels => Focus::Messages,
            Focus::Messages => Focus::Sidebar,
        };
    }

    pub fn move_up(&mut self) {
        match self.focus {
            Focus::Sidebar => {
                if self.sel_community > 0 {
                    self.sel_community -= 1;
                    self.sel_channel = 0;
                    self.msg_scroll = 0;
                }
            }
            Focus::Channels => {
                if self.sel_channel > 0 {
                    self.sel_channel -= 1;
                    self.msg_scroll = 0;
                }
            }
            Focus::Messages => {
                self.msg_scroll = self.msg_scroll.saturating_sub(1);
            }
        }
        if self.screen == Screen::Dms && self.focus != Focus::Messages {
            if self.sel_dm > 0 {
                self.sel_dm -= 1;
            }
        }
    }

    pub fn move_down(&mut self) {
        match self.focus {
            Focus::Sidebar => {
                if self.sel_community + 1 < self.communities.len() {
                    self.sel_community += 1;
                    self.sel_channel = 0;
                    self.msg_scroll = 0;
                }
            }
            Focus::Channels => {
                let n = self
                    .current_community()
                    .map(|c| c.channels.len())
                    .unwrap_or(0);
                if self.sel_channel + 1 < n {
                    self.sel_channel += 1;
                    self.msg_scroll = 0;
                }
            }
            Focus::Messages => {
                self.msg_scroll = self.msg_scroll.saturating_add(1);
            }
        }
        if self.screen == Screen::Dms && self.focus != Focus::Messages {
            if self.sel_dm + 1 < self.dms.len() {
                self.sel_dm += 1;
            }
        }
    }

    pub fn send_current_input(&mut self) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            self.input_mode = false;
            return;
        }
        let m = Message {
            author: "você".to_string(),
            content: text,
            time: "agora".to_string(),
            mine: true,
        };
        if self.screen == Screen::Dms {
            if let Some(d) = self.dms.get_mut(self.sel_dm) {
                d.messages.push(m);
                d.preview = d
                    .messages
                    .last()
                    .map(|x| x.content.clone())
                    .unwrap_or_default();
            }
            self.input.clear();
            self.input_mode = false;
            self.status = "DM mock local (NIP-17/44 no roadmap)".to_string();
            return;
        }
        // Fase 2: canal live + chave → publica no relay via pending.
        let is_live = matches!(
            (self.current_community(), self.current_channel()),
            (Some(c), Some(ch)) if c.relay.is_some() && ch.live_group.is_some()
        );
        if is_live {
            if self.secret.is_some() {
                self.status = "enviando ao relay…".to_string();
                self.pending = Some(PendingOp::Send);
            } else {
                self.status =
                    "canal live exige login com nsec (você está em modo leitura)".to_string();
                self.input_mode = false;
            }
            return;
        }
        if let Some(comm) = self.communities.get_mut(self.sel_community) {
            if let Some(ch) = comm.channels.get_mut(self.sel_channel) {
                if ch.is_voice {
                    self.status = "canal de voz: terminal mostra presença; áudio só no app gráfico"
                        .to_string();
                } else {
                    ch.messages.push(m);
                    self.status = "mensagem mock local".to_string();
                }
            }
            comm.unread = 0;
        }
        self.input.clear();
        self.input_mode = false;
    }

    fn live_target(&self) -> Option<(String, String)> {
        match (self.current_community(), self.current_channel()) {
            (Some(c), Some(ch)) => match (&c.relay, &ch.live_group) {
                (Some(r), Some(g)) => Some((r.clone(), g.clone())),
                _ => None,
            },
            _ => None,
        }
    }

    fn live_keys(&self) -> Option<nostr::Keys> {
        let secret = self.secret?;
        let secp = secp256k1::Secp256k1::new();
        let sk = secp256k1::SecretKey::from_slice(&secret).ok()?;
        let kp = secp256k1::Keypair::from_secret_key(&secp, &sk);
        let (xonly, _) = secp256k1::XOnlyPublicKey::from_keypair(&kp);
        // npub não é necessário aqui; reconstrói o mínimo para assinar.
        Some(nostr::Keys {
            secret,
            pubkey_hex: format!("{xonly}"),
            npub: String::new(),
        })
    }

    /// Fase 2: publica o input como kind 9 no grupo live (com NIP-42 se pedido).
    pub fn do_send(&mut self) {
        let text = self.input.trim().to_string();
        self.input.clear();
        self.input_mode = false;
        if text.is_empty() {
            return;
        }
        let (relay, group) = match self.live_target() {
            Some(t) => t,
            None => {
                self.status = "canal mock — nada a publicar".to_string();
                return;
            }
        };
        let keys = match self.live_keys() {
            Some(k) => k,
            None => {
                self.status = "sem chave — faça login com nsec".to_string();
                return;
            }
        };
        match nostr::send_chat(&relay, &keys, &group, &text) {
            Ok(id) => {
                self.status = format!("publicado em {group} ({id})");
                // Espelha localmente para feedback imediato.
                if let Some(comm) = self.communities.get_mut(self.sel_community) {
                    if let Some(ch) = comm.channels.get_mut(self.sel_channel) {
                        ch.messages.push(Message {
                            author: "você".to_string(),
                            content: text,
                            time: "agora".to_string(),
                            mine: true,
                        });
                    }
                }
            }
            Err(e) => self.status = format!("falha ao publicar: {e:#}"),
        }
    }

    /// Fase 2: pedido de entrada kind 9021 no grupo live atual.
    pub fn do_join(&mut self) {
        let (relay, group) = match self.live_target() {
            Some(t) => t,
            None => {
                self.status = "selecione um grupo live (r) antes do join".to_string();
                return;
            }
        };
        let keys = match self.live_keys() {
            Some(k) => k,
            None => {
                self.status = "join exige login com nsec".to_string();
                return;
            }
        };
        match nostr::send_join(&relay, &keys, &group) {
            Ok(id) => self.status = format!("join 9021 enviado p/ {group} ({id})"),
            Err(e) => self.status = format!("falha no join: {e:#}"),
        }
    }

    /// Fase 1: busca grupos públicos NIP-29 (kind 39000) no relay configurado.
    /// Mantém as comunidades mock (Concord) e troca as live anteriores.
    pub fn fetch_live_groups(&mut self) {
        let relay = self.relays.app_relays.first().cloned().unwrap_or_default();
        if relay.is_empty() {
            self.status = "sem relay configurado".to_string();
            return;
        }
        self.status = format!("buscando grupos NIP-29 em {relay}…");
        match nostr::fetch_groups(&relay) {
            Ok(groups) => {
                self.communities.retain(|c| c.relay.is_none());
                let n = groups.len();
                for g in groups {
                    self.communities.push(Community {
                        id: format!("live-{}", g.id),
                        name: g.name.clone(),
                        kind: CommunityKind::Nip29,
                        unread: 0,
                        relay: Some(relay.clone()),
                        channels: vec![Channel {
                            id: "chat".to_string(),
                            name: "chat".to_string(),
                            topic: if g.about.is_empty() {
                                "grupo live NIP-29 (m = buscar msgs)".to_string()
                            } else {
                                g.about.clone()
                            },
                            is_voice: false,
                            messages: Vec::new(),
                            live_group: Some(g.id.clone()),
                        }],
                    });
                }
                self.status = format!("{n} grupos live de {relay} (m = msgs, v = imagem)");
            }
            Err(e) => {
                self.status = format!("falha NIP-29: {e:#}");
            }
        }
    }

    /// Fase 1: busca mensagens do grupo live atual (kinds 1/9/11 + tag h).
    pub fn fetch_live_messages(&mut self) {
        let (relay, group) = match (self.current_community(), self.current_channel()) {
            (Some(c), Some(ch)) => match (&c.relay, &ch.live_group) {
                (Some(r), Some(g)) => (r.clone(), g.clone()),
                _ => {
                    self.status = "canal mock — use r p/ trazer grupos live antes".to_string();
                    return;
                }
            },
            _ => {
                self.status = "nada selecionado".to_string();
                return;
            }
        };
        self.status = format!("buscando msgs de {group}…");
        match nostr::fetch_messages(&relay, &group, 50) {
            Ok(msgs) => {
                let n = msgs.len();
                if let Some(comm) = self.communities.get_mut(self.sel_community) {
                    if let Some(ch) = comm.channels.get_mut(self.sel_channel) {
                        ch.messages = msgs
                            .into_iter()
                            .map(|m| Message {
                                author: m.author,
                                content: m.content,
                                time: m.time,
                                mine: false,
                            })
                            .collect();
                    }
                }
                self.status = format!("{n} msgs live de {group}");
            }
            Err(e) => {
                self.status = format!("falha msgs: {e:#}");
            }
        }
    }

    /// Primeira URL http(s) nas mensagens visíveis (da mais recente p/ antiga).
    pub fn first_image_url(&self) -> Option<String> {
        for m in self.current_messages().iter().rev() {
            for tok in m.content.split_whitespace() {
                let t = tok.trim_matches(|c| {
                    c == '<' || c == '>' || c == '(' || c == ')' || c == '"' || c == ','
                });
                if t.starts_with("http://") || t.starts_with("https://") {
                    return Some(t.to_string());
                }
            }
        }
        None
    }
}
