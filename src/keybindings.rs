#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutCategory {
    Navigation,
    ViewAndHud,
    Sessions,
    Windows,
    Panes,
    InputAndBuffers,
    System,
}

impl ShortcutCategory {
    pub const ALL: [ShortcutCategory; 7] = [
        ShortcutCategory::Navigation,
        ShortcutCategory::ViewAndHud,
        ShortcutCategory::Sessions,
        ShortcutCategory::Windows,
        ShortcutCategory::Panes,
        ShortcutCategory::InputAndBuffers,
        ShortcutCategory::System,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Navigation => "NAVIGATION & TREE",
            Self::ViewAndHud => "VIEWPORTS & HUD",
            Self::Sessions => "SESSION CONTROL",
            Self::Windows => "WINDOW ORCHESTRATION",
            Self::Panes => "PANE OPERATIONS",
            Self::InputAndBuffers => "INPUT, BROADCAST & BUFFERS",
            Self::System => "SYSTEM & COMMANDS",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    // Navigation
    MoveDown,
    MoveUp,
    PageDown,
    PageUp,
    JumpTop,
    JumpBottom,
    ToggleExpand,
    Expand,
    Collapse,
    ExpandAll,
    CollapseAll,
    CycleFocus,
    QuickJump,

    // View & HUD
    ViewDeck,
    ViewLive,
    ViewTelemetry,
    ViewBuffers,
    ToggleZen,
    CycleTheme,
    CycleSort,
    ToggleFavoritesOnly,
    TogglePause,
    ScrollDetailsDown,
    ScrollDetailsUp,
    ResetView,

    // Sessions
    AttachOrSwitch,
    NewSession,
    LaunchSession,
    DetachClient,

    // Windows
    NewWindow,
    NewWindowCustom,
    OpenLayoutPicker,
    ToggleSyncPanes,
    RotatePanes,
    SwapNext,
    SwapPrev,

    // Panes
    SplitHorizontal,
    SplitVertical,
    ToggleZoom,
    BreakPane,
    JoinPane,
    ToggleMarkPane,
    SwapMarkedPane,
    ResizeLeft,
    ResizeRight,
    ResizeUp,
    ResizeDown,
    ClearPane,
    RespawnPane,

    // Input & Buffers
    Rename,
    Kill,
    ToggleFavorite,
    SendKeys,
    BroadcastKeys,
    CopyPaneBuffer,
    CopySystemClipboard,
    SavePaneSnapshot,

    // System
    OpenPalette,
    Filter,
    ClearFilter,
    Refresh,
    RunCommand,
    Diagnostics,
    Help,
    Quit,
}

#[derive(Clone, Copy, Debug)]
pub struct ShortcutEntry {
    pub action: Action,
    pub category: ShortcutCategory,
    pub keys: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub in_palette: bool,
}

pub const SHORTCUTS: &[ShortcutEntry] = &[
    // Navigation
    ShortcutEntry {
        action: Action::MoveDown,
        category: ShortcutCategory::Navigation,
        keys: "j / ↓",
        title: "Move selection down",
        description: "Move cursor down in the active panel",
        in_palette: false,
    },
    ShortcutEntry {
        action: Action::MoveUp,
        category: ShortcutCategory::Navigation,
        keys: "k / ↑",
        title: "Move selection up",
        description: "Move cursor up in the active panel",
        in_palette: false,
    },
    ShortcutEntry {
        action: Action::JumpTop,
        category: ShortcutCategory::Navigation,
        keys: "g / G",
        title: "Jump to top / bottom",
        description: "Jump to first or last item in the tree",
        in_palette: false,
    },
    ShortcutEntry {
        action: Action::ToggleExpand,
        category: ShortcutCategory::Navigation,
        keys: "Space",
        title: "Toggle expand / collapse",
        description: "Expand or collapse the selected session or window",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::Collapse,
        category: ShortcutCategory::Navigation,
        keys: "h / ←",
        title: "Collapse node",
        description: "Collapse selected session or window (or jump to parent)",
        in_palette: false,
    },
    ShortcutEntry {
        action: Action::Expand,
        category: ShortcutCategory::Navigation,
        keys: "l / →",
        title: "Expand node",
        description: "Expand selected session or window",
        in_palette: false,
    },
    ShortcutEntry {
        action: Action::ExpandAll,
        category: ShortcutCategory::Navigation,
        keys: "e",
        title: "Expand all nodes",
        description: "Expand every session and window in the topology tree",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::CollapseAll,
        category: ShortcutCategory::Navigation,
        keys: "c",
        title: "Collapse all nodes",
        description: "Collapse every session and window back to compact view",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::CycleFocus,
        category: ShortcutCategory::Navigation,
        keys: "Tab",
        title: "Switch focused panel",
        description: "Cycle active focus between Topology Tree and Inspector",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::QuickJump,
        category: ShortcutCategory::Navigation,
        keys: "Ctrl+S",
        title: "Quick target jump",
        description: "Fuzzy-search and jump directly to any session, window, or pane",
        in_palette: true,
    },
    // View & HUD
    ShortcutEntry {
        action: Action::ViewDeck,
        category: ShortcutCategory::ViewAndHud,
        keys: "1",
        title: "Switch to Deck view",
        description: "Show telemetry card + scaled live ANSI multi-pane viewport",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ViewLive,
        category: ShortcutCategory::ViewAndHud,
        keys: "2",
        title: "Switch to Live Viewport",
        description: "Maximize the full ANSI terminal preview in the inspector",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ViewTelemetry,
        category: ShortcutCategory::ViewAndHud,
        keys: "3",
        title: "Switch to Telemetry Matrix",
        description: "Live CPU/RAM sparklines and per-pane process resource matrix",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ViewBuffers,
        category: ShortcutCategory::ViewAndHud,
        keys: "4",
        title: "Switch to Paste Buffers Vault",
        description: "Inspect, paste, copy, or delete tmux paste buffers",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ToggleZen,
        category: ShortcutCategory::ViewAndHud,
        keys: "v",
        title: "Toggle Zen Viewport mode",
        description: "Hide the sidebar to expand the inspector edge-to-edge",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::CycleTheme,
        category: ShortcutCategory::ViewAndHud,
        keys: "t",
        title: "Cycle futuristic UI theme",
        description: "Switch between Cyber Neon, Aurora Pulse, Solaris, Matrix, Frost",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::CycleSort,
        category: ShortcutCategory::ViewAndHud,
        keys: "O",
        title: "Cycle tree sort order",
        description: "Sort by Name, Hot CPU, Heavy Memory, or Active status",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ToggleFavoritesOnly,
        category: ShortcutCategory::ViewAndHud,
        keys: "F",
        title: "Toggle favorites-only view",
        description: "Show only starred targets or all targets",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::TogglePause,
        category: ShortcutCategory::ViewAndHud,
        keys: "P",
        title: "Pause / resume auto-refresh",
        description: "Freeze live telemetry and pane polling to inspect output",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ScrollDetailsUp,
        category: ShortcutCategory::ViewAndHud,
        keys: "[ / Ctrl+U",
        title: "Scroll inspector up",
        description: "Scroll details, preview, or telemetry content up",
        in_palette: false,
    },
    ShortcutEntry {
        action: Action::ScrollDetailsDown,
        category: ShortcutCategory::ViewAndHud,
        keys: "] / Ctrl+D",
        title: "Scroll inspector down",
        description: "Scroll details, preview, or telemetry content down",
        in_palette: false,
    },
    ShortcutEntry {
        action: Action::ResetView,
        category: ShortcutCategory::ViewAndHud,
        keys: "0",
        title: "Reset view & scroll",
        description: "Exit Zen mode, return to Deck view, reset scroll offsets",
        in_palette: true,
    },
    // Sessions
    ShortcutEntry {
        action: Action::AttachOrSwitch,
        category: ShortcutCategory::Sessions,
        keys: "Enter",
        title: "Switch / attach to target",
        description: "Switch client inside tmux or attach session outside tmux",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::NewSession,
        category: ShortcutCategory::Sessions,
        keys: "n",
        title: "Create new session",
        description: "Create a detached tmux session by name",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::LaunchSession,
        category: ShortcutCategory::Sessions,
        keys: "N",
        title: "Launch session preset",
        description: "Create session with name | start-dir | command",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::DetachClient,
        category: ShortcutCategory::Sessions,
        keys: "d",
        title: "Detach current tmux client",
        description: "Detach the active tmux client cleanly",
        in_palette: true,
    },
    // Windows
    ShortcutEntry {
        action: Action::NewWindow,
        category: ShortcutCategory::Windows,
        keys: "w",
        title: "Create new window",
        description: "Create a new window in the selected session",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::NewWindowCustom,
        category: ShortcutCategory::Windows,
        keys: "W",
        title: "Launch window preset",
        description: "Create window with name | start-dir | command",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::OpenLayoutPicker,
        category: ShortcutCategory::Windows,
        keys: "L",
        title: "Visual layout picker",
        description: "Apply even-horiz, even-vert, main-horiz, main-vert, or tiled",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ToggleSyncPanes,
        category: ShortcutCategory::Windows,
        keys: "b",
        title: "Toggle synchronize-panes",
        description: "Toggle mirrored keyboard input across all panes in window",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::RotatePanes,
        category: ShortcutCategory::Windows,
        keys: "o",
        title: "Rotate window panes",
        description: "Rotate pane positions clockwise inside the selected window",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::SwapNext,
        category: ShortcutCategory::Windows,
        keys: ">",
        title: "Swap pane / window forward",
        description: "Swap selected pane or window with the next slot",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::SwapPrev,
        category: ShortcutCategory::Windows,
        keys: "<",
        title: "Swap pane / window backward",
        description: "Swap selected pane or window with the previous slot",
        in_palette: true,
    },
    // Panes
    ShortcutEntry {
        action: Action::SplitHorizontal,
        category: ShortcutCategory::Panes,
        keys: "% / |",
        title: "Split pane horizontally",
        description: "Split selected pane left/right (tmux split-window -h)",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::SplitVertical,
        category: ShortcutCategory::Panes,
        keys: "\" / -",
        title: "Split pane vertically",
        description: "Split selected pane top/bottom (tmux split-window -v)",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ToggleZoom,
        category: ShortcutCategory::Panes,
        keys: "z",
        title: "Toggle pane zoom",
        description: "Zoom or unzoom the selected pane in tmux",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::BreakPane,
        category: ShortcutCategory::Panes,
        keys: "B",
        title: "Break pane to new window",
        description: "Extract selected pane into its own detached window",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::JoinPane,
        category: ShortcutCategory::Panes,
        keys: "J",
        title: "Join pane into target",
        description: "Move selected pane into another window/pane target",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ToggleMarkPane,
        category: ShortcutCategory::Panes,
        keys: "m",
        title: "Mark / unmark pane",
        description: "Toggle tmux pane mark for quick swapping",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::SwapMarkedPane,
        category: ShortcutCategory::Panes,
        keys: "M",
        title: "Swap with marked pane",
        description: "Swap selected pane with the currently marked pane",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ResizeLeft,
        category: ShortcutCategory::Panes,
        keys: "{ / Alt+←",
        title: "Resize pane left (-5)",
        description: "Shrink/move pane border 5 columns left",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ResizeRight,
        category: ShortcutCategory::Panes,
        keys: "} / Alt+→",
        title: "Resize pane right (+5)",
        description: "Grow/move pane border 5 columns right",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ResizeUp,
        category: ShortcutCategory::Panes,
        keys: "Alt+↑",
        title: "Resize pane up (3)",
        description: "Adjust pane height 3 rows upward",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ResizeDown,
        category: ShortcutCategory::Panes,
        keys: "Alt+↓",
        title: "Resize pane down (3)",
        description: "Adjust pane height 3 rows downward",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ClearPane,
        category: ShortcutCategory::Panes,
        keys: "Ctrl+L",
        title: "Clear pane & history",
        description: "Clear terminal screen and scrollback buffer of selected pane",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::RespawnPane,
        category: ShortcutCategory::Panes,
        keys: "X",
        title: "Respawn pane",
        description: "Restart a dead or stuck pane process cleanly",
        in_palette: true,
    },
    // Input & Buffers
    ShortcutEntry {
        action: Action::Rename,
        category: ShortcutCategory::InputAndBuffers,
        keys: "r / Cmd+R",
        title: "Rename target",
        description: "Rename selected session/window or set pane title",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::Kill,
        category: ShortcutCategory::InputAndBuffers,
        keys: "x",
        title: "Kill selected target",
        description: "Kill selected session, window, or pane (with confirmation)",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ToggleFavorite,
        category: ShortcutCategory::InputAndBuffers,
        keys: "*",
        title: "Toggle favorite pin",
        description: "Pin or unpin target near the top (persisted across launches)",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::SendKeys,
        category: ShortcutCategory::InputAndBuffers,
        keys: "s",
        title: "Send keys to pane",
        description: "Send a command + Enter to the selected pane",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::BroadcastKeys,
        category: ShortcutCategory::InputAndBuffers,
        keys: "S",
        title: "Broadcast keys to all panes",
        description: "Send a command + Enter to every pane in selected window/session",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::CopyPaneBuffer,
        category: ShortcutCategory::InputAndBuffers,
        keys: "y",
        title: "Copy pane to tmux buffer",
        description: "Capture plain pane text into tmux paste buffer",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::CopySystemClipboard,
        category: ShortcutCategory::InputAndBuffers,
        keys: "Y",
        title: "Copy pane to system clipboard",
        description: "Copy plain pane text via pbcopy (macOS) or wl-copy/xclip (Linux)",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::SavePaneSnapshot,
        category: ShortcutCategory::InputAndBuffers,
        keys: "C",
        title: "Save pane scrollback snapshot",
        description: "Export pane scrollback history to a file on disk",
        in_palette: true,
    },
    // System
    ShortcutEntry {
        action: Action::OpenPalette,
        category: ShortcutCategory::System,
        keys: "Ctrl+P / p",
        title: "Open Command Palette",
        description: "Fuzzy-search and run any lazytmux action instantly",
        in_palette: false,
    },
    ShortcutEntry {
        action: Action::Filter,
        category: ShortcutCategory::System,
        keys: "/",
        title: "Fuzzy filter tree",
        description: "Filter sessions/windows/panes by multi-token fuzzy query",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::ClearFilter,
        category: ShortcutCategory::System,
        keys: "f",
        title: "Clear active filter",
        description: "Clear tree search filter and favorites-only lock",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::Refresh,
        category: ShortcutCategory::System,
        keys: "R",
        title: "Force refresh now",
        description: "Reload tmux servers, pane previews, buffers, and metrics",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::RunCommand,
        category: ShortcutCategory::System,
        keys: ":",
        title: "Run raw tmux command",
        description: "Execute any tmux command without the leading tmux word",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::Diagnostics,
        category: ShortcutCategory::System,
        keys: "D",
        title: "Server & socket diagnostics",
        description: "Inspect discovered Linux/macOS tmux sockets and server status",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::Help,
        category: ShortcutCategory::System,
        keys: "? / F1",
        title: "Shortcuts Atlas",
        description: "Open searchable futuristic shortcuts and native tmux guide",
        in_palette: true,
    },
    ShortcutEntry {
        action: Action::Quit,
        category: ShortcutCategory::System,
        keys: "q / Esc / Ctrl+C",
        title: "Quit lazytmux",
        description: "Close active overlay or exit lazytmux cleanly",
        in_palette: true,
    },
];

pub const NATIVE_TMUX_SHORTCUTS: &[(&str, &str)] = &[
    ("Prefix c", "Create new window"),
    ("Prefix ,", "Rename current window"),
    ("Prefix $", "Rename current session"),
    ("Prefix %", "Split pane left / right"),
    ("Prefix \"", "Split pane top / bottom"),
    ("Prefix x", "Kill current pane"),
    ("Prefix z", "Toggle pane zoom"),
    ("Prefix n / p", "Next / previous window"),
    ("Prefix o", "Cycle active pane"),
    ("Prefix Space", "Cycle window layout"),
    ("Prefix [", "Enter copy / scrollback mode"),
    ("Prefix d", "Detach client"),
];

pub fn palette_entries() -> Vec<&'static ShortcutEntry> {
    SHORTCUTS.iter().filter(|entry| entry.in_palette).collect()
}

/// Fuzzy match score: higher is better, `None` means no match.
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    let candidate_lower = candidate.to_ascii_lowercase();
    let mut total_score = 0i32;

    for token in query.split_whitespace() {
        if let Some(pos) = candidate_lower.find(token) {
            let boundary_bonus = if pos == 0
                || candidate_lower
                    .as_bytes()
                    .get(pos.saturating_sub(1))
                    .is_some_and(|b| !b.is_ascii_alphanumeric())
            {
                25
            } else {
                0
            };
            total_score += 80 + boundary_bonus - (pos as i32).min(40);
            continue;
        }

        // Ordered character fuzzy scan with consecutive and boundary bonuses
        let mut token_chars = token.chars();
        let mut target_char = token_chars.next();
        let mut prev_matched = false;
        let mut prev_boundary = true;
        let mut token_score = 0i32;

        for ch in candidate_lower.chars() {
            if Some(ch) == target_char {
                token_score += 10;
                if prev_matched {
                    token_score += 8;
                }
                if prev_boundary {
                    token_score += 6;
                }
                prev_matched = true;
                target_char = token_chars.next();
                if target_char.is_none() {
                    break;
                }
            } else {
                prev_matched = false;
            }
            prev_boundary = !ch.is_ascii_alphanumeric();
        }

        if target_char.is_some() {
            return None;
        }
        total_score += token_score;
    }

    Some(total_score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_contains_rich_actions() {
        let entries = palette_entries();
        assert!(entries.len() >= 35);
        assert!(entries.iter().any(|e| e.action == Action::OpenLayoutPicker));
        assert!(entries.iter().any(|e| e.action == Action::CycleTheme));
        assert!(entries.iter().any(|e| e.action == Action::BroadcastKeys));
    }

    #[test]
    fn fuzzy_score_ranks_exact_prefix_higher() {
        let exact = fuzzy_score("theme", "Cycle futuristic UI theme").unwrap();
        let loose = fuzzy_score("theme", "Toggle hidden elements menu").unwrap();
        assert!(exact > loose);
        assert_eq!(fuzzy_score("zzz", "Cycle theme"), None);
    }
}
