use armada_tui::app::{App, Screen};
use armada_tui::{app, kitty, ui};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "armada-tui",
    about = "Cliente terminal (Ratatui) inspirado no Armada da Soapbox"
)]
struct Cli {
    /// Relay Nostr inicial (sobrescreve o primeiro app relay no status)
    #[arg(long)]
    relay: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let mut app = App::new();
    if let Some(r) = cli.relay {
        if !app.relays.app_relays.is_empty() {
            app.relays.app_relays[0] = r.clone();
        }
        app.status = format!("relay inicial: {r} (mock — sem conexão real ainda)");
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("erro: {e:?}");
    }
    Ok(())
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| ui::render(f, app))?;
        if app.should_quit {
            break;
        }
        // Operações de rede pendentes rodam após o draw (mostra "buscando…" antes).
        if let Some(op) = app.pending.take() {
            match op {
                app::PendingOp::Groups => app.fetch_live_groups(),
                app::PendingOp::Messages => app.fetch_live_messages(),
                app::PendingOp::Send => app.do_send(),
                app::PendingOp::Join => app.do_join(),
                app::PendingOp::Invite => app.do_invite(),
            }
            continue;
        }
        // Viewer de imagem: sai do alt-screen, renderiza via kitty, volta.
        if let Some(url) = app.view_url.take() {
            suspend_and_view(app, terminal, &url);
            continue;
        }
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            handle_key(app, key.code, key.modifiers);
        }
    }
    Ok(())
}

/// Sai do TUI, exibe PNG via Kitty graphics (ou a URL como fallback),
/// espera Enter e volta.
fn suspend_and_view(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    url: &str,
) {
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    println!("armada-tui · imagem: {url}");
    if !kitty::supported() {
        println!(
            "(terminal sem Kitty graphics: TERM={:?}, sem KITTY_WINDOW_ID)",
            std::env::var("TERM")
        );
        println!("abra a URL no navegador. Dica: rode dentro do kitty/wezterm/ghostty.");
    } else {
        match kitty::fetch_png(url) {
            Ok(bytes) => {
                println!("({} bytes, PNG — Enter volta)", bytes.len());
                if let Err(e) = kitty::display_png(&bytes, 80) {
                    println!("falha kitty: {e:#}");
                }
            }
            Err(e) => println!("não deu p/ exibir: {e:#}\nURL: {url}"),
        }
    }
    println!("\n[Enter] volta pra TUI");
    let _ = io::stdin().read_line(&mut String::new());
    let _ = enable_raw_mode();
    let _ = execute!(terminal.backend_mut(), EnterAlternateScreen);
    let _ = terminal.clear();
    app.status = format!("imagem vista: {url}");
}

fn handle_key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    // Ctrl+C / Ctrl+Q sempre sai
    if mods.contains(KeyModifiers::CONTROL)
        && matches!(code, KeyCode::Char('c') | KeyCode::Char('q'))
    {
        app.should_quit = true;
        return;
    }
    // Ctrl+K = quick switcher do Electron → vai pro Inbox no MVP
    if mods.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('k')) {
        app.goto(Screen::Inbox);
        return;
    }

    // Modo de digitação (login, mensagem)
    if app.screen == Screen::Welcome && !app.authed {
        match code {
            KeyCode::Char('g') if app.login_input.is_empty() => {
                app.login_generated();
                return;
            }
            KeyCode::Char(c) => app.login_input.push(c),
            KeyCode::Backspace => {
                app.login_input.pop();
            }
            KeyCode::Enter => app.login(),
            KeyCode::Esc => app.should_quit = true,
            _ => {}
        }
        return;
    }

    // Entrada do link de invite (tela Server, tecla I).
    if app.invite_mode {
        match code {
            KeyCode::Char(c) => app.invite_input.push(c),
            KeyCode::Backspace => {
                app.invite_input.pop();
            }
            KeyCode::Enter => {
                app.status = "abrindo invite…".to_string();
                app.pending = Some(app::PendingOp::Invite);
            }
            KeyCode::Esc => {
                app.invite_mode = false;
                app.invite_input.clear();
            }
            _ => {}
        }
        return;
    }

    if app.input_mode {
        match code {
            KeyCode::Char(c) => app.input.push(c),
            KeyCode::Backspace => {
                app.input.pop();
            }
            KeyCode::Enter => app.send_current_input(),
            KeyCode::Esc => {
                app.input_mode = false;
                app.input.clear();
            }
            _ => {}
        }
        return;
    }

    match code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('?') => app.goto(Screen::Help),
        KeyCode::Char('1') => app.goto(Screen::Welcome),
        KeyCode::Char('2') => app.goto(Screen::Server),
        KeyCode::Char('3') => app.goto(Screen::Dms),
        KeyCode::Char('4') => app.goto(Screen::Discover),
        KeyCode::Char('5') => app.goto(Screen::Projects),
        KeyCode::Char('6') => app.goto(Screen::Inbox),
        KeyCode::Char('7') => app.goto(Screen::Settings),
        KeyCode::Char('8') => app.goto(Screen::Parity),
        KeyCode::Tab => app.cycle_focus(),
        KeyCode::Up | KeyCode::Char('k') => app.move_up(),
        KeyCode::Down | KeyCode::Char('j') => app.move_down(),
        KeyCode::Char('i') => {
            if matches!(app.screen, Screen::Server | Screen::Dms) {
                app.input_mode = true;
            }
        }
        KeyCode::Enter => {
            if matches!(app.screen, Screen::Server | Screen::Dms) {
                app.input_mode = true;
            }
        }
        KeyCode::Esc => {
            if app.screen == Screen::Help && app.authed {
                app.goto(Screen::Server);
            }
        }
        KeyCode::Char('o') => {
            if app.screen == Screen::Settings {
                app.logout();
            }
        }
        // Fase 1 — live NIP-29 + viewer kitty
        KeyCode::Char('r') => {
            if matches!(app.screen, Screen::Server | Screen::Discover) {
                app.status = "buscando grupos NIP-29…".to_string();
                app.pending = Some(app::PendingOp::Groups);
            }
        }
        KeyCode::Char('m') => {
            if app.screen == Screen::Server {
                app.status = "buscando mensagens…".to_string();
                app.pending = Some(app::PendingOp::Messages);
            }
        }
        KeyCode::Char('v') => {
            if matches!(app.screen, Screen::Server | Screen::Dms) {
                match app.first_image_url() {
                    Some(url) => app.view_url = Some(url),
                    None => app.status = "nenhuma URL nas mensagens visíveis".to_string(),
                }
            }
        }
        KeyCode::Char('J') => {
            if app.screen == Screen::Server {
                app.status = "enviando join 9021…".to_string();
                app.pending = Some(app::PendingOp::Join);
            }
        }
        KeyCode::Char('I') => {
            if app.screen == Screen::Server {
                app.invite_mode = true;
                app.invite_input.clear();
            }
        }
        _ => {}
    }
}
