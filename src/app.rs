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
    Parity,
    Help,
}

impl Screen {
    pub fn all() -> [Screen; 9] {
        [
            Screen::Welcome,
            Screen::Server,
            Screen::Dms,
            Screen::Discover,
            Screen::Projects,
            Screen::Inbox,
            Screen::Settings,
            Screen::Parity,
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
            Screen::Parity => "8 Paridade",
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

/// Pedido de rede: vira worker em background (UI nunca bloqueia).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingOp {
    Groups,
    Messages,
    Send,
    Join,
    Invite,
}

/// Resultado que o worker devolve para a thread da UI aplicar.
pub enum OpResult {
    Groups(Vec<Community>),
    Chat {
        ci: usize,
        chi: usize,
        messages: Vec<Message>,
        presence: Vec<String>,
    },
    Sent {
        ci: usize,
        chi: usize,
        text: String,
        id: String,
    },
    Joined(String),
    Invited(Community),
    Failed(String),
}

/// Operação em voo: a UI desenha spinner e continua respondendo.
pub struct BusyOp {
    pub label: String,
    pub session: u64,
    pub rx: std::sync::mpsc::Receiver<OpResult>,
}

/// Snapshot p/ envio Concord (tudo clonado p/ worker).
#[derive(Clone)]
pub struct ConcordSend {
    pub relays: Vec<String>,
    pub sk: [u8; 32],
    pub ch_id: String,
    pub epoch: u64,
}
#[derive(Clone)]
pub enum MsgTarget {
    Nip29 {
        relay: String,
        group: String,
        voice: bool,
    },
    Concord {
        relays: Vec<String>,
        sk: [u8; 32],
        pk: String,
        ch_id: String,
        epoch: u64,
        my_pk: String,
    },
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
    pub busy: Option<BusyOp>,
    pub view_url: Option<String>,
    pub secret: Option<[u8; 32]>,
    pub live_write: bool,
    pub invite_mode: bool,
    pub invite_input: String,
    pub session: u64,
    pub should_quit: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
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
            busy: None,
            view_url: None,
            secret: None,
            live_write: false,
            invite_mode: false,
            invite_input: String::new(),
            session: 0,
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
        // P1-4: o segredo digitado sai da memória imediatamente.
        self.login_input.clear();
    }

    pub fn logout(&mut self) {
        use zeroize::Zeroize;
        self.authed = false;
        self.screen = Screen::Welcome;
        self.login_input.clear();
        self.login_input.zeroize();
        self.invite_input.clear();
        self.invite_input.zeroize();
        // Chave da sessão + chaves E2EE dos canais: zeroíza tudo.
        if let Some(mut s) = self.secret.take() {
            s.zeroize();
        }
        for c in &mut self.communities {
            for ch in &mut c.channels {
                if let Some(mut k) = ch.stream_sk.take() {
                    k.zeroize();
                }
            }
        }
        // Volta ao mock: nada da sessão anterior sobrevive.
        self.communities = mock::mock_communities();
        self.sel_community = 0;
        self.sel_channel = 0;
        self.msg_scroll = 0;
        self.input.clear();
        self.input_mode = false;
        // Workers antigos morrem órfãos: resultados de outra sessão são ignorados.
        self.busy = None;
        self.pending = None;
        self.session = self.session.wrapping_add(1);
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
        if self.screen == Screen::Dms && self.focus != Focus::Messages && self.sel_dm > 0 {
            self.sel_dm -= 1;
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
        if self.screen == Screen::Dms
            && self.focus != Focus::Messages
            && self.sel_dm + 1 < self.dms.len()
        {
            self.sel_dm += 1;
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
        // Canal live (NIP-29 ou Concord com chave) + nsec → publica via pending.
        // Sem chave, o rascunho fica no input (modo leitura).
        let can_send = matches!(
            (self.current_community(), self.current_channel()),
            (Some(c), Some(ch))
                if (c.relay.is_some() && ch.live_group.is_some()) || ch.stream_sk.is_some()
        );
        if can_send {
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

    /// Dispara o pending em background: a UI segue respondendo (spinner).
    pub fn start_pending(&mut self) {
        let op = match self.pending.take() {
            Some(o) => o,
            None => return,
        };
        if self.busy.is_some() {
            self.pending = Some(op);
            self.status = "aguarde a operação atual…".to_string();
            return;
        }
        let (label, worker): (String, Box<dyn FnOnce() -> OpResult + Send>) = match op {
            PendingOp::Groups => {
                let relay = self.relays.app_relays.first().cloned().unwrap_or_default();
                (
                    format!("grupos em {relay}"),
                    Box::new(move || Self::op_fetch_groups(relay)),
                )
            }
            PendingOp::Messages => match self.snapshot_msg_target() {
                Some(t) => {
                    let (ci, chi) = (self.sel_community, self.sel_channel);
                    (
                        "msgs…".to_string(),
                        Box::new(move || Self::op_fetch_messages(t, ci, chi)),
                    )
                }
                None => {
                    self.status = "nada live selecionado (r/I antes)".to_string();
                    return;
                }
            },
            PendingOp::Send => {
                let text = self.input.trim().to_string();
                let (ci, chi) = (self.sel_community, self.sel_channel);
                self.input_mode = false;
                if text.is_empty() {
                    return;
                }
                if let (Some((relay, group)), Some(keys)) = (self.live_target(), self.live_keys()) {
                    (
                        "enviando…".to_string(),
                        Box::new(move || Self::op_send(relay, group, keys, text, ci, chi)),
                    )
                } else if let (Some(cc), Some(keys)) =
                    (self.snapshot_concord_send(), self.live_keys())
                {
                    (
                        "selando+enviando…".to_string(),
                        Box::new(move || Self::op_concord_send(cc, keys, text, ci, chi)),
                    )
                } else {
                    self.status = "sem chave ou sem canal live".to_string();
                    return;
                }
            }
            PendingOp::Join => match (self.live_target(), self.live_keys()) {
                (Some((relay, group)), Some(keys)) => (
                    "join 9021…".to_string(),
                    Box::new(move || Self::op_join(relay, group, keys)),
                ),
                _ => {
                    self.status = "join exige grupo live + nsec".to_string();
                    return;
                }
            },
            PendingOp::Invite => {
                let link = self.invite_input.trim().to_string();
                self.invite_input.clear();
                self.invite_mode = false;
                if link.is_empty() {
                    return;
                }
                (
                    "invite…".to_string(),
                    Box::new(move || Self::op_invite(link)),
                )
            }
        };
        self.status = format!("ocupado: {label}");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let out = worker();
            let _ = tx.send(out);
        });
        self.busy = Some(BusyOp {
            label,
            rx,
            session: self.session,
        });
    }

    /// Colhe resultado do worker (não bloqueia) e aplica no estado.
    pub fn poll_busy(&mut self) {
        let stale = matches!(&self.busy, Some(b) if b.session != self.session);
        if stale {
            self.busy = None;
            return;
        }
        let done = match &self.busy {
            Some(b) => match b.rx.try_recv() {
                Ok(out) => Some(out),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(OpResult::Failed("worker morreu".to_string())),
            },
            None => return,
        };
        let out = match done {
            Some(o) => o,
            None => return,
        };
        self.busy = None;
        match out {
            OpResult::Groups(list) => {
                self.communities
                    .retain(|c| c.relay.is_none() || c.kind == CommunityKind::Concord);
                let n = list.len();
                self.communities.extend(list);
                self.status = format!("{n} grupos live (m = msgs, v = imagem)");
            }
            OpResult::Chat {
                ci,
                chi,
                messages,
                presence,
            } => {
                let n = messages.len();
                if let Some(comm) = self.communities.get_mut(ci) {
                    if let Some(ch) = comm.channels.get_mut(chi) {
                        ch.messages = messages;
                    }
                }
                self.msg_scroll = 0;
                self.status = if presence.is_empty() {
                    format!("{n} msgs")
                } else {
                    let who: Vec<String> = presence
                        .iter()
                        .map(|p| crate::models::short(p, 8))
                        .collect();
                    format!(
                        "{n} msgs · 🔊 {} na chamada ({})",
                        presence.len(),
                        who.join(", ")
                    )
                };
            }
            OpResult::Sent { ci, chi, text, id } => {
                self.input.clear();
                self.input_mode = false;
                if let Some(comm) = self.communities.get_mut(ci) {
                    if let Some(ch) = comm.channels.get_mut(chi) {
                        ch.messages.push(Message {
                            author: "você".to_string(),
                            content: text,
                            time: "agora".to_string(),
                            mine: true,
                        });
                    }
                }
                self.status = format!("publicado ({id})");
            }
            OpResult::Joined(id) => self.status = format!("join 9021 enviado ({id})"),
            OpResult::Invited(comm) => {
                let n = comm.channels.len();
                let name = comm.name.clone();
                self.communities.push(comm);
                self.sel_community = self.communities.len() - 1;
                self.sel_channel = 0;
                self.status = format!("frota '{name}' com {n} canais (m = descriptografar)");
            }
            OpResult::Failed(e) => {
                // P2-7: rascunho preservado — texto volta pro input p/ retry.
                if !self.input.trim().is_empty() {
                    self.input_mode = true;
                }
                self.status = format!("falha: {e}");
            }
        }
    }

    /// Snapshot p/ envio Concord (relays + chave da stream do canal).
    fn snapshot_concord_send(&self) -> Option<ConcordSend> {
        let (c, ch) = (self.current_community()?, self.current_channel()?);
        let sk: [u8; 32] = hex::decode(ch.stream_sk.as_ref()?).ok()?.try_into().ok()?;
        let id = hex::decode(ch.stream_id.as_ref()?).ok()?;
        if id.len() != 32 {
            return None;
        }
        Some(ConcordSend {
            relays: c.relays.clone(),
            sk,
            ch_id: ch.stream_id.clone()?,
            epoch: ch.stream_epoch?,
        })
    }

    /// Envio Concord no worker: seal+wrap e publica nos relays.
    fn op_concord_send(
        cc: ConcordSend,
        keys: nostr::Keys,
        text: String,
        ci: usize,
        chi: usize,
    ) -> OpResult {
        use crate::concord::stream;
        let wrap = match stream::build_chat_wrap(&text, &cc.ch_id, cc.epoch, &keys.secret, &cc.sk) {
            Ok(w) => w,
            Err(e) => return OpResult::Failed(format!("falha ao selar: {e:#}")),
        };
        match nostr::publish_concord(&cc.relays, wrap, Some(&keys)) {
            Ok(n) => OpResult::Sent {
                ci,
                chi,
                text,
                id: format!("wrap em {n} relays"),
            },
            Err(e) => OpResult::Failed(format!("falha ao publicar: {e:#}")),
        }
    }
    fn snapshot_msg_target(&self) -> Option<MsgTarget> {
        let (c, ch) = (self.current_community()?, self.current_channel()?);
        if let (Some(sk), Some(id), Some(ep)) = (&ch.stream_sk, &ch.stream_id, ch.stream_epoch) {
            let sk: [u8; 32] = hex::decode(sk).ok()?.try_into().ok()?;
            let secp = secp256k1::Secp256k1::new();
            let kp = secp256k1::Keypair::from_secret_key(
                &secp,
                &secp256k1::SecretKey::from_slice(&sk).ok()?,
            );
            let (xonly, _) = secp256k1::XOnlyPublicKey::from_keypair(&kp);
            return Some(MsgTarget::Concord {
                relays: c.relays.clone(),
                sk,
                pk: format!("{xonly}"),
                ch_id: id.clone(),
                epoch: ep,
                my_pk: self.live_keys().map(|k| k.pubkey_hex).unwrap_or_default(),
            });
        }
        match (&c.relay, &ch.live_group) {
            (Some(r), Some(g)) => Some(MsgTarget::Nip29 {
                relay: r.clone(),
                group: g.clone(),
                voice: c.voice,
            }),
            _ => None,
        }
    }

    /// Grupos NIP-29 (kind 39000) → comunidades prontas (roda no worker).
    fn op_fetch_groups(relay: String) -> OpResult {
        if relay.is_empty() {
            return OpResult::Failed("sem relay configurado".to_string());
        }
        match nostr::fetch_groups(&relay) {
            Ok(groups) => {
                let list = groups
                    .into_iter()
                    .map(|g| Community {
                        id: format!("live-{}", g.id),
                        name: g.name.clone(),
                        kind: CommunityKind::Nip29,
                        unread: 0,
                        relay: Some(relay.clone()),
                        relays: vec![relay.clone()],
                        voice: g.has_voice,
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
                            stream_sk: None,
                            stream_id: None,
                            stream_epoch: None,
                        }],
                    })
                    .collect();
                OpResult::Groups(list)
            }
            Err(e) => OpResult::Failed(format!("falha NIP-29: {e:#}")),
        }
    }

    /// Lê mensagens no worker (NIP-29 ou Concord E2EE).
    fn op_fetch_messages(t: MsgTarget, ci: usize, chi: usize) -> OpResult {
        match t {
            MsgTarget::Nip29 {
                relay,
                group,
                voice,
            } => {
                let presence = if voice {
                    nostr::fetch_participants(&relay, &group).unwrap_or_default()
                } else {
                    Vec::new()
                };
                match nostr::fetch_messages(&relay, &group, 50) {
                    Ok(msgs) => OpResult::Chat {
                        ci,
                        chi,
                        messages: msgs
                            .into_iter()
                            .map(|m| Message {
                                author: m.author,
                                content: m.content,
                                time: m.time,
                                mine: false,
                            })
                            .collect(),
                        presence,
                    },
                    Err(e) => OpResult::Failed(format!("falha msgs: {e:#}")),
                }
            }
            MsgTarget::Concord {
                relays,
                sk,
                pk,
                ch_id,
                epoch,
                my_pk,
            } => {
                use crate::concord::{invite as inv, stream};
                let wraps = match inv::fetch_wraps(&relays, &pk, 50) {
                    Ok(w) => w,
                    Err(e) => return OpResult::Failed(format!("wraps: {e:#}")),
                };
                let mut msgs: Vec<(i64, Message)> = Vec::new();
                for w in &wraps {
                    if let Ok(r) = stream::open_wrap(w, &sk, &pk, &ch_id, epoch) {
                        if r.kind == 9 {
                            msgs.push((
                                r.ms,
                                Message {
                                    author: if r.author == my_pk {
                                        "você".to_string()
                                    } else {
                                        crate::models::short(&r.author, 8)
                                    },
                                    content: r.content,
                                    time: chrono::DateTime::from_timestamp_millis(r.ms)
                                        .map(|d| d.format("%d/%m %H:%M").to_string())
                                        .unwrap_or_else(|| "?".to_string()),
                                    mine: r.author == my_pk,
                                },
                            ));
                        }
                    }
                }
                msgs.sort_by_key(|(ms, _)| *ms);
                OpResult::Chat {
                    ci,
                    chi,
                    messages: msgs.into_iter().map(|(_, m)| m).collect(),
                    presence: Vec::new(),
                }
            }
        }
    }

    /// Publica kind 9 no worker (NIP-42 com reenvio dentro do publish).
    fn op_send(
        relay: String,
        group: String,
        keys: nostr::Keys,
        text: String,
        ci: usize,
        chi: usize,
    ) -> OpResult {
        match nostr::send_chat(&relay, &keys, &group, &text) {
            Ok(id) => OpResult::Sent { ci, chi, text, id },
            Err(e) => OpResult::Failed(format!("falha ao publicar: {e:#}")),
        }
    }

    /// Join 9021 no worker.
    fn op_join(relay: String, group: String, keys: nostr::Keys) -> OpResult {
        match nostr::send_join(&relay, &keys, &group) {
            Ok(id) => OpResult::Joined(id),
            Err(e) => OpResult::Failed(format!("falha no join: {e:#}")),
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

    /// Aplica chaves (login com nsec ou conta gerada).
    pub fn apply_keys(&mut self, keys: &nostr::Keys, label: &str) {
        let suffix: String = keys
            .npub
            .chars()
            .rev()
            .take(8)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        self.npub = format!("{}…{}{}", &keys.npub[..9], suffix, label);
        self.secret = Some(keys.secret);
        self.live_write = true;
        self.authed = true;
        self.screen = Screen::Server;
        self.login_error = None;
        self.login_input.clear();
        self.status = format!(
            "chave ok ({}) — escrita live via {}",
            self.npub,
            self.relays
                .app_relays
                .first()
                .map(|s| s.as_str())
                .unwrap_or("?")
        );
    }

    /// Conta de brincadeira: gera identidade aleatória e entra.
    pub fn login_generated(&mut self) {
        match nostr::generate() {
            Ok(k) => self.apply_keys(&k, " (brincadeira)"),
            Err(e) => self.status = format!("falha ao gerar: {e:#}"),
        }
    }

    /// Invite Concord no worker: parse → bundle → control → comunidade pronta.
    fn op_invite(link: String) -> OpResult {
        use crate::concord::invite as inv;
        let p = match inv::parse_invite_link(&link) {
            Some(p) => p,
            None => {
                return OpResult::Failed(
                    "link não parece invite Concord (…/invite/<naddr>#…)".to_string(),
                )
            }
        };
        let ev = match inv::fetch_bundle_event(&p.relays, &p.link_signer) {
            Ok(ev) => ev,
            Err(e) => return OpResult::Failed(format!("bundle não achado: {e:#}")),
        };
        let now_ms = chrono::Utc::now().timestamp_millis();
        let b = match inv::open_bundle(&ev, &p.link_signer, &p.token, now_ms) {
            Ok(b) => b,
            Err(e) => return OpResult::Failed(format!("convite inválido: {e:#}")),
        };
        let primary = b.relays.first().cloned().unwrap_or_default();
        // Control plane: descobre canais (públicos derivam do root).
        use crate::concord::{control, derive};
        let root: Option<[u8; 32]> = hex::decode(&b.community_root)
            .ok()
            .and_then(|v| v.try_into().ok());
        let cid: Option<[u8; 32]> = hex::decode(&b.community_id)
            .ok()
            .and_then(|v| v.try_into().ok());
        let folded: Vec<control::ControlChannel> = match (root, cid) {
            (Some(r), Some(c)) => {
                control::fetch_control_channels(&b.relays, &r, &hex::encode(c), b.root_epoch)
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        };
        let mut channels: Vec<Channel> = Vec::new();
        // Grants privados do bundle, por id.
        let grants: std::collections::HashMap<String, (String, u64)> = b
            .channels
            .iter()
            .map(|c| (c.id.clone(), (c.key.clone(), c.epoch)))
            .collect();
        if let (Some(r), Some(_c)) = (root, cid) {
            for f in &folded {
                if f.is_private {
                    // Privado só com grant; sem chave, omite (igual ao TS).
                    if let Some((key, epoch)) = grants.get(&f.id) {
                        channels.push(Channel {
                            id: f.id.clone(),
                            name: f.name.clone(),
                            topic: format!("privado · epoch {epoch}"),
                            is_voice: false,
                            messages: Vec::new(),
                            live_group: None,
                            stream_sk: Some(key.clone()),
                            stream_id: Some(f.id.clone()),
                            stream_epoch: Some(*epoch),
                        });
                    }
                } else if let Ok(id) = hex::decode(&f.id).and_then(|v| {
                    v.try_into()
                        .map_err(|_| hex::FromHexError::InvalidStringLength)
                }) {
                    let g = derive::group_key(derive::label::CHANNEL, &r, &id, Some(b.root_epoch));
                    channels.push(Channel {
                        id: f.id.clone(),
                        name: f.name.clone(),
                        topic: format!("público · epoch {} · m = msgs", b.root_epoch),
                        is_voice: false,
                        messages: Vec::new(),
                        live_group: None,
                        stream_sk: Some(hex::encode(g.sk)),
                        stream_id: Some(f.id.clone()),
                        stream_epoch: Some(b.root_epoch),
                    });
                }
            }
        }
        // Grants do bundle ainda sem fold (o fold pode atrasar num join fresco).
        for gc in &b.channels {
            if !channels.iter().any(|ch| ch.id == gc.id) {
                channels.push(Channel {
                    id: gc.id.clone(),
                    name: if gc.name.is_empty() {
                        format!("#{}", crate::models::short(&gc.id, 8))
                    } else {
                        gc.name.clone()
                    },
                    topic: format!("grant · epoch {}", gc.epoch),
                    is_voice: false,
                    messages: Vec::new(),
                    live_group: None,
                    stream_sk: Some(gc.key.clone()),
                    stream_id: Some(gc.id.clone()),
                    stream_epoch: Some(gc.epoch),
                });
            }
        }
        OpResult::Invited(Community {
            id: format!("concord-{}", b.community_id),
            name: b.name.clone(),
            kind: CommunityKind::Concord,
            unread: 0,
            relay: if primary.is_empty() {
                None
            } else {
                Some(primary)
            },
            relays: b.relays.clone(),
            voice: false,
            channels,
        })
    }
}
