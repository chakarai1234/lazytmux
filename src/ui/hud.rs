use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Sparkline};
use ratatui::Frame;

use crate::app::{App, FocusPane, LayoutRegions, StatusLevel, TreeItemKind, ViewMode};
use crate::metrics::{format_bytes_compact, format_cpu_usage};
use crate::theme::render_meter_bar;

pub fn draw_header(frame: &mut Frame, area: Rect, app: &App, regions: &mut LayoutRegions) {
    let theme = app.theme();
    let (sessions, windows, panes) = app.counts();
    let metrics = app.system_metrics();

    let pulse_frames = ["●", "◉", "◎", "◉"];
    let pulse = pulse_frames[(app.tick() as usize / 3) % pulse_frames.len()];

    let live_label = if app.paused() {
        "❚❚ PAUSED".to_string()
    } else {
        format!("{pulse} LIVE {}s", app.refresh_seconds())
    };
    let live_color = if app.paused() {
        theme.warning
    } else {
        theme.success
    };

    let theme_label = format!("◈ {}", theme.id.name());

    let brand_text = " ◆ LAZYTMUX ";
    let sep_text = " // NEXUS ";
    let live_pill_w = (live_label.chars().count() + 2) as u16;
    let theme_tag_w = (theme_label.chars().count() + 2) as u16;

    let title_left = Line::from(vec![
        Span::raw(" "),
        theme.pill("◆ LAZYTMUX", theme.accent),
        Span::styled(
            sep_text,
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::BOLD),
        ),
        theme.pill(&live_label, live_color),
        Span::raw(" "),
        theme.outline_tag(&theme_label, theme.accent_alt),
        Span::raw(" "),
    ]);

    // Compute exact clickable coordinates matching title_left border layout.
    let live_x = area
        .x
        .saturating_add(2)
        .saturating_add(brand_text.chars().count() as u16)
        .saturating_add(sep_text.chars().count() as u16);
    regions.live_badge_rect = Some(Rect::new(live_x, area.y, live_pill_w, 1));
    let theme_x = live_x.saturating_add(live_pill_w).saturating_add(1);
    regions.theme_badge_rect = Some(Rect::new(theme_x, area.y, theme_tag_w, 1));

    let host_badge = if area.width >= 96 {
        let mut right_spans = vec![Span::raw(" ")];
        if let Some(version) = app.tmux_version() {
            right_spans.push(Span::styled(
                format!("{version} · "),
                Style::default().fg(theme.text_muted),
            ));
        }
        if !metrics.os_label.is_empty() {
            right_spans.push(Span::styled(
                metrics.os_label.clone(),
                Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
            ));
        }
        right_spans.push(Span::raw(" "));
        Some(Line::from(right_spans))
    } else {
        None
    };

    let mut block = Block::default()
        .title(title_left)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.surface).fg(theme.text));

    if let Some(right) = host_badge {
        block = block.title_top(right.alignment(Alignment::Right));
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width < 12 {
        return;
    }

    // Split inner telemetry strip into Counters | CPU Sparkline | Mem Gauge
    if inner.width >= 94 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(36),
                Constraint::Length(28),
                Constraint::Length(34),
            ])
            .split(inner);

        draw_counters_strip(frame, cols[0], app, sessions, windows, panes);
        draw_cpu_telemetry(frame, cols[1], app);
        draw_mem_telemetry(frame, cols[2], app);
    } else {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(inner);
        draw_counters_strip(frame, cols[0], app, sessions, windows, panes);
        draw_compact_telemetry(frame, cols[1], app);
    }
}

fn draw_counters_strip(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    sessions: usize,
    windows: usize,
    panes: usize,
) {
    let theme = app.theme();
    let mut line1 = vec![
        Span::styled(" SES ", Style::default().fg(theme.text_dim)),
        Span::styled(
            format!("{sessions:02}"),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  WIN ", Style::default().fg(theme.text_dim)),
        Span::styled(
            format!("{windows:02}"),
            Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  PNE ", Style::default().fg(theme.text_dim)),
        Span::styled(
            format!("{panes:02}"),
            Style::default()
                .fg(theme.success)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ★ ", Style::default().fg(theme.warning)),
        Span::styled(
            format!("{:02}", app.favorites_count()),
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if app.server_count() > 1 {
        line1.push(Span::styled("  SRV ", Style::default().fg(theme.text_dim)));
        line1.push(Span::styled(
            format!("{:02}", app.server_count()),
            Style::default()
                .fg(theme.accent_alt)
                .add_modifier(Modifier::BOLD),
        ));
    }

    if !app.buffers().is_empty() {
        line1.push(Span::styled("  BUF ", Style::default().fg(theme.text_dim)));
        line1.push(Span::styled(
            format!("{:02}", app.buffers().len()),
            Style::default()
                .fg(theme.accent_soft)
                .add_modifier(Modifier::BOLD),
        ));
    }

    let mut lines = vec![Line::from(line1)];

    if area.height >= 2 {
        let mut line2 = vec![Span::raw(" ")];
        if !app.filter().is_empty() {
            line2.push(theme.pill(format!("FILTER: {}", app.filter()), theme.accent_alt));
            line2.push(Span::raw(" "));
        }
        if app.favorites_only() {
            line2.push(theme.pill("★ STARRED ONLY", theme.warning));
            line2.push(Span::raw(" "));
        }
        if app.zen_mode() {
            line2.push(theme.pill("ZEN DECK", theme.accent_soft));
            line2.push(Span::raw(" "));
        }
        if line2.len() == 1 {
            let hint = if area.width >= 58 {
                "Ctrl+P Palette · Ctrl+S Quick Jump · 1-4 Decks · ? Atlas"
            } else if area.width >= 38 {
                "Ctrl+P Palette · 1-4 Decks · ? Atlas"
            } else {
                "Ctrl+P Palette · ? Atlas"
            };
            line2.push(Span::styled(hint, Style::default().fg(theme.text_dim)));
        }
        lines.push(Line::from(line2));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_cpu_telemetry(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let metrics = app.system_metrics();
    let cpu_val = metrics.cpu_usage.unwrap_or(0.0);
    let cpu_str = metrics
        .cpu_usage
        .map(|u| format_cpu_usage(u, app.cpu_metrics_ready()))
        .unwrap_or_else(|| "n/a".into());
    let heat = if app.cpu_metrics_ready() {
        theme.heat_color(cpu_val)
    } else {
        theme.text_muted
    };

    let load_str = metrics
        .load_average
        .as_ref()
        .map(|l| format!("LD {:.2}", l.one))
        .unwrap_or_default();

    let header = Line::from(vec![
        Span::styled("│ ", Style::default().fg(theme.border_dim)),
        Span::styled("CPU ", Style::default().fg(theme.text_dim)),
        Span::styled(
            format!("{cpu_str:<8}"),
            Style::default().fg(heat).add_modifier(Modifier::BOLD),
        ),
        Span::styled(load_str, Style::default().fg(theme.text_muted)),
    ]);

    if area.height >= 2 {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .split(area);
        frame.render_widget(Paragraph::new(header), rows[0]);

        let divider_area = Rect::new(rows[1].x, rows[1].y, 2.min(rows[1].width), 1);
        frame.render_widget(
            Paragraph::new(Span::styled("│ ", Style::default().fg(theme.border_dim))),
            divider_area,
        );

        let spark_area = Rect::new(
            rows[1].x.saturating_add(2),
            rows[1].y,
            rows[1].width.saturating_sub(3),
            1,
        );
        let history = app.cpu_history();
        if history.is_empty() {
            let meter = render_meter_bar((cpu_val / 100.0) as f64, spark_area.width as usize);
            frame.render_widget(
                Paragraph::new(Span::styled(meter, Style::default().fg(theme.border))),
                spark_area,
            );
        } else {
            let spark = Sparkline::default()
                .data(history)
                .max(100)
                .style(Style::default().fg(heat).bg(theme.surface));
            frame.render_widget(spark, spark_area);
        }
    } else {
        frame.render_widget(Paragraph::new(header), area);
    }
}

fn draw_mem_telemetry(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let metrics = app.system_metrics();
    let mem_pct = metrics.memory_percent();
    let heat = theme.heat_color(mem_pct);

    let line1 = Line::from(vec![
        Span::styled("│ ", Style::default().fg(theme.border_dim)),
        Span::styled("MEM ", Style::default().fg(theme.text_dim)),
        Span::styled(
            format!("{mem_pct:>4.0}% "),
            Style::default().fg(heat).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "{}/{}",
                format_bytes_compact(metrics.memory_used),
                format_bytes_compact(metrics.memory_total)
            ),
            Style::default().fg(theme.text_muted),
        ),
    ]);

    if area.height >= 2 {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .split(area);
        frame.render_widget(Paragraph::new(line1), rows[0]);

        let bar_width = rows[1].width.saturating_sub(4) as usize;
        let bar = render_meter_bar((mem_pct / 100.0) as f64, bar_width);
        let line2 = Line::from(vec![
            Span::styled("│ ", Style::default().fg(theme.border_dim)),
            Span::styled(bar, Style::default().fg(heat)),
        ]);
        frame.render_widget(Paragraph::new(line2), rows[1]);
    } else {
        frame.render_widget(Paragraph::new(line1), area);
    }
}

fn draw_compact_telemetry(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let metrics = app.system_metrics();
    let cpu_str = metrics
        .cpu_usage
        .map(|u| format_cpu_usage(u, app.cpu_metrics_ready()))
        .unwrap_or_else(|| "n/a".into());
    let mem_pct = metrics.memory_percent();

    let line = Line::from(vec![
        Span::styled("CPU ", Style::default().fg(theme.text_dim)),
        Span::styled(
            cpu_str,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  MEM ", Style::default().fg(theme.text_dim)),
        Span::styled(
            format!("{mem_pct:.0}%"),
            Style::default()
                .fg(theme.heat_color(mem_pct))
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(Paragraph::new(line).alignment(Alignment::Right), area);
}

pub fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();

    let (badge_label, badge_color) = match app.status_level() {
        StatusLevel::Info => ("◈ READY", theme.accent),
        StatusLevel::Success => ("✓ OK", theme.success),
        StatusLevel::Warning => ("▲ NOTICE", theme.warning),
        StatusLevel::Error => ("✕ ERROR", theme.danger),
    };

    let focus_tag = match app.focus() {
        FocusPane::Tree => "FOCUS: TREE",
        FocusPane::Inspector => "FOCUS: DECK",
    };

    let title_left = Line::from(vec![
        Span::raw(" "),
        theme.pill(badge_label, badge_color),
        Span::raw(" "),
        Span::styled(app.status(), Style::default().fg(theme.text)),
        Span::raw(" "),
    ]);

    let title_right = Line::from(vec![
        Span::raw(" "),
        theme.outline_tag(focus_tag, theme.text_muted),
        Span::raw(" "),
    ]);

    let block = Block::default()
        .title(title_left)
        .title_top(title_right.alignment(Alignment::Right))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.surface).fg(theme.text));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Context-aware glowing shortcut dock
    let hints: Vec<(&str, &str)> =
        if app.view_mode() == ViewMode::Buffers && app.focus() == FocusPane::Inspector {
            vec![
                ("Ctrl+P", "Palette"),
                ("⏎", "Paste"),
                ("Y", "Clipboard"),
                ("x", "Delete"),
                ("Tab", "Focus"),
                ("1-4", "Decks"),
                ("?", "Atlas"),
                ("q", "Quit"),
            ]
        } else {
            match app.selected_item().map(|item| &item.kind) {
                Some(TreeItemKind::Pane) => vec![
                    ("Ctrl+P", "Palette"),
                    ("⏎", "Attach"),
                    ("% / \"", "Split"),
                    ("z", "Zoom"),
                    ("s / S", "Send/Bcast"),
                    ("L", "Layout"),
                    ("b", "Sync"),
                    ("y / Y", "Copy"),
                    ("t", "Theme"),
                    ("?", "Atlas"),
                ],
                Some(TreeItemKind::Window) => vec![
                    ("Ctrl+P", "Palette"),
                    ("⏎", "Switch"),
                    ("Space", "Expand"),
                    ("L", "Layout"),
                    ("b", "Sync"),
                    ("o", "Rotate"),
                    ("S", "Broadcast"),
                    ("r", "Rename"),
                    ("t", "Theme"),
                    ("?", "Atlas"),
                ],
                _ => vec![
                    ("Ctrl+P", "Palette"),
                    ("Ctrl+S", "Jump"),
                    ("⏎", "Attach"),
                    ("Space", "Expand"),
                    ("n / N", "Session"),
                    ("w", "Window"),
                    ("e / c", "All"),
                    ("v", "Zen"),
                    ("t", "Theme"),
                    ("?", "Atlas"),
                ],
            }
        };

    let mut spans = vec![Span::raw(" ")];
    let mut used_width = 1usize;
    let max_width = inner.width as usize;

    for (key, label) in hints {
        let item_len = key.chars().count() + 2 + label.chars().count() + 3;
        if used_width + item_len > max_width {
            break;
        }
        spans.push(theme.keycap(key));
        spans.push(Span::styled(
            format!(" {label}  "),
            Style::default().fg(theme.text_muted),
        ));
        used_width += item_len;
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}
