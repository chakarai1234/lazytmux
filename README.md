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

`lazytmux` scans the default tmux server, `$TMUX`, common Linux and macOS
socket locations, `$TMUX_TMPDIR`, `$TMPDIR`, `$XDG_RUNTIME_DIR`, real Unix
sockets in the current directory, and likely shared tmux sockets in temp
roots. For custom tmux layouts, add explicit socket inputs:

```sh
lazytmux --socket /tmp/tmux-1000/default
lazytmux --socket-name work
lazytmux --socket-dir /run/user/1000
```

The same values may be provided with `LAZYTMUX_SOCKET`, `LAZYTMUX_SOCKETS`,
`LAZYTMUX_SOCKET_NAME`, `LAZYTMUX_SOCKET_NAMES`, `LAZYTMUX_SOCKET_DIR`, or
`LAZYTMUX_SOCKET_DIRS`.

This covers common `tmux -S name` mistakes where the socket was created in
the launch directory, and shared socket setups such as `/tmp/my_tmux_socket`.

## Features

- Multi-server Linux/macOS discovery with diagnostics (`D`)
- Server labels in session/window/pane details
- Persistent favorites pinned to the top (`*`)
- Session launcher presets (`N`) with `name | start-dir | command`
- Pane actions: send keys (`s`) and copy pane text to tmux buffer (`y`)
- Fuzzy multi-token search across names, commands, paths, titles, IDs, and sockets

## Install

```sh
./install.sh
```

The installer builds a release binary with Cargo and installs it to
`~/.local/bin/lazytmux`.

## Install from a release

Download the matching archive from the GitHub release assets:

- `lazytmux-linux-x86_64.tar.gz` for Linux
- `lazytmux-macos-universal.tar.gz` for macOS Intel and Apple Silicon

```sh
tar -xzf lazytmux-linux-x86_64.tar.gz
install -m 755 lazytmux-linux-x86_64/lazytmux ~/.local/bin/lazytmux
```

For macOS, replace the archive and directory names with
`lazytmux-macos-universal`.

## Release

Releases are created manually from the GitHub Actions `Release` workflow.

1. Update `Cargo.toml` with the release version and commit the change.
2. Push the commit to the release branch, normally `main`.
3. Open GitHub Actions, select `Release`, then choose `Run workflow`.
4. Enter a tag such as `v0.1.0` and choose whether it is a prerelease.
5. Run the workflow and wait for the Linux and macOS build jobs to finish.
6. Confirm the GitHub release contains `lazytmux-linux-x86_64.tar.gz` and
   `lazytmux-macos-universal.tar.gz`.

## Keys

- `j` / `Down`: move down
- `k` / `Up`: move up
- `g` / `G`: jump to top or bottom
- `[` / `]` or `Ctrl-U` / `Ctrl-D`: scroll details
- `Space`: expand or collapse selected session/window
- `h` / `Left`: collapse selected session/window
- `l` / `Right`: expand selected session/window
- `Enter`: switch to the selected tmux target, or attach from outside tmux
- `/`: fuzzy multi-token filter tree
- `f`: clear filter
- `R`: refresh now
- `n`: create session
- `N`: launch session from `name | start-dir | command`
- `w`: create window in selected session
- `%`: split selected pane left/right
- `"`: split selected pane top/bottom
- `Cmd-R` or `r`: rename selected session/window or set pane title
- `x`: kill selected session/window/pane after confirmation
- `z`: toggle zoom on selected pane
- `*`: toggle favorite and pin item near the top
- `s`: send keys to selected pane, followed by Enter
- `y`: copy selected pane text to the tmux buffer
- `D`: show tmux diagnostics
- `d`: detach current tmux client
- `:`: run an arbitrary tmux command
- `?` / `F1`: show the shortcuts page
- `j` / `k`, `Up` / `Down`, `PageUp` / `PageDown`, `[` / `]`, or
  `Ctrl-U` / `Ctrl-D`: scroll the shortcuts page while it is open
- `?` / `F1` / `q` / `Esc`: close the shortcuts page while it is open
- `q` / `Esc` / `Ctrl-C`: quit
