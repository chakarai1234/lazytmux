use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io;
use std::process::{Command, Output};

use anyhow::{anyhow, Context, Result};

const SEP: char = '\u{1f}';
const TMUX_FORMAT: &str = "#{session_id}\u{1f}#{session_name}\u{1f}#{session_attached}\u{1f}#{session_created}\u{1f}#{session_windows}\u{1f}#{window_id}\u{1f}#{window_index}\u{1f}#{window_name}\u{1f}#{window_active}\u{1f}#{window_panes}\u{1f}#{window_layout}\u{1f}#{window_flags}\u{1f}#{pane_id}\u{1f}#{pane_index}\u{1f}#{pane_active}\u{1f}#{pane_current_command}\u{1f}#{pane_current_path}\u{1f}#{pane_title}\u{1f}#{pane_left}\u{1f}#{pane_top}\u{1f}#{pane_width}\u{1f}#{pane_height}\u{1f}#{pane_pid}\u{1f}#{pane_dead}\u{1f}#{pane_in_mode}";
const FIELD_COUNT: usize = 25;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TmuxState {
    pub sessions: Vec<Session>,
    pub notice: Option<String>,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub attached: bool,
    pub created: Option<i64>,
    pub window_count: Option<usize>,
    pub windows: Vec<Window>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Window {
    pub id: String,
    pub index: String,
    pub name: String,
    pub active: bool,
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
    pub session_id: String,
    pub window_id: Option<String>,
    pub pane_id: Option<String>,
}

impl TmuxTarget {
    pub fn session(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            window_id: None,
            pane_id: None,
        }
    }

    pub fn window(session_id: impl Into<String>, window_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            window_id: Some(window_id.into()),
            pane_id: None,
        }
    }

    pub fn pane(
        session_id: impl Into<String>,
        window_id: impl Into<String>,
        pane_id: impl Into<String>,
    ) -> Self {
        Self {
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, Default)]
pub struct TmuxClient;

impl TmuxClient {
    pub fn load(&self) -> Result<TmuxState> {
        let output = match tmux_output(["list-panes", "-a", "-F", TMUX_FORMAT]) {
            Ok(output) => output,
            Err(error) if is_not_found(&error) => {
                return Err(anyhow!("tmux executable was not found in PATH"));
            }
            Err(error) => return Err(error),
        };

        if !output.status.success() {
            let message = output_message(&output);
            if message.contains("no server running") || message.contains("failed to connect") {
                return Ok(TmuxState {
                    sessions: Vec::new(),
                    notice: Some("No tmux server is running. Press n to create a session.".into()),
                });
            }
            return Err(anyhow!("tmux list-panes failed: {message}"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut state = parse_list_panes(&stdout);
        self.capture_visible_content(&mut state);
        if state.sessions.is_empty() {
            state.notice = Some("No tmux sessions found. Press n to create a session.".into());
        }
        Ok(state)
    }

    pub fn switch_to_target(&self, target: &TmuxTarget) -> Result<()> {
        self.select_target(target)?;
        run_tmux(["switch-client", "-t", target.session_id.as_str()])
    }

    pub fn attach_to_target(&self, target: &TmuxTarget) -> Result<()> {
        self.select_target(target)?;
        let status = Command::new("tmux")
            .args(["attach-session", "-t", target.session_id.as_str()])
            .status()
            .context("failed to run tmux attach-session")?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow!("tmux attach-session exited with status {status}"))
        }
    }

    pub fn create_session(&self, name: &str) -> Result<()> {
        run_tmux(["new-session", "-d", "-s", name])
    }

    pub fn create_window(&self, session_id: &str, name: &str) -> Result<()> {
        run_tmux(["new-window", "-d", "-t", session_id, "-n", name])
    }

    pub fn split_pane(&self, pane_id: &str, direction: SplitDirection) -> Result<()> {
        let flag = match direction {
            SplitDirection::Horizontal => "-h",
            SplitDirection::Vertical => "-v",
        };
        run_tmux(["split-window", flag, "-t", pane_id])
    }

    pub fn rename_session(&self, session_id: &str, name: &str) -> Result<()> {
        run_tmux(["rename-session", "-t", session_id, name])
    }

    pub fn rename_window(&self, window_id: &str, name: &str) -> Result<()> {
        run_tmux(["rename-window", "-t", window_id, name])
    }

    pub fn rename_pane(&self, pane_id: &str, title: &str) -> Result<()> {
        run_tmux(["select-pane", "-t", pane_id, "-T", title])
    }

    pub fn kill_session(&self, session_id: &str) -> Result<()> {
        run_tmux(["kill-session", "-t", session_id])
    }

    pub fn kill_window(&self, window_id: &str) -> Result<()> {
        run_tmux(["kill-window", "-t", window_id])
    }

    pub fn kill_pane(&self, pane_id: &str) -> Result<()> {
        run_tmux(["kill-pane", "-t", pane_id])
    }

    pub fn toggle_zoom(&self, pane_id: &str) -> Result<()> {
        run_tmux(["resize-pane", "-Z", "-t", pane_id])
    }

    pub fn detach_client(&self) -> Result<()> {
        run_tmux(["detach-client"])
    }

    pub fn run_args(&self, args: &[String]) -> Result<()> {
        run_tmux_owned(args)
    }

    fn select_target(&self, target: &TmuxTarget) -> Result<()> {
        if let Some(window_id) = &target.window_id {
            run_tmux(["select-window", "-t", window_id.as_str()])?;
        }
        if let Some(pane_id) = &target.pane_id {
            run_tmux(["select-pane", "-t", pane_id.as_str()])?;
        }
        Ok(())
    }

    fn capture_visible_content(&self, state: &mut TmuxState) {
        for pane in state
            .sessions
            .iter_mut()
            .flat_map(|session| &mut session.windows)
            .flat_map(|window| &mut window.panes)
        {
            pane.content = capture_pane_content(&pane.id)
                .unwrap_or_else(|error| format!("Unable to capture pane content: {error}"));
        }
    }
}

fn parse_list_panes(output: &str) -> TmuxState {
    let mut sessions: BTreeMap<String, Session> = BTreeMap::new();

    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split(SEP).collect();
        if fields.len() < FIELD_COUNT {
            continue;
        }

        let session_id = fields[0].to_string();
        let window_id = fields[5].to_string();
        let pane_id = fields[12].to_string();

        let session = sessions
            .entry(session_id.clone())
            .or_insert_with(|| Session {
                id: session_id.clone(),
                name: fields[1].to_string(),
                attached: parse_bool(fields[2]),
                created: fields[3].parse().ok(),
                window_count: fields[4].parse().ok(),
                windows: Vec::new(),
            });

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
                    pane_count: fields[9].parse().ok(),
                    layout: fields[10].to_string(),
                    flags: fields[11].to_string(),
                    panes: Vec::new(),
                });
                session.windows.len() - 1
            });

        let window = &mut session.windows[window_index];
        if !window.panes.iter().any(|pane| pane.id == pane_id) {
            window.panes.push(Pane {
                id: pane_id,
                index: fields[13].to_string(),
                active: parse_bool(fields[14]),
                command: fields[15].to_string(),
                path: fields[16].to_string(),
                title: fields[17].to_string(),
                left: fields[18].parse().ok(),
                top: fields[19].parse().ok(),
                width: fields[20].parse().ok(),
                height: fields[21].parse().ok(),
                pid: fields[22].to_string(),
                dead: parse_bool(fields[23]),
                in_mode: parse_bool(fields[24]),
                content: String::new(),
            });
        }
    }

    let mut sessions: Vec<Session> = sessions.into_values().collect();
    for session in &mut sessions {
        session
            .windows
            .sort_by_key(|window| window.index.parse::<usize>().unwrap_or(usize::MAX));
        for window in &mut session.windows {
            window
                .panes
                .sort_by_key(|pane| pane.index.parse::<usize>().unwrap_or(usize::MAX));
        }
    }

    TmuxState {
        sessions,
        notice: None,
    }
}

fn parse_bool(value: &str) -> bool {
    matches!(value, "1" | "true" | "yes" | "on")
}

fn tmux_output<I, S>(args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("tmux")
        .args(args)
        .output()
        .context("failed to run tmux")
}

fn run_tmux<I, S>(args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = tmux_output(args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow!("tmux command failed: {}", output_message(&output)))
    }
}

fn run_tmux_owned(args: &[String]) -> Result<()> {
    let output = tmux_output(args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow!("tmux command failed: {}", output_message(&output)))
    }
}

fn capture_pane_content(pane_id: &str) -> Result<String> {
    let output = tmux_output(["capture-pane", "-p", "-t", pane_id])?;
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
        .join("\u{1f}");

        let state = parse_list_panes(&row);

        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].windows.len(), 1);
        assert_eq!(state.sessions[0].windows[0].panes.len(), 1);
        assert_eq!(state.sessions[0].windows[0].panes[0].command, "nvim");
    }

    #[test]
    fn normalizes_captured_content() {
        assert_eq!(normalize_captured_content("line one   \n\n"), "line one");
    }
}
