# lazytmux

`lazytmux` is a futuristic, cyber-deck Rust TUI framework and command center for
`tmux`. Built with `ratatui` + `crossterm`, it combines a fast lazygit-style
topology workflow with live **ANSI truecolor** multi-pane terminal previews,
real-time CPU/memory sparklines, a fuzzy Command Palette, visual layout
schematics, paste buffer vault, and full mouse support across **Linux** and
**macOS**.

Thanks to `lazygit` and `lazydocker` for the inspiration.

## Requirements

- Rust toolchain with `cargo`
- `tmux` available in `PATH`
- A terminal with ANSI / 24-bit color support

## Run

```sh
cargo run --release
```

Choose a theme or initial deck directly from the CLI:

```sh
lazytmux --theme cyber       # cyber | aurora | solaris | matrix | frost
lazytmux --view telemetry    # deck | live | telemetry | buffers
lazytmux --refresh-seconds 2
lazytmux --no-mouse
```

`lazytmux` automatically scans the default tmux server, `$TMUX`, common Linux
and macOS socket locations, `$TMUX_TMPDIR`, `$TMPDIR`, `$XDG_RUNTIME_DIR`,
`/run/user/$UID`, real Unix sockets in the current directory, and likely shared
tmux sockets in temp roots. For custom tmux layouts, add explicit socket inputs:

```sh
lazytmux --socket /tmp/tmux-1000/default
lazytmux --socket-name work
lazytmux --socket-dir /run/user/1000
```

The same values may be provided with `LAZYTMUX_SOCKET`, `LAZYTMUX_SOCKETS`,
`LAZYTMUX_SOCKET_NAME`, `LAZYTMUX_SOCKET_NAMES`, `LAZYTMUX_SOCKET_DIR`, or
`LAZYTMUX_SOCKET_DIRS`.

## Highlights

- **5 Futuristic Themes** (`t` to cycle live, persisted across runs):
  `CYBER NEON`, `AURORA PULSE`, `SOLARIS AMBER`, `MATRIX GRID`, `FROST MINIMAL`
- **Live ANSI Truecolor Multi-Pane Viewports**: Captures `tmux capture-pane -e`
  with 16/256/24-bit RGB SGR parsing scaled to the exact tmux split geometry
- **4 Switchable Inspector Decks** (`1`-`4` or mouse click):
  - `1 DECK`: Telemetry summary card + scaled live ANSI multi-pane preview
  - `2 LIVE`: Edge-to-edge live ANSI terminal viewport (pair with `v` Zen Mode)
  - `3 TELEMETRY`: Live CPU/MEM rolling sparklines, host node card, and sortable per-pane process matrix
  - `4 BUFFERS`: Tmux paste buffer vault (preview, paste into pane, copy to clipboard, delete)
- **Command Palette (`Ctrl+P` / `p`) & Quick Target Jump (`Ctrl+S`)**: Fuzzy search
  over 50+ built-in actions or jump directly into any session/window/pane across servers
- **Visual Layout Engine (`L`)**: Interactive Unicode schematic preview for all 5 tmux layouts (`1`-`5`)
- **Deep Telemetry**: Rolling CPU/RAM sparklines in the HUD + deep process-tree CPU/RAM per pane/window/session
- **Power Pane & Window Orchestration**: Broadcast commands (`S`), synchronize-panes (`b`),
  break/join/rotate/swap/mark/resize/clear/respawn panes, scrollback snapshot export (`C`),
  and cross-platform system clipboard copy (`Y` via `pbcopy` on macOS, `wl-copy`/`xclip`/`xsel` on Linux)
- **Full Mouse Support**: Click tree rows (double-click to expand/attach), click tabs,
  click preview panes to focus them, click HUD pills to cycle themes or pause/resume, and wheel scroll
- **Multi-Server Linux & macOS Discovery** with Diagnostics Deck (`D`)

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

Every push and pull request to `main` automatically runs tests and builds both
Linux (`x86_64`) and macOS universal (`x86_64` + `aarch64`) packages via GitHub
Actions.

To publish a GitHub release:

1. Update `Cargo.toml` and `Cargo.lock` with the release version and push to `main`.
2. Push a version tag such as `0.1.4` (`git tag 0.1.4 && git push origin 0.1.4`),
   or trigger the `Build and Release` workflow manually from GitHub Actions.
3. Wait for `build-linux`, `build-macos`, and `release` jobs to finish and verify
   the release contains `lazytmux-linux-x86_64.tar.gz` and
   `lazytmux-macos-universal.tar.gz`.

## Shortcuts

Press `Ctrl+P` (or `p`) anytime for the interactive Command Palette, or `?` / `F1`
for the full Shortcuts Atlas inside the TUI.

### Navigation & Tree
- `j` / `k` or `Down` / `Up`: move selection (or scroll inspector when focused)
- `PageUp` / `PageDown`, `g` / `G`: page jump, top / bottom
- `Space`: expand or collapse selected session/window
- `h` / `Left`: collapse node (or jump to parent)
- `l` / `Right`: expand node
- `e` / `c`: expand all / collapse all
- `Tab`: switch focus between Topology Tree and Inspector Deck
- `Ctrl+S`: Quick Target Jump across all sessions, windows, and panes

### Viewports & HUD
- `1` / `2` / `3` / `4`: switch Inspector Deck (`Deck`, `Live`, `Telemetry`, `Buffers`)
- `v`: toggle Zen Viewport mode (maximize right deck edge-to-edge)
- `t`: cycle futuristic UI theme (persisted)
- `O`: cycle tree sort order (`NAME`, `CPU↓`, `MEM↓`, `ACTIVE`)
- `F`: toggle favorites-only view
- `P`: pause / resume live auto-refresh
- `[` / `]` or `Ctrl+U` / `Ctrl+D`: scroll inspector / preview
- `0`: reset Zen mode, view, and scroll offsets

### Sessions & Windows
- `Enter`: switch to target inside tmux, or attach outside tmux
- `n`: create new session
- `N`: launch session preset (`name | start-dir | command`)
- `w`: create new window in selected session
- `W`: launch window preset (`name | start-dir | command`)
- `L`: open Visual Layout Engine (`1`-`5` to apply layout)
- `b`: toggle `synchronize-panes` on selected window
- `o`: rotate panes clockwise in selected window
- `>` / `<`: swap selected pane or window forward / backward
- `d`: detach current tmux client

### Pane Operations
- `%` or `|`: split selected pane left/right
- `"` or `-`: split selected pane top/bottom
- `z`: toggle pane zoom
- `B`: break pane into its own window
- `J`: join pane into another window/pane target
- `m` / `M`: mark/unmark pane / swap with marked pane
- `{` / `}` or `Alt+Arrow`: resize pane (`L/R/U/D`)
- `Ctrl+L`: clear pane screen and scrollback history
- `X`: respawn dead or stuck pane

### Edit, Input, Buffers & System
- `r` / `Cmd+R`: rename session/window or set pane title
- `x`: kill selected target (with confirmation)
- `*`: pin / unpin favorite (persisted)
- `s`: send keys + Enter to selected pane
- `S`: broadcast keys + Enter to all panes in selected window/session
- `y`: copy selected pane text to tmux buffer
- `Y`: copy selected pane text to system clipboard (`pbcopy` / `wl-copy` / `xclip` / `xsel`)
- `C`: save last 2000 scrollback lines of selected pane to a file
- `Ctrl+P` / `p`: open Command Palette
- `/`: fuzzy multi-token filter tree
- `f`: clear active filter & favorites-only lock
- `R`: force refresh now
- `:`: run arbitrary tmux command (omit leading `tmux`)
- `D`: open multi-server Diagnostics Deck
- `?` / `F1`: open Shortcuts Atlas
- `q` / `Esc` / `Ctrl+C`: close overlay / exit Zen mode / quit
