use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Wrap,
};
use ratatui::Frame;

use crate::app::{App, LayoutPickerState, PaletteState, Prompt, QuickJumpState, TreeItemKind};
use crate::keybindings::{ShortcutCategory, NATIVE_TMUX_SHORTCUTS, SHORTCUTS};
use crate::metrics::format_memory_usage;
use crate::tmux::TmuxLayout;
use crate::ui::{centered_rect, truncate_text};

pub fn draw_palette(frame: &mut Frame, area: Rect, app: &App, palette: &PaletteState) {
    let theme = app.theme();
    let popup = centered_rect(76, 76, area);
    frame.render_widget(Clear, popup);

    let entries = palette.filtered_entries();

    let title = Line::from(vec![
        Span::raw(" "),
        theme.pill("» COMMAND PALETTE", theme.accent),
        Span::styled(
            format!("  {} commands ", entries.len()),
            Style::default().fg(theme.text_muted),
        ),
    ]);

    let block = theme.modal_block(title, theme.accent).title_bottom(
        Line::from(vec![
            theme.keycap("↑/↓"),
            Span::styled(" Select  ", Style::default().fg(theme.text_muted)),
            theme.keycap("⏎"),
            Span::styled(" Execute  ", Style::default().fg(theme.text_muted)),
            theme.keycap("Ctrl+U"),
            Span::styled(" Clear  ", Style::default().fg(theme.text_muted)),
            theme.keycap("Esc"),
            Span::styled(" Close ", Style::default().fg(theme.text_muted)),
        ])
        .alignment(Alignment::Right),
    );
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.height < 4 || inner.width < 12 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(inner);

    // Search input box with hardware cursor
    let search_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent_alt))
        .style(Style::default().bg(theme.surface));
    let search_inner = search_block.inner(chunks[0]);
    let prefix = " ❯ ";
    let query_display =
        if palette.input.value.is_empty() {
            Line::from(vec![
            Span::styled(prefix, Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(
                "Type to fuzzy-search any lazytmux command (e.g. layout, split, theme, sync)...",
                Style::default().fg(theme.text_dim),
            ),
        ])
        } else {
            Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(palette.input.value.clone(), Style::default().fg(theme.text)),
            ])
        };
    frame.render_widget(Paragraph::new(query_display).block(search_block), chunks[0]);

    if search_inner.width > 4 {
        let cursor_col = palette
            .input
            .cursor_char_index()
            .min(search_inner.width.saturating_sub(4) as usize) as u16;
        frame.set_cursor_position((search_inner.x + 3 + cursor_col, search_inner.y));
    }

    let row_width = chunks[1].width.saturating_sub(2) as usize;
    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            let selected = idx == palette.selected;
            let key_len = entry.keys.chars().count() + 2;
            let cat = match entry.category {
                ShortcutCategory::Navigation => "NAV",
                ShortcutCategory::ViewAndHud => "HUD",
                ShortcutCategory::Sessions => "SES",
                ShortcutCategory::Windows => "WIN",
                ShortcutCategory::Panes => "PNE",
                ShortcutCategory::InputAndBuffers => "BUF",
                ShortcutCategory::System => "SYS",
            };
            let title_text = truncate_text(entry.title, 28);
            let left_len = 2 + 5 + 1 + title_text.chars().count();
            let desc_budget = row_width
                .saturating_sub(left_len)
                .saturating_sub(key_len)
                .saturating_sub(4);
            let desc_text = if desc_budget >= 8 {
                truncate_text(entry.description, desc_budget)
            } else {
                String::new()
            };
            let pad = row_width
                .saturating_sub(
                    left_len + desc_text.chars().count() + if desc_text.is_empty() { 0 } else { 2 },
                )
                .saturating_sub(key_len);

            let mut spans = vec![
                Span::styled(
                    if selected { "▎ " } else { "  " },
                    Style::default().fg(theme.accent),
                ),
                Span::styled(
                    format!("[{cat}] "),
                    Style::default()
                        .fg(theme.accent_soft)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    title_text,
                    Style::default()
                        .fg(if selected {
                            theme.selection_fg
                        } else {
                            theme.text
                        })
                        .add_modifier(Modifier::BOLD),
                ),
            ];
            if !desc_text.is_empty() {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    desc_text,
                    Style::default().fg(theme.text_muted),
                ));
            }
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
            spans.push(theme.keycap(entry.keys));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut state = ListState::default();
    if !entries.is_empty() {
        state.select(Some(palette.selected.min(entries.len() - 1)));
    }

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(theme.selection_bg)
                .fg(theme.selection_fg),
        )
        .style(Style::default().bg(theme.overlay_bg));
    frame.render_stateful_widget(list, chunks[1], &mut state);
}

pub fn draw_quick_jump(frame: &mut Frame, area: Rect, app: &App, jump: &QuickJumpState) {
    let theme = app.theme();
    let popup = centered_rect(76, 70, area);
    frame.render_widget(Clear, popup);

    let matches = app.quick_jump_matches();

    let title = Line::from(vec![
        Span::raw(" "),
        theme.pill("◈ QUICK TARGET JUMP", theme.info),
        Span::styled(
            format!("  {} targets ", matches.len()),
            Style::default().fg(theme.text_muted),
        ),
    ]);

    let block = theme.modal_block(title, theme.info).title_bottom(
        Line::from(vec![
            theme.keycap("↑/↓"),
            Span::styled(" Navigate  ", Style::default().fg(theme.text_muted)),
            theme.keycap("⏎"),
            Span::styled(" Jump & Expand  ", Style::default().fg(theme.text_muted)),
            theme.keycap("Esc"),
            Span::styled(" Close ", Style::default().fg(theme.text_muted)),
        ])
        .alignment(Alignment::Right),
    );
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.height < 4 || inner.width < 12 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(inner);

    let search_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.info))
        .style(Style::default().bg(theme.surface));
    let search_inner = search_block.inner(chunks[0]);
    let line = if jump.input.value.is_empty() {
        Line::from(vec![
            Span::styled(
                " ⌕ ",
                Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Jump to any session, window, pane, command, or path...",
                Style::default().fg(theme.text_dim),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                " ⌕ ",
                Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
            ),
            Span::styled(jump.input.value.clone(), Style::default().fg(theme.text)),
        ])
    };
    frame.render_widget(Paragraph::new(line).block(search_block), chunks[0]);
    if search_inner.width > 4 {
        let col = jump
            .input
            .cursor_char_index()
            .min(search_inner.width.saturating_sub(4) as usize) as u16;
        frame.set_cursor_position((search_inner.x + 3 + col, search_inner.y));
    }

    let row_width = chunks[1].width.saturating_sub(2) as usize;
    let items: Vec<ListItem> = matches
        .iter()
        .enumerate()
        .map(|(idx, candidate)| {
            let selected = idx == jump.selected;
            let (tag, color) = match candidate.kind {
                TreeItemKind::Session => ("SES", theme.accent),
                TreeItemKind::Window => ("WIN", theme.info),
                TreeItemKind::Pane => ("PNE", theme.success),
            };
            let star = if candidate.favorite { "★ " } else { "" };
            let bread_budget = (row_width / 2).max(16);
            let bread = truncate_text(&candidate.breadcrumb, bread_budget);
            let sub_budget = row_width.saturating_sub(bread.chars().count() + 12).max(6);
            let sub = truncate_text(&candidate.subtitle, sub_budget);

            ListItem::new(Line::from(vec![
                Span::styled(
                    if selected { "▎ " } else { "  " },
                    Style::default().fg(theme.info),
                ),
                Span::styled(
                    format!("[{tag}] "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(star, Style::default().fg(theme.warning)),
                Span::styled(
                    bread,
                    Style::default()
                        .fg(if selected {
                            theme.selection_fg
                        } else {
                            theme.text
                        })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(sub, Style::default().fg(theme.text_muted)),
            ]))
        })
        .collect();

    let mut state = ListState::default();
    if !matches.is_empty() {
        state.select(Some(jump.selected.min(matches.len() - 1)));
    }
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(theme.selection_bg)
                .fg(theme.selection_fg),
        )
        .style(Style::default().bg(theme.overlay_bg));
    frame.render_stateful_widget(list, chunks[1], &mut state);
}

pub fn draw_layout_picker(frame: &mut Frame, area: Rect, app: &App, picker: &LayoutPickerState) {
    let theme = app.theme();
    let popup = centered_rect(78, 52, area);
    frame.render_widget(Clear, popup);

    let title = Line::from(vec![
        Span::raw(" "),
        theme.pill("⊞ VISUAL LAYOUT ENGINE", theme.accent_alt),
        Span::styled(
            format!("  Window {} ", picker.window_label),
            Style::default().fg(theme.text_muted),
        ),
    ]);

    let block = theme.modal_block(title, theme.accent_alt).title_bottom(
        Line::from(vec![
            theme.keycap("1-5"),
            Span::styled(" Instant apply  ", Style::default().fg(theme.text_muted)),
            theme.keycap("←/→"),
            Span::styled(" Choose  ", Style::default().fg(theme.text_muted)),
            theme.keycap("⏎"),
            Span::styled(" Apply  ", Style::default().fg(theme.text_muted)),
            theme.keycap("Esc"),
            Span::styled(" Cancel ", Style::default().fg(theme.text_muted)),
        ])
        .alignment(Alignment::Right),
    );
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.width < 20 || inner.height < 8 {
        return;
    }

    if inner.width >= 76 {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(7), Constraint::Length(3)])
            .split(inner);

        let card_constraints = [Constraint::Ratio(1, 5); 5];
        let cards = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(card_constraints)
            .split(rows[0]);

        for (idx, layout) in TmuxLayout::ALL.iter().enumerate() {
            let selected = idx == picker.selected;
            let card_block = Block::default()
                .title(Line::from(vec![
                    Span::raw(" "),
                    theme.keycap((idx + 1).to_string()),
                    Span::raw(" "),
                ]))
                .borders(Borders::ALL)
                .border_type(if selected {
                    BorderType::Thick
                } else {
                    BorderType::Rounded
                })
                .border_style(Style::default().fg(if selected {
                    theme.accent
                } else {
                    theme.border
                }))
                .style(Style::default().bg(if selected {
                    theme.selection_bg
                } else {
                    theme.surface
                }));

            let mut lines: Vec<Line> = layout
                .diagram()
                .iter()
                .map(|row_str| {
                    Line::from(Span::styled(
                        *row_str,
                        Style::default().fg(if selected {
                            theme.accent
                        } else {
                            theme.text_muted
                        }),
                    ))
                })
                .collect();
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                layout.label(),
                Style::default()
                    .fg(if selected {
                        theme.selection_fg
                    } else {
                        theme.text
                    })
                    .add_modifier(Modifier::BOLD),
            )));

            let para = Paragraph::new(lines)
                .block(card_block)
                .alignment(Alignment::Center);
            frame.render_widget(para, cards[idx]);
        }

        let active_layout = TmuxLayout::ALL[picker.selected];
        let footer = Paragraph::new(vec![Line::from(vec![
            Span::styled(
                format!(" {} ", active_layout.tmux_name()),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("— ", Style::default().fg(theme.text_dim)),
            Span::styled(active_layout.description(), Style::default().fg(theme.text)),
        ])])
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.overlay_bg));
        frame.render_widget(footer, rows[1]);
    } else {
        // Compact 2-column layout picker for 80-column terminals
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
            .split(inner);

        let items: Vec<ListItem> = TmuxLayout::ALL
            .iter()
            .enumerate()
            .map(|(idx, layout)| {
                let selected = idx == picker.selected;
                ListItem::new(Line::from(vec![
                    Span::styled(
                        if selected { "▎ " } else { "  " },
                        Style::default().fg(theme.accent),
                    ),
                    theme.keycap((idx + 1).to_string()),
                    Span::raw(" "),
                    Span::styled(
                        layout.label(),
                        Style::default()
                            .fg(if selected {
                                theme.selection_fg
                            } else {
                                theme.text
                            })
                            .add_modifier(Modifier::BOLD),
                    ),
                ]))
            })
            .collect();
        let mut state = ListState::default();
        state.select(Some(picker.selected));
        let list = List::new(items)
            .highlight_style(Style::default().bg(theme.selection_bg))
            .style(Style::default().bg(theme.overlay_bg));
        frame.render_stateful_widget(list, cols[0], &mut state);

        let active_layout = TmuxLayout::ALL[picker.selected];
        let mut preview_lines: Vec<Line> = active_layout
            .diagram()
            .iter()
            .map(|row_str| Line::from(Span::styled(*row_str, Style::default().fg(theme.accent))))
            .collect();
        preview_lines.push(Line::from(""));
        preview_lines.push(Line::from(Span::styled(
            active_layout.description(),
            Style::default().fg(theme.text_muted),
        )));
        let preview = Paragraph::new(preview_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme.accent))
                    .style(Style::default().bg(theme.surface)),
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        frame.render_widget(preview, cols[1]);
    }
}

pub fn draw_prompt(frame: &mut Frame, area: Rect, app: &App, prompt: &Prompt) {
    let theme = app.theme();
    let popup = centered_rect(70, 26, area);
    frame.render_widget(Clear, popup);

    let inner_width = popup.width.saturating_sub(4) as usize;
    let caret = prompt.cursor_char_index();
    let chars: Vec<char> = prompt.value.chars().collect();
    let scroll = if inner_width == 0 {
        0
    } else {
        caret.saturating_sub(inner_width.saturating_sub(1))
    };
    let visible: String = chars.iter().skip(scroll).take(inner_width.max(1)).collect();

    let title = Line::from(vec![
        Span::raw(" "),
        theme.pill(format!("» {}", prompt.title.to_uppercase()), theme.accent),
        Span::raw(" "),
    ]);

    let prompt_widget = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "❯ ",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(visible, Style::default().fg(theme.text)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            prompt.hint.clone(),
            Style::default().fg(theme.text_muted),
        )),
    ])
    .block(theme.modal_block(title, theme.accent))
    .wrap(Wrap { trim: false });
    frame.render_widget(prompt_widget, popup);

    if inner_width > 0 {
        let column = (caret - scroll) as u16;
        let cursor_x = popup.x + 3 + column;
        let cursor_y = popup.y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

pub fn draw_confirm(frame: &mut Frame, area: Rect, app: &App, message: &str) {
    let theme = app.theme();
    let popup = centered_rect(66, 22, area);
    frame.render_widget(Clear, popup);

    let title = Line::from(vec![
        Span::raw(" "),
        theme.pill("▲ DESTRUCTIVE ACTION", theme.danger),
        Span::raw(" "),
    ]);

    let confirm = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            message.to_string(),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            theme.pill("y Confirm", theme.danger),
            Span::raw("    "),
            theme.keycap("n / Esc"),
            Span::styled(" Cancel", Style::default().fg(theme.text_muted)),
        ]),
    ])
    .block(theme.modal_block(title, theme.danger))
    .alignment(Alignment::Center)
    .wrap(Wrap { trim: true });
    frame.render_widget(confirm, popup);
}

pub fn draw_diagnostics(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let popup = centered_rect(84, 80, area);
    frame.render_widget(Clear, popup);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                " ◈ MULTI-SERVER SOCKET & DISCOVERY TELEMETRY",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            "   Scans default server, $TMUX, /tmp, /private/tmp, $XDG_RUNTIME_DIR, /run/user/$UID, and cwd sockets.",
            Style::default().fg(theme.text_muted),
        )),
        Line::from(""),
    ];

    if let Some(version) = app.tmux_version() {
        lines.push(Line::from(vec![
            Span::styled("   TMUX VERSION    ", Style::default().fg(theme.text_dim)),
            Span::styled(
                version,
                Style::default()
                    .fg(theme.success)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("   HOST PLATFORM   ", Style::default().fg(theme.text_dim)),
        Span::styled(
            format!(
                "{} ({})",
                app.system_metrics().os_label,
                app.system_metrics().host_name
            ),
            Style::default().fg(theme.info),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("   ACTIVE THEME    ", Style::default().fg(theme.text_dim)),
        Span::styled(
            app.theme_id().name(),
            Style::default()
                .fg(theme.accent_alt)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("   SYSTEM MEMORY   ", Style::default().fg(theme.text_dim)),
        Span::styled(
            format_memory_usage(
                app.system_metrics().memory_used,
                app.system_metrics().memory_total,
            ),
            Style::default().fg(theme.accent),
        ),
    ]));

    if let Some(item) = app.selected_item() {
        lines.push(Line::from(vec![
            Span::styled("   ACTIVE TARGET   ", Style::default().fg(theme.text_dim)),
            Span::styled(item.target.display(), Style::default().fg(theme.accent)),
            Span::styled("  on  ", Style::default().fg(theme.text_dim)),
            Span::styled(item.target.server_label(), Style::default().fg(theme.text)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " ── SOCKET PROBE LOG ─────────────────────────────────────────────────────────",
        Style::default().fg(theme.border),
    )));

    if app.diagnostics().is_empty() {
        lines.push(Line::from(Span::styled(
            "   No diagnostics collected yet.",
            Style::default().fg(theme.text_dim),
        )));
    } else {
        for diagnostic in app.diagnostics() {
            let (dot, color) = if diagnostic.contains("loaded ") {
                ("● ", theme.success)
            } else if diagnostic.contains("empty") {
                ("○ ", theme.warning)
            } else if diagnostic.contains("duplicate") {
                ("≈ ", theme.info)
            } else if diagnostic.contains("unavailable") {
                ("✕ ", theme.text_dim)
            } else if diagnostic.contains("error") {
                ("✕ ", theme.danger)
            } else {
                ("▸ ", theme.accent)
            };

            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(dot, Style::default().fg(color)),
                Span::styled(diagnostic.clone(), Style::default().fg(color)),
            ]));
        }
    }

    let total_lines = lines.len();
    let title = Line::from(vec![
        Span::raw(" "),
        theme.pill("◈ DIAGNOSTICS DECK", theme.info),
        Span::raw(" "),
    ]);

    let block = theme.modal_block(title, theme.info).title_bottom(
        Line::from(vec![
            theme.keycap("R"),
            Span::styled(" Re-probe  ", Style::default().fg(theme.text_muted)),
            theme.keycap("j/k"),
            Span::styled(" Scroll  ", Style::default().fg(theme.text_muted)),
            theme.keycap("D / Esc"),
            Span::styled(" Close ", Style::default().fg(theme.text_muted)),
        ])
        .alignment(Alignment::Right),
    );

    let widget = Paragraph::new(lines)
        .block(block)
        .scroll((app.diagnostics_scroll(), 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, popup);

    let mut scroll_state =
        ScrollbarState::new(total_lines).position(app.diagnostics_scroll() as usize);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .thumb_symbol("┃")
        .thumb_style(Style::default().fg(theme.info));
    frame.render_stateful_widget(scrollbar, popup, &mut scroll_state);
}

pub fn draw_help(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let popup = centered_rect(90, 88, area);
    frame.render_widget(Clear, popup);

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(
                " ◆ LAZYTMUX // NEXUS SHORTCUTS ATLAS ",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" ({} built-in shortcuts)", SHORTCUTS.len()),
                Style::default().fg(theme.text_muted),
            ),
        ]),
        Line::from(vec![
            Span::raw(" "),
            Span::styled("Tip: Press ", Style::default().fg(theme.text_muted)),
            theme.keycap("Ctrl+P"),
            Span::styled(
                " anytime to fuzzy-search & run commands from the Command Palette!",
                Style::default().fg(theme.text_muted),
            ),
        ]),
        Line::from(""),
    ];

    for category in ShortcutCategory::ALL {
        lines.push(Line::from(vec![
            Span::styled(
                format!(" ▸ {} ", category.title()),
                Style::default()
                    .fg(theme.accent_alt)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "──────────────────────────────────────",
                Style::default().fg(theme.border_dim),
            ),
        ]));

        for entry in SHORTCUTS.iter().filter(|e| e.category == category) {
            let key_padded = format!("{:<14}", entry.keys);
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(
                    format!(" {key_padded} "),
                    Style::default()
                        .bg(theme.keycap_bg)
                        .fg(theme.keycap_fg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("{:<26}", truncate_text(entry.title, 26)),
                    Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
                ),
                Span::styled(entry.description, Style::default().fg(theme.text_muted)),
            ]));
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(vec![
        Span::styled(
            " ▸ NATIVE TMUX SHORTCUTS (Default Prefix: Ctrl+b) ",
            Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "────────────────────",
            Style::default().fg(theme.border_dim),
        ),
    ]));
    for (keys, desc) in NATIVE_TMUX_SHORTCUTS {
        lines.push(Line::from(vec![
            Span::raw("   "),
            Span::styled(
                format!(" {:<14} ", keys),
                Style::default()
                    .bg(theme.surface_alt)
                    .fg(theme.info)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(*desc, Style::default().fg(theme.text_muted)),
        ]));
    }

    let total_lines = lines.len();
    let title = Line::from(vec![
        Span::raw(" "),
        theme.pill("? SHORTCUTS ATLAS", theme.accent),
        Span::raw(" "),
    ]);

    let block = theme.modal_block(title, theme.accent).title_bottom(
        Line::from(vec![
            theme.keycap("j/k / PgUp/PgDn"),
            Span::styled(" Scroll  ", Style::default().fg(theme.text_muted)),
            theme.keycap("g"),
            Span::styled(" Top  ", Style::default().fg(theme.text_muted)),
            theme.keycap("? / F1 / Esc"),
            Span::styled(" Close ", Style::default().fg(theme.text_muted)),
        ])
        .alignment(Alignment::Right),
    );

    let widget = Paragraph::new(lines)
        .block(block)
        .scroll((app.help_scroll(), 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, popup);

    let mut scroll_state = ScrollbarState::new(total_lines).position(app.help_scroll() as usize);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .thumb_symbol("┃")
        .thumb_style(Style::default().fg(theme.accent));
    frame.render_stateful_widget(scrollbar, popup, &mut scroll_state);
}
