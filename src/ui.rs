use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Gauge, List, ListItem, ListState, Paragraph, Row, Table, Tabs, Wrap,
    },
    Frame,
};

use crate::app::{App, Focus, Screen};
use crate::parity;

fn screen_index(s: Screen) -> usize {
    Screen::all().iter().position(|x| *x == s).unwrap_or(0)
}

pub fn render(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(f.area());

    let titles: Vec<Line> = Screen::all()
        .iter()
        .map(|s| Line::from(s.title()))
        .collect();
    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" armada-tui — terminal p/ Armada (Soapbox) "),
        )
        .select(screen_index(app.screen))
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, chunks[0]);

    match app.screen {
        Screen::Welcome => render_welcome(f, app, chunks[1]),
        Screen::Server => render_server(f, app, chunks[1]),
        Screen::Dms => render_dms(f, app, chunks[1]),
        Screen::Discover => render_discover(f, app, chunks[1]),
        Screen::Projects => render_projects(f, app, chunks[1]),
        Screen::Inbox => render_inbox(f, app, chunks[1]),
        Screen::Settings => render_settings(f, app, chunks[1]),
        Screen::Parity => render_parity(f, chunks[1]),
        Screen::Help => render_help(f, app, chunks[1]),
    }

    let status = format!(
        " {} | {} | {} | q sair · 1-8 telas · r grupos · m msgs · v imagem · ? ajuda ",
        app.npub,
        app.status,
        voice_note()
    );
    let bar = Paragraph::new(status)
        .block(Block::default().borders(Borders::ALL).title(" status "))
        .wrap(Wrap { trim: true });
    f.render_widget(bar, chunks[2]);
}

fn voice_note() -> &'static str {
    "voz: presença no TUI, áudio só no app gráfico"
}

fn render_welcome(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Discord sem empresa. Suas chaves, sua frota.",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("Comunidades E2EE serverless (Concord) + grupos NIP-29 via relays Nostr."),
        Line::from(""),
        Line::from("Login (nsec1/hex = escrita · g com campo vazio = conta de brincadeira):"),
        Line::from(format!("> {}", app.login_input)),
        Line::from(""),
        Line::from(match &app.login_error {
            Some(e) => format!("(!) {e}"),
            None => "Enter = entrar · qualquer nsec vale no mock".to_string(),
        }),
        Line::from(""),
        Line::from("Defaults do Electron espelhados em Settings (relays, Blossom, broker de voz)."),
    ];
    let p = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" boas-vindas / auth "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn pane_title(base: &str, focused: bool) -> String {
    if focused {
        format!(" {base} ● ")
    } else {
        format!(" {base} ")
    }
}

fn render_server(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(50),
        ])
        .split(area);

    // Sidebar: comunidades
    let items: Vec<ListItem> = app
        .communities
        .iter()
        .map(|c| {
            let unread = if c.unread > 0 {
                format!(" ({})", c.unread)
            } else {
                String::new()
            };
            let live = if c.relay.is_some() { "⚡" } else { "" };
            ListItem::new(format!("{live}{}{} [{}]", c.name, unread, c.kind.label()))
        })
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(pane_title(
            "frota / comunidades",
            app.focus == Focus::Sidebar,
        )))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    let mut st = ListState::default();
    st.select(Some(app.sel_community));
    f.render_stateful_widget(list, cols[0], &mut st);

    // Canais
    let ch_items: Vec<ListItem> = app
        .current_community()
        .map(|c| {
            c.channels
                .iter()
                .map(|ch| {
                    let icon = if ch.is_voice { "🔊 " } else { "# " };
                    ListItem::new(format!("{icon}{}", ch.name))
                })
                .collect()
        })
        .unwrap_or_default();
    let ch_list = List::new(ch_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(pane_title("canais", app.focus == Focus::Channels)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    let mut cst = ListState::default();
    cst.select(Some(app.sel_channel));
    f.render_stateful_widget(ch_list, cols[1], &mut cst);

    // Mensagens + input
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(cols[2]);

    let msgs = app.current_messages();
    let topic = app
        .current_channel()
        .map(|c| c.topic.clone())
        .unwrap_or_default();
    let lines: Vec<Line> = msgs
        .iter()
        .rev()
        .take(50)
        .rev()
        .map(|m| {
            let who = if m.mine {
                Span::styled(
                    format!("{} [{}] ", m.author, m.time),
                    Style::default().fg(Color::Green),
                )
            } else {
                Span::styled(
                    format!("{} [{}] ", m.author, m.time),
                    Style::default().fg(Color::Cyan),
                )
            };
            Line::from(vec![who, Span::raw(m.content.clone())])
        })
        .collect();
    let title = format!(
        " {} — {} ",
        app.current_channel()
            .map(|c| c.name.clone())
            .unwrap_or_default(),
        topic
    );
    let mp = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false });
    f.render_widget(mp, right[0]);

    let prompt = if app.invite_mode {
        format!("convite: {}▌  (Enter abre · Esc cancela)", app.invite_input)
    } else if app.input_mode {
        format!("> {}▌", app.input)
    } else {
        "i digitar · I invite · r grupos · m msgs/E2EE · v imagem · J join".to_string()
    };
    let inp = Paragraph::new(prompt).block(Block::default().borders(Borders::ALL).title(
        pane_title("mensagem", app.input_mode || app.focus == Focus::Messages),
    ));
    f.render_widget(inp, right[1]);
}

fn render_dms(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);
    let items: Vec<ListItem> = app
        .dms
        .iter()
        .map(|d| ListItem::new(format!("{} — {}", d.peer, d.preview)))
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" DMs E2EE "))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    let mut st = ListState::default();
    st.select(Some(app.sel_dm));
    f.render_stateful_widget(list, cols[0], &mut st);

    let lines: Vec<Line> = app
        .current_messages()
        .iter()
        .map(|m| Line::from(format!("[{}] {}: {}", m.time, m.author, m.content)))
        .collect();
    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" conversa "))
        .wrap(Wrap { trim: false });
    f.render_widget(p, cols[1]);
}

fn render_discover(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let items: Vec<ListItem> = app
        .discover
        .iter()
        .map(|(n, d)| ListItem::new(format!("{n} — {d}")))
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" descobrir (espelha DiscoverPage) "),
    );
    f.render_widget(list, area);
}

fn render_projects(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let items: Vec<ListItem> = app
        .projects
        .iter()
        .map(|p| {
            ListItem::new(format!(
                "[{}] {} ({})",
                p.status,
                p.title,
                p.labels.join(",")
            ))
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" projetos (workspace do changelog v0.40) "),
    );
    f.render_widget(list, area);
}

fn render_inbox(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let text = vec![
        Line::from("Inbox / outbox (PublishOutbox, SyncGate):"),
        Line::from(format!("- status: {}", app.status)),
        Line::from("- fila mock: 0 pendentes"),
        Line::from("- Ctrl+K no Electron = QuickSwitcher → aqui use 1-7"),
    ];
    f.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" inbox ")),
        area,
    );
}

fn render_settings(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let mut lines = vec![
        Line::from(Span::styled(
            "Relays e mídia (editável no Electron; aqui só leitura no MVP):",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("app relays: {}", app.relays.app_relays.join(", "))),
        Line::from(format!(
            "search relays: {}",
            app.relays.search_relays.join(", ")
        )),
        Line::from(format!(
            "blossom: {}",
            app.relays.blossom_servers.join(", ")
        )),
        Line::from(format!(
            "voice brokers (CORD-07): {}",
            app.relays.voice_brokers.join(", ")
        )),
        Line::from(""),
        Line::from("o = logout · voz real (LiveKit WebRTC) fora do escopo do terminal"),
    ];
    let _ = &mut lines;
    f.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" settings "))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_parity(f: &mut Frame, area: ratatui::layout::Rect) {
    let fs = parity::load();
    let s = parity::summarize(&fs);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    let g = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(format!(
            " paridade {}% · done {} · partial {} · missing {} · fora-escopo {} ",
            s.percent(),
            s.done,
            s.partial,
            s.missing,
            s.out_of_scope
        )))
        .gauge_style(Style::default().fg(Color::Green))
        .percent(s.percent());
    f.render_widget(g, chunks[0]);

    let rows: Vec<Row> = fs
        .iter()
        .map(|feat| {
            let (st, col) = match feat.status.as_str() {
                "done" => ("done", Color::Green),
                "partial" => ("partial", Color::Yellow),
                "missing" => ("missing", Color::Red),
                _ => ("fora", Color::Gray),
            };
            Row::new(vec![
                Cell::from(feat.id.clone()),
                Cell::from(feat.area.clone()),
                Cell::from(Span::styled(
                    st,
                    Style::default().fg(col).add_modifier(Modifier::BOLD),
                )),
                Cell::from(feat.evidence.clone()),
                Cell::from(feat.tui.clone()),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(22),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Min(0),
        ],
    )
    .header(
        Row::new(vec!["feature", "área", "status", "evidência", "tui faz"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" paridade x Electron (parity.json) "),
    );
    f.render_widget(table, chunks[1]);
}

fn render_help(f: &mut Frame, _app: &mut App, area: ratatui::layout::Rect) {
    let text = vec![
        Line::from("q sair · 1-8 trocar tela · ? ajuda"),
        Line::from("Tab alterna foco (frota → canais → mensagens)"),
        Line::from("j/k ou ↑/↓ navegar · i digitar · Enter enviar · Esc cancelar"),
        Line::from("r grupos NIP-29 · m msgs (Concord com chave = descriptografa E2EE)"),
        Line::from("I abre invite …/invite/<naddr>#… · v imagem (kitty) · J join (9021)"),
        Line::from("Enter em canal ⚡ com nsec publica de verdade (NIP-42 se pedido)"),
        Line::from("o logout (na tela Settings) · ⚡ = comunidade live"),
        Line::from(""),
        Line::from("Paridade com Electron (roadmap):"),
        Line::from(
            "- feito: layout 3 painéis, DMs, Discover, Projects, Inbox, Settings, auth mock",
        ),
        Line::from("- fase 1: leitura NIP-29 real (39000 + kinds c/ tag h) · viewer PNG via kitty"),
        Line::from(
            "- falta: NIP-42 auth p/ grupos privados, escrita, E2EE Concord, LiveKit voz, zaps",
        ),
    ];
    f.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" ajuda ")),
        area,
    );
}
