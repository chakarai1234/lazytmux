use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use clap::ValueEnum;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use ratatui::Terminal;

use crate::ansi::strip_ansi;
use crate::keybindings::{fuzzy_score, palette_entries, Action, ShortcutEntry};
use crate::metrics::{
    format_bytes, format_cpu_usage, MetricsSampler, MetricsSnapshot, ProcessMetrics, SystemMetrics,
};
use crate::theme::{load_saved_theme, save_theme, Theme, ThemeId};
use crate::tmux::{
    copy_to_system_clipboard, Pane, ResizeDirection, Session, SplitDirection, TmuxBuffer,
    TmuxClient, TmuxLayout, TmuxState, TmuxTarget, Window,
};
use crate::ui;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum ViewMode {
    #[default]
    Deck,
    Live,
    Telemetry,
    Buffers,
}

impl ViewMode {
    pub const ALL: [ViewMode; 4] = [
        ViewMode::Deck,
        ViewMode::Live,
        ViewMode::Telemetry,
        ViewMode::Buffers,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Deck => "1 DECK",
            Self::Live => "2 LIVE",
            Self::Telemetry => "3 TELEMETRY",
            Self::Buffers => "4 BUFFERS",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FocusPane {
    #[default]
    Tree,
    Inspector,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SortMode {
    #[default]
    Name,
    CpuDesc,
    MemoryDesc,
    ActiveFirst,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            Self::Name => Self::CpuDesc,
            Self::CpuDesc => Self::MemoryDesc,
            Self::MemoryDesc => Self::ActiveFirst,
            Self::ActiveFirst => Self::Name,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "NAME",
            Self::CpuDesc => "CPU↓",
            Self::MemoryDesc => "MEM↓",
            Self::ActiveFirst => "ACTIVE",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusLevel {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TreeItemKind {
    Session,
    Window,
    Pane,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TreeItem {
    pub kind: TreeItemKind,
    pub depth: usize,
    pub name: String,
    pub label: String,
    pub subtitle: String,
    pub target: TmuxTarget,
    pub favorite_key: String,
    pub favorite: bool,
    pub rename_value: String,
    pub details: Vec<(String, String)>,
    pub preview: Option<TerminalPreview>,
    pub expanded: Option<bool>,
    pub active: bool,
    pub dead: bool,
    pub zoomed: bool,
    pub synchronized: bool,
    pub marked: bool,
    pub in_mode: bool,
    pub cpu_percent: Option<f32>,
    pub memory_bytes: u64,
    pub process_count: usize,
    pub server_label: String,
    pub cwd: String,
    pub command: String,
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
    pub marked: bool,
    pub target: TmuxTarget,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TelemetryRow {
    pub session_name: String,
    pub window_label: String,
    pub pane_label: String,
    pub command: String,
    pub pid: String,
    pub cpu_percent: Option<f32>,
    pub memory_bytes: u64,
    pub process_count: usize,
    pub active: bool,
    pub target: TmuxTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JumpCandidate {
    pub kind: TreeItemKind,
    pub breadcrumb: String,
    pub subtitle: String,
    pub target: TmuxTarget,
    pub active: bool,
    pub favorite: bool,
    pub searchable: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
}

impl TextInput {
    pub fn new(value: String) -> Self {
        let cursor = value.len();
        Self { value, cursor }
    }

    pub fn insert_char(&mut self, character: char) {
        self.value.insert(self.cursor, character);
        self.cursor += character.len_utf8();
    }

    pub fn delete_backward(&mut self) {
        if let Some((offset, _)) = self.value[..self.cursor].char_indices().next_back() {
            self.value.remove(offset);
            self.cursor = offset;
        }
    }

    pub fn delete_forward(&mut self) {
        if self.cursor < self.value.len() {
            self.value.remove(self.cursor);
        }
    }

    pub fn move_left(&mut self) {
        if let Some((offset, _)) = self.value[..self.cursor].char_indices().next_back() {
            self.cursor = offset;
        }
    }

    pub fn move_right(&mut self) {
        if let Some(character) = self.value[self.cursor..].chars().next() {
            self.cursor += character.len_utf8();
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.value.len();
    }

    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }

    pub fn cursor_char_index(&self) -> usize {
        self.value[..self.cursor].chars().count()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prompt {
    pub title: String,
    pub value: String,
    pub hint: String,
    /// Byte offset of the caret within `value`. Always kept on a UTF-8
    /// character boundary so string operations never panic.
    pub cursor: usize,
    kind: PromptKind,
}

impl Prompt {
    fn new(title: String, value: String, hint: String, kind: PromptKind) -> Self {
        let cursor = value.len();
        Self {
            title,
            value,
            hint,
            cursor,
            kind,
        }
    }

    fn insert_char(&mut self, character: char) {
        self.value.insert(self.cursor, character);
        self.cursor += character.len_utf8();
    }

    fn delete_backward(&mut self) {
        if let Some((offset, _)) = self.value[..self.cursor].char_indices().next_back() {
            self.value.remove(offset);
            self.cursor = offset;
        }
    }

    fn delete_forward(&mut self) {
        if self.cursor < self.value.len() {
            self.value.remove(self.cursor);
        }
    }

    fn move_left(&mut self) {
        if let Some((offset, _)) = self.value[..self.cursor].char_indices().next_back() {
            self.cursor = offset;
        }
    }

    fn move_right(&mut self) {
        if let Some(character) = self.value[self.cursor..].chars().next() {
            self.cursor += character.len_utf8();
        }
    }

    fn move_home(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.value.len();
    }

    fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }

    pub fn cursor_char_index(&self) -> usize {
        self.value[..self.cursor].chars().count()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Confirm {
    pub message: String,
    target: ConfirmAction,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteState {
    pub input: TextInput,
    pub selected: usize,
}

impl PaletteState {
    fn new() -> Self {
        Self {
            input: TextInput::new(String::new()),
            selected: 0,
        }
    }

    pub fn filtered_entries(&self) -> Vec<&'static ShortcutEntry> {
        let query = self.input.value.trim();
        let mut scored: Vec<(i32, &'static ShortcutEntry)> = palette_entries()
            .into_iter()
            .filter_map(|entry| {
                let haystack = format!(
                    "{} {} {} {}",
                    entry.title,
                    entry.description,
                    entry.keys,
                    entry.category.title()
                );
                fuzzy_score(query, &haystack).map(|score| (score, entry))
            })
            .collect();
        if !query.is_empty() {
            scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        }
        scored.into_iter().map(|(_, entry)| entry).collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickJumpState {
    pub input: TextInput,
    pub selected: usize,
}

impl QuickJumpState {
    fn new() -> Self {
        Self {
            input: TextInput::new(String::new()),
            selected: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayoutPickerState {
    pub selected: usize,
    pub target: TmuxTarget,
    pub window_label: String,
}

#[derive(Clone, Debug, Default)]
pub struct LayoutRegions {
    pub tree_inner: Rect,
    pub tree_scroll_offset: usize,
    pub inspector_area: Rect,
    pub tab_rects: Vec<(Rect, ViewMode)>,
    pub preview_pane_rects: Vec<(Rect, TmuxTarget)>,
    pub theme_badge_rect: Option<Rect>,
    pub live_badge_rect: Option<Rect>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PromptKind {
    Filter,
    Rename(RenameTarget),
    NewSession,
    LaunchSession,
    NewWindow {
        target: TmuxTarget,
    },
    LaunchWindow {
        target: TmuxTarget,
    },
    SendKeys {
        target: TmuxTarget,
    },
    BroadcastKeys {
        targets: Vec<TmuxTarget>,
        scope: String,
    },
    JoinPane {
        source: TmuxTarget,
    },
    SaveSnapshot {
        target: TmuxTarget,
    },
    Command,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RenameTarget {
    Session { target: TmuxTarget },
    Window { target: TmuxTarget },
    Pane { target: TmuxTarget },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ConfirmAction {
    KillSession { target: TmuxTarget },
    KillWindow { target: TmuxTarget },
    KillPane { target: TmuxTarget },
    DeleteBuffer { buffer: TmuxBuffer },
}

#[derive(Debug)]
pub struct App {
    client: TmuxClient,
    state: TmuxState,
    items: Vec<TreeItem>,
    selected: usize,
    detail_scroll: u16,
    help_scroll: u16,
    diagnostics_scroll: u16,
    telemetry_selected: usize,
    buffer_selected: usize,
    buffer_scroll: u16,
    expanded_sessions: HashSet<String>,
    expanded_windows: HashSet<String>,
    favorites: HashSet<String>,
    favorites_only: bool,
    sort_mode: SortMode,
    view_mode: ViewMode,
    focus: FocusPane,
    zen_mode: bool,
    paused: bool,
    theme_id: ThemeId,
    metrics_sampler: MetricsSampler,
    metrics: MetricsSnapshot,
    filter: String,
    prompt: Option<Prompt>,
    confirm: Option<Confirm>,
    palette: Option<PaletteState>,
    quick_jump: Option<QuickJumpState>,
    layout_picker: Option<LayoutPickerState>,
    status: String,
    status_level: StatusLevel,
    show_help: bool,
    show_diagnostics: bool,
    should_quit: bool,
    attach_target: Option<TmuxTarget>,
    last_refresh: Instant,
    refresh_interval: Duration,
    tick: u64,
    last_click: Option<(Instant, usize)>,
    regions: RefCell<LayoutRegions>,
}

impl App {
    pub fn with_options(
        refresh_interval: Duration,
        client: TmuxClient,
        theme_override: Option<ThemeId>,
        initial_view: ViewMode,
    ) -> Self {
        Self {
            client,
            state: TmuxState::default(),
            items: Vec::new(),
            selected: 0,
            detail_scroll: 0,
            help_scroll: 0,
            diagnostics_scroll: 0,
            telemetry_selected: 0,
            buffer_selected: 0,
            buffer_scroll: 0,
            expanded_sessions: HashSet::new(),
            expanded_windows: HashSet::new(),
            favorites: load_favorites(),
            favorites_only: false,
            sort_mode: SortMode::Name,
            view_mode: initial_view,
            focus: FocusPane::Tree,
            zen_mode: false,
            paused: false,
            theme_id: theme_override.unwrap_or_else(load_saved_theme),
            metrics_sampler: MetricsSampler::new(),
            metrics: MetricsSnapshot::default(),
            filter: String::new(),
            prompt: None,
            confirm: None,
            palette: None,
            quick_jump: None,
            layout_picker: None,
            status: "System online — press Ctrl+P for Command Palette or ? for Shortcuts Atlas"
                .into(),
            status_level: StatusLevel::Info,
            show_help: false,
            show_diagnostics: false,
            should_quit: false,
            attach_target: None,
            last_refresh: Instant::now(),
            refresh_interval,
            tick: 0,
            last_click: None,
            regions: RefCell::new(LayoutRegions::default()),
        }
    }

    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        self.refresh();

        while !self.should_quit {
            terminal.draw(|frame| ui::draw(frame, self))?;

            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(key) => self.handle_key(key)?,
                    Event::Mouse(mouse) => self.handle_mouse(mouse),
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            } else {
                self.tick = self.tick.wrapping_add(1);
            }

            if !self.paused
                && !self.has_modal_open()
                && self.last_refresh.elapsed() >= self.refresh_interval
            {
                self.refresh();
            }
        }

        Ok(())
    }

    pub fn has_modal_open(&self) -> bool {
        self.prompt.is_some()
            || self.confirm.is_some()
            || self.palette.is_some()
            || self.quick_jump.is_some()
            || self.layout_picker.is_some()
            || self.show_help
            || self.show_diagnostics
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

    pub fn diagnostics_scroll(&self) -> u16 {
        self.diagnostics_scroll
    }

    pub fn selected_item(&self) -> Option<&TreeItem> {
        self.items.get(self.selected)
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn status_level(&self) -> StatusLevel {
        self.status_level
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn favorites_only(&self) -> bool {
        self.favorites_only
    }

    pub fn sort_mode(&self) -> SortMode {
        self.sort_mode
    }

    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    pub fn focus(&self) -> FocusPane {
        self.focus
    }

    pub fn zen_mode(&self) -> bool {
        self.zen_mode
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn refresh_seconds(&self) -> u64 {
        self.refresh_interval.as_secs()
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn theme_id(&self) -> ThemeId {
        self.theme_id
    }

    pub fn theme(&self) -> Theme {
        self.theme_id.palette()
    }

    pub fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }

    pub fn confirm(&self) -> Option<&Confirm> {
        self.confirm.as_ref()
    }

    pub fn palette(&self) -> Option<&PaletteState> {
        self.palette.as_ref()
    }

    pub fn quick_jump(&self) -> Option<&QuickJumpState> {
        self.quick_jump.as_ref()
    }

    pub fn layout_picker(&self) -> Option<&LayoutPickerState> {
        self.layout_picker.as_ref()
    }

    pub fn show_help(&self) -> bool {
        self.show_help
    }

    pub fn show_diagnostics(&self) -> bool {
        self.show_diagnostics
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.state.diagnostics
    }

    pub fn tmux_version(&self) -> Option<&str> {
        self.state.version.as_deref()
    }

    pub fn buffers(&self) -> &[TmuxBuffer] {
        &self.state.buffers
    }

    pub fn buffer_selected(&self) -> usize {
        self.buffer_selected
    }

    pub fn buffer_scroll(&self) -> u16 {
        self.buffer_scroll
    }

    pub fn telemetry_selected(&self) -> usize {
        self.telemetry_selected
    }

    pub fn favorites_count(&self) -> usize {
        self.favorites.len()
    }

    pub fn counts(&self) -> (usize, usize, usize) {
        self.state.counts()
    }

    pub fn server_count(&self) -> usize {
        self.state.server_count()
    }

    pub fn system_metrics(&self) -> &SystemMetrics {
        &self.metrics.system
    }

    pub fn cpu_metrics_ready(&self) -> bool {
        self.metrics.cpu_ready
    }

    pub fn cpu_history(&self) -> &[u64] {
        &self.metrics.cpu_history
    }

    pub fn mem_history(&self) -> &[u64] {
        &self.metrics.mem_history
    }

    pub fn set_regions(&self, regions: LayoutRegions) {
        *self.regions.borrow_mut() = regions;
    }

    pub fn telemetry_rows(&self) -> Vec<TelemetryRow> {
        let mut rows = Vec::new();
        for session in &self.state.sessions {
            for window in &session.windows {
                for pane in &window.panes {
                    let proc_metrics = self.metrics.panes.get(&pane.pid);
                    rows.push(TelemetryRow {
                        session_name: session.name.clone(),
                        window_label: format!("{}:{}", window.index, window.name),
                        pane_label: format!("{}:{}", pane.index, pane_name(pane)),
                        command: pane.command.clone(),
                        pid: pane.pid.clone(),
                        cpu_percent: proc_metrics.map(|m| m.cpu_usage),
                        memory_bytes: proc_metrics.map(|m| m.memory_bytes).unwrap_or(0),
                        process_count: proc_metrics.map(|m| m.process_count).unwrap_or(0),
                        active: pane.active && window.active,
                        target: TmuxTarget::pane_on(
                            session.id.clone(),
                            window.id.clone(),
                            pane.id.clone(),
                            session.server.clone(),
                        ),
                    });
                }
            }
        }
        rows.sort_by(|left, right| {
            right
                .cpu_percent
                .unwrap_or(-1.0)
                .total_cmp(&left.cpu_percent.unwrap_or(-1.0))
                .then_with(|| right.memory_bytes.cmp(&left.memory_bytes))
                .then_with(|| left.session_name.cmp(&right.session_name))
        });
        rows
    }

    pub fn quick_jump_matches(&self) -> Vec<JumpCandidate> {
        let query = self
            .quick_jump
            .as_ref()
            .map(|state| state.input.value.trim())
            .unwrap_or("");
        let mut candidates = Vec::new();

        for session in &self.state.sessions {
            let s_target = TmuxTarget::session_on(session.id.clone(), session.server.clone());
            let s_fav = self.favorites.contains(&s_target.favorite_key());
            candidates.push(JumpCandidate {
                kind: TreeItemKind::Session,
                breadcrumb: session.name.clone(),
                subtitle: format!(
                    "{}w · {}{}",
                    session.windows.len(),
                    session.server.label(),
                    if session.attached { " · attached" } else { "" }
                ),
                searchable: format!("session {} {}", session.name, session.server.label()),
                target: s_target,
                active: session.attached,
                favorite: s_fav,
            });

            for window in &session.windows {
                let w_target = TmuxTarget::window_on(
                    session.id.clone(),
                    window.id.clone(),
                    session.server.clone(),
                );
                let w_fav = self.favorites.contains(&w_target.favorite_key());
                candidates.push(JumpCandidate {
                    kind: TreeItemKind::Window,
                    breadcrumb: format!("{} › {}:{}", session.name, window.index, window.name),
                    subtitle: format!("{}p · {}", window.panes.len(), window.layout),
                    searchable: format!(
                        "window {} {}:{} {}",
                        session.name, window.index, window.name, window.flags
                    ),
                    target: w_target,
                    active: window.active,
                    favorite: w_fav,
                });

                for pane in &window.panes {
                    let p_target = TmuxTarget::pane_on(
                        session.id.clone(),
                        window.id.clone(),
                        pane.id.clone(),
                        session.server.clone(),
                    );
                    let p_fav = self.favorites.contains(&p_target.favorite_key());
                    let pname = pane_name(pane);
                    candidates.push(JumpCandidate {
                        kind: TreeItemKind::Pane,
                        breadcrumb: format!(
                            "{} › {}:{} › {}:{}",
                            session.name, window.index, window.name, pane.index, pname
                        ),
                        subtitle: format!("{} · {}", pane.command, pane.path),
                        searchable: format!(
                            "pane {} {} {} {} {} {}",
                            session.name, window.name, pname, pane.command, pane.path, pane.id
                        ),
                        target: p_target,
                        active: pane.active,
                        favorite: p_fav,
                    });
                }
            }
        }

        if query.is_empty() {
            candidates.sort_by(|left, right| {
                right
                    .favorite
                    .cmp(&left.favorite)
                    .then_with(|| right.active.cmp(&left.active))
                    .then_with(|| left.breadcrumb.cmp(&right.breadcrumb))
            });
            return candidates;
        }

        let mut scored: Vec<(i32, JumpCandidate)> = candidates
            .into_iter()
            .filter_map(|candidate| {
                fuzzy_score(query, &candidate.searchable).map(|score| (score, candidate))
            })
            .collect();
        scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        scored.into_iter().map(|(_, candidate)| candidate).collect()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
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

        if self.palette.is_some() {
            return self.handle_palette_key(key);
        }

        if self.quick_jump.is_some() {
            self.handle_quick_jump_key(key);
            return Ok(());
        }

        if self.layout_picker.is_some() {
            self.handle_layout_picker_key(key);
            return Ok(());
        }

        if self.show_help {
            self.handle_help_key(key);
            return Ok(());
        }

        if self.show_diagnostics {
            self.handle_diagnostics_key(key);
            return Ok(());
        }

        // Buffers tab focused shortcuts when user is inside Buffers view & Inspector focus
        if self.view_mode == ViewMode::Buffers
            && self.focus == FocusPane::Inspector
            && self.handle_buffers_focused_key(key)
        {
            return Ok(());
        }

        // Telemetry tab focused shortcuts when user is inside Telemetry view & Inspector focus
        if self.view_mode == ViewMode::Telemetry
            && self.focus == FocusPane::Inspector
            && self.handle_telemetry_focused_key(key)
        {
            return Ok(());
        }

        // Alt+Arrows resize pane
        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Left => {
                    self.execute_action(Action::ResizeLeft)?;
                    return Ok(());
                }
                KeyCode::Right => {
                    self.execute_action(Action::ResizeRight)?;
                    return Ok(());
                }
                KeyCode::Up => {
                    self.execute_action(Action::ResizeUp)?;
                    return Ok(());
                }
                KeyCode::Down => {
                    self.execute_action(Action::ResizeDown)?;
                    return Ok(());
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.execute_action(Action::OpenPalette)?;
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.execute_action(Action::QuickJump)?;
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.execute_action(Action::ClearPane)?;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.execute_action(Action::ScrollDetailsUp)?;
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.execute_action(Action::ScrollDetailsDown)?;
            }
            KeyCode::Esc | KeyCode::Char('q') => self.execute_action(Action::Quit)?,
            KeyCode::Tab | KeyCode::BackTab => self.execute_action(Action::CycleFocus)?,
            KeyCode::Down | KeyCode::Char('j') => {
                if self.focus == FocusPane::Inspector
                    && matches!(self.view_mode, ViewMode::Deck | ViewMode::Live)
                {
                    self.scroll_details(2);
                } else {
                    self.execute_action(Action::MoveDown)?;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.focus == FocusPane::Inspector
                    && matches!(self.view_mode, ViewMode::Deck | ViewMode::Live)
                {
                    self.scroll_details(-2);
                } else {
                    self.execute_action(Action::MoveUp)?;
                }
            }
            KeyCode::PageDown => self.execute_action(Action::PageDown)?,
            KeyCode::PageUp => self.execute_action(Action::PageUp)?,
            KeyCode::Char('g') => self.execute_action(Action::JumpTop)?,
            KeyCode::Char('G') => self.execute_action(Action::JumpBottom)?,
            KeyCode::Char('[') => self.execute_action(Action::ScrollDetailsUp)?,
            KeyCode::Char(']') => self.execute_action(Action::ScrollDetailsDown)?,
            KeyCode::Char(' ') => self.execute_action(Action::ToggleExpand)?,
            KeyCode::Left | KeyCode::Char('h') => self.execute_action(Action::Collapse)?,
            KeyCode::Right | KeyCode::Char('l') => self.execute_action(Action::Expand)?,
            KeyCode::Char('e') => self.execute_action(Action::ExpandAll)?,
            KeyCode::Char('c') => self.execute_action(Action::CollapseAll)?,
            KeyCode::Enter => self.execute_action(Action::AttachOrSwitch)?,
            KeyCode::Char('1') => self.execute_action(Action::ViewDeck)?,
            KeyCode::Char('2') => self.execute_action(Action::ViewLive)?,
            KeyCode::Char('3') => self.execute_action(Action::ViewTelemetry)?,
            KeyCode::Char('4') => self.execute_action(Action::ViewBuffers)?,
            KeyCode::Char('0') => self.execute_action(Action::ResetView)?,
            KeyCode::Char('p') => self.execute_action(Action::OpenPalette)?,
            KeyCode::Char('v') => self.execute_action(Action::ToggleZen)?,
            KeyCode::Char('t') => self.execute_action(Action::CycleTheme)?,
            KeyCode::Char('O') => self.execute_action(Action::CycleSort)?,
            KeyCode::Char('F') => self.execute_action(Action::ToggleFavoritesOnly)?,
            KeyCode::Char('P') => self.execute_action(Action::TogglePause)?,
            KeyCode::Char('/') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.execute_action(Action::Help)?;
            }
            KeyCode::Char('/') => self.execute_action(Action::Filter)?,
            KeyCode::Char('f') => self.execute_action(Action::ClearFilter)?,
            KeyCode::Char('R') if key.modifiers.contains(KeyModifiers::SUPER) => {
                self.execute_action(Action::Rename)?;
            }
            KeyCode::Char('R') => self.execute_action(Action::Refresh)?,
            KeyCode::Char('n') => self.execute_action(Action::NewSession)?,
            KeyCode::Char('N') => self.execute_action(Action::LaunchSession)?,
            KeyCode::Char('w') => self.execute_action(Action::NewWindow)?,
            KeyCode::Char('W') => self.execute_action(Action::NewWindowCustom)?,
            KeyCode::Char('%') | KeyCode::Char('|') => {
                self.execute_action(Action::SplitHorizontal)?;
            }
            KeyCode::Char('"') | KeyCode::Char('-') => {
                self.execute_action(Action::SplitVertical)?;
            }
            KeyCode::Char('r') => self.execute_action(Action::Rename)?,
            KeyCode::Char('x') => self.execute_action(Action::Kill)?,
            KeyCode::Char('z') => self.execute_action(Action::ToggleZoom)?,
            KeyCode::Char('L') => self.execute_action(Action::OpenLayoutPicker)?,
            KeyCode::Char('b') => self.execute_action(Action::ToggleSyncPanes)?,
            KeyCode::Char('B') => self.execute_action(Action::BreakPane)?,
            KeyCode::Char('J') => self.execute_action(Action::JoinPane)?,
            KeyCode::Char('o') => self.execute_action(Action::RotatePanes)?,
            KeyCode::Char('>') => self.execute_action(Action::SwapNext)?,
            KeyCode::Char('<') => self.execute_action(Action::SwapPrev)?,
            KeyCode::Char('m') => self.execute_action(Action::ToggleMarkPane)?,
            KeyCode::Char('M') => self.execute_action(Action::SwapMarkedPane)?,
            KeyCode::Char('{') => self.execute_action(Action::ResizeLeft)?,
            KeyCode::Char('}') => self.execute_action(Action::ResizeRight)?,
            KeyCode::Char('X') => self.execute_action(Action::RespawnPane)?,
            KeyCode::Char('*') => self.execute_action(Action::ToggleFavorite)?,
            KeyCode::Char('s') => self.execute_action(Action::SendKeys)?,
            KeyCode::Char('S') => self.execute_action(Action::BroadcastKeys)?,
            KeyCode::Char('y') => self.execute_action(Action::CopyPaneBuffer)?,
            KeyCode::Char('Y') => self.execute_action(Action::CopySystemClipboard)?,
            KeyCode::Char('C') => self.execute_action(Action::SavePaneSnapshot)?,
            KeyCode::Char('D') => self.execute_action(Action::Diagnostics)?,
            KeyCode::Char('d') => self.execute_action(Action::DetachClient)?,
            KeyCode::Char(':') => self.execute_action(Action::RunCommand)?,
            KeyCode::F(1) | KeyCode::Char('?') => self.execute_action(Action::Help)?,
            _ => {}
        }

        Ok(())
    }

    pub fn execute_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::MoveDown => self.move_selection(1),
            Action::MoveUp => self.move_selection(-1),
            Action::PageDown => self.move_selection(10),
            Action::PageUp => self.move_selection(-10),
            Action::JumpTop => self.select_index(0),
            Action::JumpBottom => self.select_index(self.items.len().saturating_sub(1)),
            Action::ToggleExpand => self.toggle_selected(),
            Action::Expand => self.expand_selected(),
            Action::Collapse => self.collapse_selected(),
            Action::ExpandAll => self.expand_all(),
            Action::CollapseAll => self.collapse_all(),
            Action::CycleFocus => {
                self.focus = match self.focus {
                    FocusPane::Tree => FocusPane::Inspector,
                    FocusPane::Inspector => FocusPane::Tree,
                };
                self.set_status(
                    StatusLevel::Info,
                    format!(
                        "Focus switched to {}",
                        match self.focus {
                            FocusPane::Tree => "Topology Tree",
                            FocusPane::Inspector => "Inspector Deck",
                        }
                    ),
                );
            }
            Action::QuickJump => {
                self.quick_jump = Some(QuickJumpState::new());
            }
            Action::ViewDeck => self.set_view_mode(ViewMode::Deck),
            Action::ViewLive => self.set_view_mode(ViewMode::Live),
            Action::ViewTelemetry => self.set_view_mode(ViewMode::Telemetry),
            Action::ViewBuffers => self.set_view_mode(ViewMode::Buffers),
            Action::ToggleZen => {
                self.zen_mode = !self.zen_mode;
                if self.zen_mode {
                    self.focus = FocusPane::Inspector;
                }
                self.set_status(
                    StatusLevel::Info,
                    if self.zen_mode {
                        "Zen Viewport enabled — press v or 0 to restore sidebar"
                    } else {
                        "Split Cyber-Deck layout restored"
                    },
                );
            }
            Action::CycleTheme => {
                self.theme_id = self.theme_id.next();
                let _ = save_theme(self.theme_id);
                self.set_status(
                    StatusLevel::Success,
                    format!("Theme switched to {}", self.theme_id.name()),
                );
            }
            Action::CycleSort => {
                self.sort_mode = self.sort_mode.next();
                self.rebuild_items();
                self.set_status(
                    StatusLevel::Info,
                    format!("Topology sort order: {}", self.sort_mode.label()),
                );
            }
            Action::ToggleFavoritesOnly => {
                self.favorites_only = !self.favorites_only;
                self.rebuild_items();
                self.set_status(
                    StatusLevel::Info,
                    if self.favorites_only {
                        "Showing starred favorites only (press F to show all)"
                    } else {
                        "Showing all targets"
                    },
                );
            }
            Action::TogglePause => {
                self.paused = !self.paused;
                self.set_status(
                    if self.paused {
                        StatusLevel::Warning
                    } else {
                        StatusLevel::Success
                    },
                    if self.paused {
                        "Auto-refresh PAUSED — press P to resume or R for manual refresh"
                    } else {
                        "Live telemetry & tmux auto-refresh RESUMED"
                    },
                );
            }
            Action::ScrollDetailsDown => self.scroll_details(6),
            Action::ScrollDetailsUp => self.scroll_details(-6),
            Action::ResetView => {
                self.zen_mode = false;
                self.view_mode = ViewMode::Deck;
                self.focus = FocusPane::Tree;
                self.detail_scroll = 0;
                self.buffer_scroll = 0;
                self.set_status(StatusLevel::Info, "Viewports and scroll offsets reset");
            }
            Action::AttachOrSwitch => self.open_selected(),
            Action::NewSession => self.open_new_session_prompt(),
            Action::LaunchSession => self.open_launch_session_prompt(),
            Action::DetachClient => self.detach_client(),
            Action::NewWindow => self.open_new_window_prompt(),
            Action::NewWindowCustom => self.open_launch_window_prompt(),
            Action::OpenLayoutPicker => self.open_layout_picker(),
            Action::ToggleSyncPanes => self.toggle_sync_panes(),
            Action::RotatePanes => self.rotate_panes(),
            Action::SwapNext => self.swap_selected(true),
            Action::SwapPrev => self.swap_selected(false),
            Action::SplitHorizontal => self.split_selected(SplitDirection::Horizontal),
            Action::SplitVertical => self.split_selected(SplitDirection::Vertical),
            Action::ToggleZoom => self.zoom_selected(),
            Action::BreakPane => self.break_selected_pane(),
            Action::JoinPane => self.open_join_pane_prompt(),
            Action::ToggleMarkPane => self.toggle_mark_selected_pane(),
            Action::SwapMarkedPane => self.swap_with_marked_pane(),
            Action::ResizeLeft => self.resize_selected_pane(ResizeDirection::Left, 5),
            Action::ResizeRight => self.resize_selected_pane(ResizeDirection::Right, 5),
            Action::ResizeUp => self.resize_selected_pane(ResizeDirection::Up, 3),
            Action::ResizeDown => self.resize_selected_pane(ResizeDirection::Down, 3),
            Action::ClearPane => self.clear_selected_pane(),
            Action::RespawnPane => self.respawn_selected_pane(),
            Action::Rename => self.open_rename_prompt(),
            Action::Kill => self.open_kill_confirm(),
            Action::ToggleFavorite => self.toggle_favorite(),
            Action::SendKeys => self.open_send_keys_prompt(),
            Action::BroadcastKeys => self.open_broadcast_keys_prompt(),
            Action::CopyPaneBuffer => self.copy_selected_pane(),
            Action::CopySystemClipboard => self.copy_selected_pane_clipboard(),
            Action::SavePaneSnapshot => self.open_save_snapshot_prompt(),
            Action::OpenPalette => {
                self.palette = Some(PaletteState::new());
            }
            Action::Filter => self.open_filter_prompt(),
            Action::ClearFilter => self.clear_filter(),
            Action::Refresh => self.refresh(),
            Action::RunCommand => self.open_command_prompt(),
            Action::Diagnostics => self.open_diagnostics(),
            Action::Help => self.open_help(),
            Action::Quit => {
                if self.zen_mode {
                    self.zen_mode = false;
                    self.set_status(StatusLevel::Info, "Exited Zen mode");
                } else {
                    self.should_quit = true;
                }
            }
        }
        Ok(())
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.has_modal_open() {
            match mouse.kind {
                MouseEventKind::ScrollDown => {
                    if self.show_help {
                        self.scroll_help(3);
                    } else if self.show_diagnostics {
                        self.scroll_diagnostics(3);
                    } else if let Some(palette) = self.palette.as_mut() {
                        let count = palette.filtered_entries().len();
                        if count > 0 {
                            palette.selected = (palette.selected + 1).min(count - 1);
                        }
                    } else if self.quick_jump.is_some() {
                        let count = self.quick_jump_matches().len();
                        if let Some(jump) = self.quick_jump.as_mut() {
                            if count > 0 {
                                jump.selected = (jump.selected + 1).min(count - 1);
                            }
                        }
                    }
                }
                MouseEventKind::ScrollUp => {
                    if self.show_help {
                        self.scroll_help(-3);
                    } else if self.show_diagnostics {
                        self.scroll_diagnostics(-3);
                    } else if let Some(palette) = self.palette.as_mut() {
                        palette.selected = palette.selected.saturating_sub(1);
                    } else if let Some(jump) = self.quick_jump.as_mut() {
                        jump.selected = jump.selected.saturating_sub(1);
                    }
                }
                _ => {}
            }
            return;
        }

        let regions = self.regions.borrow().clone();
        let col = mouse.column;
        let row = mouse.row;

        match mouse.kind {
            MouseEventKind::ScrollDown => {
                if rect_contains(regions.tree_inner, col, row) && !self.zen_mode {
                    self.move_selection(2);
                } else if rect_contains(regions.inspector_area, col, row) {
                    match self.view_mode {
                        ViewMode::Buffers => {
                            let max = self.state.buffers.len().saturating_sub(1);
                            self.buffer_selected = (self.buffer_selected + 1).min(max);
                            self.buffer_scroll = 0;
                        }
                        ViewMode::Telemetry => {
                            let max = self.telemetry_rows().len().saturating_sub(1);
                            self.telemetry_selected = (self.telemetry_selected + 1).min(max);
                        }
                        _ => self.scroll_details(3),
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                if rect_contains(regions.tree_inner, col, row) && !self.zen_mode {
                    self.move_selection(-2);
                } else if rect_contains(regions.inspector_area, col, row) {
                    match self.view_mode {
                        ViewMode::Buffers => {
                            self.buffer_selected = self.buffer_selected.saturating_sub(1);
                            self.buffer_scroll = 0;
                        }
                        ViewMode::Telemetry => {
                            self.telemetry_selected = self.telemetry_selected.saturating_sub(1);
                        }
                        _ => self.scroll_details(-3),
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(theme_rect) = regions.theme_badge_rect {
                    if rect_contains(theme_rect, col, row) {
                        let _ = self.execute_action(Action::CycleTheme);
                        return;
                    }
                }
                if let Some(live_rect) = regions.live_badge_rect {
                    if rect_contains(live_rect, col, row) {
                        let _ = self.execute_action(Action::TogglePause);
                        return;
                    }
                }
                for (tab_rect, mode) in &regions.tab_rects {
                    if rect_contains(*tab_rect, col, row) {
                        self.set_view_mode(*mode);
                        return;
                    }
                }
                if !self.zen_mode && rect_contains(regions.tree_inner, col, row) {
                    self.focus = FocusPane::Tree;
                    let clicked_idx = regions.tree_scroll_offset
                        + (row.saturating_sub(regions.tree_inner.y)) as usize;
                    if clicked_idx < self.items.len() {
                        let now = Instant::now();
                        let is_double_click = self.last_click.is_some_and(|(t, idx)| {
                            idx == clicked_idx && now.duration_since(t) < Duration::from_millis(450)
                        });
                        self.select_index(clicked_idx);
                        self.last_click = Some((now, clicked_idx));
                        if is_double_click {
                            if self.items[clicked_idx].kind == TreeItemKind::Pane {
                                self.open_selected();
                            } else {
                                self.toggle_selected();
                            }
                        }
                    }
                    return;
                }
                for (pane_rect, target) in &regions.preview_pane_rects {
                    if rect_contains(*pane_rect, col, row) {
                        self.jump_to_target(target);
                        return;
                    }
                }
                if rect_contains(regions.inspector_area, col, row) {
                    self.focus = FocusPane::Inspector;
                }
            }
            _ => {}
        }
    }

    fn handle_buffers_focused_key(&mut self, key: KeyEvent) -> bool {
        let count = self.state.buffers.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if count > 0 {
                    self.buffer_selected = (self.buffer_selected + 1).min(count - 1);
                    self.buffer_scroll = 0;
                }
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.buffer_selected = self.buffer_selected.saturating_sub(1);
                self.buffer_scroll = 0;
                true
            }
            KeyCode::Char(']') => {
                self.buffer_scroll = self.buffer_scroll.saturating_add(4);
                true
            }
            KeyCode::Char('[') => {
                self.buffer_scroll = self.buffer_scroll.saturating_sub(4);
                true
            }
            KeyCode::Enter => {
                self.paste_selected_buffer();
                true
            }
            KeyCode::Char('Y') => {
                if let Some(buffer) = self.state.buffers.get(self.buffer_selected) {
                    match copy_to_system_clipboard(&buffer.content) {
                        Ok(tool) => self.set_status(
                            StatusLevel::Success,
                            format!("Copied buffer {} via {tool}", buffer.name),
                        ),
                        Err(err) => self.set_status(StatusLevel::Error, err.to_string()),
                    }
                }
                true
            }
            KeyCode::Char('x') => {
                if let Some(buffer) = self.state.buffers.get(self.buffer_selected).cloned() {
                    self.confirm = Some(Confirm {
                        message: format!(
                            "Delete tmux buffer {} ({} bytes)? Press y to confirm or n/Esc to cancel.",
                            buffer.name, buffer.size
                        ),
                        target: ConfirmAction::DeleteBuffer { buffer },
                    });
                }
                true
            }
            _ => false,
        }
    }

    fn handle_telemetry_focused_key(&mut self, key: KeyEvent) -> bool {
        let rows = self.telemetry_rows();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if !rows.is_empty() {
                    self.telemetry_selected = (self.telemetry_selected + 1).min(rows.len() - 1);
                }
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.telemetry_selected = self.telemetry_selected.saturating_sub(1);
                true
            }
            KeyCode::Enter => {
                if let Some(row) = rows.get(self.telemetry_selected) {
                    self.jump_to_target(&row.target);
                    self.view_mode = ViewMode::Deck;
                    self.focus = FocusPane::Tree;
                }
                true
            }
            _ => false,
        }
    }

    fn handle_palette_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.palette = None;
            }
            KeyCode::Enter => {
                let selected_action = self.palette.as_ref().and_then(|palette| {
                    let entries = palette.filtered_entries();
                    entries.get(palette.selected).map(|entry| entry.action)
                });
                self.palette = None;
                if let Some(action) = selected_action {
                    self.execute_action(action)?;
                }
            }
            KeyCode::Down => {
                if let Some(palette) = self.palette.as_mut() {
                    let count = palette.filtered_entries().len();
                    if count > 0 {
                        palette.selected = (palette.selected + 1) % count;
                    }
                }
            }
            KeyCode::Up => {
                if let Some(palette) = self.palette.as_mut() {
                    let count = palette.filtered_entries().len();
                    if count > 0 {
                        palette.selected = if palette.selected == 0 {
                            count - 1
                        } else {
                            palette.selected - 1
                        };
                    }
                }
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(palette) = self.palette.as_mut() {
                    let count = palette.filtered_entries().len();
                    if count > 0 {
                        palette.selected = (palette.selected + 1) % count;
                    }
                }
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(palette) = self.palette.as_mut() {
                    let count = palette.filtered_entries().len();
                    if count > 0 {
                        palette.selected = if palette.selected == 0 {
                            count - 1
                        } else {
                            palette.selected - 1
                        };
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.input.delete_backward();
                    palette.selected = 0;
                }
            }
            KeyCode::Delete => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.input.delete_forward();
                    palette.selected = 0;
                }
            }
            KeyCode::Left => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.input.move_left();
                }
            }
            KeyCode::Right => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.input.move_right();
                }
            }
            KeyCode::Home => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.input.move_home();
                }
            }
            KeyCode::End => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.input.move_end();
                }
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.input.move_home();
                }
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.input.move_end();
                }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.input.clear();
                    palette.selected = 0;
                }
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.input.insert_char(ch);
                    palette.selected = 0;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_quick_jump_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.quick_jump = None,
            KeyCode::Enter => {
                let target = self.quick_jump.as_ref().and_then(|state| {
                    self.quick_jump_matches()
                        .get(state.selected)
                        .map(|candidate| candidate.target.clone())
                });
                self.quick_jump = None;
                if let Some(target) = target {
                    self.jump_to_target(&target);
                }
            }
            KeyCode::Down => {
                let count = self.quick_jump_matches().len();
                if let Some(state) = self.quick_jump.as_mut() {
                    if count > 0 {
                        state.selected = (state.selected + 1) % count;
                    }
                }
            }
            KeyCode::Up => {
                let count = self.quick_jump_matches().len();
                if let Some(state) = self.quick_jump.as_mut() {
                    if count > 0 {
                        state.selected = if state.selected == 0 {
                            count - 1
                        } else {
                            state.selected - 1
                        };
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(state) = self.quick_jump.as_mut() {
                    state.input.delete_backward();
                    state.selected = 0;
                }
            }
            KeyCode::Delete => {
                if let Some(state) = self.quick_jump.as_mut() {
                    state.input.delete_forward();
                    state.selected = 0;
                }
            }
            KeyCode::Left => {
                if let Some(state) = self.quick_jump.as_mut() {
                    state.input.move_left();
                }
            }
            KeyCode::Right => {
                if let Some(state) = self.quick_jump.as_mut() {
                    state.input.move_right();
                }
            }
            KeyCode::Home => {
                if let Some(state) = self.quick_jump.as_mut() {
                    state.input.move_home();
                }
            }
            KeyCode::End => {
                if let Some(state) = self.quick_jump.as_mut() {
                    state.input.move_end();
                }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(state) = self.quick_jump.as_mut() {
                    state.input.clear();
                    state.selected = 0;
                }
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(state) = self.quick_jump.as_mut() {
                    state.input.insert_char(ch);
                    state.selected = 0;
                }
            }
            _ => {}
        }
    }

    fn handle_layout_picker_key(&mut self, key: KeyEvent) {
        let count = TmuxLayout::ALL.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('L') => {
                self.layout_picker = None;
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Right | KeyCode::Char('l') => {
                if let Some(picker) = self.layout_picker.as_mut() {
                    picker.selected = (picker.selected + 1) % count;
                }
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Left | KeyCode::Char('h') => {
                if let Some(picker) = self.layout_picker.as_mut() {
                    picker.selected = if picker.selected == 0 {
                        count - 1
                    } else {
                        picker.selected - 1
                    };
                }
            }
            KeyCode::Char(d @ '1'..='5') => {
                let idx = (d as u8 - b'1') as usize;
                if let Some(picker) = self.layout_picker.take() {
                    let layout = TmuxLayout::ALL[idx];
                    self.apply_tmux_result(
                        &format!("Applied {} layout", layout.tmux_name()),
                        self.client.select_layout(&picker.target, layout),
                    );
                    self.refresh();
                }
            }
            KeyCode::Enter => {
                if let Some(picker) = self.layout_picker.take() {
                    let layout = TmuxLayout::ALL[picker.selected];
                    self.apply_tmux_result(
                        &format!("Applied {} layout", layout.tmux_name()),
                        self.client.select_layout(&picker.target, layout),
                    );
                    self.refresh();
                }
            }
            _ => {}
        }
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

    fn handle_diagnostics_key(&mut self, key: KeyEvent) {
        if matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('D')
        ) {
            self.show_diagnostics = false;
            self.diagnostics_scroll = 0;
            return;
        }

        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.scroll_diagnostics(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_diagnostics(-1),
            KeyCode::PageDown => self.scroll_diagnostics(8),
            KeyCode::PageUp => self.scroll_diagnostics(-8),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_diagnostics(8)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_diagnostics(-8)
            }
            KeyCode::Home | KeyCode::Char('g') => self.diagnostics_scroll = 0,
            KeyCode::Char('R') => self.refresh(),
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
                    prompt.delete_backward();
                }
            }
            KeyCode::Delete => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.delete_forward();
                }
            }
            KeyCode::Left => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.move_left();
                }
            }
            KeyCode::Right => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.move_right();
                }
            }
            KeyCode::Home => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.move_home();
                }
            }
            KeyCode::End => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.move_end();
                }
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.move_home();
                }
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.move_end();
                }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.clear();
                }
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.insert_char(character);
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
                    self.apply_confirm(confirm.target);
                }
            }
            _ => {}
        }
    }

    fn set_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
        self.detail_scroll = 0;
        self.buffer_scroll = 0;
        if matches!(mode, ViewMode::Buffers | ViewMode::Telemetry) {
            self.focus = FocusPane::Inspector;
        }
        self.set_status(StatusLevel::Info, format!("Active deck: {}", mode.label()));
    }

    fn set_status(&mut self, level: StatusLevel, message: impl Into<String>) {
        self.status_level = level;
        self.status = message.into();
    }

    pub fn refresh(&mut self) {
        match self.client.load() {
            Ok(state) => {
                let pane_pids = pane_pids(&state);
                self.metrics = self
                    .metrics_sampler
                    .sample(pane_pids.iter().map(String::as_str));
                self.state = state;
                self.rebuild_items();
                if self.state.buffers.is_empty() {
                    self.buffer_selected = 0;
                } else {
                    self.buffer_selected = self
                        .buffer_selected
                        .min(self.state.buffers.len().saturating_sub(1));
                }
                if let Some(notice) = self.state.notice.clone() {
                    self.set_status(StatusLevel::Warning, notice);
                } else {
                    let msg = self.count_status("Synced");
                    self.set_status(StatusLevel::Info, msg);
                }
            }
            Err(error) => {
                self.set_status(StatusLevel::Error, format!("Refresh failed: {error}"));
            }
        }
        self.last_refresh = Instant::now();
    }

    fn rebuild_items(&mut self) {
        let previous_key = self
            .items
            .get(self.selected)
            .map(|item| item.favorite_key.clone());

        let mut items = Vec::new();
        let filter = self.filter.trim().to_lowercase();
        let mut sessions: Vec<&Session> = self.state.sessions.iter().collect();
        self.sort_sessions(&mut sessions);

        for session in sessions {
            if filter.is_empty() && !self.favorites_only {
                self.push_unfiltered_session(&mut items, session);
            } else {
                self.push_filtered_session(&mut items, session, &filter);
            }
        }

        self.items = items;
        if self.items.is_empty() {
            self.selected = 0;
            self.detail_scroll = 0;
            return;
        }

        if let Some(key) = previous_key {
            if let Some(pos) = self.items.iter().position(|item| item.favorite_key == key) {
                self.selected = pos;
                return;
            }
        }
        self.selected = self.selected.min(self.items.len() - 1);
        self.detail_scroll = 0;
    }

    fn sort_sessions(&self, sessions: &mut Vec<&Session>) {
        sessions.sort_by(|left, right| {
            let fav_cmp = self
                .is_session_favorite(right)
                .cmp(&self.is_session_favorite(left));
            if fav_cmp != std::cmp::Ordering::Equal {
                return fav_cmp;
            }
            match self.sort_mode {
                SortMode::Name => left.name.cmp(&right.name),
                SortMode::CpuDesc => {
                    let l_cpu = self.session_cpu(left);
                    let r_cpu = self.session_cpu(right);
                    r_cpu
                        .total_cmp(&l_cpu)
                        .then_with(|| left.name.cmp(&right.name))
                }
                SortMode::MemoryDesc => {
                    let l_mem = self.session_memory(left);
                    let r_mem = self.session_memory(right);
                    r_mem.cmp(&l_mem).then_with(|| left.name.cmp(&right.name))
                }
                SortMode::ActiveFirst => right
                    .attached
                    .cmp(&left.attached)
                    .then_with(|| right.activity.cmp(&left.activity))
                    .then_with(|| left.name.cmp(&right.name)),
            }
        });
    }

    fn session_cpu(&self, session: &Session) -> f32 {
        session
            .windows
            .iter()
            .flat_map(|w| &w.panes)
            .filter_map(|p| self.metrics.panes.get(&p.pid))
            .map(|m| m.cpu_usage)
            .sum()
    }

    fn session_memory(&self, session: &Session) -> u64 {
        session
            .windows
            .iter()
            .flat_map(|w| &w.panes)
            .filter_map(|p| self.metrics.panes.get(&p.pid))
            .map(|m| m.memory_bytes)
            .sum()
    }

    fn push_unfiltered_session(&self, items: &mut Vec<TreeItem>, session: &Session) {
        items.push(self.decorate_item(
            session_item(session),
            Some(self.expanded_sessions.contains(&session.id)),
        ));
        if !self.expanded_sessions.contains(&session.id) {
            return;
        }

        let mut windows: Vec<&Window> = session.windows.iter().collect();
        windows.sort_by(|left, right| {
            self.is_window_favorite(session, right)
                .cmp(&self.is_window_favorite(session, left))
                .then_with(|| left.index.cmp(&right.index))
        });

        for window in windows {
            items.push(self.decorate_item(
                window_item(session, window),
                Some(self.expanded_windows.contains(&window.id)),
            ));
            if !self.expanded_windows.contains(&window.id) {
                continue;
            }

            let mut panes: Vec<&Pane> = window.panes.iter().collect();
            panes.sort_by(|left, right| {
                self.is_pane_favorite(session, window, right)
                    .cmp(&self.is_pane_favorite(session, window, left))
                    .then_with(|| left.index.cmp(&right.index))
            });

            for pane in panes {
                items.push(self.decorate_item(pane_item(session, window, pane), None));
            }
        }
    }

    fn push_filtered_session(&self, items: &mut Vec<TreeItem>, session: &Session, filter: &str) {
        let session_item = self.decorate_item(session_item(session), Some(true));
        let session_match = self.item_matches(&session_item, filter);
        let mut block = Vec::new();

        let mut windows: Vec<&Window> = session.windows.iter().collect();
        windows.sort_by(|left, right| {
            self.is_window_favorite(session, right)
                .cmp(&self.is_window_favorite(session, left))
                .then_with(|| left.index.cmp(&right.index))
        });

        for window in windows {
            let window_item = self.decorate_item(window_item(session, window), Some(true));
            let window_match = self.item_matches(&window_item, filter);
            let mut pane_block = Vec::new();

            let mut panes: Vec<&Pane> = window.panes.iter().collect();
            panes.sort_by(|left, right| {
                self.is_pane_favorite(session, window, right)
                    .cmp(&self.is_pane_favorite(session, window, left))
                    .then_with(|| left.index.cmp(&right.index))
            });

            for pane in panes {
                let pane_item = self.decorate_item(pane_item(session, window, pane), None);
                let include = if self.favorites_only {
                    self.item_matches(&pane_item, filter)
                } else {
                    session_match || window_match || self.item_matches(&pane_item, filter)
                };
                if include {
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

    fn item_matches(&self, item: &TreeItem, filter: &str) -> bool {
        if self.favorites_only && !item.favorite {
            return false;
        }
        if filter.is_empty() {
            return true;
        }
        matches_filter(item, filter)
    }

    fn decorate_item(&self, mut item: TreeItem, expanded: Option<bool>) -> TreeItem {
        item.expanded = expanded;
        item.favorite = self.favorites.contains(&item.favorite_key);
        if item.favorite {
            item.label = format!("* {}", item.label);
        }
        self.append_resource_details(&mut item);
        item.details
            .push(("Favorite".into(), item.favorite.to_string()));
        item
    }

    fn is_session_favorite(&self, session: &Session) -> bool {
        let target = TmuxTarget::session_on(session.id.clone(), session.server.clone());
        self.favorites.contains(&target.favorite_key())
    }

    fn is_window_favorite(&self, session: &Session, window: &Window) -> bool {
        let target = TmuxTarget::window_on(
            session.id.clone(),
            window.id.clone(),
            session.server.clone(),
        );
        self.favorites.contains(&target.favorite_key())
    }

    fn is_pane_favorite(&self, session: &Session, window: &Window, pane: &Pane) -> bool {
        let target = TmuxTarget::pane_on(
            session.id.clone(),
            window.id.clone(),
            pane.id.clone(),
            session.server.clone(),
        );
        self.favorites.contains(&target.favorite_key())
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

    fn jump_to_target(&mut self, target: &TmuxTarget) {
        self.expanded_sessions.insert(target.session_id.clone());
        if let Some(window_id) = &target.window_id {
            self.expanded_windows.insert(window_id.clone());
        }
        self.rebuild_items();
        let key = target.favorite_key();
        if let Some(pos) = self.items.iter().position(|item| item.favorite_key == key) {
            self.select_index(pos);
            self.set_status(
                StatusLevel::Success,
                format!("Jumped to {}", self.items[pos].name),
            );
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

    fn scroll_diagnostics(&mut self, delta: i16) {
        let next = self.diagnostics_scroll as i32 + delta as i32;
        self.diagnostics_scroll = next.clamp(0, u16::MAX as i32) as u16;
    }

    fn open_help(&mut self) {
        self.show_help = true;
        self.help_scroll = 0;
        self.set_status(
            StatusLevel::Info,
            "Shortcuts Atlas open — scroll with j/k or PgUp/PgDn; close with ?/F1/q/Esc.",
        );
    }

    fn open_diagnostics(&mut self) {
        self.show_diagnostics = true;
        self.diagnostics_scroll = 0;
        self.set_status(
            StatusLevel::Info,
            "Diagnostics Deck open — press R to re-probe sockets, D/q/Esc to close.",
        );
    }

    fn toggle_selected(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            return;
        };

        match item.kind {
            TreeItemKind::Session => {
                toggle_set(&mut self.expanded_sessions, item.target.session_id)
            }
            TreeItemKind::Window => {
                if let Some(window_id) = item.target.window_id {
                    toggle_set(&mut self.expanded_windows, window_id);
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
                self.expanded_sessions.remove(&item.target.session_id);
            }
            TreeItemKind::Window => {
                if let Some(window_id) = item.target.window_id {
                    if self.expanded_windows.remove(&window_id) {
                        self.rebuild_items();
                        return;
                    }
                }
                // Already collapsed window: jump selection up to parent session
                let parent_session = item.target.session_id.clone();
                if let Some(pos) = self.items.iter().position(|row| {
                    row.kind == TreeItemKind::Session && row.target.session_id == parent_session
                }) {
                    self.select_index(pos);
                }
                return;
            }
            TreeItemKind::Pane => {
                // Jump selection up to parent window
                if let Some(window_id) = item.target.window_id {
                    if let Some(pos) = self.items.iter().position(|row| {
                        row.kind == TreeItemKind::Window
                            && row.target.window_id.as_deref() == Some(window_id.as_str())
                    }) {
                        self.select_index(pos);
                    }
                }
                return;
            }
        }
        self.rebuild_items();
    }

    fn expand_selected(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            return;
        };

        match item.kind {
            TreeItemKind::Session => {
                self.expanded_sessions.insert(item.target.session_id);
            }
            TreeItemKind::Window => {
                if let Some(window_id) = item.target.window_id {
                    self.expanded_windows.insert(window_id);
                }
            }
            TreeItemKind::Pane => {}
        }
        self.rebuild_items();
    }

    fn expand_all(&mut self) {
        for session in &self.state.sessions {
            self.expanded_sessions.insert(session.id.clone());
            for window in &session.windows {
                self.expanded_windows.insert(window.id.clone());
            }
        }
        self.rebuild_items();
        self.set_status(StatusLevel::Info, "Expanded full topology tree");
    }

    fn collapse_all(&mut self) {
        self.expanded_sessions.clear();
        self.expanded_windows.clear();
        self.rebuild_items();
        self.set_status(StatusLevel::Info, "Collapsed all sessions and windows");
    }

    fn open_selected(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            self.set_status(StatusLevel::Warning, "No tmux target selected");
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
        self.prompt = Some(Prompt::new(
            "Fuzzy Topology Filter".into(),
            self.filter.clone(),
            "Multi-token fuzzy match across names, commands, paths, IDs, servers · Ctrl+U clear"
                .into(),
            PromptKind::Filter,
        ));
    }

    fn clear_filter(&mut self) {
        self.filter.clear();
        self.favorites_only = false;
        self.rebuild_items();
        let msg = self.count_status("Filter cleared");
        self.set_status(StatusLevel::Info, msg);
    }

    fn open_new_session_prompt(&mut self) {
        self.prompt = Some(Prompt::new(
            "New Session".into(),
            "".into(),
            "Enter a tmux session name · Enter to create · Esc to cancel".into(),
            PromptKind::NewSession,
        ));
    }

    fn open_launch_session_prompt(&mut self) {
        self.prompt = Some(Prompt::new(
            "Launch Session Preset".into(),
            "".into(),
            "Format: name | start-dir | command (only name is required)".into(),
            PromptKind::LaunchSession,
        ));
    }

    fn open_new_window_prompt(&mut self) {
        let Some(item) = self.selected_item() else {
            self.set_status(
                StatusLevel::Warning,
                "Select a session/window/pane before creating a window",
            );
            return;
        };

        self.prompt = Some(Prompt::new(
            "New Window".into(),
            "".into(),
            "Enter a window name".into(),
            PromptKind::NewWindow {
                target: item.target.clone(),
            },
        ));
    }

    fn open_launch_window_prompt(&mut self) {
        let Some(item) = self.selected_item() else {
            self.set_status(
                StatusLevel::Warning,
                "Select a session/window/pane before launching a window",
            );
            return;
        };

        self.prompt = Some(Prompt::new(
            "Launch Window Preset".into(),
            "".into(),
            "Format: name | start-dir | command (only name is required)".into(),
            PromptKind::LaunchWindow {
                target: item.target.clone(),
            },
        ));
    }

    fn open_layout_picker(&mut self) {
        let Some((target, label)) = self.selected_window_target_with_label() else {
            self.set_status(
                StatusLevel::Warning,
                "Select a window or pane to change its tmux layout",
            );
            return;
        };
        self.layout_picker = Some(LayoutPickerState {
            selected: 0,
            target,
            window_label: label,
        });
    }

    fn toggle_sync_panes(&mut self) {
        let Some((target, label)) = self.selected_window_target_with_label() else {
            self.set_status(
                StatusLevel::Warning,
                "Select a window or pane to toggle synchronize-panes",
            );
            return;
        };
        self.apply_tmux_result(
            &format!("Toggled synchronize-panes on {label}"),
            self.client.toggle_sync_panes(&target),
        );
        self.refresh();
    }

    fn rotate_panes(&mut self) {
        let Some((target, label)) = self.selected_window_target_with_label() else {
            self.set_status(
                StatusLevel::Warning,
                "Select a window or pane to rotate panes",
            );
            return;
        };
        self.apply_tmux_result(
            &format!("Rotated panes in {label}"),
            self.client.rotate_window(&target),
        );
        self.refresh();
    }

    fn swap_selected(&mut self, next: bool) {
        let Some(item) = self.selected_item().cloned() else {
            self.set_status(StatusLevel::Warning, "Select a window or pane to swap");
            return;
        };
        let dir = if next { "forward" } else { "backward" };
        match item.kind {
            TreeItemKind::Pane => {
                self.apply_tmux_result(
                    &format!("Swapped pane {dir}"),
                    self.client.swap_pane(&item.target, next),
                );
                self.refresh();
            }
            TreeItemKind::Window => {
                self.apply_tmux_result(
                    &format!("Swapped window {dir}"),
                    self.client.swap_window(&item.target, next),
                );
                self.refresh();
            }
            TreeItemKind::Session => {
                self.set_status(
                    StatusLevel::Warning,
                    "Select a window or pane to swap positions",
                );
            }
        }
    }

    fn split_selected(&mut self, direction: SplitDirection) {
        let Some(target) = self.selected_pane_target() else {
            self.set_status(StatusLevel::Warning, "Select a pane before splitting");
            return;
        };

        let label = match direction {
            SplitDirection::Horizontal => "Split pane horizontally",
            SplitDirection::Vertical => "Split pane vertically",
        };
        self.apply_tmux_result(label, self.client.split_pane(&target, direction));
        self.refresh();
    }

    fn break_selected_pane(&mut self) {
        let Some(target) = self.selected_pane_target() else {
            self.set_status(
                StatusLevel::Warning,
                "Select a pane to break into a new window",
            );
            return;
        };
        self.apply_tmux_result(
            "Broke pane into new window",
            self.client.break_pane(&target),
        );
        self.refresh();
    }

    fn open_join_pane_prompt(&mut self) {
        let Some(source) = self.selected_pane_target() else {
            self.set_status(
                StatusLevel::Warning,
                "Select a pane to join into another window",
            );
            return;
        };
        self.prompt = Some(Prompt::new(
            "Join Pane Into Target".into(),
            "".into(),
            "Enter target window/session (e.g. dev:1 or @2 or %4)".into(),
            PromptKind::JoinPane { source },
        ));
    }

    fn toggle_mark_selected_pane(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            self.set_status(StatusLevel::Warning, "Select a pane to mark/unmark");
            return;
        };
        if item.target.pane_id.is_none() {
            self.set_status(StatusLevel::Warning, "Select a pane to mark/unmark");
            return;
        }
        let label = if item.marked {
            "Cleared pane mark"
        } else {
            "Marked pane"
        };
        self.apply_tmux_result(
            label,
            self.client.toggle_mark_pane(&item.target, item.marked),
        );
        self.refresh();
    }

    fn swap_with_marked_pane(&mut self) {
        let Some(target) = self.selected_pane_target() else {
            self.set_status(
                StatusLevel::Warning,
                "Select a pane to swap with the marked pane",
            );
            return;
        };
        self.apply_tmux_result(
            "Swapped with marked pane",
            self.client.swap_marked_pane(&target),
        );
        self.refresh();
    }

    fn resize_selected_pane(&mut self, direction: ResizeDirection, amount: u16) {
        let Some(target) = self.selected_pane_target() else {
            self.set_status(StatusLevel::Warning, "Select a pane to resize");
            return;
        };
        self.apply_tmux_result(
            "Resized pane",
            self.client.resize_pane(&target, direction, amount),
        );
        self.refresh();
    }

    fn clear_selected_pane(&mut self) {
        let Some(target) = self.selected_pane_target() else {
            self.set_status(
                StatusLevel::Warning,
                "Select a pane to clear screen/history",
            );
            return;
        };
        self.apply_tmux_result(
            "Cleared pane screen & history",
            self.client.clear_pane(&target),
        );
        self.refresh();
    }

    fn respawn_selected_pane(&mut self) {
        let Some(target) = self.selected_pane_target() else {
            self.set_status(StatusLevel::Warning, "Select a pane to respawn");
            return;
        };
        self.apply_tmux_result("Respawned pane", self.client.respawn_pane(&target));
        self.refresh();
    }

    fn open_rename_prompt(&mut self) {
        let Some(item) = self.selected_item() else {
            self.set_status(StatusLevel::Warning, "No item selected to rename");
            return;
        };

        let (title, value, kind) = match item.kind {
            TreeItemKind::Session => (
                "Rename Session",
                item.rename_value.clone(),
                PromptKind::Rename(RenameTarget::Session {
                    target: item.target.clone(),
                }),
            ),
            TreeItemKind::Window => (
                "Rename Window",
                item.rename_value.clone(),
                PromptKind::Rename(RenameTarget::Window {
                    target: item.target.clone(),
                }),
            ),
            TreeItemKind::Pane => (
                "Set Pane Title",
                item.rename_value.clone(),
                PromptKind::Rename(RenameTarget::Pane {
                    target: item.target.clone(),
                }),
            ),
        };

        self.prompt = Some(Prompt::new(
            title.into(),
            value,
            "Left/Right move caret · Ctrl+A/E edges · Ctrl+U clear · Enter apply".into(),
            kind,
        ));
    }

    fn open_kill_confirm(&mut self) {
        let Some(item) = self.selected_item() else {
            self.set_status(StatusLevel::Warning, "No item selected to kill");
            return;
        };

        let (label, target) = match item.kind {
            TreeItemKind::Session => (
                format!("Kill session {}?", item.label),
                ConfirmAction::KillSession {
                    target: item.target.clone(),
                },
            ),
            TreeItemKind::Window => (
                format!("Kill window {}?", item.label),
                ConfirmAction::KillWindow {
                    target: item.target.clone(),
                },
            ),
            TreeItemKind::Pane => (
                format!("Kill pane {}?", item.label),
                ConfirmAction::KillPane {
                    target: item.target.clone(),
                },
            ),
        };

        self.confirm = Some(Confirm {
            message: format!("{label} Press y to confirm or n/Esc to cancel."),
            target,
        });
    }

    fn zoom_selected(&mut self) {
        let Some(target) = self.selected_pane_target() else {
            self.set_status(StatusLevel::Warning, "Select a pane before toggling zoom");
            return;
        };
        self.apply_tmux_result("Toggled pane zoom", self.client.toggle_zoom(&target));
        self.refresh();
    }

    fn open_send_keys_prompt(&mut self) {
        let Some(target) = self.selected_pane_target() else {
            self.set_status(StatusLevel::Warning, "Select a pane before sending keys");
            return;
        };

        self.prompt = Some(Prompt::new(
            "Send Keys to Pane".into(),
            "".into(),
            "Command text is sent to the selected pane followed by Enter".into(),
            PromptKind::SendKeys { target },
        ));
    }

    fn open_broadcast_keys_prompt(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            self.set_status(
                StatusLevel::Warning,
                "Select a session, window, or pane to broadcast keys",
            );
            return;
        };
        let (targets, scope) = self.broadcast_targets_for(&item.target);
        if targets.is_empty() {
            self.set_status(StatusLevel::Warning, "No panes found in selected scope");
            return;
        }
        self.prompt = Some(Prompt::new(
            format!("Broadcast Keys → {scope} ({} panes)", targets.len()),
            "".into(),
            "Sent + Enter simultaneously to every pane in this scope".into(),
            PromptKind::BroadcastKeys { targets, scope },
        ));
    }

    fn broadcast_targets_for(&self, target: &TmuxTarget) -> (Vec<TmuxTarget>, String) {
        let mut out = Vec::new();
        for session in &self.state.sessions {
            if session.id != target.session_id || session.server.label() != target.server_label() {
                continue;
            }
            if let Some(window_id) = &target.window_id {
                if let Some(window) = session.windows.iter().find(|w| w.id == *window_id) {
                    for pane in &window.panes {
                        out.push(TmuxTarget::pane_on(
                            session.id.clone(),
                            window.id.clone(),
                            pane.id.clone(),
                            session.server.clone(),
                        ));
                    }
                    return (out, format!("window {}:{}", window.index, window.name));
                }
            } else {
                for window in &session.windows {
                    for pane in &window.panes {
                        out.push(TmuxTarget::pane_on(
                            session.id.clone(),
                            window.id.clone(),
                            pane.id.clone(),
                            session.server.clone(),
                        ));
                    }
                }
                return (out, format!("session {}", session.name));
            }
        }
        (out, "scope".into())
    }

    fn copy_selected_pane(&mut self) {
        let Some(target) = self.selected_pane_target() else {
            self.set_status(
                StatusLevel::Warning,
                "Select a pane before copying pane text",
            );
            return;
        };

        self.apply_tmux_result(
            "Copied pane text to tmux buffer",
            self.client.copy_pane_to_buffer(&target),
        );
        self.refresh();
    }

    fn copy_selected_pane_clipboard(&mut self) {
        let Some(target) = self.selected_pane_target() else {
            self.set_status(
                StatusLevel::Warning,
                "Select a pane before copying to system clipboard",
            );
            return;
        };

        match self.client.capture_pane_plain_text(&target) {
            Ok(content) => {
                let clean = strip_ansi(&content);
                match copy_to_system_clipboard(&clean) {
                    Ok(tool) => self.set_status(
                        StatusLevel::Success,
                        format!("Copied pane output ({} bytes) via {tool}", clean.len()),
                    ),
                    Err(err) => self.set_status(
                        StatusLevel::Error,
                        format!("System clipboard unavailable: {err}"),
                    ),
                }
            }
            Err(err) => self.set_status(StatusLevel::Error, format!("Capture failed: {err}")),
        }
    }

    fn open_save_snapshot_prompt(&mut self) {
        let Some(target) = self.selected_pane_target() else {
            self.set_status(
                StatusLevel::Warning,
                "Select a pane before saving scrollback snapshot",
            );
            return;
        };
        let default_name = format!(
            "lazytmux-pane-{}.txt",
            target
                .pane_id
                .as_deref()
                .unwrap_or("0")
                .trim_start_matches('%')
        );
        self.prompt = Some(Prompt::new(
            "Save Pane Scrollback Snapshot".into(),
            default_name,
            "Saves last 2000 scrollback lines as clean plain text to path".into(),
            PromptKind::SaveSnapshot { target },
        ));
    }

    fn paste_selected_buffer(&mut self) {
        let Some(buffer) = self.state.buffers.get(self.buffer_selected).cloned() else {
            self.set_status(StatusLevel::Warning, "No tmux buffer selected");
            return;
        };
        let Some(target) = self.selected_pane_target() else {
            self.set_status(
                StatusLevel::Warning,
                "Select a pane in the tree to paste this buffer into",
            );
            return;
        };
        self.apply_tmux_result(
            &format!("Pasted buffer {} into pane", buffer.name),
            self.client.paste_buffer(&target, &buffer),
        );
        self.refresh();
    }

    fn toggle_favorite(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            self.set_status(StatusLevel::Warning, "No item selected to favorite");
            return;
        };

        if !self.favorites.remove(&item.favorite_key) {
            self.favorites.insert(item.favorite_key.clone());
            self.set_status(StatusLevel::Success, format!("Pinned ★ {}", item.name));
        } else {
            self.set_status(StatusLevel::Info, format!("Unpinned {}", item.name));
        }

        if let Err(error) = save_favorites(&self.favorites) {
            self.set_status(
                StatusLevel::Warning,
                format!("Favorite updated, but save failed: {error}"),
            );
        }
        self.rebuild_items();
    }

    fn detach_client(&mut self) {
        self.apply_tmux_result("Detached tmux client", self.client.detach_client());
    }

    fn open_command_prompt(&mut self) {
        self.prompt = Some(Prompt::new(
            "Raw tmux Command".into(),
            "".into(),
            "Example: rename-session work (omit leading tmux)".into(),
            PromptKind::Command,
        ));
    }

    fn apply_prompt(&mut self, prompt: Prompt) -> Result<()> {
        let value = prompt.value.trim();
        match prompt.kind {
            PromptKind::Filter => {
                self.filter = value.to_string();
                self.rebuild_items();
                let msg = self.count_status("Filter applied");
                self.set_status(StatusLevel::Info, msg);
            }
            PromptKind::Rename(target) => {
                if value.is_empty() {
                    self.set_status(StatusLevel::Warning, "Name cannot be empty");
                    return Ok(());
                }
                match target {
                    RenameTarget::Session { target } => {
                        self.apply_tmux_result(
                            "Renamed session",
                            self.client.rename_session(&target, value),
                        );
                    }
                    RenameTarget::Window { target } => {
                        self.apply_tmux_result(
                            "Renamed window",
                            self.client.rename_window(&target, value),
                        );
                    }
                    RenameTarget::Pane { target } => {
                        self.apply_tmux_result(
                            "Set pane title",
                            self.client.rename_pane(&target, value),
                        );
                    }
                }
                self.refresh();
            }
            PromptKind::NewSession => {
                if value.is_empty() {
                    self.set_status(StatusLevel::Warning, "Session name cannot be empty");
                    return Ok(());
                }
                self.apply_tmux_result("Created session", self.client.create_session(value));
                self.refresh();
            }
            PromptKind::LaunchSession => match parse_session_launcher(value) {
                Ok((name, start_dir, command)) => {
                    self.apply_tmux_result(
                        "Launched session",
                        self.client.create_session_with(
                            &name,
                            start_dir.as_deref(),
                            command.as_deref(),
                        ),
                    );
                    self.refresh();
                }
                Err(err) => self.set_status(StatusLevel::Error, err.to_string()),
            },
            PromptKind::NewWindow { target } => {
                if value.is_empty() {
                    self.set_status(StatusLevel::Warning, "Window name cannot be empty");
                    return Ok(());
                }
                self.apply_tmux_result("Created window", self.client.create_window(&target, value));
                self.refresh();
            }
            PromptKind::LaunchWindow { target } => match parse_session_launcher(value) {
                Ok((name, start_dir, command)) => {
                    self.apply_tmux_result(
                        "Launched window",
                        self.client.create_window_with(
                            &target,
                            &name,
                            start_dir.as_deref(),
                            command.as_deref(),
                        ),
                    );
                    self.refresh();
                }
                Err(err) => self.set_status(StatusLevel::Error, err.to_string()),
            },
            PromptKind::Command => match split_tmux_args(value) {
                Ok(args) if args.is_empty() => {
                    self.set_status(StatusLevel::Warning, "No tmux command entered");
                }
                Ok(args) => {
                    self.apply_tmux_result("Ran tmux command", self.client.run_args(&args));
                    self.refresh();
                }
                Err(err) => self.set_status(StatusLevel::Error, err.to_string()),
            },
            PromptKind::SendKeys { target } => {
                if value.is_empty() {
                    self.set_status(StatusLevel::Warning, "No keys entered");
                    return Ok(());
                }
                self.apply_tmux_result("Sent keys", self.client.send_keys(&target, value));
                self.refresh();
            }
            PromptKind::BroadcastKeys { targets, scope } => {
                if value.is_empty() {
                    self.set_status(StatusLevel::Warning, "No broadcast command entered");
                    return Ok(());
                }
                match self.client.broadcast_keys(&targets, value) {
                    Ok(count) => self.set_status(
                        StatusLevel::Success,
                        format!("Broadcasted command to {count} panes in {scope}"),
                    ),
                    Err(err) => {
                        self.set_status(StatusLevel::Error, format!("Broadcast failed: {err}"))
                    }
                }
                self.refresh();
            }
            PromptKind::JoinPane { source } => {
                if value.is_empty() {
                    self.set_status(StatusLevel::Warning, "Target window/pane cannot be empty");
                    return Ok(());
                }
                self.apply_tmux_result(
                    &format!("Joined pane into {value}"),
                    self.client.join_pane(&source, value),
                );
                self.refresh();
            }
            PromptKind::SaveSnapshot { target } => {
                if value.is_empty() {
                    self.set_status(StatusLevel::Warning, "Output path cannot be empty");
                    return Ok(());
                }
                match self.client.capture_pane_scrollback(&target, 2000) {
                    Ok(content) => {
                        let clean = strip_ansi(&content);
                        match fs::write(value, &clean) {
                            Ok(()) => self.set_status(
                                StatusLevel::Success,
                                format!("Saved snapshot ({} bytes) → {value}", clean.len()),
                            ),
                            Err(err) => self.set_status(
                                StatusLevel::Error,
                                format!("Failed writing snapshot: {err}"),
                            ),
                        }
                    }
                    Err(err) => self.set_status(
                        StatusLevel::Error,
                        format!("Snapshot capture failed: {err}"),
                    ),
                }
            }
        }

        Ok(())
    }

    fn apply_confirm(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::KillSession { target } => {
                self.apply_tmux_result("Killed session", self.client.kill_session(&target));
            }
            ConfirmAction::KillWindow { target } => {
                self.apply_tmux_result("Killed window", self.client.kill_window(&target));
            }
            ConfirmAction::KillPane { target } => {
                self.apply_tmux_result("Killed pane", self.client.kill_pane(&target));
            }
            ConfirmAction::DeleteBuffer { buffer } => {
                self.apply_tmux_result(
                    &format!("Deleted buffer {}", buffer.name),
                    self.client.delete_buffer(&buffer),
                );
            }
        }
        self.refresh();
    }

    fn selected_pane_target(&self) -> Option<TmuxTarget> {
        let item = self.selected_item()?;
        if item.target.pane_id.is_some() {
            return Some(item.target.clone());
        }
        // Smart convenience: if a window or session is selected, resolve its active pane!
        for session in &self.state.sessions {
            if session.id != item.target.session_id
                || session.server.label() != item.target.server_label()
            {
                continue;
            }
            let window = if let Some(wid) = &item.target.window_id {
                session.windows.iter().find(|w| w.id == *wid)
            } else {
                session
                    .windows
                    .iter()
                    .find(|w| w.active)
                    .or_else(|| session.windows.first())
            };
            if let Some(window) = window {
                if let Some(pane) = window
                    .panes
                    .iter()
                    .find(|p| p.active)
                    .or_else(|| window.panes.first())
                {
                    return Some(TmuxTarget::pane_on(
                        session.id.clone(),
                        window.id.clone(),
                        pane.id.clone(),
                        session.server.clone(),
                    ));
                }
            }
        }
        None
    }

    fn selected_window_target_with_label(&self) -> Option<(TmuxTarget, String)> {
        let item = self.selected_item()?;
        for session in &self.state.sessions {
            if session.id != item.target.session_id
                || session.server.label() != item.target.server_label()
            {
                continue;
            }
            let window = if let Some(wid) = &item.target.window_id {
                session.windows.iter().find(|w| w.id == *wid)
            } else {
                session
                    .windows
                    .iter()
                    .find(|w| w.active)
                    .or_else(|| session.windows.first())
            };
            if let Some(window) = window {
                return Some((
                    TmuxTarget::window_on(
                        session.id.clone(),
                        window.id.clone(),
                        session.server.clone(),
                    ),
                    format!("{}:{}", window.index, window.name),
                ));
            }
        }
        None
    }

    fn apply_tmux_result(&mut self, success: &str, result: Result<()>) {
        match result {
            Ok(()) => self.set_status(StatusLevel::Success, success),
            Err(error) => self.set_status(StatusLevel::Error, format!("{success} failed: {error}")),
        }
    }

    fn count_status(&self, prefix: &str) -> String {
        let (sessions, windows, panes) = self.counts();
        format!("{prefix}: {sessions} sessions, {windows} windows, {panes} panes")
    }

    fn append_resource_details(&self, item: &mut TreeItem) {
        let summary = self.resource_summary_for_target(&item.target);
        if let Some(ref metrics) = summary.metrics {
            item.cpu_percent = if self.metrics.cpu_ready {
                Some(metrics.cpu_usage)
            } else {
                None
            };
            item.memory_bytes = metrics.memory_bytes;
            item.process_count = metrics.process_count;
        }

        if item.kind == TreeItemKind::Pane {
            item.details.push((
                "Process tree CPU".into(),
                summary
                    .metrics
                    .as_ref()
                    .map(|metrics| format_cpu_usage(metrics.cpu_usage, self.metrics.cpu_ready))
                    .unwrap_or_else(|| "unavailable".into()),
            ));
            item.details.push((
                "Process tree memory".into(),
                summary
                    .metrics
                    .as_ref()
                    .map(|metrics| format_bytes(metrics.memory_bytes))
                    .unwrap_or_else(|| "unavailable".into()),
            ));
            item.details.push((
                "Process tree processes".into(),
                summary
                    .metrics
                    .as_ref()
                    .map(|metrics| metrics.process_count.to_string())
                    .unwrap_or_else(|| "0".into()),
            ));
            return;
        }

        item.details.push((
            "Aggregate CPU".into(),
            summary
                .metrics
                .as_ref()
                .map(|metrics| format_cpu_usage(metrics.cpu_usage, self.metrics.cpu_ready))
                .unwrap_or_else(|| "unavailable".into()),
        ));
        item.details.push((
            "Aggregate memory".into(),
            summary
                .metrics
                .as_ref()
                .map(|metrics| format_bytes(metrics.memory_bytes))
                .unwrap_or_else(|| "unavailable".into()),
        ));
        item.details.push((
            "Tracked panes".into(),
            format!("{}/{}", summary.tracked_panes, summary.total_panes),
        ));
        item.details.push((
            "Tracked processes".into(),
            summary
                .metrics
                .as_ref()
                .map(|metrics| metrics.process_count.to_string())
                .unwrap_or_else(|| "0".into()),
        ));
    }

    fn resource_summary_for_target(&self, target: &TmuxTarget) -> ResourceSummary {
        let mut summary = ResourceSummary::default();
        for session in &self.state.sessions {
            if session.id != target.session_id || session.server.label() != target.server_label() {
                continue;
            }

            match (&target.window_id, &target.pane_id) {
                (_, Some(pane_id)) => {
                    for pane in session.windows.iter().flat_map(|window| &window.panes) {
                        if pane.id == *pane_id {
                            summary.add_pane(pane, &self.metrics);
                            return summary;
                        }
                    }
                }
                (Some(window_id), None) => {
                    for window in &session.windows {
                        if window.id == *window_id {
                            for pane in &window.panes {
                                summary.add_pane(pane, &self.metrics);
                            }
                            return summary;
                        }
                    }
                }
                (None, None) => {
                    for pane in session.windows.iter().flat_map(|window| &window.panes) {
                        summary.add_pane(pane, &self.metrics);
                    }
                    return summary;
                }
            }
        }
        summary
    }
}

#[derive(Default)]
struct ResourceSummary {
    metrics: Option<ProcessMetrics>,
    total_panes: usize,
    tracked_panes: usize,
}

impl ResourceSummary {
    fn add_pane(&mut self, pane: &Pane, snapshot: &MetricsSnapshot) {
        self.total_panes = self.total_panes.saturating_add(1);
        if let Some(metrics) = snapshot.panes.get(&pane.pid) {
            self.tracked_panes = self.tracked_panes.saturating_add(1);
            self.metrics
                .get_or_insert_with(ProcessMetrics::default)
                .add(metrics);
        }
    }
}

fn rect_contains(rect: Rect, col: u16, row: u16) -> bool {
    rect.width > 0
        && rect.height > 0
        && col >= rect.x
        && col < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

fn pane_pids(state: &TmuxState) -> Vec<String> {
    state
        .sessions
        .iter()
        .flat_map(|session| &session.windows)
        .flat_map(|window| &window.panes)
        .filter_map(|pane| {
            let pid = pane.pid.trim();
            if pid.is_empty() {
                None
            } else {
                Some(pid.to_string())
            }
        })
        .collect()
}

fn session_item(session: &Session) -> TreeItem {
    let active_window = session
        .windows
        .iter()
        .find(|window| window.active)
        .or_else(|| session.windows.first());
    let any_synced = session.windows.iter().any(|w| w.synchronized);
    let any_zoomed = session.windows.iter().any(|w| w.zoomed);

    let target = TmuxTarget::session_on(session.id.clone(), session.server.clone());

    TreeItem {
        kind: TreeItemKind::Session,
        depth: 0,
        name: session.name.clone(),
        label: format!("[S] {}", session.name),
        subtitle: format!(
            "{}w{} · {}",
            session.window_count.unwrap_or(session.windows.len()),
            if session.attached { " · ATT" } else { "" },
            session.server.label()
        ),
        favorite_key: target.favorite_key(),
        target,
        favorite: false,
        rename_value: session.name.clone(),
        details: vec![
            ("Type".into(), "Session".into()),
            ("ID".into(), session.id.clone()),
            ("Server".into(), session.server.label()),
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
                    "Session {} // {}:{}",
                    session.name, window.index, window.name
                ),
                session,
                window,
                false,
            )
        }),
        expanded: None,
        active: session.attached,
        dead: false,
        zoomed: any_zoomed,
        synchronized: any_synced,
        marked: false,
        in_mode: false,
        cpu_percent: None,
        memory_bytes: 0,
        process_count: 0,
        server_label: session.server.label(),
        cwd: active_window
            .and_then(|w| w.panes.first())
            .map(|p| p.path.clone())
            .unwrap_or_default(),
        command: active_window
            .and_then(|w| w.panes.first())
            .map(|p| p.command.clone())
            .unwrap_or_default(),
    }
}

fn window_item(session: &Session, window: &Window) -> TreeItem {
    let pane_names = window
        .panes
        .iter()
        .map(pane_name)
        .collect::<Vec<_>>()
        .join(", ");

    let target = TmuxTarget::window_on(
        session.id.clone(),
        window.id.clone(),
        session.server.clone(),
    );

    TreeItem {
        kind: TreeItemKind::Window,
        depth: 1,
        name: format!("{}:{}", window.index, window.name),
        label: format!("[W] {}:{}", window.index, window.name),
        subtitle: format!(
            "{}p {}",
            window.pane_count.unwrap_or(window.panes.len()),
            window.flags
        ),
        favorite_key: target.favorite_key(),
        target,
        favorite: false,
        rename_value: window.name.clone(),
        details: vec![
            ("Type".into(), "Window".into()),
            ("ID".into(), window.id.clone()),
            ("Server".into(), session.server.label()),
            ("Session".into(), session.name.clone()),
            ("Index".into(), window.index.clone()),
            ("Name".into(), window.name.clone()),
            ("Active".into(), window.active.to_string()),
            ("Flags".into(), window.flags.clone()),
            ("Sync panes".into(), window.synchronized.to_string()),
            ("Zoomed".into(), window.zoomed.to_string()),
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
            session,
            window,
            false,
        )),
        expanded: None,
        active: window.active,
        dead: false,
        zoomed: window.zoomed,
        synchronized: window.synchronized,
        marked: window.panes.iter().any(|p| p.marked),
        in_mode: window.panes.iter().any(|p| p.in_mode),
        cpu_percent: None,
        memory_bytes: 0,
        process_count: 0,
        server_label: session.server.label(),
        cwd: window
            .panes
            .iter()
            .find(|p| p.active)
            .or_else(|| window.panes.first())
            .map(|p| p.path.clone())
            .unwrap_or_default(),
        command: window
            .panes
            .iter()
            .find(|p| p.active)
            .or_else(|| window.panes.first())
            .map(|p| p.command.clone())
            .unwrap_or_default(),
    }
}

fn pane_item(session: &Session, window: &Window, pane: &Pane) -> TreeItem {
    let size = match (pane.width, pane.height) {
        (Some(width), Some(height)) => format!("{width}x{height}"),
        _ => "unknown".into(),
    };
    let name = pane_name(pane);
    let target = TmuxTarget::pane_on(
        session.id.clone(),
        window.id.clone(),
        pane.id.clone(),
        session.server.clone(),
    );

    TreeItem {
        kind: TreeItemKind::Pane,
        depth: 2,
        name: format!("{}: {}", pane.index, name),
        label: format!("[P] {}: {}", pane.index, name),
        subtitle: if pane.command.is_empty() {
            pane.path.clone()
        } else {
            format!("{} · {}", pane.command, shorten_path(&pane.path))
        },
        favorite_key: target.favorite_key(),
        target: target.clone(),
        favorite: false,
        rename_value: if pane.title.is_empty() {
            name.clone()
        } else {
            pane.title.clone()
        },
        details: vec![
            ("Type".into(), "Pane".into()),
            ("ID".into(), pane.id.clone()),
            ("Server".into(), session.server.label()),
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
            ("Marked".into(), pane.marked.to_string()),
            ("Copy mode".into(), pane.in_mode.to_string()),
        ],
        preview: Some(TerminalPreview {
            title: format!("Pane {} // {}", pane.index, name),
            panes: vec![pane_preview(session, window, pane, true)],
        }),
        expanded: None,
        active: pane.active,
        dead: pane.dead,
        zoomed: window.zoomed && pane.active,
        synchronized: window.synchronized,
        marked: pane.marked,
        in_mode: pane.in_mode,
        cpu_percent: None,
        memory_bytes: 0,
        process_count: 0,
        server_label: session.server.label(),
        cwd: pane.path.clone(),
        command: pane.command.clone(),
    }
}

fn shorten_path(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() && path.starts_with(&home) {
            return format!("~{}", &path[home.len()..]);
        }
    }
    path.to_string()
}

fn matches_filter(item: &TreeItem, filter: &str) -> bool {
    let searchable = std::iter::once(item.label.as_str())
        .chain(std::iter::once(item.name.as_str()))
        .chain(std::iter::once(item.subtitle.as_str()))
        .chain(std::iter::once(item.favorite_key.as_str()))
        .chain(
            item.details
                .iter()
                .flat_map(|(label, value)| [label.as_str(), value.as_str()]),
        )
        .collect::<Vec<_>>();

    filter.split_whitespace().all(|token| {
        searchable
            .iter()
            .any(|value| matches_search_token(value, token))
    })
}

fn matches_search_token(value: &str, token: &str) -> bool {
    let value = value.to_lowercase();
    let token = token.to_lowercase();
    value.contains(&token) || fuzzy_match(&value, &token)
}

fn fuzzy_match(value: &str, token: &str) -> bool {
    if token.is_empty() {
        return true;
    }

    let mut token_chars = token.chars();
    let mut current = token_chars.next();
    for character in value.chars() {
        if Some(character) == current {
            current = token_chars.next();
            if current.is_none() {
                return true;
            }
        }
    }
    false
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

fn window_preview(
    title: String,
    session: &Session,
    window: &Window,
    full_panes: bool,
) -> TerminalPreview {
    TerminalPreview {
        title,
        panes: window
            .panes
            .iter()
            .map(|pane| pane_preview(session, window, pane, full_panes))
            .collect(),
    }
}

fn pane_preview(
    session: &Session,
    window: &Window,
    pane: &Pane,
    full: bool,
) -> TerminalPanePreview {
    let dims = match (pane.width, pane.height) {
        (Some(w), Some(h)) => format!(" {w}x{h}"),
        _ => String::new(),
    };
    TerminalPanePreview {
        title: format!("{} {}{}", pane.index, pane_name(pane), dims),
        content: if strip_ansi(&pane.content).trim().is_empty() {
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
        marked: pane.marked,
        target: TmuxTarget::pane_on(
            session.id.clone(),
            window.id.clone(),
            pane.id.clone(),
            session.server.clone(),
        ),
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

fn parse_session_launcher(input: &str) -> Result<(String, Option<String>, Option<String>)> {
    let parts = input.splitn(3, '|').map(str::trim).collect::<Vec<_>>();
    let name = parts.first().copied().unwrap_or_default();
    if name.is_empty() {
        bail!("session name cannot be empty");
    }

    let start_dir = parts
        .get(1)
        .copied()
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let command = parts
        .get(2)
        .copied()
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    Ok((name.to_string(), start_dir, command))
}

fn load_favorites() -> HashSet<String> {
    let Some(path) = favorites_path() else {
        return HashSet::new();
    };
    let Ok(content) = fs::read_to_string(path) else {
        return HashSet::new();
    };

    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn save_favorites(favorites: &HashSet<String>) -> Result<()> {
    let Some(path) = favorites_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut values = favorites.iter().cloned().collect::<Vec<_>>();
    values.sort();
    fs::write(path, values.join("\n"))?;
    Ok(())
}

fn favorites_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(path).join("lazytmux/favorites"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state/lazytmux/favorites"))
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

    fn sample_prompt(value: &str) -> Prompt {
        Prompt::new(
            "Test".into(),
            value.into(),
            "hint".into(),
            PromptKind::NewSession,
        )
    }

    #[test]
    fn prompt_starts_with_caret_at_end() {
        let prompt = sample_prompt("work");
        assert_eq!(prompt.cursor, 4);
        assert_eq!(prompt.cursor_char_index(), 4);
    }

    #[test]
    fn prompt_inserts_at_caret() {
        let mut prompt = sample_prompt("wrk");
        prompt.move_home();
        prompt.move_right();
        prompt.insert_char('o');
        assert_eq!(prompt.value, "work");
        assert_eq!(prompt.cursor_char_index(), 2);
    }

    #[test]
    fn prompt_backspace_deletes_before_caret() {
        let mut prompt = sample_prompt("work");
        prompt.move_left();
        prompt.delete_backward();
        assert_eq!(prompt.value, "wok");
        assert_eq!(prompt.cursor_char_index(), 2);
    }

    #[test]
    fn prompt_delete_removes_at_caret() {
        let mut prompt = sample_prompt("work");
        prompt.move_home();
        prompt.delete_forward();
        assert_eq!(prompt.value, "ork");
        assert_eq!(prompt.cursor_char_index(), 0);
    }

    #[test]
    fn prompt_arrows_clamp_at_boundaries() {
        let mut prompt = sample_prompt("hi");
        prompt.move_home();
        prompt.move_left();
        assert_eq!(prompt.cursor, 0);
        prompt.move_end();
        prompt.move_right();
        assert_eq!(prompt.cursor, prompt.value.len());
    }

    #[test]
    fn prompt_home_end_jump_to_edges() {
        let mut prompt = sample_prompt("session");
        prompt.move_home();
        assert_eq!(prompt.cursor_char_index(), 0);
        prompt.move_end();
        assert_eq!(prompt.cursor_char_index(), 7);
    }

    #[test]
    fn prompt_clear_resets_value_and_caret() {
        let mut prompt = sample_prompt("something");
        prompt.clear();
        assert_eq!(prompt.value, "");
        assert_eq!(prompt.cursor, 0);
    }

    #[test]
    fn prompt_handles_multibyte_characters() {
        let mut prompt = sample_prompt("café");
        assert_eq!(prompt.cursor_char_index(), 4);
        prompt.delete_backward();
        assert_eq!(prompt.value, "caf");
        prompt.move_home();
        prompt.move_right();
        prompt.insert_char('ñ');
        assert_eq!(prompt.value, "cñaf");
        assert_eq!(prompt.cursor_char_index(), 2);
    }

    #[test]
    fn parses_session_launcher_input() {
        let (name, start_dir, command) =
            parse_session_launcher("work | /tmp | nvim README.md").unwrap();

        assert_eq!(name, "work");
        assert_eq!(start_dir.as_deref(), Some("/tmp"));
        assert_eq!(command.as_deref(), Some("nvim README.md"));
    }

    #[test]
    fn fuzzy_matches_ordered_characters() {
        assert!(fuzzy_match("openai_dashboard", "oad"));
        assert!(!fuzzy_match("logging", "zg"));
    }

    #[test]
    fn starts_with_sessions_and_windows_collapsed() {
        let mut app = app_with_sample_state();

        app.rebuild_items();

        assert_eq!(app.items.len(), 1);
        assert_eq!(app.items[0].kind, TreeItemKind::Session);
        assert_eq!(app.items[0].expanded, Some(false));

        app.expand_selected();
        assert_eq!(app.items.len(), 2);
        assert_eq!(app.items[1].kind, TreeItemKind::Window);
        assert_eq!(app.items[1].expanded, Some(false));

        app.select_index(1);
        app.expand_selected();
        assert_eq!(app.items.len(), 3);
        assert_eq!(app.items[2].kind, TreeItemKind::Pane);
    }

    #[test]
    fn expand_all_and_collapse_all_toggle_entire_tree() {
        let mut app = app_with_sample_state();
        app.rebuild_items();
        assert_eq!(app.items.len(), 1);

        app.expand_all();
        assert_eq!(app.items.len(), 3);

        app.collapse_all();
        assert_eq!(app.items.len(), 1);
    }

    #[test]
    fn filter_reveals_matching_descendants_even_when_collapsed() {
        let mut app = app_with_sample_state();
        app.filter = "nvim".into();

        app.rebuild_items();

        assert_eq!(app.items.len(), 3);
        assert_eq!(app.items[0].kind, TreeItemKind::Session);
        assert_eq!(app.items[1].kind, TreeItemKind::Window);
        assert_eq!(app.items[2].kind, TreeItemKind::Pane);
    }

    #[test]
    fn appends_resource_metrics_to_details() {
        let mut app = app_with_sample_state();
        app.metrics.cpu_ready = true;
        app.metrics.panes.insert(
            "123".into(),
            ProcessMetrics {
                cpu_usage: 12.5,
                memory_bytes: 2048,
                process_count: 3,
            },
        );

        app.rebuild_items();

        assert!(app.items[0]
            .details
            .contains(&("Aggregate CPU".into(), "12.5%".into())));
        assert!(app.items[0]
            .details
            .contains(&("Tracked processes".into(), "3".into())));
        assert_eq!(app.items[0].cpu_percent, Some(12.5));
        assert_eq!(app.items[0].memory_bytes, 2048);
    }

    #[test]
    fn command_palette_filters_and_executes_theme_cycle() {
        let mut app = app_with_sample_state();
        let start_theme = app.theme_id();
        app.execute_action(Action::OpenPalette).unwrap();
        assert!(app.palette().is_some());

        for ch in "theme".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
                .unwrap();
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert!(app.palette().is_none());
        assert_eq!(app.theme_id(), start_theme.next());
    }

    #[test]
    fn quick_jump_expands_and_selects_collapsed_pane() {
        let mut app = app_with_sample_state();
        app.rebuild_items();
        assert_eq!(app.items.len(), 1);

        app.execute_action(Action::QuickJump).unwrap();
        for ch in "nvim".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
                .unwrap();
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.items.len(), 3);
        assert_eq!(app.selected_item().unwrap().kind, TreeItemKind::Pane);
    }

    #[test]
    fn view_mode_and_zen_shortcuts_work() {
        let mut app = app_with_sample_state();
        app.execute_action(Action::ViewTelemetry).unwrap();
        assert_eq!(app.view_mode(), ViewMode::Telemetry);

        app.execute_action(Action::ToggleZen).unwrap();
        assert!(app.zen_mode());

        app.execute_action(Action::ResetView).unwrap();
        assert!(!app.zen_mode());
        assert_eq!(app.view_mode(), ViewMode::Deck);
    }

    #[test]
    fn renders_full_futuristic_ui_across_sizes_and_overlays() {
        use ratatui::backend::TestBackend;

        let mut app = app_with_sample_state();
        app.metrics.cpu_ready = true;
        app.metrics.system.cpu_usage = Some(42.5);
        app.metrics.system.memory_used = 8 * 1024 * 1024 * 1024;
        app.metrics.system.memory_total = 16 * 1024 * 1024 * 1024;
        app.metrics.system.cpu_cores = 10;
        app.metrics.system.host_name = "nexus-host".into();
        app.metrics.system.os_label = "macOS/aarch64".into();
        app.metrics.cpu_history = vec![12, 24, 36, 48, 32, 65, 42];
        app.metrics.mem_history = vec![48, 49, 50, 50, 51, 50];
        app.metrics.panes.insert(
            "123".into(),
            ProcessMetrics {
                cpu_usage: 14.2,
                memory_bytes: 256 * 1024 * 1024,
                process_count: 4,
            },
        );
        app.expand_all();

        for (width, height) in [(140, 40), (80, 24), (48, 16)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).unwrap();

            for mode in ViewMode::ALL {
                app.view_mode = mode;
                terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
            }

            app.view_mode = ViewMode::Deck;
            app.palette = Some(PaletteState::new());
            terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
            app.palette = None;

            app.quick_jump = Some(QuickJumpState::new());
            terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
            app.quick_jump = None;

            app.open_layout_picker();
            terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
            app.layout_picker = None;

            app.open_help();
            terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
            app.show_help = false;

            app.open_diagnostics();
            terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
            app.show_diagnostics = false;
        }
    }

    #[test]
    fn inspect_rendered_frames_visually() {
        use ratatui::backend::TestBackend;

        let mut app = app_with_rich_demo_state();
        app.expand_all();

        let render_str = |app: &App, w: u16, h: u16| -> String {
            let backend = TestBackend::new(w, h);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal.draw(|frame| ui::draw(frame, app)).unwrap();
            let buf = terminal.backend().buffer();
            let mut out = String::new();
            for y in 0..h {
                for x in 0..w {
                    out.push_str(buf[(x, y)].symbol());
                }
                out.push('\n');
            }
            out
        };

        let deck_130 = render_str(&app, 130, 32);
        assert!(deck_130.contains("LAZYTMUX"));
        assert!(deck_130.contains("TOPOLOGY"));
        assert!(deck_130.contains("LIVE VIEWPORT"));

        let deck_80 = render_str(&app, 80, 24);
        assert!(deck_80.contains("LAZYTMUX"));

        app.view_mode = ViewMode::Telemetry;
        let telem_130 = render_str(&app, 130, 30);
        assert!(telem_130.contains("LIVE PROCESS MATRIX"));

        app.view_mode = ViewMode::Deck;
        app.palette = Some(PaletteState::new());
        let pal_130 = render_str(&app, 130, 30);
        assert!(pal_130.contains("COMMAND PALETTE"));
        app.palette = None;

        app.open_layout_picker();
        let lay_130 = render_str(&app, 130, 30);
        assert!(lay_130.contains("VISUAL LAYOUT ENGINE"));
        let lay_80 = render_str(&app, 80, 24);
        assert!(lay_80.contains("VISUAL LAYOUT ENGINE"));
        app.layout_picker = None;

        if std::env::var_os("PRINT_FRAMES").is_some() {
            println!("=== DECK 130x32 ===\n{deck_130}");
            println!("=== DECK 80x24 ===\n{deck_80}");
            println!("=== TELEMETRY 130x30 ===\n{telem_130}");
            println!("=== PALETTE 130x30 ===\n{pal_130}");
            println!("=== LAYOUT 130x30 ===\n{lay_130}");
            println!("=== LAYOUT 80x24 ===\n{lay_80}");
        }
    }

    fn app_with_rich_demo_state() -> App {
        let mut app = app_with_sample_state();
        app.state.sessions[0].windows[0].synchronized = true;
        app.state.sessions[0].windows[0].panes = vec![
            Pane {
                id: "%1".into(),
                index: "0".into(),
                active: true,
                marked: false,
                command: "nvim".into(),
                path: "/Users/dev/lazytmux".into(),
                title: "main.rs".into(),
                left: Some(0),
                top: Some(0),
                width: Some(80),
                height: Some(36),
                pid: "123".into(),
                dead: false,
                in_mode: false,
                content: "\x1b[36mfn\x1b[0m main() -> Result<()> {\n    \x1b[32mlet\x1b[0m cli = Cli::parse();\n    run_nexus(cli)\n}".into(),
            },
            Pane {
                id: "%2".into(),
                index: "1".into(),
                active: false,
                marked: true,
                command: "cargo".into(),
                path: "/Users/dev/lazytmux".into(),
                title: "build".into(),
                left: Some(81),
                top: Some(0),
                width: Some(50),
                height: Some(18),
                pid: "124".into(),
                dead: false,
                in_mode: false,
                content: "\x1b[32m   Compiling\x1b[0m lazytmux v0.1.4\n\x1b[32m    Finished\x1b[0m release in 4.2s".into(),
            },
            Pane {
                id: "%3".into(),
                index: "2".into(),
                active: false,
                marked: false,
                command: "zsh".into(),
                path: "/Users/dev/lazytmux".into(),
                title: "logs".into(),
                left: Some(81),
                top: Some(19),
                width: Some(50),
                height: Some(17),
                pid: "125".into(),
                dead: false,
                in_mode: false,
                content: "14:05:11 [INFO] server listening :8080\n14:05:12 [OK] healthcheck 200".into(),
            },
        ];
        app.state.sessions.push(Session {
            server: crate::tmux::TmuxServer::Default,
            id: "$2".into(),
            name: "prod-ops".into(),
            attached: false,
            created: Some(1_700_000_000),
            activity: Some(1_700_000_900),
            window_count: Some(1),
            windows: vec![Window {
                id: "@2".into(),
                index: "0".into(),
                name: "monitor".into(),
                active: true,
                zoomed: true,
                synchronized: false,
                pane_count: Some(1),
                layout: "tiled".into(),
                flags: "*Z".into(),
                panes: vec![Pane {
                    id: "%4".into(),
                    index: "0".into(),
                    active: true,
                    marked: false,
                    command: "htop".into(),
                    path: "/var/log".into(),
                    title: "cluster".into(),
                    left: Some(0),
                    top: Some(0),
                    width: Some(120),
                    height: Some(36),
                    pid: "126".into(),
                    dead: false,
                    in_mode: false,
                    content: "CPU [|||||    42%]  MEM [|||||||  68%]".into(),
                }],
            }],
        });
        app.metrics.cpu_ready = true;
        app.metrics.system.cpu_usage = Some(34.6);
        app.metrics.system.memory_used = 11 * 1024 * 1024 * 1024;
        app.metrics.system.memory_total = 16 * 1024 * 1024 * 1024;
        app.metrics.system.cpu_cores = 12;
        app.metrics.system.uptime_secs = 285_000;
        app.metrics.system.host_name = "orbital-01".into();
        app.metrics.system.os_label = "macOS/aarch64".into();
        app.metrics.system.load_average = Some(crate::metrics::LoadAverage {
            one: 1.84,
            five: 1.52,
            fifteen: 1.31,
        });
        app.metrics.cpu_history = vec![12, 18, 25, 44, 62, 51, 38, 29, 34, 48, 35];
        app.metrics.mem_history = vec![64, 65, 66, 68, 69, 69, 68, 69];
        app.metrics.panes.insert(
            "123".into(),
            ProcessMetrics {
                cpu_usage: 9.4,
                memory_bytes: 280 * 1024 * 1024,
                process_count: 3,
            },
        );
        app.metrics.panes.insert(
            "124".into(),
            ProcessMetrics {
                cpu_usage: 18.2,
                memory_bytes: 512 * 1024 * 1024,
                process_count: 6,
            },
        );
        app.metrics.panes.insert(
            "126".into(),
            ProcessMetrics {
                cpu_usage: 3.1,
                memory_bytes: 64 * 1024 * 1024,
                process_count: 1,
            },
        );
        app.favorites.insert("default|$1".into());
        app
    }

    pub(crate) fn app_with_sample_state() -> App {
        let mut app = App::with_options(
            Duration::from_secs(1),
            TmuxClient::default(),
            Some(ThemeId::Cyber),
            ViewMode::Deck,
        );
        app.state = TmuxState {
            sessions: vec![Session {
                server: crate::tmux::TmuxServer::Default,
                id: "$1".into(),
                name: "dev".into(),
                attached: true,
                created: Some(1_700_000_000),
                activity: Some(1_700_000_500),
                window_count: Some(1),
                windows: vec![Window {
                    id: "@1".into(),
                    index: "0".into(),
                    name: "editor".into(),
                    active: true,
                    zoomed: false,
                    synchronized: false,
                    pane_count: Some(1),
                    layout: "main-vertical".into(),
                    flags: "*".into(),
                    panes: vec![Pane {
                        id: "%1".into(),
                        index: "0".into(),
                        active: true,
                        marked: false,
                        command: "nvim".into(),
                        path: "/tmp".into(),
                        title: "editor".into(),
                        left: Some(0),
                        top: Some(0),
                        width: Some(120),
                        height: Some(40),
                        pid: "123".into(),
                        dead: false,
                        in_mode: false,
                        content: "\x1b[36mfn main()\x1b[0m { println!(\"hello\"); }".into(),
                    }],
                }],
            }],
            buffers: vec![TmuxBuffer {
                server: crate::tmux::TmuxServer::Default,
                name: "buffer0000".into(),
                size: 18,
                sample: "cargo test --locked".into(),
                content: "cargo test --locked".into(),
            }],
            version: Some("tmux 3.8".into()),
            notice: None,
            diagnostics: vec![
                "Candidate servers: 2".into(),
                "default: loaded 1 sessions".into(),
                "tmux process running: true".into(),
            ],
        };
        app
    }
}
