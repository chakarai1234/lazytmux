mod ansi;
mod app;
mod keybindings;
mod metrics;
mod theme;
mod tmux;
mod ui;

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use app::{App, ViewMode};
use clap::Parser;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use theme::ThemeId;
use tmux::{TmuxClient, TmuxClientOptions};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Futuristic Cyber-Deck TUI framework & controller for tmux"
)]
struct Cli {
    #[arg(long, default_value_t = 2, help = "Auto-refresh interval in seconds")]
    refresh_seconds: u64,

    #[arg(long, help = "Disable mouse capture")]
    no_mouse: bool,

    #[arg(
        long,
        value_enum,
        help = "Override UI theme (cyber, aurora, solaris, matrix, frost)"
    )]
    theme: Option<ThemeId>,

    #[arg(
        long,
        value_enum,
        default_value_t = ViewMode::Deck,
        help = "Initial inspector deck (deck, live, telemetry, buffers)"
    )]
    view: ViewMode,

    #[arg(
        short = 'S',
        long = "socket",
        value_name = "PATH",
        help = "Add an explicit tmux socket path to scan"
    )]
    sockets: Vec<PathBuf>,

    #[arg(
        short = 'L',
        long = "socket-name",
        value_name = "NAME",
        help = "Add a tmux -L socket name to scan"
    )]
    socket_names: Vec<String>,

    #[arg(
        long = "socket-dir",
        value_name = "DIR",
        help = "Add a directory or tmux socket root to scan"
    )]
    socket_dirs: Vec<PathBuf>,
}

impl Cli {
    fn tmux_options(&self) -> TmuxClientOptions {
        let mut options = TmuxClientOptions::from_env();
        options.socket_paths.extend(self.sockets.iter().cloned());
        options
            .socket_names
            .extend(self.socket_names.iter().cloned());
        options.socket_dirs.extend(self.socket_dirs.iter().cloned());
        options
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    install_panic_hook(cli.no_mouse);

    let tmux_options = cli.tmux_options();
    let mut terminal = setup_terminal(cli.no_mouse)?;
    let mut app = App::with_options(
        Duration::from_secs(cli.refresh_seconds.max(1)),
        TmuxClient::with_options(tmux_options),
        cli.theme,
        cli.view,
    );

    let run_result = app.run(&mut terminal);
    let attach_target = app.take_attach_target();
    let restore_result = restore_terminal(&mut terminal, cli.no_mouse);

    restore_result?;
    run_result?;

    if let Some(target) = attach_target {
        TmuxClient::default().attach_to_target(&target)?;
    }

    Ok(())
}

fn install_panic_hook(no_mouse: bool) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        if no_mouse {
            let _ = execute!(stdout, LeaveAlternateScreen);
        } else {
            let _ = execute!(stdout, LeaveAlternateScreen, DisableMouseCapture);
        }
        previous(panic_info);
    }));
}

fn setup_terminal(no_mouse: bool) -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if no_mouse {
        execute!(stdout, EnterAlternateScreen)?;
    } else {
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    Ok(terminal)
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    no_mouse: bool,
) -> Result<()> {
    disable_raw_mode()?;
    if no_mouse {
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    } else {
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
    }
    terminal.show_cursor()?;
    Ok(())
}
