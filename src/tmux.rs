use std::collections::{BTreeMap, HashSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{anyhow, Context, Result};

const PRIMARY_SEP: char = '\u{1f}';
const FALLBACK_SEP: char = '\t';
const REQUIRED_FIELD_COUNT: usize = 23;
const TMUX_FIELDS: [&str; 29] = [
    "session_id",
    "session_name",
    "session_attached",
    "session_created",
    "session_windows",
    "window_id",
    "window_index",
    "window_name",
    "window_active",
    "window_panes",
    "window_layout",
    "window_flags",
    "pane_id",
    "pane_index",
    "pane_active",
    "pane_current_command",
    "pane_current_path",
    "pane_title",
    "pane_left",
    "pane_top",
    "pane_width",
    "pane_height",
    "pane_pid",
    "pane_dead",
    "pane_in_mode",
    "window_zoomed_flag",
    "pane_synchronized",
    "pane_marked",
    "session_activity",
];

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum TmuxServer {
    #[default]
    Default,
    Name(String),
    Socket(PathBuf),
}

impl TmuxServer {
    fn from_tmux_env() -> Option<Self> {
        let value = env::var_os("TMUX")?;
        let value = value.to_string_lossy();
        let socket = value.split(',').next().unwrap_or_default();
        if socket.is_empty() {
            None
        } else {
            Some(Self::Socket(PathBuf::from(socket)))
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Default => "default".into(),
            Self::Name(name) => format!("-L {name}"),
            Self::Socket(socket) => format!("-S {}", socket.display()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TmuxBuffer {
    pub server: TmuxServer,
    pub name: String,
    pub size: usize,
    pub sample: String,
    pub content: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TmuxState {
    pub sessions: Vec<Session>,
    pub buffers: Vec<TmuxBuffer>,
    pub version: Option<String>,
    pub notice: Option<String>,
    pub diagnostics: Vec<String>,
}

impl TmuxState {
    pub fn counts(&self) -> (usize, usize, usize) {
        let windows = self
            .sessions
            .iter()
            .map(|session| session.windows.len())
            .sum();
        let panes = self
            .sessions
            .iter()
            .flat_map(|session| &session.windows)
            .map(|window| window.panes.len())
            .sum();
        (self.sessions.len(), windows, panes)
    }

    pub fn server_count(&self) -> usize {
        let servers: HashSet<String> = self
            .sessions
            .iter()
            .map(|session| session.server.label())
            .collect();
        servers.len()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    pub server: TmuxServer,
    pub id: String,
    pub name: String,
    pub attached: bool,
    pub created: Option<i64>,
    pub activity: Option<i64>,
    pub window_count: Option<usize>,
    pub windows: Vec<Window>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Window {
    pub id: String,
    pub index: String,
    pub name: String,
    pub active: bool,
    pub zoomed: bool,
    pub synchronized: bool,
    pub pane_count: Option<usize>,
    pub layout: String,
    pub flags: String,
    pub panes: Vec<Pane>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pane {
    pub id: String,
    pub index: String,
    pub active: bool,
    pub marked: bool,
    pub command: String,
    pub path: String,
    pub title: String,
    pub left: Option<u16>,
    pub top: Option<u16>,
    pub width: Option<u16>,
    pub height: Option<u16>,
    pub pid: String,
    pub dead: bool,
    pub in_mode: bool,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TmuxTarget {
    server: TmuxServer,
    pub session_id: String,
    pub window_id: Option<String>,
    pub pane_id: Option<String>,
}

impl TmuxTarget {
    pub fn session_on(session_id: impl Into<String>, server: TmuxServer) -> Self {
        Self {
            server,
            session_id: session_id.into(),
            window_id: None,
            pane_id: None,
        }
    }

    pub fn window_on(
        session_id: impl Into<String>,
        window_id: impl Into<String>,
        server: TmuxServer,
    ) -> Self {
        Self {
            server,
            session_id: session_id.into(),
            window_id: Some(window_id.into()),
            pane_id: None,
        }
    }

    pub fn pane_on(
        session_id: impl Into<String>,
        window_id: impl Into<String>,
        pane_id: impl Into<String>,
        server: TmuxServer,
    ) -> Self {
        Self {
            server,
            session_id: session_id.into(),
            window_id: Some(window_id.into()),
            pane_id: Some(pane_id.into()),
        }
    }

    pub fn display(&self) -> String {
        match (&self.window_id, &self.pane_id) {
            (_, Some(pane_id)) => pane_id.clone(),
            (Some(window_id), None) => window_id.clone(),
            (None, None) => self.session_id.clone(),
        }
    }

    pub fn server_label(&self) -> String {
        self.server.label()
    }

    pub fn favorite_key(&self) -> String {
        let server = self.server.label();
        match (&self.window_id, &self.pane_id) {
            (_, Some(pane_id)) => format!("{server}|{}|{pane_id}", self.session_id),
            (Some(window_id), None) => format!("{server}|{}|{window_id}", self.session_id),
            (None, None) => format!("{server}|{}", self.session_id),
        }
    }

    fn server(&self) -> &TmuxServer {
        &self.server
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResizeDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TmuxLayout {
    EvenHorizontal,
    EvenVertical,
    MainHorizontal,
    MainVertical,
    Tiled,
}

impl TmuxLayout {
    pub const ALL: [TmuxLayout; 5] = [
        TmuxLayout::EvenHorizontal,
        TmuxLayout::EvenVertical,
        TmuxLayout::MainHorizontal,
        TmuxLayout::MainVertical,
        TmuxLayout::Tiled,
    ];

    pub fn tmux_name(self) -> &'static str {
        match self {
            Self::EvenHorizontal => "even-horizontal",
            Self::EvenVertical => "even-vertical",
            Self::MainHorizontal => "main-horizontal",
            Self::MainVertical => "main-vertical",
            Self::Tiled => "tiled",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::EvenHorizontal => "Even Horizontal",
            Self::EvenVertical => "Even Vertical",
            Self::MainHorizontal => "Main Horizontal",
            Self::MainVertical => "Main Vertical",
            Self::Tiled => "Balanced Tiled",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::EvenHorizontal => "Equal-width side-by-side vertical columns",
            Self::EvenVertical => "Equal-height stacked horizontal rows",
            Self::MainHorizontal => "Large hero pane on top, stacked strip below",
            Self::MainVertical => "Large hero pane on left, stacked column right",
            Self::Tiled => "Symmetric grid automatically balanced across panes",
        }
    }

    pub fn diagram(self) -> [&'static str; 5] {
        match self {
            Self::EvenHorizontal => [
                "┌───┬───┬───┐",
                "│ 0 │ 1 │ 2 │",
                "│   │   │   │",
                "│   │   │   │",
                "└───┴───┴───┘",
            ],
            Self::EvenVertical => [
                "┌───────────┐",
                "│     0     │",
                "├───────────┤",
                "│     1     │",
                "└───────────┘",
            ],
            Self::MainHorizontal => [
                "┌───────────┐",
                "│   MAIN    │",
                "├─────┬─────┤",
                "│  1  │  2  │",
                "└─────┴─────┘",
            ],
            Self::MainVertical => [
                "┌──────┬────┐",
                "│      │ 1  │",
                "│ MAIN ├────┤",
                "│      │ 2  │",
                "└──────┴────┘",
            ],
            Self::Tiled => [
                "┌─────┬─────┐",
                "│  0  │  1  │",
                "├─────┼─────┤",
                "│  2  │  3  │",
                "└─────┴─────┘",
            ],
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TmuxClientOptions {
    pub socket_paths: Vec<PathBuf>,
    pub socket_names: Vec<String>,
    pub socket_dirs: Vec<PathBuf>,
}

impl TmuxClientOptions {
    pub fn from_env() -> Self {
        let mut options = Self::default();
        options.socket_paths.extend(env_paths("LAZYTMUX_SOCKET"));
        options.socket_paths.extend(env_paths("LAZYTMUX_SOCKETS"));
        options
            .socket_names
            .extend(env_values("LAZYTMUX_SOCKET_NAME"));
        options
            .socket_names
            .extend(env_values("LAZYTMUX_SOCKET_NAMES"));
        options.socket_dirs.extend(env_paths("LAZYTMUX_SOCKET_DIR"));
        options
            .socket_dirs
            .extend(env_paths("LAZYTMUX_SOCKET_DIRS"));
        options
    }
}

#[derive(Clone, Debug, Default)]
pub struct TmuxClient {
    server: TmuxServer,
    extra_servers: Vec<TmuxServer>,
    socket_dirs: Vec<PathBuf>,
}

impl TmuxClient {
    pub fn with_options(options: TmuxClientOptions) -> Self {
        let mut extra_servers = Vec::new();
        for socket in options.socket_paths {
            push_unique_server(&mut extra_servers, TmuxServer::Socket(socket));
        }
        for name in options.socket_names {
            let name = name.trim();
            if !name.is_empty() {
                push_unique_server(&mut extra_servers, TmuxServer::Name(name.to_string()));
            }
        }

        Self {
            server: TmuxServer::Default,
            extra_servers,
            socket_dirs: options.socket_dirs,
        }
    }

    pub fn load(&mut self) -> Result<TmuxState> {
        let mut empty_server = None;
        let mut primary_server = None;
        let mut loaded_servers = HashSet::new();
        let mut active_servers = Vec::new();
        let mut sessions = Vec::new();
        let candidates = candidate_servers(&self.server, &self.extra_servers, &self.socket_dirs);
        let mut diagnostics = vec![format!("Candidate servers: {}", candidates.len())];
        let fallback_server = candidates.first().cloned().unwrap_or(TmuxServer::Default);
        let version = detect_tmux_version();

        for server in candidates {
            let server_label = server.label();
            let state = match load_state_from_server(&server) {
                Ok(ServerLoad::State(state)) => state,
                Ok(ServerLoad::Unavailable) => {
                    diagnostics.push(format!("{server_label}: unavailable"));
                    continue;
                }
                Err(error) if is_not_found(&error) => {
                    return Err(anyhow!("tmux executable was not found in PATH"));
                }
                Err(error) => {
                    diagnostics.push(format!("{server_label}: error: {error}"));
                    return Err(error);
                }
            };

            if state.sessions.is_empty() {
                diagnostics.push(format!("{server_label}: empty"));
                empty_server.get_or_insert(server);
                continue;
            }

            if !loaded_servers.insert(server_identity(&server)) {
                diagnostics.push(format!("{server_label}: duplicate"));
                continue;
            }

            diagnostics.push(format!(
                "{server_label}: loaded {} sessions",
                state.sessions.len()
            ));
            primary_server.get_or_insert_with(|| server.clone());
            active_servers.push(server);
            sessions.extend(state.sessions);
        }

        diagnostics.push(format!(
            "tmux process running: {}",
            tmux_processes_running()
        ));
        if let Some(ref ver) = version {
            diagnostics.push(format!("tmux binary version: {ver}"));
        }

        if !sessions.is_empty() {
            self.server = primary_server.unwrap_or(TmuxServer::Default);
            sort_sessions(&mut sessions);
            let buffers = load_buffers_from_servers(&active_servers);
            let mut state = TmuxState {
                sessions,
                buffers,
                version,
                notice: None,
                diagnostics,
            };
            self.capture_visible_content(&mut state);
            return Ok(state);
        }

        if let Some(server) = empty_server {
            self.server = server;
            return Ok(TmuxState {
                sessions: Vec::new(),
                buffers: Vec::new(),
                version,
                notice: Some("No tmux sessions found. Press n to create a session.".into()),
                diagnostics,
            });
        }

        self.server = fallback_server;
        Ok(TmuxState {
            sessions: Vec::new(),
            buffers: Vec::new(),
            version,
            notice: Some(unreachable_server_notice()),
            diagnostics,
        })
    }

    pub fn switch_to_target(&self, target: &TmuxTarget) -> Result<()> {
        self.select_target(target)?;
        run_tmux_on(
            target.server(),
            ["switch-client", "-t", target.session_id.as_str()],
        )
    }

    pub fn attach_to_target(&self, target: &TmuxTarget) -> Result<()> {
        self.select_target(target)?;
        let mut command = tmux_command(
            target.server(),
            ["attach-session", "-t", target.session_id.as_str()],
        );
        let status = command
            .status()
            .context("failed to run tmux attach-session")?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow!("tmux attach-session exited with status {status}"))
        }
    }

    pub fn create_session(&self, name: &str) -> Result<()> {
        run_tmux_on(&self.server, ["new-session", "-d", "-s", name])
    }

    pub fn create_session_with(
        &self,
        name: &str,
        start_dir: Option<&str>,
        command: Option<&str>,
    ) -> Result<()> {
        let mut args = vec![
            "new-session".to_string(),
            "-d".to_string(),
            "-s".to_string(),
            name.to_string(),
        ];
        if let Some(start_dir) = start_dir.filter(|value| !value.trim().is_empty()) {
            args.push("-c".into());
            args.push(start_dir.to_string());
        }
        if let Some(command) = command.filter(|value| !value.trim().is_empty()) {
            args.push(command.to_string());
        }

        run_tmux_owned_on(&self.server, &args)
    }

    pub fn create_window(&self, target: &TmuxTarget, name: &str) -> Result<()> {
        run_tmux_on(
            target.server(),
            [
                "new-window",
                "-d",
                "-t",
                target.session_id.as_str(),
                "-n",
                name,
            ],
        )
    }

    pub fn create_window_with(
        &self,
        target: &TmuxTarget,
        name: &str,
        start_dir: Option<&str>,
        command: Option<&str>,
    ) -> Result<()> {
        let mut args = vec![
            "new-window".to_string(),
            "-d".to_string(),
            "-t".to_string(),
            target.session_id.clone(),
            "-n".to_string(),
            name.to_string(),
        ];
        if let Some(start_dir) = start_dir.filter(|value| !value.trim().is_empty()) {
            args.push("-c".into());
            args.push(start_dir.to_string());
        }
        if let Some(command) = command.filter(|value| !value.trim().is_empty()) {
            args.push(command.to_string());
        }
        run_tmux_owned_on(target.server(), &args)
    }

    pub fn split_pane(&self, target: &TmuxTarget, direction: SplitDirection) -> Result<()> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        let flag = match direction {
            SplitDirection::Horizontal => "-h",
            SplitDirection::Vertical => "-v",
        };
        run_tmux_on(target.server(), ["split-window", flag, "-t", pane_id])
    }

    pub fn rename_session(&self, target: &TmuxTarget, name: &str) -> Result<()> {
        run_tmux_on(
            target.server(),
            ["rename-session", "-t", target.session_id.as_str(), name],
        )
    }

    pub fn rename_window(&self, target: &TmuxTarget, name: &str) -> Result<()> {
        let window_id = target
            .window_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux window target is missing"))?;
        run_tmux_on(target.server(), ["rename-window", "-t", window_id, name])
    }

    pub fn rename_pane(&self, target: &TmuxTarget, title: &str) -> Result<()> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        run_tmux_on(target.server(), ["select-pane", "-t", pane_id, "-T", title])
    }

    pub fn kill_session(&self, target: &TmuxTarget) -> Result<()> {
        run_tmux_on(
            target.server(),
            ["kill-session", "-t", target.session_id.as_str()],
        )
    }

    pub fn kill_window(&self, target: &TmuxTarget) -> Result<()> {
        let window_id = target
            .window_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux window target is missing"))?;
        run_tmux_on(target.server(), ["kill-window", "-t", window_id])
    }

    pub fn kill_pane(&self, target: &TmuxTarget) -> Result<()> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        run_tmux_on(target.server(), ["kill-pane", "-t", pane_id])
    }

    pub fn toggle_zoom(&self, target: &TmuxTarget) -> Result<()> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        run_tmux_on(target.server(), ["resize-pane", "-Z", "-t", pane_id])
    }

    pub fn select_layout(&self, target: &TmuxTarget, layout: TmuxLayout) -> Result<()> {
        let window_id = target
            .window_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux window target is missing"))?;
        run_tmux_on(
            target.server(),
            ["select-layout", "-t", window_id, layout.tmux_name()],
        )
    }

    pub fn toggle_sync_panes(&self, target: &TmuxTarget) -> Result<()> {
        let window_id = target
            .window_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux window target is missing"))?;
        run_tmux_on(
            target.server(),
            ["set-window-option", "-t", window_id, "synchronize-panes"],
        )
    }

    pub fn rotate_window(&self, target: &TmuxTarget) -> Result<()> {
        let window_id = target
            .window_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux window target is missing"))?;
        run_tmux_on(target.server(), ["rotate-window", "-t", window_id])
    }

    pub fn swap_pane(&self, target: &TmuxTarget, next: bool) -> Result<()> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        let flag = if next { "-D" } else { "-U" };
        run_tmux_on(target.server(), ["swap-pane", flag, "-t", pane_id])
    }

    pub fn swap_window(&self, target: &TmuxTarget, next: bool) -> Result<()> {
        let window_id = target
            .window_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux window target is missing"))?;
        let offset = if next { "+1" } else { "-1" };
        run_tmux_on(
            target.server(),
            ["swap-window", "-d", "-s", window_id, "-t", offset],
        )
    }

    pub fn break_pane(&self, target: &TmuxTarget) -> Result<()> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        run_tmux_on(target.server(), ["break-pane", "-d", "-s", pane_id])
    }

    pub fn join_pane(&self, source: &TmuxTarget, dest_target: &str) -> Result<()> {
        let pane_id = source
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        run_tmux_on(
            source.server(),
            ["join-pane", "-s", pane_id, "-t", dest_target],
        )
    }

    pub fn toggle_mark_pane(&self, target: &TmuxTarget, currently_marked: bool) -> Result<()> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        let flag = if currently_marked { "-M" } else { "-m" };
        run_tmux_on(target.server(), ["select-pane", flag, "-t", pane_id])
    }

    pub fn swap_marked_pane(&self, target: &TmuxTarget) -> Result<()> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        run_tmux_on(target.server(), ["swap-pane", "-t", pane_id])
    }

    pub fn resize_pane(
        &self,
        target: &TmuxTarget,
        direction: ResizeDirection,
        amount: u16,
    ) -> Result<()> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        let flag = match direction {
            ResizeDirection::Left => "-L",
            ResizeDirection::Right => "-R",
            ResizeDirection::Up => "-U",
            ResizeDirection::Down => "-D",
        };
        let amount_str = amount.to_string();
        run_tmux_on(
            target.server(),
            ["resize-pane", "-t", pane_id, flag, amount_str.as_str()],
        )
    }

    pub fn clear_pane(&self, target: &TmuxTarget) -> Result<()> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        run_tmux_on(target.server(), ["send-keys", "-t", pane_id, "C-l"])?;
        run_tmux_on(target.server(), ["clear-history", "-t", pane_id])
    }

    pub fn respawn_pane(&self, target: &TmuxTarget) -> Result<()> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        run_tmux_on(target.server(), ["respawn-pane", "-k", "-t", pane_id])
    }

    pub fn send_keys(&self, target: &TmuxTarget, keys: &str) -> Result<()> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        run_tmux_on(target.server(), ["send-keys", "-t", pane_id, keys, "Enter"])
    }

    pub fn broadcast_keys(&self, targets: &[TmuxTarget], keys: &str) -> Result<usize> {
        let mut sent = 0usize;
        for target in targets {
            if let Some(pane_id) = target.pane_id.as_deref() {
                run_tmux_on(target.server(), ["send-keys", "-t", pane_id, keys, "Enter"])?;
                sent += 1;
            }
        }
        Ok(sent)
    }

    pub fn copy_pane_to_buffer(&self, target: &TmuxTarget) -> Result<()> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        let content = capture_pane_plain(target.server(), pane_id)?;
        let args = vec!["set-buffer".to_string(), content];
        run_tmux_owned_on(target.server(), &args)
    }

    pub fn capture_pane_plain_text(&self, target: &TmuxTarget) -> Result<String> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        capture_pane_plain(target.server(), pane_id)
    }

    pub fn capture_pane_scrollback(&self, target: &TmuxTarget, lines: usize) -> Result<String> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        let start_arg = format!("-{lines}");
        let output = tmux_output_on(
            target.server(),
            [
                "capture-pane",
                "-p",
                "-S",
                start_arg.as_str(),
                "-t",
                pane_id,
            ],
        )?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(normalize_captured_content(&stdout))
        } else {
            Err(anyhow!(
                "tmux capture-pane scrollback failed: {}",
                output_message(&output)
            ))
        }
    }

    pub fn paste_buffer(&self, target: &TmuxTarget, buffer: &TmuxBuffer) -> Result<()> {
        let pane_id = target
            .pane_id
            .as_deref()
            .ok_or_else(|| anyhow!("tmux pane target is missing"))?;
        run_tmux_on(
            &buffer.server,
            ["paste-buffer", "-b", buffer.name.as_str(), "-t", pane_id],
        )
    }

    pub fn delete_buffer(&self, buffer: &TmuxBuffer) -> Result<()> {
        run_tmux_on(
            &buffer.server,
            ["delete-buffer", "-b", buffer.name.as_str()],
        )
    }

    pub fn detach_client(&self) -> Result<()> {
        run_tmux_on(&self.server, ["detach-client"])
    }

    pub fn run_args(&self, args: &[String]) -> Result<()> {
        run_tmux_owned_on(&self.server, args)
    }

    fn select_target(&self, target: &TmuxTarget) -> Result<()> {
        if let Some(window_id) = &target.window_id {
            run_tmux_on(target.server(), ["select-window", "-t", window_id.as_str()])?;
        }
        if let Some(pane_id) = &target.pane_id {
            run_tmux_on(target.server(), ["select-pane", "-t", pane_id.as_str()])?;
        }
        Ok(())
    }

    fn capture_visible_content(&self, state: &mut TmuxState) {
        for session in &mut state.sessions {
            let server = session.server.clone();
            for pane in session
                .windows
                .iter_mut()
                .flat_map(|window| &mut window.panes)
            {
                pane.content = capture_pane_content(&server, &pane.id)
                    .unwrap_or_else(|error| format!("Unable to capture pane content: {error}"));
            }
        }
    }
}

/// Copy plain text into the OS clipboard across macOS and Linux.
pub fn copy_to_system_clipboard(text: &str) -> Result<&'static str> {
    #[cfg(target_os = "macos")]
    const CANDIDATES: &[(&str, &[&str])] = &[("pbcopy", &[])];

    #[cfg(not(target_os = "macos"))]
    const CANDIDATES: &[(&str, &[&str])] = &[
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];

    let mut last_error = None;
    for (program, args) in CANDIDATES {
        match Command::new(program)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    if let Err(err) = stdin.write_all(text.as_bytes()) {
                        last_error = Some(format!("{program}: {err}"));
                        continue;
                    }
                }
                match child.wait() {
                    Ok(status) if status.success() => return Ok(*program),
                    Ok(status) => last_error = Some(format!("{program} exited with {status}")),
                    Err(err) => last_error = Some(format!("{program}: {err}")),
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => last_error = Some(format!("{program}: {err}")),
        }
    }

    Err(anyhow!(
        "{}",
        last_error.unwrap_or_else(|| {
            "no clipboard utility found (tried pbcopy / wl-copy / xclip / xsel)".into()
        })
    ))
}

enum ServerLoad {
    State(TmuxState),
    Unavailable,
}

fn load_state_from_server(server: &TmuxServer) -> Result<ServerLoad> {
    let mut unparsed_output = false;

    for separator in [PRIMARY_SEP, FALLBACK_SEP] {
        let format = tmux_format(separator);
        let output = tmux_output_on(server, ["list-panes", "-a", "-F", format.as_str()])?;
        if !output.status.success() {
            let message = output_message(&output);
            if is_no_server_message(&message) {
                return Ok(ServerLoad::Unavailable);
            }
            return Err(anyhow!("tmux list-panes failed: {message}"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let state = parse_list_panes(&stdout, server.clone(), separator);
        if !state.sessions.is_empty() || stdout.trim().is_empty() {
            return Ok(ServerLoad::State(state));
        }
        unparsed_output = true;
    }

    if unparsed_output {
        Err(anyhow!(
            "tmux list-panes returned data, but lazytmux could not parse it"
        ))
    } else {
        Ok(ServerLoad::Unavailable)
    }
}

fn load_buffers_from_servers(servers: &[TmuxServer]) -> Vec<TmuxBuffer> {
    let mut buffers = Vec::new();
    let format =
        format!("#{{buffer_name}}{PRIMARY_SEP}#{{buffer_size}}{PRIMARY_SEP}#{{buffer_sample}}");

    for server in servers {
        let Ok(output) = tmux_output_on(server, ["list-buffers", "-F", format.as_str()]) else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines().filter(|l| !l.trim().is_empty()).take(24) {
            let parts: Vec<&str> = line.splitn(3, PRIMARY_SEP).collect();
            if parts.len() < 2 {
                continue;
            }
            let name = parts[0].to_string();
            let size = parts[1].parse::<usize>().unwrap_or(0);
            let sample = parts.get(2).copied().unwrap_or_default().to_string();
            let content = tmux_output_on(server, ["show-buffer", "-b", name.as_str()])
                .ok()
                .filter(|out| out.status.success())
                .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
                .unwrap_or_else(|| sample.clone());

            buffers.push(TmuxBuffer {
                server: server.clone(),
                name,
                size,
                sample,
                content,
            });
        }
    }

    buffers
}

fn detect_tmux_version() -> Option<String> {
    let output = Command::new("tmux").arg("-V").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn tmux_format(separator: char) -> String {
    let separator = separator.to_string();
    TMUX_FIELDS
        .iter()
        .map(|field| format!("#{{{field}}}"))
        .collect::<Vec<_>>()
        .join(&separator)
}

fn parse_list_panes(output: &str, server: TmuxServer, separator: char) -> TmuxState {
    let mut sessions: BTreeMap<String, Session> = BTreeMap::new();

    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split(separator).collect();
        if fields.len() < REQUIRED_FIELD_COUNT {
            continue;
        }

        let session_id = fields[0].to_string();
        let window_id = fields[5].to_string();
        let pane_id = fields[12].to_string();

        let session = sessions
            .entry(session_id.clone())
            .or_insert_with(|| Session {
                server: server.clone(),
                id: session_id.clone(),
                name: fields[1].to_string(),
                attached: parse_bool(fields[2]),
                created: fields[3].parse().ok(),
                activity: fields.get(28).and_then(|v| v.parse().ok()),
                window_count: fields[4].parse().ok(),
                windows: Vec::new(),
            });

        let window_zoomed = fields.get(25).copied().is_some_and(parse_bool);
        let pane_synced = fields.get(26).copied().is_some_and(parse_bool);
        let pane_marked = fields.get(27).copied().is_some_and(parse_bool);

        let window_index = session
            .windows
            .iter()
            .position(|window| window.id == window_id)
            .unwrap_or_else(|| {
                session.windows.push(Window {
                    id: window_id.clone(),
                    index: fields[6].to_string(),
                    name: fields[7].to_string(),
                    active: parse_bool(fields[8]),
                    zoomed: window_zoomed || fields[11].contains('Z'),
                    synchronized: pane_synced,
                    pane_count: fields[9].parse().ok(),
                    layout: fields[10].to_string(),
                    flags: fields[11].to_string(),
                    panes: Vec::new(),
                });
                session.windows.len() - 1
            });

        let window = &mut session.windows[window_index];
        if pane_synced {
            window.synchronized = true;
        }
        if window_zoomed {
            window.zoomed = true;
        }

        if !window.panes.iter().any(|pane| pane.id == pane_id) {
            window.panes.push(Pane {
                id: pane_id,
                index: fields[13].to_string(),
                active: parse_bool(fields[14]),
                marked: pane_marked,
                command: fields[15].to_string(),
                path: fields[16].to_string(),
                title: fields[17].to_string(),
                left: fields[18].parse().ok(),
                top: fields[19].parse().ok(),
                width: fields[20].parse().ok(),
                height: fields[21].parse().ok(),
                pid: fields[22].to_string(),
                dead: fields.get(23).copied().is_some_and(parse_bool),
                in_mode: fields.get(24).copied().is_some_and(parse_bool),
                content: String::new(),
            });
        }
    }

    let mut sessions: Vec<Session> = sessions.into_values().collect();
    sort_sessions(&mut sessions);

    TmuxState {
        sessions,
        buffers: Vec::new(),
        version: None,
        notice: None,
        diagnostics: Vec::new(),
    }
}

fn sort_sessions(sessions: &mut [Session]) {
    sessions.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    for session in sessions {
        session
            .windows
            .sort_by_key(|window| window.index.parse::<usize>().unwrap_or(usize::MAX));
        for window in &mut session.windows {
            window
                .panes
                .sort_by_key(|pane| pane.index.parse::<usize>().unwrap_or(usize::MAX));
        }
    }
}

fn parse_bool(value: &str) -> bool {
    matches!(value, "1" | "true" | "yes" | "on")
}

fn candidate_servers(
    active: &TmuxServer,
    extra_servers: &[TmuxServer],
    extra_socket_dirs: &[PathBuf],
) -> Vec<TmuxServer> {
    let mut servers = Vec::new();
    for server in extra_servers {
        push_unique_server(&mut servers, server.clone());
    }
    if !matches!(active, TmuxServer::Default) {
        push_unique_server(&mut servers, active.clone());
    }
    if let Some(server) = TmuxServer::from_tmux_env() {
        push_unique_server(&mut servers, server);
    }
    push_unique_server(&mut servers, TmuxServer::Default);
    for socket in discover_socket_paths(extra_socket_dirs) {
        push_unique_server(&mut servers, TmuxServer::Socket(socket));
    }
    servers
}

fn push_unique_server(servers: &mut Vec<TmuxServer>, server: TmuxServer) {
    if !servers.contains(&server) {
        servers.push(server);
    }
}

fn discover_socket_paths(extra_socket_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut sockets = Vec::new();
    for dir in socket_dirs(extra_socket_dirs) {
        collect_socket_paths(&dir, &mut sockets);
    }
    for root in socket_roots(extra_socket_dirs) {
        collect_likely_socket_paths(&root, &mut sockets);
    }
    collect_current_dir_socket_paths(&mut sockets);
    sockets.sort();
    sockets.dedup();
    sockets
}

fn socket_dirs(extra_socket_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let tmux_env_socket = TmuxServer::from_tmux_env().and_then(|server| match server {
        TmuxServer::Socket(socket) => socket.parent().map(Path::to_path_buf),
        TmuxServer::Name(_) => None,
        TmuxServer::Default => None,
    });

    let roots = socket_roots(extra_socket_dirs);
    let uid = current_uid();
    let mut dirs = Vec::new();
    for root in roots {
        if is_tmux_dir(&root) {
            push_unique_path(&mut dirs, root.clone());
        }
        if let Some(uid) = &uid {
            push_unique_path(&mut dirs, root.join(format!("tmux-{uid}")));
        }
        push_unique_path(&mut dirs, root.join("tmux"));
        collect_tmux_dirs(&root, &mut dirs);
    }
    if let Some(dir) = tmux_env_socket {
        push_unique_path(&mut dirs, dir);
    }
    dirs
}

fn socket_roots(extra_socket_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for dir in extra_socket_dirs {
        push_unique_path(&mut roots, dir.clone());
    }
    push_unique_path(&mut roots, env::temp_dir());
    push_unique_path(&mut roots, PathBuf::from("/tmp"));
    push_unique_path(&mut roots, PathBuf::from("/private/tmp"));
    if let Some(path) = env::var_os("TMUX_TMPDIR") {
        push_unique_path(&mut roots, PathBuf::from(path));
    }
    if let Some(path) = env::var_os("TMPDIR") {
        push_unique_path(&mut roots, PathBuf::from(path));
    }
    if let Some(path) = env::var_os("XDG_RUNTIME_DIR") {
        push_unique_path(&mut roots, PathBuf::from(path));
    }
    let uid = current_uid();
    if let Some(uid) = &uid {
        push_unique_path(&mut roots, PathBuf::from(format!("/run/user/{uid}")));
    }
    roots
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

fn env_paths(name: &str) -> Vec<PathBuf> {
    env::var_os(name)
        .map(|value| env::split_paths(&value).collect())
        .unwrap_or_default()
}

fn env_values(name: &str) -> Vec<String> {
    env::var(name)
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn current_uid() -> Option<String> {
    if let Some(uid) = env::var_os("UID").and_then(valid_uid) {
        return Some(uid);
    }

    let output = Command::new("id").arg("-u").output().ok()?;
    if !output.status.success() {
        return None;
    }

    valid_uid(OsString::from(
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}

fn valid_uid(value: OsString) -> Option<String> {
    let value = value.to_string_lossy().trim().to_string();
    if !value.is_empty() && value.chars().all(|character| character.is_ascii_digit()) {
        Some(value)
    } else {
        None
    }
}

fn collect_socket_paths(dir: &Path, sockets: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if is_socket_candidate(&file_type) {
            sockets.push(entry.path());
        }
    }
}

fn collect_likely_socket_paths(dir: &Path, sockets: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if is_strict_socket_candidate(&file_type) && is_likely_tmux_socket_path(&path) {
            sockets.push(path);
        }
    }
}

fn collect_current_dir_socket_paths(sockets: &mut Vec<PathBuf>) {
    let Ok(current_dir) = env::current_dir() else {
        return;
    };
    collect_strict_socket_paths(&current_dir, sockets);
}

fn collect_strict_socket_paths(dir: &Path, sockets: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if is_strict_socket_candidate(&file_type) {
            sockets.push(entry.path());
        }
    }
}

fn collect_tmux_dirs(root: &Path, dirs: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() && is_tmux_dir(&entry.path()) {
            push_unique_path(dirs, entry.path());
        }
    }
}

fn is_tmux_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("tmux"))
}

fn is_likely_tmux_socket_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains("tmux"))
}

#[cfg(unix)]
fn is_socket_candidate(file_type: &fs::FileType) -> bool {
    use std::os::unix::fs::FileTypeExt;

    file_type.is_socket() || file_type.is_symlink() || file_type.is_file()
}

#[cfg(unix)]
fn is_strict_socket_candidate(file_type: &fs::FileType) -> bool {
    use std::os::unix::fs::FileTypeExt;

    file_type.is_socket() || file_type.is_symlink()
}

#[cfg(not(unix))]
fn is_socket_candidate(_: &fs::FileType) -> bool {
    false
}

#[cfg(not(unix))]
fn is_strict_socket_candidate(_: &fs::FileType) -> bool {
    false
}

fn server_identity(server: &TmuxServer) -> String {
    if let Some(path) = server_socket_path(server) {
        return format!("socket:{}", path_key(&path));
    }

    match server {
        TmuxServer::Default => "default".into(),
        TmuxServer::Name(name) => format!("name:{name}"),
        TmuxServer::Socket(path) => format!("socket:{}", path_key(path)),
    }
}

fn server_socket_path(server: &TmuxServer) -> Option<PathBuf> {
    let output = tmux_output_on(server, ["display-message", "-p", "#{socket_path}"]).ok()?;
    if !output.status.success() {
        return None;
    }

    let socket_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if socket_path.is_empty() || socket_path.contains("#{") {
        None
    } else {
        Some(PathBuf::from(socket_path))
    }
}

fn path_key(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn tmux_command<I, S>(server: &TmuxServer, args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("tmux");
    if matches!(server, TmuxServer::Default) {
        command.env_remove("TMUX");
    }
    command.args(tmux_command_args(server, args));
    command
}

fn tmux_command_args<I, S>(server: &TmuxServer, args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command_args = Vec::new();
    match server {
        TmuxServer::Default => {}
        TmuxServer::Name(name) => {
            command_args.push(OsString::from("-L"));
            command_args.push(OsString::from(name));
        }
        TmuxServer::Socket(socket) => {
            command_args.push(OsString::from("-S"));
            command_args.push(socket.as_os_str().to_os_string());
        }
    }
    command_args.extend(args.into_iter().map(|arg| arg.as_ref().to_os_string()));
    command_args
}

fn tmux_output_on<I, S>(server: &TmuxServer, args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    tmux_command(server, args)
        .output()
        .context("failed to run tmux")
}

fn run_tmux_on<I, S>(server: &TmuxServer, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = tmux_output_on(server, args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow!("tmux command failed: {}", output_message(&output)))
    }
}

fn run_tmux_owned_on(server: &TmuxServer, args: &[String]) -> Result<()> {
    let output = tmux_output_on(server, args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow!("tmux command failed: {}", output_message(&output)))
    }
}

/// Captures visible pane screen WITH ANSI color sequences (`-e`), falling back
/// to plain capture if `-e` is unsupported.
fn capture_pane_content(server: &TmuxServer, pane_id: &str) -> Result<String> {
    let output = tmux_output_on(server, ["capture-pane", "-p", "-e", "-t", pane_id])?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Ok(normalize_captured_content(&stdout));
    }
    capture_pane_plain(server, pane_id)
}

fn capture_pane_plain(server: &TmuxServer, pane_id: &str) -> Result<String> {
    let output = tmux_output_on(server, ["capture-pane", "-p", "-t", pane_id])?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(normalize_captured_content(&stdout))
    } else {
        Err(anyhow!(
            "tmux capture-pane failed: {}",
            output_message(&output)
        ))
    }
}

fn normalize_captured_content(content: &str) -> String {
    let mut lines: Vec<String> = content
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

fn output_message(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn is_no_server_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("no server running")
        || message.contains("failed to connect")
        || message.contains("error connecting")
        || message.contains("no such file or directory")
}

fn unreachable_server_notice() -> String {
    if tmux_processes_running() {
        return "No reachable tmux socket found, but tmux processes are running. The socket may be outside scanned paths or deleted; try --socket PATH.".into();
    }

    "No tmux server is running. Press n to create a session.".into()
}

fn tmux_processes_running() -> bool {
    let Ok(output) = Command::new("pgrep").args(["-x", "tmux"]).output() else {
        return false;
    };

    output.status.success() && !String::from_utf8_lossy(&output.stdout).trim().is_empty()
}

fn is_not_found(error: &anyhow::Error) -> bool {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<io::Error>())
        .is_some_and(|io_error| io_error.kind() == io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list_panes_output() {
        let row = [
            "$1",
            "dev",
            "1",
            "1700000000",
            "1",
            "@2",
            "0",
            "editor",
            "1",
            "1",
            "layout",
            "*",
            "%3",
            "0",
            "1",
            "nvim",
            "/tmp",
            "title",
            "0",
            "0",
            "120",
            "40",
            "123",
            "0",
            "0",
        ]
        .join(&PRIMARY_SEP.to_string());

        let server = TmuxServer::Socket(PathBuf::from("/tmp/tmux-1000/default"));
        let state = parse_list_panes(&row, server.clone(), PRIMARY_SEP);

        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].server, server);
        assert_eq!(state.sessions[0].windows.len(), 1);
        assert_eq!(state.sessions[0].windows[0].panes.len(), 1);
        assert_eq!(state.sessions[0].windows[0].panes[0].command, "nvim");
    }

    #[test]
    fn parses_extended_flags_zoom_sync_marked() {
        let row = [
            "$1",
            "dev",
            "1",
            "1700000000",
            "1",
            "@2",
            "0",
            "editor",
            "1",
            "1",
            "tiled",
            "*Z",
            "%3",
            "0",
            "1",
            "nvim",
            "/tmp",
            "title",
            "0",
            "0",
            "120",
            "40",
            "123",
            "0",
            "0",
            "1",
            "1",
            "1",
            "1700000500",
        ]
        .join(&PRIMARY_SEP.to_string());

        let state = parse_list_panes(&row, TmuxServer::Default, PRIMARY_SEP);
        assert_eq!(state.sessions[0].activity, Some(1_700_000_500));
        assert!(state.sessions[0].windows[0].zoomed);
        assert!(state.sessions[0].windows[0].synchronized);
        assert!(state.sessions[0].windows[0].panes[0].marked);
    }

    #[test]
    fn parses_list_panes_output_with_fallback_separator() {
        let row = [
            "$1",
            "dev",
            "1",
            "1700000000",
            "1",
            "@2",
            "0",
            "editor",
            "1",
            "1",
            "layout",
            "*",
            "%3",
            "0",
            "1",
            "nvim",
            "/tmp",
            "title",
            "0",
            "0",
            "120",
            "40",
            "123",
            "0",
            "0",
        ]
        .join(&FALLBACK_SEP.to_string());

        let state = parse_list_panes(&row, TmuxServer::Default, FALLBACK_SEP);

        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].name, "dev");
        assert_eq!(state.sessions[0].windows[0].panes[0].command, "nvim");
    }

    #[test]
    fn prefixes_socket_server_args() {
        let args = tmux_command_args(
            &TmuxServer::Socket(PathBuf::from("/tmp/tmux-1000/default")),
            ["list-panes", "-a"],
        );

        assert_eq!(
            args,
            vec![
                OsString::from("-S"),
                OsString::from("/tmp/tmux-1000/default"),
                OsString::from("list-panes"),
                OsString::from("-a"),
            ]
        );
    }

    #[test]
    fn prefixes_named_server_args() {
        let args = tmux_command_args(&TmuxServer::Name("work".into()), ["list-panes", "-a"]);

        assert_eq!(
            args,
            vec![
                OsString::from("-L"),
                OsString::from("work"),
                OsString::from("list-panes"),
                OsString::from("-a"),
            ]
        );
    }

    #[test]
    fn leaves_default_server_args_unprefixed() {
        let args = tmux_command_args(&TmuxServer::Default, ["list-panes", "-a"]);

        assert_eq!(
            args,
            vec![OsString::from("list-panes"), OsString::from("-a")]
        );
    }

    #[test]
    fn identifies_only_tmux_socket_dirs() {
        assert!(is_tmux_dir(Path::new("/tmp/tmux-1000")));
        assert!(is_tmux_dir(Path::new("/run/user/1000/tmux")));
        assert!(!is_tmux_dir(Path::new("/run/user/1000/bus")));
        assert!(!is_tmux_dir(Path::new("/run/user/1000/systemd")));
    }

    #[test]
    fn prioritizes_explicit_candidate_servers() {
        let explicit = vec![TmuxServer::Name("work".into())];
        let servers = candidate_servers(&TmuxServer::Default, &explicit, &[]);

        assert_eq!(servers.first(), Some(&TmuxServer::Name("work".into())));
        assert!(servers.contains(&TmuxServer::Default));
    }

    #[test]
    fn identifies_likely_top_level_tmux_socket_names() {
        assert!(is_likely_tmux_socket_path(Path::new("/tmp/my_tmux_socket")));
        assert!(!is_likely_tmux_socket_path(Path::new("/tmp/project")));
    }

    #[test]
    fn collects_non_directory_candidates_from_tmux_dirs() {
        let root = env::temp_dir().join(format!("lazytmux-test-{}", std::process::id()));
        let tmux_dir = root.join("tmux-9999");
        let socket_candidate = tmux_dir.join("default");
        fs::create_dir_all(&tmux_dir).expect("create tmux test dir");
        fs::write(&socket_candidate, "").expect("create socket candidate");

        let mut sockets = Vec::new();
        collect_socket_paths(&tmux_dir, &mut sockets);

        fs::remove_dir_all(&root).expect("remove tmux test dir");
        assert!(sockets.contains(&socket_candidate));
    }

    #[cfg(unix)]
    #[test]
    fn collects_strict_socket_candidates_from_arbitrary_dirs() {
        use std::os::unix::net::UnixListener;

        let root = env::temp_dir().join(format!("lazytmux-strict-test-{}", std::process::id()));
        let socket_candidate = root.join("project_session");
        let regular_file = root.join("regular_file");
        fs::create_dir_all(&root).expect("create strict socket test dir");
        let listener = UnixListener::bind(&socket_candidate).expect("create unix socket");
        fs::write(&regular_file, "not a socket").expect("create regular file");

        let mut sockets = Vec::new();
        collect_strict_socket_paths(&root, &mut sockets);

        drop(listener);
        fs::remove_dir_all(&root).expect("remove strict socket test dir");
        assert!(sockets.contains(&socket_candidate));
        assert!(!sockets.contains(&regular_file));
    }

    #[cfg(unix)]
    #[test]
    fn collects_likely_tmux_sockets_from_temp_roots() {
        use std::os::unix::net::UnixListener;

        let root = env::temp_dir().join(format!("lazytmux-likely-test-{}", std::process::id()));
        let tmux_socket = root.join("my_tmux_socket");
        let other_socket = root.join("project_session");
        fs::create_dir_all(&root).expect("create likely socket test dir");
        let tmux_listener = UnixListener::bind(&tmux_socket).expect("create tmux socket");
        let other_listener = UnixListener::bind(&other_socket).expect("create other socket");

        let mut sockets = Vec::new();
        collect_likely_socket_paths(&root, &mut sockets);

        drop(tmux_listener);
        drop(other_listener);
        fs::remove_dir_all(&root).expect("remove likely socket test dir");
        assert!(sockets.contains(&tmux_socket));
        assert!(!sockets.contains(&other_socket));
    }

    #[test]
    fn normalizes_captured_content() {
        assert_eq!(normalize_captured_content("line one   \n\n"), "line one");
    }
}
