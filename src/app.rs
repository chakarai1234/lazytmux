use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::backend::Backend;
use ratatui::Terminal;

use crate::tmux::{Pane, Session, SplitDirection, TmuxClient, TmuxState, TmuxTarget, Window};
use crate::ui;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TreeItemKind {
    Session,
    Window,
    Pane,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeItem {
    pub kind: TreeItemKind,
    pub depth: usize,
    pub name: String,
    pub label: String,
    pub subtitle: String,
    pub target: TmuxTarget,
    pub rename_value: String,
    pub details: Vec<(String, String)>,
    pub preview: Option<TerminalPreview>,
    pub active: bool,
    pub dead: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalPreview {
    pub title: String,
    pub panes: Vec<TerminalPanePreview>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalPanePreview {
    pub title: String,
    pub content: String,
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
    pub active: bool,
    pub dead: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prompt {
    pub title: String,
    pub value: String,
    pub hint: String,
    kind: PromptKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Confirm {
    pub message: String,
    target: KillTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PromptKind {
    Filter,
    Rename(RenameTarget),
    NewSession,
    NewWindow { session_id: String },
    Command,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RenameTarget {
    Session { id: String },
    Window { id: String },
    Pane { id: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum KillTarget {
    Session { id: String },
    Window { id: String },
    Pane { id: String },
}

#[derive(Debug)]
pub struct App {
    client: TmuxClient,
    state: TmuxState,
    items: Vec<TreeItem>,
    selected: usize,
    detail_scroll: u16,
    help_scroll: u16,
    collapsed_sessions: HashSet<String>,
    collapsed_windows: HashSet<String>,
    filter: String,
    prompt: Option<Prompt>,
    confirm: Option<Confirm>,
    status: String,
    show_help: bool,
    should_quit: bool,
    attach_target: Option<TmuxTarget>,
    last_refresh: Instant,
    refresh_interval: Duration,
}

impl App {
    pub fn new(refresh_interval: Duration) -> Self {
        Self {
            client: TmuxClient::default(),
            state: TmuxState::default(),
            items: Vec::new(),
            selected: 0,
            detail_scroll: 0,
            help_scroll: 0,
            collapsed_sessions: HashSet::new(),
            collapsed_windows: HashSet::new(),
            filter: String::new(),
            prompt: None,
            confirm: None,
            status: "Starting lazytmux".into(),
            show_help: false,
            should_quit: false,
            attach_target: None,
            last_refresh: Instant::now(),
            refresh_interval,
        }
    }

    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        self.refresh();

        while !self.should_quit {
            terminal.draw(|frame| ui::draw(frame, self))?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key)?;
                }
            }

            if self.prompt.is_none()
                && self.confirm.is_none()
                && self.last_refresh.elapsed() >= self.refresh_interval
            {
                self.refresh();
            }
        }

        Ok(())
    }

    pub fn take_attach_target(&mut self) -> Option<TmuxTarget> {
        self.attach_target.take()
    }

    pub fn items(&self) -> &[TreeItem] {
        &self.items
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn detail_scroll(&self) -> u16 {
        self.detail_scroll
    }

    pub fn help_scroll(&self) -> u16 {
        self.help_scroll
    }

    pub fn selected_item(&self) -> Option<&TreeItem> {
        self.items.get(self.selected)
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }

    pub fn confirm(&self) -> Option<&Confirm> {
        self.confirm.as_ref()
    }

    pub fn show_help(&self) -> bool {
        self.show_help
    }

    pub fn counts(&self) -> (usize, usize, usize) {
        self.state.counts()
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.kind == KeyEventKind::Release {
            return Ok(());
        }

        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return Ok(());
        }

        if self.prompt.is_some() {
            return self.handle_prompt_key(key);
        }

        if self.confirm.is_some() {
            self.handle_confirm_key(key);
            return Ok(());
        }

        if self.show_help {
            self.handle_help_key(key);
            return Ok(());
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Char('g') => self.select_index(0),
            KeyCode::Char('G') => self.select_index(self.items.len().saturating_sub(1)),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_details(-8)
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_details(8)
            }
            KeyCode::Char('[') => self.scroll_details(-4),
            KeyCode::Char(']') => self.scroll_details(4),
            KeyCode::Char(' ') => self.toggle_selected(),
            KeyCode::Left | KeyCode::Char('h') => self.collapse_selected(),
            KeyCode::Right | KeyCode::Char('l') => self.expand_selected(),
            KeyCode::Enter => self.open_selected(),
            KeyCode::Char('/') if key.modifiers.contains(KeyModifiers::SHIFT) => self.open_help(),
            KeyCode::Char('/') => self.open_filter_prompt(),
            KeyCode::Char('f') => self.clear_filter(),
            KeyCode::Char('R') if key.modifiers.contains(KeyModifiers::SUPER) => {
                self.open_rename_prompt()
            }
            KeyCode::Char('R') => self.refresh(),
            KeyCode::Char('n') => self.open_new_session_prompt(),
            KeyCode::Char('w') => self.open_new_window_prompt(),
            KeyCode::Char('%') => self.split_selected(SplitDirection::Horizontal),
            KeyCode::Char('"') => self.split_selected(SplitDirection::Vertical),
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::SUPER) => {
                self.open_rename_prompt()
            }
            KeyCode::Char('r') => self.open_rename_prompt(),
            KeyCode::Char('x') => self.open_kill_confirm(),
            KeyCode::Char('z') => self.zoom_selected(),
            KeyCode::Char('d') => self.detach_client(),
            KeyCode::Char(':') => self.open_command_prompt(),
            KeyCode::F(1) | KeyCode::Char('?') => self.open_help(),
            _ => {}
        }

        Ok(())
    }

    fn handle_help_key(&mut self, key: KeyEvent) {
        if is_help_key(&key) || matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            self.show_help = false;
            self.help_scroll = 0;
            return;
        }

        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.scroll_help(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_help(-1),
            KeyCode::PageDown => self.scroll_help(8),
            KeyCode::PageUp => self.scroll_help(-8),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_help(8)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_help(-8)
            }
            KeyCode::Char(']') => self.scroll_help(4),
            KeyCode::Char('[') => self.scroll_help(-4),
            KeyCode::Home | KeyCode::Char('g') => self.help_scroll = 0,
            _ => {}
        }
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => self.prompt = None,
            KeyCode::Enter => {
                if let Some(prompt) = self.prompt.take() {
                    self.apply_prompt(prompt)?;
                }
            }
            KeyCode::Backspace => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.value.pop();
                }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.value.clear();
                }
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.value.push(character);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => self.confirm = None,
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(confirm) = self.confirm.take() {
                    self.apply_kill(confirm.target);
                }
            }
            _ => {}
        }
    }

    fn refresh(&mut self) {
        match self.client.load() {
            Ok(state) => {
                self.state = state;
                self.rebuild_items();
                self.status = self
                    .state
                    .notice
                    .clone()
                    .unwrap_or_else(|| self.count_status("Loaded"));
            }
            Err(error) => {
                self.status = format!("Refresh failed: {error}");
            }
        }
        self.last_refresh = Instant::now();
    }

    fn rebuild_items(&mut self) {
        let mut items = Vec::new();
        let filter = self.filter.trim().to_lowercase();

        for session in &self.state.sessions {
            if filter.is_empty() {
                self.push_unfiltered_session(&mut items, session);
            } else {
                self.push_filtered_session(&mut items, session, &filter);
            }
        }

        self.items = items;
        let previous = self.selected;
        if self.items.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.items.len() - 1);
        }
        if self.selected != previous {
            self.detail_scroll = 0;
        }
    }

    fn push_unfiltered_session(&self, items: &mut Vec<TreeItem>, session: &Session) {
        items.push(session_item(session));
        if self.collapsed_sessions.contains(&session.id) {
            return;
        }

        for window in &session.windows {
            items.push(window_item(session, window));
            if self.collapsed_windows.contains(&window.id) {
                continue;
            }

            for pane in &window.panes {
                items.push(pane_item(session, window, pane));
            }
        }
    }

    fn push_filtered_session(&self, items: &mut Vec<TreeItem>, session: &Session, filter: &str) {
        let session_item = session_item(session);
        let session_match = matches_filter(&session_item, filter);
        let mut block = Vec::new();

        for window in &session.windows {
            let window_item = window_item(session, window);
            let window_match = matches_filter(&window_item, filter);
            let mut pane_block = Vec::new();

            for pane in &window.panes {
                let pane_item = pane_item(session, window, pane);
                if session_match || window_match || matches_filter(&pane_item, filter) {
                    pane_block.push(pane_item);
                }
            }

            if session_match || window_match || !pane_block.is_empty() {
                block.push(window_item);
                block.extend(pane_block);
            }
        }

        if session_match || !block.is_empty() {
            items.push(session_item);
            items.extend(block);
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.items.is_empty() {
            self.selected = 0;
            self.detail_scroll = 0;
            return;
        }

        let next = self.selected as isize + delta;
        let previous = self.selected;
        self.selected = next.clamp(0, self.items.len() as isize - 1) as usize;
        if self.selected != previous {
            self.detail_scroll = 0;
        }
    }

    fn select_index(&mut self, index: usize) {
        let previous = self.selected;
        self.selected = index.min(self.items.len().saturating_sub(1));
        if self.selected != previous {
            self.detail_scroll = 0;
        }
    }

    fn scroll_details(&mut self, delta: i16) {
        let next = self.detail_scroll as i32 + delta as i32;
        self.detail_scroll = next.clamp(0, u16::MAX as i32) as u16;
    }

    fn scroll_help(&mut self, delta: i16) {
        let next = self.help_scroll as i32 + delta as i32;
        self.help_scroll = next.clamp(0, u16::MAX as i32) as u16;
    }

    fn open_help(&mut self) {
        self.show_help = true;
        self.help_scroll = 0;
        self.status =
            "Shortcuts page open. Scroll with j/k or arrows; close with ?/F1/q/Esc.".into();
    }

    fn toggle_selected(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            return;
        };

        match item.kind {
            TreeItemKind::Session => {
                toggle_set(&mut self.collapsed_sessions, item.target.session_id)
            }
            TreeItemKind::Window => {
                if let Some(window_id) = item.target.window_id {
                    toggle_set(&mut self.collapsed_windows, window_id);
                }
            }
            TreeItemKind::Pane => {}
        }
        self.rebuild_items();
    }

    fn collapse_selected(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            return;
        };

        match item.kind {
            TreeItemKind::Session => {
                self.collapsed_sessions.insert(item.target.session_id);
            }
            TreeItemKind::Window => {
                if let Some(window_id) = item.target.window_id {
                    self.collapsed_windows.insert(window_id);
                }
            }
            TreeItemKind::Pane => {}
        }
        self.rebuild_items();
    }

    fn expand_selected(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            return;
        };

        match item.kind {
            TreeItemKind::Session => {
                self.collapsed_sessions.remove(&item.target.session_id);
            }
            TreeItemKind::Window => {
                if let Some(window_id) = item.target.window_id {
                    self.collapsed_windows.remove(&window_id);
                }
            }
            TreeItemKind::Pane => {}
        }
        self.rebuild_items();
    }

    fn open_selected(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            self.status = "No tmux target selected".into();
            return;
        };

        if std::env::var_os("TMUX").is_some() {
            self.apply_tmux_result(
                "Switched target",
                self.client.switch_to_target(&item.target),
            );
        } else {
            self.attach_target = Some(item.target);
            self.should_quit = true;
        }
    }

    fn open_filter_prompt(&mut self) {
        self.prompt = Some(Prompt {
            title: "Filter".into(),
            value: self.filter.clone(),
            hint: "Enter to apply, Esc to cancel, Ctrl-U to clear".into(),
            kind: PromptKind::Filter,
        });
    }

    fn clear_filter(&mut self) {
        self.filter.clear();
        self.rebuild_items();
        self.status = self.count_status("Filter cleared");
    }

    fn open_new_session_prompt(&mut self) {
        self.prompt = Some(Prompt {
            title: "New session".into(),
            value: "".into(),
            hint: "Enter a tmux session name".into(),
            kind: PromptKind::NewSession,
        });
    }

    fn open_new_window_prompt(&mut self) {
        let Some(item) = self.selected_item() else {
            self.status = "Select a session/window/pane before creating a window".into();
            return;
        };

        self.prompt = Some(Prompt {
            title: "New window".into(),
            value: "".into(),
            hint: "Enter a window name".into(),
            kind: PromptKind::NewWindow {
                session_id: item.target.session_id.clone(),
            },
        });
    }

    fn split_selected(&mut self, direction: SplitDirection) {
        let Some(pane_id) = self.selected_pane_id() else {
            self.status = "Select a pane before splitting".into();
            return;
        };

        let label = match direction {
            SplitDirection::Horizontal => "Split pane horizontally",
            SplitDirection::Vertical => "Split pane vertically",
        };
        self.apply_tmux_result(label, self.client.split_pane(&pane_id, direction));
        self.refresh();
    }

    fn open_rename_prompt(&mut self) {
        let Some(item) = self.selected_item() else {
            self.status = "No item selected to rename".into();
            return;
        };

        let (title, value, kind) = match item.kind {
            TreeItemKind::Session => (
                "Rename session",
                item.rename_value.clone(),
                PromptKind::Rename(RenameTarget::Session {
                    id: item.target.session_id.clone(),
                }),
            ),
            TreeItemKind::Window => (
                "Rename window",
                item.rename_value.clone(),
                PromptKind::Rename(RenameTarget::Window {
                    id: item.target.window_id.clone().unwrap_or_default(),
                }),
            ),
            TreeItemKind::Pane => (
                "Set pane title",
                item.rename_value.clone(),
                PromptKind::Rename(RenameTarget::Pane {
                    id: item.target.pane_id.clone().unwrap_or_default(),
                }),
            ),
        };

        self.prompt = Some(Prompt {
            title: title.into(),
            value,
            hint: "Enter to apply, Esc to cancel".into(),
            kind,
        });
    }

    fn open_kill_confirm(&mut self) {
        let Some(item) = self.selected_item() else {
            self.status = "No item selected to kill".into();
            return;
        };

        let (label, target) = match item.kind {
            TreeItemKind::Session => (
                format!("Kill session {}?", item.label),
                KillTarget::Session {
                    id: item.target.session_id.clone(),
                },
            ),
            TreeItemKind::Window => (
                format!("Kill window {}?", item.label),
                KillTarget::Window {
                    id: item.target.window_id.clone().unwrap_or_default(),
                },
            ),
            TreeItemKind::Pane => (
                format!("Kill pane {}?", item.label),
                KillTarget::Pane {
                    id: item.target.pane_id.clone().unwrap_or_default(),
                },
            ),
        };

        self.confirm = Some(Confirm {
            message: format!("{label} Press y to confirm or n/Esc to cancel."),
            target,
        });
    }

    fn zoom_selected(&mut self) {
        let Some(pane_id) = self.selected_pane_id() else {
            self.status = "Select a pane before toggling zoom".into();
            return;
        };
        self.apply_tmux_result("Toggled pane zoom", self.client.toggle_zoom(&pane_id));
        self.refresh();
    }

    fn detach_client(&mut self) {
        self.apply_tmux_result("Detached tmux client", self.client.detach_client());
    }

    fn open_command_prompt(&mut self) {
        self.prompt = Some(Prompt {
            title: "tmux command".into(),
            value: "".into(),
            hint: "Example: rename-session work. Do not include the leading tmux word.".into(),
            kind: PromptKind::Command,
        });
    }

    fn apply_prompt(&mut self, prompt: Prompt) -> Result<()> {
        let value = prompt.value.trim();
        match prompt.kind {
            PromptKind::Filter => {
                self.filter = value.to_string();
                self.rebuild_items();
                self.status = self.count_status("Filter applied");
            }
            PromptKind::Rename(target) => {
                if value.is_empty() {
                    self.status = "Name cannot be empty".into();
                    return Ok(());
                }
                match target {
                    RenameTarget::Session { id } => {
                        self.apply_tmux_result(
                            "Renamed session",
                            self.client.rename_session(&id, value),
                        );
                    }
                    RenameTarget::Window { id } => {
                        self.apply_tmux_result(
                            "Renamed window",
                            self.client.rename_window(&id, value),
                        );
                    }
                    RenameTarget::Pane { id } => {
                        self.apply_tmux_result(
                            "Set pane title",
                            self.client.rename_pane(&id, value),
                        );
                    }
                }
                self.refresh();
            }
            PromptKind::NewSession => {
                if value.is_empty() {
                    self.status = "Session name cannot be empty".into();
                    return Ok(());
                }
                self.apply_tmux_result("Created session", self.client.create_session(value));
                self.refresh();
            }
            PromptKind::NewWindow { session_id } => {
                if value.is_empty() {
                    self.status = "Window name cannot be empty".into();
                    return Ok(());
                }
                self.apply_tmux_result(
                    "Created window",
                    self.client.create_window(&session_id, value),
                );
                self.refresh();
            }
            PromptKind::Command => {
                let args = split_tmux_args(value)?;
                if args.is_empty() {
                    self.status = "No tmux command entered".into();
                    return Ok(());
                }
                self.apply_tmux_result("Ran tmux command", self.client.run_args(&args));
                self.refresh();
            }
        }

        Ok(())
    }

    fn apply_kill(&mut self, target: KillTarget) {
        match target {
            KillTarget::Session { id } => {
                self.apply_tmux_result("Killed session", self.client.kill_session(&id));
            }
            KillTarget::Window { id } => {
                self.apply_tmux_result("Killed window", self.client.kill_window(&id));
            }
            KillTarget::Pane { id } => {
                self.apply_tmux_result("Killed pane", self.client.kill_pane(&id));
            }
        }
        self.refresh();
    }

    fn selected_pane_id(&self) -> Option<String> {
        self.selected_item()
            .and_then(|item| item.target.pane_id.clone())
    }

    fn apply_tmux_result(&mut self, success: &str, result: Result<()>) {
        self.status = match result {
            Ok(()) => success.into(),
            Err(error) => format!("{success} failed: {error}"),
        };
    }

    fn count_status(&self, prefix: &str) -> String {
        let (sessions, windows, panes) = self.counts();
        format!("{prefix}: {sessions} sessions, {windows} windows, {panes} panes")
    }
}

fn session_item(session: &Session) -> TreeItem {
    let active_window = session
        .windows
        .iter()
        .find(|window| window.active)
        .or_else(|| session.windows.first());

    TreeItem {
        kind: TreeItemKind::Session,
        depth: 0,
        name: session.name.clone(),
        label: format!("[S] {}", session.name),
        subtitle: format!(
            "{} windows{}",
            session.window_count.unwrap_or(session.windows.len()),
            if session.attached { ", attached" } else { "" }
        ),
        target: TmuxTarget::session_on(session.id.clone(), session.server.clone()),
        rename_value: session.name.clone(),
        details: vec![
            ("Type".into(), "Session".into()),
            ("ID".into(), session.id.clone()),
            ("Name".into(), session.name.clone()),
            ("Attached".into(), session.attached.to_string()),
            (
                "Windows".into(),
                session
                    .window_count
                    .unwrap_or(session.windows.len())
                    .to_string(),
            ),
            (
                "Created".into(),
                session
                    .created
                    .map(|created| created.to_string())
                    .unwrap_or_else(|| "unknown".into()),
            ),
            (
                "Preview window".into(),
                active_window
                    .map(|window| format!("{}:{}", window.index, window.name))
                    .unwrap_or_else(|| "none".into()),
            ),
        ],
        preview: active_window.map(|window| {
            window_preview(
                format!(
                    "Session {} - window {}:{}",
                    session.name, window.index, window.name
                ),
                window,
                false,
            )
        }),
        active: session.attached,
        dead: false,
    }
}

fn window_item(session: &Session, window: &Window) -> TreeItem {
    let pane_names = window
        .panes
        .iter()
        .map(pane_name)
        .collect::<Vec<_>>()
        .join(", ");

    TreeItem {
        kind: TreeItemKind::Window,
        depth: 1,
        name: window.name.clone(),
        label: format!("[W] {}:{}", window.index, window.name),
        subtitle: format!(
            "{} panes {}",
            window.pane_count.unwrap_or(window.panes.len()),
            window.flags
        ),
        target: TmuxTarget::window_on(
            session.id.clone(),
            window.id.clone(),
            session.server.clone(),
        ),
        rename_value: window.name.clone(),
        details: vec![
            ("Type".into(), "Window".into()),
            ("ID".into(), window.id.clone()),
            ("Session".into(), session.name.clone()),
            ("Index".into(), window.index.clone()),
            ("Name".into(), window.name.clone()),
            ("Active".into(), window.active.to_string()),
            ("Flags".into(), window.flags.clone()),
            (
                "Panes".into(),
                window.pane_count.unwrap_or(window.panes.len()).to_string(),
            ),
            (
                "Pane names".into(),
                if pane_names.is_empty() {
                    "none".into()
                } else {
                    pane_names
                },
            ),
            ("Layout".into(), window.layout.clone()),
        ],
        preview: Some(window_preview(
            format!("Window {}:{}", window.index, window.name),
            window,
            false,
        )),
        active: window.active,
        dead: false,
    }
}

fn pane_item(session: &Session, window: &Window, pane: &Pane) -> TreeItem {
    let size = match (pane.width, pane.height) {
        (Some(width), Some(height)) => format!("{width}x{height}"),
        _ => "unknown".into(),
    };
    let name = pane_name(pane);

    TreeItem {
        kind: TreeItemKind::Pane,
        depth: 2,
        name: name.clone(),
        label: format!("[P] {}: {}", pane.index, name),
        subtitle: if pane.command.is_empty() {
            pane.path.clone()
        } else {
            format!("{} - {}", pane.command, pane.path)
        },
        target: TmuxTarget::pane_on(
            session.id.clone(),
            window.id.clone(),
            pane.id.clone(),
            session.server.clone(),
        ),
        rename_value: if pane.title.is_empty() {
            name.clone()
        } else {
            pane.title.clone()
        },
        details: vec![
            ("Type".into(), "Pane".into()),
            ("ID".into(), pane.id.clone()),
            ("Session".into(), session.name.clone()),
            ("Window".into(), format!("{}:{}", window.index, window.name)),
            ("Name".into(), name.clone()),
            ("Index".into(), pane.index.clone()),
            ("Active".into(), pane.active.to_string()),
            ("Command".into(), pane.command.clone()),
            ("Title".into(), pane.title.clone()),
            ("Path".into(), pane.path.clone()),
            (
                "Position".into(),
                format!(
                    "{},{}",
                    pane.left
                        .map(|left| left.to_string())
                        .unwrap_or_else(|| "?".into()),
                    pane.top
                        .map(|top| top.to_string())
                        .unwrap_or_else(|| "?".into())
                ),
            ),
            ("PID".into(), pane.pid.clone()),
            ("Size".into(), size),
            ("Dead".into(), pane.dead.to_string()),
            ("Copy mode".into(), pane.in_mode.to_string()),
        ],
        preview: Some(TerminalPreview {
            title: format!("Pane {} - {}", pane.index, name),
            panes: vec![pane_preview(pane, true)],
        }),
        active: pane.active,
        dead: pane.dead,
    }
}

fn matches_filter(item: &TreeItem, filter: &str) -> bool {
    item.label.to_lowercase().contains(filter)
        || item.name.to_lowercase().contains(filter)
        || item.subtitle.to_lowercase().contains(filter)
        || item
            .details
            .iter()
            .any(|(_, value)| value.to_lowercase().contains(filter))
}

fn pane_name(pane: &Pane) -> String {
    let title = pane.title.trim();
    if !title.is_empty() {
        title.to_string()
    } else if !pane.command.trim().is_empty() {
        pane.command.clone()
    } else {
        pane.id.clone()
    }
}

fn window_preview(title: String, window: &Window, full_panes: bool) -> TerminalPreview {
    TerminalPreview {
        title,
        panes: window
            .panes
            .iter()
            .map(|pane| pane_preview(pane, full_panes))
            .collect(),
    }
}

fn pane_preview(pane: &Pane, full: bool) -> TerminalPanePreview {
    TerminalPanePreview {
        title: format!("{} {}", pane.index, pane_name(pane)),
        content: if pane.content.trim().is_empty() {
            "(no captured content)".into()
        } else {
            pane.content.clone()
        },
        left: if full { 0 } else { pane.left.unwrap_or(0) },
        top: if full { 0 } else { pane.top.unwrap_or(0) },
        width: pane.width.filter(|width| *width > 0).unwrap_or(1),
        height: pane.height.filter(|height| *height > 0).unwrap_or(1),
        active: pane.active,
        dead: pane.dead,
    }
}

fn toggle_set(set: &mut HashSet<String>, value: String) {
    if !set.remove(&value) {
        set.insert(value);
    }
}

fn is_help_key(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::F(1) | KeyCode::Char('?'))
        || (key.code == KeyCode::Char('/') && key.modifiers.contains(KeyModifiers::SHIFT))
}

fn split_tmux_args(input: &str) -> Result<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut in_arg = false;

    for character in input.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            in_arg = true;
            continue;
        }

        match character {
            '\\' => {
                escaped = true;
                in_arg = true;
            }
            '\'' | '"' if quote == Some(character) => {
                quote = None;
                in_arg = true;
            }
            '\'' | '"' if quote.is_none() => {
                quote = Some(character);
                in_arg = true;
            }
            character if character.is_whitespace() && quote.is_none() => {
                if in_arg {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                }
            }
            character => {
                current.push(character);
                in_arg = true;
            }
        }
    }

    if escaped {
        current.push('\\');
    }

    if let Some(quote) = quote {
        bail!("unclosed quote {quote}");
    }

    if in_arg {
        args.push(current);
    }

    if args.first().is_some_and(|arg| arg == "tmux") {
        return Err(anyhow!("omit the leading tmux word"));
    }

    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_tmux_command_with_quotes() {
        let args = split_tmux_args("rename-window -t @1 'work notes'").unwrap();
        assert_eq!(args, vec!["rename-window", "-t", "@1", "work notes"]);
    }

    #[test]
    fn rejects_leading_tmux_word() {
        let error = split_tmux_args("tmux list-sessions").unwrap_err();
        assert!(error.to_string().contains("omit"));
    }
}
