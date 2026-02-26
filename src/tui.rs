use anyhow::{Context, Result};
use crossterm::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{prelude::*, widgets::*};
use sqlx::SqlitePool;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::db::{self, RequestStats};
use crate::log_capture::LogBuffer;

// ── Public config ─────────────────────────────────────────────────────────────

pub struct TuiConfig {
    pub host: String,
    pub port: u16,
    pub db: SqlitePool,
    pub env_path: PathBuf,
    pub www_root: PathBuf,
    pub base_url: Option<String>,
    pub log_buffer: LogBuffer,
}

// ── Screens / state machine ───────────────────────────────────────────────────

const MENU_ITEMS: &[&str] = &[
    "Add Admin User",
    "Setup Content Directory",
    "Set Domain",
    "Restart Server",
    "Stop Server",
];

const TICK_MS: u64 = 2000;
const MSG_SECS: u64 = 4;

#[derive(Default)]
enum Screen {
    #[default]
    Menu,
    AddUser(AddUserState),
    ContentDir {
        selected: usize,
    },
    ContentDirPath {
        input: String,
    },
    SetDomain {
        input: String,
    },
}

struct AddUserState {
    username: String,
    password: String,
    step: AddUserStep,
    error: Option<String>,
}

impl Default for AddUserState {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            step: AddUserStep::Username,
            error: None,
        }
    }
}

#[derive(PartialEq)]
enum AddUserStep {
    Username,
    Password,
}

struct App {
    menu_idx: usize,
    screen: Screen,
    stats: RequestStats,
    server_addr: String,
    www_root: PathBuf,
    base_url: Option<String>,
    db: SqlitePool,
    env_path: PathBuf,
    message: Option<(String, bool, Instant)>, // (text, is_error, when)
    restart_pending: bool,
    stop_pending: bool,
    log_buffer: LogBuffer,
    log_scroll: u16, // lines scrolled up from tail (0 = follow tail)
}

impl App {
    fn set_msg(&mut self, text: impl Into<String>, is_error: bool) {
        self.message = Some((text.into(), is_error, Instant::now()));
    }

    fn clear_expired_msg(&mut self) {
        if let Some((_, _, t)) = self.message {
            if t.elapsed() > Duration::from_secs(MSG_SECS) {
                self.message = None;
            }
        }
    }
}

// ── Actions returned by key handlers ─────────────────────────────────────────

enum Action {
    None,
    CreateUser { username: String, password: String },
    CreateWwwDir,
    SetWwwRoot { path: String },
    SetDomain { domain: String },
    Restart,
    Stop,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn run(config: TuiConfig) -> Result<()> {
    let server_addr = format!("{}:{}", config.host, config.port);

    // Build initial AppState and start the HTTP server task.
    let initial_state = crate::build_state(
        config.www_root.clone(),
        config.base_url.clone(),
        config.db.clone(),
    )
    .await?;

    let host = config.host.clone();
    let port = config.port;
    let mut server_handle = {
        let state = initial_state.clone();
        let h = host.clone();
        tokio::spawn(async move { crate::run_http_server(h, port, state).await })
    };

    // Setup terminal.
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Spawn a dedicated thread for blocking crossterm event reads.
    let (event_tx, mut event_rx) =
        tokio::sync::mpsc::unbounded_channel::<crossterm::event::Event>();
    std::thread::spawn(move || {
        loop {
            match crossterm::event::poll(Duration::from_millis(50)) {
                Ok(true) => match crossterm::event::read() {
                    Ok(evt) => {
                        if event_tx.send(evt).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                Ok(false) => {}
                Err(_) => break,
            }
        }
    });

    let mut app = App {
        menu_idx: 0,
        screen: Screen::Menu,
        stats: RequestStats::default(),
        server_addr,
        www_root: config.www_root,
        base_url: config.base_url,
        db: config.db,
        env_path: config.env_path,
        message: None,
        restart_pending: false,
        stop_pending: false,
        log_buffer: config.log_buffer,
        log_scroll: 0,
    };

    let mut tick = tokio::time::interval(Duration::from_millis(TICK_MS));

    let result: Result<()> = 'main: loop {
        // Execute any pending server operations before rendering.
        if app.restart_pending {
            app.restart_pending = false;
            server_handle.abort();
            // Wait for the old task to fully stop so the OS releases the port.
            let _ = (&mut server_handle).await;
            match crate::build_state(app.www_root.clone(), app.base_url.clone(), app.db.clone())
                .await
            {
                Ok(new_state) => {
                    let h = host.clone();
                    let p = port;
                    server_handle =
                        tokio::spawn(async move { crate::run_http_server(h, p, new_state).await });
                    app.set_msg("Server restarted.", false);
                }
                Err(e) => {
                    app.set_msg(format!("Restart failed: {e}"), true);
                }
            }
        }

        if app.stop_pending {
            server_handle.abort();
            break 'main Ok(());
        }

        if let Err(e) = terminal.draw(|f| render(f, &app)) {
            break 'main Err(e.into());
        }

        tokio::select! {
            Some(event) = event_rx.recv() => {
                if let Event::Key(key) = event {
                    if key.kind != KeyEventKind::Press {
                        continue 'main;
                    }
                    // Ctrl+C / Ctrl+Q → stop
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q'))
                    {
                        server_handle.abort();
                        break 'main Ok(());
                    }

                    let action = handle_key(&mut app, key.code);
                    execute_action(&mut app, action).await;
                }
            }
            _ = tick.tick() => {
                if let Ok(stats) = db::get_request_stats(&app.db).await {
                    app.stats = stats;
                }
                app.clear_expired_msg();
            }
        }
    };

    // Always restore the terminal.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
}

// ── Key handling ─────────────────────────────────────────────────────────────

fn handle_key(app: &mut App, key: KeyCode) -> Action {
    match &mut app.screen {
        Screen::Menu => handle_menu_key(app, key),
        Screen::AddUser(_) => handle_add_user_key(app, key),
        Screen::ContentDir { .. } => handle_content_dir_key(app, key),
        Screen::ContentDirPath { .. } => handle_content_dir_path_key(app, key),
        Screen::SetDomain { .. } => handle_set_domain_key(app, key),
    }
}

fn handle_menu_key(app: &mut App, key: KeyCode) -> Action {
    match key {
        KeyCode::Up | KeyCode::Char('k') => {
            if app.menu_idx > 0 {
                app.menu_idx -= 1;
            }
            Action::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.menu_idx + 1 < MENU_ITEMS.len() {
                app.menu_idx += 1;
            }
            Action::None
        }
        KeyCode::PageUp => {
            app.log_scroll = app
                .log_scroll
                .saturating_add(10)
                .min(crate::log_capture::MAX_LOG_LINES as u16);
            Action::None
        }
        KeyCode::PageDown => {
            app.log_scroll = app.log_scroll.saturating_sub(10);
            Action::None
        }
        KeyCode::Enter => activate_menu_item(app),
        KeyCode::Char(c @ '1'..='9') => {
            let idx = (c as usize) - ('1' as usize);
            if idx < MENU_ITEMS.len() {
                app.menu_idx = idx;
                activate_menu_item(app)
            } else {
                Action::None
            }
        }
        _ => Action::None,
    }
}

fn activate_menu_item(app: &mut App) -> Action {
    match app.menu_idx {
        0 => {
            app.screen = Screen::AddUser(AddUserState::default());
            Action::None
        }
        1 => {
            app.screen = Screen::ContentDir { selected: 0 };
            Action::None
        }
        2 => {
            let current = app.base_url.clone().unwrap_or_default();
            app.screen = Screen::SetDomain { input: current };
            Action::None
        }
        3 => Action::Restart,
        4 => Action::Stop,
        _ => Action::None,
    }
}

fn handle_add_user_key(app: &mut App, key: KeyCode) -> Action {
    let Screen::AddUser(ref mut state) = app.screen else {
        return Action::None;
    };

    match key {
        KeyCode::Esc => {
            app.screen = Screen::Menu;
            Action::None
        }
        KeyCode::Backspace => {
            match state.step {
                AddUserStep::Username => {
                    state.username.pop();
                }
                AddUserStep::Password => {
                    state.password.pop();
                }
            }
            state.error = None;
            Action::None
        }
        KeyCode::Char(c) => {
            match state.step {
                AddUserStep::Username => state.username.push(c),
                AddUserStep::Password => state.password.push(c),
            }
            state.error = None;
            Action::None
        }
        KeyCode::Enter => match state.step {
            AddUserStep::Username => {
                if state.username.is_empty() {
                    state.error = Some("Username cannot be empty.".into());
                    return Action::None;
                }
                state.step = AddUserStep::Password;
                Action::None
            }
            AddUserStep::Password => {
                if state.password.is_empty() {
                    state.error = Some("Password cannot be empty.".into());
                    return Action::None;
                }
                let username = state.username.clone();
                let password = state.password.clone();
                app.screen = Screen::Menu;
                Action::CreateUser { username, password }
            }
        },
        _ => Action::None,
    }
}

fn handle_content_dir_key(app: &mut App, key: KeyCode) -> Action {
    let Screen::ContentDir { ref mut selected } = app.screen else {
        return Action::None;
    };

    match key {
        KeyCode::Esc => {
            app.screen = Screen::Menu;
            Action::None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if *selected > 0 {
                *selected -= 1;
            }
            Action::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if *selected < 1 {
                *selected += 1;
            }
            Action::None
        }
        KeyCode::Char('1') => {
            app.screen = Screen::Menu;
            Action::CreateWwwDir
        }
        KeyCode::Char('2') => {
            app.screen = Screen::ContentDirPath {
                input: String::new(),
            };
            Action::None
        }
        KeyCode::Enter => {
            let sel = *selected;
            app.screen = Screen::Menu;
            if sel == 0 {
                Action::CreateWwwDir
            } else {
                app.screen = Screen::ContentDirPath {
                    input: String::new(),
                };
                Action::None
            }
        }
        _ => Action::None,
    }
}

fn handle_content_dir_path_key(app: &mut App, key: KeyCode) -> Action {
    let Screen::ContentDirPath { ref mut input } = app.screen else {
        return Action::None;
    };

    match key {
        KeyCode::Esc => {
            app.screen = Screen::Menu;
            Action::None
        }
        KeyCode::Backspace => {
            input.pop();
            Action::None
        }
        KeyCode::Char(c) => {
            input.push(c);
            Action::None
        }
        KeyCode::Enter => {
            let path = input.clone();
            app.screen = Screen::Menu;
            Action::SetWwwRoot { path }
        }
        _ => Action::None,
    }
}

fn handle_set_domain_key(app: &mut App, key: KeyCode) -> Action {
    let Screen::SetDomain { ref mut input } = app.screen else {
        return Action::None;
    };

    match key {
        KeyCode::Esc => {
            app.screen = Screen::Menu;
            Action::None
        }
        KeyCode::Backspace => {
            input.pop();
            Action::None
        }
        KeyCode::Char(c) => {
            input.push(c);
            Action::None
        }
        KeyCode::Enter => {
            let domain = input.clone();
            app.screen = Screen::Menu;
            Action::SetDomain { domain }
        }
        _ => Action::None,
    }
}

// ── Async action execution ────────────────────────────────────────────────────

async fn execute_action(app: &mut App, action: Action) {
    match action {
        Action::None => {}

        Action::CreateUser { username, password } => {
            match db::add_user(&app.db, &username, &password).await {
                Ok(()) => app.set_msg(format!("User '{username}' created."), false),
                Err(e) => app.set_msg(format!("Error: {e}"), true),
            }
        }

        Action::CreateWwwDir => {
            // Create www/ adjacent to the .env file (i.e. next to the binary).
            let www = app
                .env_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("www");
            match std::fs::create_dir_all(&www) {
                Err(e) => app.set_msg(format!("Could not create www/: {e}"), true),
                Ok(()) => {
                    let path_str = www.to_string_lossy().to_string();
                    if let Err(e) = update_env_var(&app.env_path, "WWW_ROOT", &path_str) {
                        app.set_msg(format!("Could not write .env: {e}"), true);
                        return;
                    }
                    app.www_root = www;
                    app.restart_pending = true;
                    app.set_msg(format!("Created {path_str}, restarting…"), false);
                }
            }
        }

        Action::SetWwwRoot { path } => {
            let p = PathBuf::from(&path);
            if !p.exists() {
                app.set_msg(format!("Path does not exist: {path}"), true);
                return;
            }
            if let Err(e) = update_env_var(&app.env_path, "WWW_ROOT", &path) {
                app.set_msg(format!("Could not write .env: {e}"), true);
                return;
            }
            app.www_root = p;
            app.restart_pending = true;
            app.set_msg("Content directory updated, restarting…", false);
        }

        Action::SetDomain { domain } => {
            let domain = domain.trim().to_string();
            if domain.is_empty() {
                app.set_msg("Domain cannot be empty.", true);
                return;
            }
            if let Err(e) = update_env_var(&app.env_path, "BASE_URL", &domain) {
                app.set_msg(format!("Could not write .env: {e}"), true);
                return;
            }
            app.base_url = Some(domain.clone());
            app.restart_pending = true;
            app.set_msg(format!("Domain set to {domain}, restarting…"), false);
        }

        Action::Restart => {
            app.restart_pending = true;
            app.set_msg("Restarting…", false);
        }

        Action::Stop => {
            app.stop_pending = true;
        }
    }
}

// ── .env helpers ──────────────────────────────────────────────────────────────

/// Update or add a key=value line in the .env file. Returns an error on I/O failure.
///
/// Writes atomically via a sibling temp file + rename to avoid truncating the
/// .env file if the process is killed mid-write.
fn update_env_var(env_path: &PathBuf, key: &str, value: &str) -> std::io::Result<()> {
    let content = std::fs::read_to_string(env_path).unwrap_or_default();
    let prefix = format!("{key}=");
    let new_line = format!("{key}={value}");

    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    match lines.iter().position(|l| l.starts_with(&prefix)) {
        Some(i) => lines[i] = new_line,
        None => lines.push(new_line),
    }

    let tmp_path = env_path.with_extension("env.tmp");
    std::fs::write(&tmp_path, lines.join("\n") + "\n")?;
    std::fs::rename(&tmp_path, env_path)
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Outer layout: title (3) | body (fill) | status bar (3)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);

    render_title(frame, outer[0]);
    render_body(frame, outer[1], app);
    render_status_bar(frame, outer[2], app);
}

fn render_title(frame: &mut Frame, area: Rect) {
    let title = Paragraph::new("mdServer")
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL))
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(title, area);
}

fn render_body(frame: &mut Frame, area: Rect, app: &App) {
    match &app.screen {
        Screen::Menu => render_menu(frame, area, app),
        Screen::AddUser(state) => render_add_user(frame, area, state, app),
        Screen::ContentDir { selected } => render_content_dir(frame, area, *selected, app),
        Screen::ContentDirPath { input } => render_text_input(
            frame,
            area,
            "Setup Content Directory",
            "Directory path:",
            input,
            app,
        ),
        Screen::SetDomain { input } => render_text_input(
            frame,
            area,
            "Set Domain",
            "Domain URL (e.g. https://example.com):",
            input,
            app,
        ),
    }
}

fn render_menu(frame: &mut Frame, area: Rect, app: &App) {
    // Split body: left (menu) | right (info + logs)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(1), Constraint::Fill(2)])
        .split(area);

    // ── Left: menu list ───────────────────────────────────────────────────────
    let items: Vec<ListItem> = MENU_ITEMS
        .iter()
        .enumerate()
        .map(|(i, &label)| {
            let prefix = format!("{}. ", i + 1);
            ListItem::new(format!(" {prefix}{label}"))
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(app.menu_idx));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Server Management "),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, cols[0], &mut list_state);

    // ── Right: split vertically — Info (25%) on top, Logs (75%) on bottom ────
    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(cols[1]);

    // Info panel (top 25%)
    let info_text = if let Some((msg, is_err, _)) = &app.message {
        let style = if *is_err {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };
        vec![
            Line::from(""),
            Line::from(Span::styled(msg.as_str(), style)),
        ]
    } else {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "↑↓ or j/k  navigate",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "1–5        shortcut",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "Enter      select",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "PgUp/PgDn  scroll logs",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "Ctrl+C     quit",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!(" www: {}", app.www_root.display()),
                Style::default().fg(Color::DarkGray),
            )),
        ]
    };

    let info =
        Paragraph::new(info_text).block(Block::default().borders(Borders::ALL).title(" Info "));
    frame.render_widget(info, right_rows[0]);

    // Logs panel (bottom 75%)
    render_logs_panel(frame, right_rows[1], app);
}

fn render_logs_panel(frame: &mut Frame, area: Rect, app: &App) {
    // Snapshot the ring buffer under a brief lock.
    let entries: Vec<crate::log_capture::LogEntry> = app
        .log_buffer
        .lock()
        .map(|buf| buf.iter().cloned().collect())
        .unwrap_or_default();

    let inner_height = area.height.saturating_sub(2) as usize;
    let total = entries.len();

    // log_scroll counts lines scrolled *up* from the tail.
    // 0 = follow tail. Clamped so you cannot scroll above the first entry.
    let max_scroll = total.saturating_sub(inner_height) as u16;
    let scroll_up = app.log_scroll.min(max_scroll);

    // Ratatui Paragraph::scroll((row, col)) is top-anchored.
    // display_row = max_scroll - scroll_up maps our tail-anchored offset to it.
    let display_row = max_scroll.saturating_sub(scroll_up);

    let lines: Vec<Line> = entries
        .iter()
        .flat_map(|entry| {
            vec![
                Line::from(Span::styled(
                    entry.header.clone(),
                    log_level_style(entry.level).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    entry.formatted.clone(),
                    log_level_style(entry.level),
                )),
                Line::from(""), // gap between entries
            ]
        })
        .collect();

    let title = if scroll_up > 0 {
        format!(" Logs  ↑{scroll_up}  PgDn to follow ")
    } else {
        " Logs  [tail]  PgUp to scroll ".to_string()
    };

    let logs = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .scroll((display_row, 0));

    frame.render_widget(logs, area);
}

fn log_level_style(level: tracing::Level) -> Style {
    match level {
        tracing::Level::ERROR => Style::default().fg(Color::Red),
        tracing::Level::WARN => Style::default().fg(Color::Yellow),
        tracing::Level::INFO => Style::default().fg(Color::White),
        tracing::Level::DEBUG => Style::default().fg(Color::DarkGray),
        tracing::Level::TRACE => Style::default().fg(Color::DarkGray),
    }
}

fn render_add_user(frame: &mut Frame, area: Rect, state: &AddUserState, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Add Admin User ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // spacer
            Constraint::Length(3), // username
            Constraint::Length(3), // password
            Constraint::Length(1), // spacer
            Constraint::Length(1), // hints
            Constraint::Length(1), // error
            Constraint::Min(0),
        ])
        .split(inner);

    // Username field
    let u_style = if state.step == AddUserStep::Username {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let u_field = Paragraph::new(format!(" {}", state.username)).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Username")
            .style(u_style),
    );
    frame.render_widget(u_field, rows[1]);

    // Password field (masked)
    let p_style = if state.step == AddUserStep::Password {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let masked: String = "*".repeat(state.password.len());
    let p_field = Paragraph::new(format!(" {masked}")).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Password")
            .style(p_style),
    );
    frame.render_widget(p_field, rows[2]);

    let hint = Paragraph::new(Span::styled(
        " Enter to advance/confirm   Esc to cancel",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(hint, rows[4]);

    if let Some(err) = &state.error {
        let err_p = Paragraph::new(Span::styled(err.as_str(), Style::default().fg(Color::Red)));
        frame.render_widget(err_p, rows[5]);
    } else if let Some((msg, is_err, _)) = &app.message {
        let style = if *is_err {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };
        let msg_p = Paragraph::new(Span::styled(msg.as_str(), style));
        frame.render_widget(msg_p, rows[5]);
    }
}

fn render_content_dir(frame: &mut Frame, area: Rect, selected: usize, _app: &App) {
    let items = vec![
        ListItem::new(" 1. Create www/ directory adjacent to server"),
        ListItem::new(" 2. Choose a folder (enter path)"),
    ];

    let mut list_state = ListState::default();
    list_state.select(Some(selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Setup Content Directory "),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    let cols = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .split(area);

    frame.render_stateful_widget(list, cols[0], &mut list_state);

    let hint = Paragraph::new(Span::styled(
        " Enter to select   Esc to go back",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(hint, cols[1]);
}

fn render_text_input(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    label: &str,
    input: &str,
    app: &App,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    let label_p = Paragraph::new(format!(" {label}"));
    frame.render_widget(label_p, rows[1]);

    let field = Paragraph::new(format!(" {input}")).block(
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::Yellow)),
    );
    frame.render_widget(field, rows[2]);

    let hint = Paragraph::new(Span::styled(
        " Enter to confirm   Esc to cancel",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(hint, rows[4]);

    if let Some((msg, is_err, _)) = &app.message {
        let style = if *is_err {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };
        let msg_p = Paragraph::new(Span::styled(msg.as_str(), style));
        frame.render_widget(msg_p, rows[5]);
    }
}

fn render_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let text = format!(
        " {}  │  7m: {}  │  1h: {}  │  24h: {}",
        app.server_addr, app.stats.last_7m, app.stats.last_1h, app.stats.last_24h
    );
    let bar = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(bar, area);
}
