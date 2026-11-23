use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, LineGauge, List, ListItem, ListState, Paragraph, Row,
    Sparkline, Table, TableState, Wrap,
};
use ratatui::Frame;

use crate::ansi::parse_ansi_text;
use crate::app::{
    App, FocusPane, LayoutRegions, TerminalPanePreview, TerminalPreview, TreeItem, TreeItemKind,
    ViewMode,
};
use crate::metrics::{format_bytes, format_bytes_compact, format_cpu_usage, format_uptime};
use crate::theme::render_meter_bar;
use crate::ui::truncate_text;

pub fn draw_inspector(frame: &mut Frame, area: Rect, app: &App, regions: &mut LayoutRegions) {
    let theme = app.theme();
    let focused = app.focus() == FocusPane::Inspector;

    // Build clickable tab header right on the panel top border!
    let mut title_spans = vec![Span::raw(" ")];
    let mut cursor_x = area.x.saturating_add(2);
    for mode in ViewMode::ALL {
        let active = app.view_mode() == mode;
        let label = mode.label();
        let width = (label.chars().count() + 2) as u16;
        regions
            .tab_rects
            .push((Rect::new(cursor_x, area.y, width, 1), mode));
        cursor_x = cursor_x.saturating_add(width + 1);

        if active {
            title_spans.push(theme.pill(label, theme.accent));
        } else {
            title_spans.push(Span::styled(
                format!(" {label} "),
                Style::default().fg(theme.text_muted),
            ));
        }
        title_spans.push(Span::raw(" "));
    }

    let right_hint = Line::from(vec![
        Span::raw(" "),
        theme.outline_tag(
            if app.zen_mode() { "v:SPLIT" } else { "v:ZEN" },
            theme.accent_soft,
        ),
        Span::raw(" "),
    ]);

    let block = theme
        .panel_block(Line::from(title_spans), focused)
        .title_top(right_hint.alignment(Alignment::Right));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 6 || inner.height < 4 {
        return;
    }

    match app.view_mode() {
        ViewMode::Deck => draw_deck_view(frame, inner, app, regions),
        ViewMode::Live => draw_live_view(frame, inner, app, regions),
        ViewMode::Telemetry => draw_telemetry_view(frame, inner, app),
        ViewMode::Buffers => draw_buffers_view(frame, inner, app),
    }
}

fn draw_deck_view(frame: &mut Frame, area: Rect, app: &App, regions: &mut LayoutRegions) {
    let theme = app.theme();
    let Some(item) = app.selected_item() else {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "◈ SELECT A SESSION, WINDOW, OR PANE TO INSPECT",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press Ctrl+P to open the Command Palette or n to spin up a new session.",
                Style::default().fg(theme.text_muted),
            )),
        ])
        .alignment(Alignment::Center);
        frame.render_widget(empty, area);
        return;
    };

    let card_height = if area.height >= 20 { 6 } else { 5 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(card_height), Constraint::Min(4)])
        .split(area);

    draw_target_telemetry_card(frame, chunks[0], app, item);

    if let Some(preview) = &item.preview {
        draw_terminal_preview(frame, chunks[1], app, preview, regions);
    } else {
        draw_details_fallback(frame, chunks[1], app, item);
    }
}

fn draw_live_view(frame: &mut Frame, area: Rect, app: &App, regions: &mut LayoutRegions) {
    let theme = app.theme();
    let Some(item) = app.selected_item() else {
        frame.render_widget(
            Paragraph::new("No target selected for Live Viewport").alignment(Alignment::Center),
            area,
        );
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(area);

    let bar = Line::from(vec![
        Span::raw(" "),
        theme.outline_tag(item.target.display(), theme.accent),
        Span::raw(" "),
        Span::styled(
            item.name.clone(),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", Style::default().fg(theme.text_dim)),
        Span::styled(item.command.clone(), Style::default().fg(theme.success)),
        Span::styled("  ·  ", Style::default().fg(theme.text_dim)),
        Span::styled(
            truncate_text(&item.cwd, chunks[0].width.saturating_sub(36) as usize),
            Style::default().fg(theme.text_muted),
        ),
    ]);
    frame.render_widget(Paragraph::new(bar), chunks[0]);

    if let Some(preview) = &item.preview {
        draw_terminal_preview(frame, chunks[1], app, preview, regions);
    } else {
        draw_details_fallback(frame, chunks[1], app, item);
    }
}

fn draw_target_telemetry_card(frame: &mut Frame, area: Rect, app: &App, item: &TreeItem) {
    let theme = app.theme();

    let kind_badge = match item.kind {
        TreeItemKind::Session => ("SESSION", theme.accent),
        TreeItemKind::Window => ("WINDOW", theme.info),
        TreeItemKind::Pane => ("PANE", theme.success),
    };

    let mut header_spans = vec![
        Span::raw(" "),
        theme.pill(kind_badge.0, kind_badge.1),
        Span::raw(" "),
        Span::styled(
            item.name.clone(),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];

    if item.active {
        header_spans.push(theme.outline_tag(
            if item.kind == TreeItemKind::Session {
                "ATTACHED"
            } else {
                "ACTIVE"
            },
            theme.success,
        ));
        header_spans.push(Span::raw(" "));
    }
    if item.favorite {
        header_spans.push(theme.outline_tag("★ PINNED", theme.warning));
        header_spans.push(Span::raw(" "));
    }
    if item.synchronized {
        header_spans.push(theme.outline_tag("⇄ SYNC", theme.accent_alt));
        header_spans.push(Span::raw(" "));
    }
    if item.zoomed {
        header_spans.push(theme.outline_tag("⊕ ZOOM", theme.accent_soft));
        header_spans.push(Span::raw(" "));
    }
    if item.marked {
        header_spans.push(theme.outline_tag("✦ MARKED", theme.warning));
        header_spans.push(Span::raw(" "));
    }
    if item.dead {
        header_spans.push(theme.pill("DEAD", theme.danger));
    }

    let cpu_str = item
        .cpu_percent
        .map(|c| format_cpu_usage(c, app.cpu_metrics_ready()))
        .unwrap_or_else(|| "sampling".into());
    let cpu_val = item.cpu_percent.unwrap_or(0.0);
    let cpu_bar = render_meter_bar((cpu_val / 100.0) as f64, 8);
    let cpu_color = if app.cpu_metrics_ready() {
        theme.heat_color(cpu_val)
    } else {
        theme.text_muted
    };

    let mem_str = if item.memory_bytes > 0 {
        format!(
            "{} ({} proc)",
            format_bytes(item.memory_bytes),
            item.process_count
        )
    } else {
        "n/a".into()
    };

    let extra_label = match item.kind {
        TreeItemKind::Session => ("PREVIEW", find_detail(item, "Preview window")),
        TreeItemKind::Window => ("LAYOUT", find_detail(item, "Layout")),
        TreeItemKind::Pane => (
            "SIZE/PID",
            format!(
                "{} · pid {}",
                find_detail(item, "Size"),
                find_detail(item, "PID")
            ),
        ),
    };

    let path_display = if item.cwd.is_empty() {
        item.subtitle.clone()
    } else {
        item.cwd.clone()
    };

    let col_max = (area.width / 2).saturating_sub(12) as usize;
    let lines = vec![
        Line::from(header_spans),
        Line::from(Span::styled(
            "─".repeat(area.width.saturating_sub(2) as usize),
            Style::default().fg(theme.border_dim),
        )),
        Line::from(vec![
            Span::styled(" TARGET  ", Style::default().fg(theme.text_dim)),
            Span::styled(
                format!("{:<12}", truncate_text(&item.target.display(), 12)),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  SERVER  ", Style::default().fg(theme.text_dim)),
            Span::styled(
                truncate_text(&item.server_label, col_max.max(10)),
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(vec![
            Span::styled(" CPU     ", Style::default().fg(theme.text_dim)),
            Span::styled(
                format!("{cpu_str:<9}"),
                Style::default().fg(cpu_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{cpu_bar}  "), Style::default().fg(cpu_color)),
            Span::styled("MEM  ", Style::default().fg(theme.text_dim)),
            Span::styled(
                truncate_text(&mem_str, col_max.max(12)),
                Style::default().fg(theme.info),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!(" {:<7} ", extra_label.0),
                Style::default().fg(theme.text_dim),
            ),
            Span::styled(
                format!("{:<18}", truncate_text(&extra_label.1, 18)),
                Style::default().fg(theme.text),
            ),
            Span::styled("  PATH ", Style::default().fg(theme.text_dim)),
            Span::styled(
                truncate_text(&path_display, col_max.max(12)),
                Style::default().fg(theme.text_muted),
            ),
        ]),
    ];

    let card = Paragraph::new(lines).style(Style::default().bg(theme.surface));
    frame.render_widget(card, area);
}

fn draw_details_fallback(frame: &mut Frame, area: Rect, app: &App, item: &TreeItem) {
    let theme = app.theme();
    let lines: Vec<Line> = item
        .details
        .iter()
        .map(|(k, v)| {
            Line::from(vec![
                Span::styled(
                    format!(" {:<20} ", k),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(v.clone(), Style::default().fg(theme.text)),
            ])
        })
        .collect();
    let para = Paragraph::new(lines)
        .scroll((app.detail_scroll(), 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn draw_terminal_preview(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    preview: &TerminalPreview,
    regions: &mut LayoutRegions,
) {
    if area.width < 4 || area.height < 3 {
        return;
    }
    let theme = app.theme();

    let title = Line::from(vec![
        Span::raw(" "),
        Span::styled(
            "◉ LIVE VIEWPORT // ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(preview.title.clone(), Style::default().fg(theme.text)),
        Span::raw(" "),
    ]);

    let scroll_hint = if app.detail_scroll() > 0 {
        format!(" scroll +{} · [/] ", app.detail_scroll())
    } else {
        " click pane · [/] scroll · ⏎ attach ".to_string()
    };

    let block = Block::default()
        .title(title)
        .title_bottom(
            Line::from(Span::styled(
                scroll_hint,
                Style::default().fg(theme.text_dim),
            ))
            .alignment(Alignment::Right),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if preview.panes.is_empty() {
        let empty = Paragraph::new("No panes to preview")
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.text_dim));
        frame.render_widget(empty, inner);
        return;
    }

    let source_width = preview
        .panes
        .iter()
        .map(|pane| pane.left.saturating_add(pane.width))
        .max()
        .unwrap_or(1)
        .max(1);
    let source_height = preview
        .panes
        .iter()
        .map(|pane| pane.top.saturating_add(pane.height))
        .max()
        .unwrap_or(1)
        .max(1);

    for pane in &preview.panes {
        if let Some(pane_area) = scaled_pane_rect(inner, pane, source_width, source_height) {
            regions
                .preview_pane_rects
                .push((pane_area, pane.target.clone()));
            draw_preview_pane(frame, pane_area, app, pane);
        }
    }
}

fn draw_preview_pane(frame: &mut Frame, area: Rect, app: &App, pane: &TerminalPanePreview) {
    let theme = app.theme();
    let (border_color, icon) = if pane.dead {
        (theme.danger, "✕ ")
    } else if pane.marked {
        (theme.warning, "✦ ")
    } else if pane.active {
        (theme.success, "▣ ")
    } else {
        (theme.border, "▪ ")
    };

    let title = Line::from(vec![
        Span::raw(" "),
        Span::styled(icon, Style::default().fg(border_color)),
        Span::styled(
            pane.title.clone(),
            Style::default()
                .fg(if pane.active {
                    theme.text
                } else {
                    theme.text_muted
                })
                .add_modifier(if pane.active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ),
        Span::raw(" "),
    ]);

    let base_style = Style::default().fg(theme.text).bg(theme.bg);
    let ansi_text = parse_ansi_text(&pane.content, base_style);

    let widget = Paragraph::new(ansi_text)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color))
                .style(Style::default().bg(theme.bg)),
        )
        .scroll((app.detail_scroll(), 0));
    frame.render_widget(widget, area);
}

fn draw_telemetry_view(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let metrics = app.system_metrics();

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(5)])
        .split(area);

    // Top 3 Telemetry Cards
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(34),
            Constraint::Percentage(32),
        ])
        .split(rows[0]);

    // Card 1: CPU
    let cpu_val = metrics.cpu_usage.unwrap_or(0.0);
    let cpu_color = if app.cpu_metrics_ready() {
        theme.heat_color(cpu_val)
    } else {
        theme.accent
    };
    let cpu_block = Block::default()
        .title(Line::from(vec![
            Span::styled(
                " ◆ CPU ",
                Style::default().fg(cpu_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} ", format_cpu_usage(cpu_val, app.cpu_metrics_ready())),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.surface_alt));
    let cpu_inner = cpu_block.inner(cards[0]);
    frame.render_widget(cpu_block, cards[0]);
    if cpu_inner.height >= 3 {
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(2), Constraint::Length(1)])
            .split(cpu_inner);
        let history = app.cpu_history();
        let spark = Sparkline::default()
            .data(history)
            .max(100)
            .style(Style::default().fg(cpu_color).bg(theme.surface_alt));
        frame.render_widget(spark, split[0]);
        let gauge = LineGauge::default()
            .ratio((cpu_val / 100.0).clamp(0.0, 1.0) as f64)
            .filled_style(Style::default().fg(cpu_color))
            .unfilled_style(Style::default().fg(theme.border_dim))
            .label(format!("{} cores", metrics.cpu_cores));
        frame.render_widget(gauge, split[1]);
    }

    // Card 2: Memory
    let mem_pct = metrics.memory_percent();
    let mem_color = theme.heat_color(mem_pct);
    let mem_block = Block::default()
        .title(Line::from(vec![
            Span::styled(
                " ◆ MEM ",
                Style::default().fg(mem_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{mem_pct:.0}% "),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.surface_alt));
    let mem_inner = mem_block.inner(cards[1]);
    frame.render_widget(mem_block, cards[1]);
    if mem_inner.height >= 3 {
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(2), Constraint::Length(1)])
            .split(mem_inner);
        let spark = Sparkline::default()
            .data(app.mem_history())
            .max(100)
            .style(Style::default().fg(mem_color).bg(theme.surface_alt));
        frame.render_widget(spark, split[0]);
        let gauge = LineGauge::default()
            .ratio((mem_pct / 100.0).clamp(0.0, 1.0) as f64)
            .filled_style(Style::default().fg(mem_color))
            .unfilled_style(Style::default().fg(theme.border_dim))
            .label(format!(
                "{}/{}",
                format_bytes_compact(metrics.memory_used),
                format_bytes_compact(metrics.memory_total)
            ));
        frame.render_widget(gauge, split[1]);
    }

    // Card 3: System Node
    let load = metrics
        .load_average
        .as_ref()
        .map(|l| format!("{:.2} {:.2} {:.2}", l.one, l.five, l.fifteen))
        .unwrap_or_else(|| "n/a".into());
    let sys_lines = vec![
        Line::from(vec![
            Span::styled(" HOST ", Style::default().fg(theme.text_dim)),
            Span::styled(
                truncate_text(&metrics.host_name, 16),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(" OS   ", Style::default().fg(theme.text_dim)),
            Span::styled(
                truncate_text(&metrics.os_label, 16),
                Style::default().fg(theme.info),
            ),
        ]),
        Line::from(vec![
            Span::styled(" LOAD ", Style::default().fg(theme.text_dim)),
            Span::styled(load, Style::default().fg(theme.accent)),
        ]),
        Line::from(vec![
            Span::styled(" UP   ", Style::default().fg(theme.text_dim)),
            Span::styled(
                format_uptime(metrics.uptime_secs),
                Style::default().fg(theme.success),
            ),
            Span::styled(
                if metrics.swap_total > 0 {
                    format!("  SWP {}", format_bytes_compact(metrics.swap_used))
                } else {
                    String::new()
                },
                Style::default().fg(theme.text_muted),
            ),
        ]),
    ];
    let sys_card = Paragraph::new(sys_lines).block(
        Block::default()
            .title(Line::from(Span::styled(
                " ◆ NODE ",
                Style::default()
                    .fg(theme.accent_alt)
                    .add_modifier(Modifier::BOLD),
            )))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.surface_alt)),
    );
    frame.render_widget(sys_card, cards[2]);

    // Bottom Pane Resource Matrix Table
    let telemetry_rows = app.telemetry_rows();
    let header = Row::new(vec![
        Cell::from("SESSION"),
        Cell::from("WINDOW"),
        Cell::from("PANE"),
        Cell::from("CMD"),
        Cell::from("PID"),
        Cell::from("PROC"),
        Cell::from("CPU"),
        Cell::from("MEMORY"),
    ])
    .style(
        Style::default()
            .fg(theme.accent)
            .bg(theme.surface_alt)
            .add_modifier(Modifier::BOLD),
    );

    let table_rows: Vec<Row> = telemetry_rows
        .iter()
        .map(|row| {
            let cpu = row.cpu_percent.unwrap_or(0.0);
            let cpu_text = row
                .cpu_percent
                .map(|c| format_cpu_usage(c, app.cpu_metrics_ready()))
                .unwrap_or_else(|| "n/a".into());
            let cpu_bar = render_meter_bar((cpu / 100.0) as f64, 5);
            let cpu_cell = Cell::from(format!("{cpu_text:>6} {cpu_bar}"))
                .style(Style::default().fg(theme.heat_color(cpu)));

            Row::new(vec![
                Cell::from(truncate_text(&row.session_name, 14)).style(Style::default().fg(
                    if row.active {
                        theme.success
                    } else {
                        theme.text
                    },
                )),
                Cell::from(truncate_text(&row.window_label, 14))
                    .style(Style::default().fg(theme.text_muted)),
                Cell::from(truncate_text(&row.pane_label, 14))
                    .style(Style::default().fg(theme.text)),
                Cell::from(truncate_text(&row.command, 12)).style(Style::default().fg(theme.info)),
                Cell::from(row.pid.clone()).style(Style::default().fg(theme.text_dim)),
                Cell::from(row.process_count.to_string())
                    .style(Style::default().fg(theme.text_muted)),
                cpu_cell,
                Cell::from(format_bytes_compact(row.memory_bytes))
                    .style(Style::default().fg(theme.accent_soft)),
            ])
        })
        .collect();

    let mut table_state = TableState::default();
    if !telemetry_rows.is_empty() {
        table_state.select(Some(app.telemetry_selected().min(telemetry_rows.len() - 1)));
    }

    let table = Table::new(
        table_rows,
        [
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(13),
            Constraint::Length(7),
            Constraint::Length(5),
            Constraint::Length(13),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(Line::from(vec![
                Span::styled(
                    " ◉ LIVE PROCESS MATRIX ",
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({} panes · j/k select · ⏎ jump) ", telemetry_rows.len()),
                    Style::default().fg(theme.text_dim),
                ),
            ]))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.surface)),
    )
    .row_highlight_style(
        Style::default()
            .bg(theme.selection_bg)
            .fg(theme.selection_fg)
            .add_modifier(Modifier::BOLD),
    );

    frame.render_stateful_widget(table, rows[1], &mut table_state);
}

fn draw_buffers_view(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let buffers = app.buffers();

    if buffers.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "◈ PASTE BUFFER VAULT IS EMPTY",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "Select a pane and press ",
                    Style::default().fg(theme.text_muted),
                ),
                theme.keycap("y"),
                Span::styled(
                    " to capture its contents into tmux's buffer vault.",
                    Style::default().fg(theme.text_muted),
                ),
            ]),
        ])
        .alignment(Alignment::Center);
        frame.render_widget(empty, area);
        return;
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(36), Constraint::Percentage(64)])
        .split(area);

    let items: Vec<ListItem> = buffers
        .iter()
        .map(|buf| {
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        format!(" ◆ {}", buf.name),
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  {}B", buf.size),
                        Style::default().fg(theme.warning),
                    ),
                ]),
                Line::from(Span::styled(
                    format!("   {}", truncate_text(buf.sample.trim(), 28)),
                    Style::default().fg(theme.text_dim),
                )),
            ])
        })
        .collect();

    let mut state = ListState::default();
    let idx = app.buffer_selected().min(buffers.len().saturating_sub(1));
    state.select(Some(idx));

    let list = List::new(items)
        .block(
            Block::default()
                .title(Line::from(Span::styled(
                    format!(" ◈ BUFFERS ({}) ", buffers.len()),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                )))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.border))
                .style(Style::default().bg(theme.surface_alt)),
        )
        .highlight_style(
            Style::default()
                .bg(theme.selection_bg)
                .fg(theme.selection_fg)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, cols[0], &mut state);

    let selected_buf = &buffers[idx];
    let preview = Paragraph::new(selected_buf.content.clone())
        .block(
            Block::default()
                .title(Line::from(vec![
                    Span::styled(
                        format!(" {} ", selected_buf.name),
                        Style::default()
                            .fg(theme.success)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(
                            "({} bytes · {}) ",
                            selected_buf.size,
                            selected_buf.server.label()
                        ),
                        Style::default().fg(theme.text_muted),
                    ),
                ]))
                .title_bottom(
                    Line::from(vec![
                        theme.keycap("⏎"),
                        Span::styled(" Paste  ", Style::default().fg(theme.text_muted)),
                        theme.keycap("Y"),
                        Span::styled(" Clipboard  ", Style::default().fg(theme.text_muted)),
                        theme.keycap("x"),
                        Span::styled(" Delete ", Style::default().fg(theme.text_muted)),
                    ])
                    .alignment(Alignment::Right),
                )
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.border))
                .style(Style::default().bg(theme.bg).fg(theme.text)),
        )
        .scroll((app.buffer_scroll(), 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(preview, cols[1]);
}

fn find_detail(item: &TreeItem, key: &str) -> String {
    item.details
        .iter()
        .find(|(label, _)| label == key)
        .map(|(_, val)| val.clone())
        .unwrap_or_else(|| "—".into())
}

fn scaled_pane_rect(
    area: Rect,
    pane: &TerminalPanePreview,
    source_width: u16,
    source_height: u16,
) -> Option<Rect> {
    if area.width == 0 || area.height == 0 {
        return None;
    }

    let left = scale_coordinate(pane.left, source_width, area.width);
    let top = scale_coordinate(pane.top, source_height, area.height);
    if left >= area.width || top >= area.height {
        return None;
    }

    let right = scale_coordinate(
        pane.left.saturating_add(pane.width),
        source_width,
        area.width,
    )
    .max(left.saturating_add(1));
    let bottom = scale_coordinate(
        pane.top.saturating_add(pane.height),
        source_height,
        area.height,
    )
    .max(top.saturating_add(1));

    let width = right
        .saturating_sub(left)
        .max(1)
        .min(area.width.saturating_sub(left));
    let height = bottom
        .saturating_sub(top)
        .max(1)
        .min(area.height.saturating_sub(top));

    Some(Rect::new(
        area.x.saturating_add(left),
        area.y.saturating_add(top),
        width,
        height,
    ))
}

fn scale_coordinate(value: u16, source: u16, target: u16) -> u16 {
    if source == 0 {
        0
    } else {
        ((value as u32 * target as u32) / source as u32) as u16
    }
}
