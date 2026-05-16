# lazytmux

`lazytmux` is a Rust TUI for tmux that shows sessions, windows, and
panes in a lazygit/lazydocker-style interface.
Selecting a session, window, or pane shows a terminal-style actual view
built from the current tmux pane layout and captured pane screens.

## Requirements

- Rust toolchain with `cargo`
- `tmux` available in `PATH`
- A terminal with ANSI support

## Run

```sh
cargo run --release
```

## Install

```sh
./install.sh
```

The installer builds a release binary with Cargo and installs it to
`~/.local/bin/lazytmux`.

## Keys

- `j` / `Down`: move down
- `k` / `Up`: move up
- `g` / `G`: jump to top or bottom
- `[` / `]` or `Ctrl-U` / `Ctrl-D`: scroll details
- `Space`: expand or collapse selected session/window
- `h` / `Left`: collapse selected session/window
- `l` / `Right`: expand selected session/window
- `Enter`: switch to the selected tmux target, or attach from outside tmux
- `/`: filter tree
- `f`: clear filter
- `R`: refresh now
- `n`: create session
- `w`: create window in selected session
- `%`: split selected pane left/right
- `"`: split selected pane top/bottom
- `Cmd-R` or `r`: rename selected session/window or set pane title
- `x`: kill selected session/window/pane after confirmation
- `z`: toggle zoom on selected pane
- `d`: detach current tmux client
- `:`: run an arbitrary tmux command
- `?` / `F1`: show the shortcuts page
- `j` / `k`, `Up` / `Down`, `PageUp` / `PageDown`, `[` / `]`, or
  `Ctrl-U` / `Ctrl-D`: scroll the shortcuts page while it is open
- `?` / `F1` / `q` / `Esc`: close the shortcuts page while it is open
- `q` / `Esc` / `Ctrl-C`: quit
