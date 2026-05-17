use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, TerminalPanePreview, TerminalPreview, TreeItem, TreeItemKind};

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(8),
            Constraint::Length(4),
        ])
        .split(area);

    draw_header(frame, chunks[0], app);
    draw_body(frame, chunks[1], app);
    draw_status(frame, chunks[2], app);

    if app.show_help() {
        draw_help(frame, area, app.help_scroll());
    }

    if app.show_diagnostics() {
        draw_diagnostics(frame, area, app);
    }

    if let Some(confirm) = app.confirm() {
        draw_confirm(frame, area, &confirm.message);
    }

    if let Some(prompt) = app.prompt() {
        draw_prompt(frame, area, &prompt.title, &prompt.value, &prompt.hint);
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let (sessions, windows, panes) = app.counts();
    let filter = if app.filter().is_empty() {
        "none".to_string()
    } else {
        app.filter().to_string()
    };

    let title = Line::from(vec![
        Span::styled(
            "lazytmux",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("tmux visualizer", Style::default().fg(Color::DarkGray)),
    ]);
    let stats = Line::from(format!(
        "sessions: {sessions}  windows: {windows}  panes: {panes}  favorites: {}  filter: {filter}  ?/F1 shortcuts",
        app.favorites_count()
    ));

    let header = Paragraph::new(vec![title, stats])
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Left);
    frame.render_widget(header, area);
}

fn draw_body(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    draw_tree(frame, chunks[0], app);
    draw_details(frame, chunks[1], app);
}

fn draw_tree(frame: &mut Frame, area: Rect, app: &App) {
    let list_items: Vec<ListItem> = if app.items().is_empty() {
        vec![ListItem::new(
            "No tmux targets. Press n to create a session.",
        )]
    } else {
        app.items().iter().map(render_tree_item).collect()
    };

    let mut state = ListState::default();
    if !app.items().is_empty() {
        state.select(Some(app.selected()));
    }

    let list = List::new(list_items)
        .block(
            Block::default()
                .title("Sessions / Windows / Panes")
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut state);
}

fn render_tree_item(item: &TreeItem) -> ListItem<'_> {
    let indent = "  ".repeat(item.depth);
    let mut style = Style::default();
    if item.active {
        style = style.fg(Color::Green).add_modifier(Modifier::BOLD);
    }
    if item.dead {
        style = style.fg(Color::Red);
    }
    if item.favorite {
        style = style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
    }

    let kind = match item.kind {
        TreeItemKind::Session => "session",
        TreeItemKind::Window => "window",
        TreeItemKind::Pane => "pane",
    };

    let line = Line::from(vec![
        Span::raw(indent),
        Span::styled(item.label.clone(), style),
        Span::raw("  "),
        Span::styled(kind, Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(item.subtitle.clone(), Style::default().fg(Color::Gray)),
    ]);
    ListItem::new(line)
}

fn draw_details(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title("Details / Actual View")
        .borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    if let Some(item) = app.selected_item() {
        lines.push(Line::from(vec![Span::styled(
            item.label.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(item.subtitle.clone()));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Target: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(item.target.display()),
        ]));
        lines.push(Line::from(""));
        for (label, value) in &item.details {
            lines.push(Line::from(vec![
                Span::styled(format!("{label}: "), Style::default().fg(Color::Yellow)),
                Span::raw(value.clone()),
            ]));
        }
    } else {
        lines.push(Line::from("No selection"));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "Press ? or F1 for shortcuts. Enter switches/attaches to the real tmux target.",
        Style::default().fg(Color::DarkGray),
    )]));

    if let Some(item) = app.selected_item() {
        if let Some(preview) = &item.preview {
            let detail_height = inner.height.min(8);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(detail_height), Constraint::Min(1)])
                .split(inner);
            let details = Paragraph::new(lines)
                .scroll((app.detail_scroll(), 0))
                .wrap(Wrap { trim: false });
            frame.render_widget(details, chunks[0]);
            draw_terminal_preview(frame, chunks[1], preview);
            return;
        }
    }

    let details = Paragraph::new(lines)
        .scroll((app.detail_scroll(), 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(details, inner);
}

fn draw_terminal_preview(frame: &mut Frame, area: Rect, preview: &TerminalPreview) {
    if area.width < 4 || area.height < 3 {
        return;
    }

    let block = Block::default()
        .title(preview.title.clone())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if preview.panes.is_empty() {
        let empty = Paragraph::new("No panes to preview").alignment(Alignment::Center);
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
            draw_preview_pane(frame, pane_area, pane);
        }
    }
}

fn draw_preview_pane(frame: &mut Frame, area: Rect, pane: &TerminalPanePreview) {
    let border_color = if pane.dead {
        Color::Red
    } else if pane.active {
        Color::Green
    } else {
        Color::DarkGray
    };

    let pane = Paragraph::new(pane.content.clone())
        .block(
            Block::default()
                .title(pane.title.clone())
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        )
        .style(Style::default().fg(Color::Gray));
    frame.render_widget(pane, area);
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

fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let status = Paragraph::new(vec![
        Line::from(app.status().to_string()),
        Line::from(Span::styled(
            "?/F1 shortcuts  |  D diagnostics  |  * favorite  |  s send keys  |  y copy pane",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .block(Block::default().title("Status").borders(Borders::ALL))
    .wrap(Wrap { trim: true });
    frame.render_widget(status, area);
}

fn draw_prompt(frame: &mut Frame, area: Rect, title: &str, value: &str, hint: &str) {
    let popup = centered_rect(70, 25, area);
    frame.render_widget(Clear, popup);

    let prompt = Paragraph::new(vec![
        Line::from(value.to_string()),
        Line::from(""),
        Line::from(Span::styled(
            hint.to_string(),
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .wrap(Wrap { trim: false });
    frame.render_widget(prompt, popup);
}

fn draw_confirm(frame: &mut Frame, area: Rect, message: &str) {
    let popup = centered_rect(70, 20, area);
    frame.render_widget(Clear, popup);
    let confirm = Paragraph::new(message.to_string())
        .block(
            Block::default()
                .title("Confirm destructive action")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red)),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(confirm, popup);
}

fn draw_diagnostics(frame: &mut Frame, area: Rect, app: &App) {
    frame.render_widget(Clear, area);
    let mut lines = vec![
        Line::from(vec![Span::styled(
            "tmux diagnostics",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(
            "Close with D/q/Esc. Scroll with j/k, arrows, PageUp/PageDown, or Ctrl-U/Ctrl-D.",
        ),
        Line::from(""),
    ];

    if let Some(item) = app.selected_item() {
        lines.push(Line::from(format!(
            "Selected target: {}",
            item.target.display()
        )));
        lines.push(Line::from(format!(
            "Selected server: {}",
            item.target.server_label()
        )));
        lines.push(Line::from(""));
    }

    if app.diagnostics().is_empty() {
        lines.push(Line::from("No diagnostics collected yet."));
    } else {
        for diagnostic in app.diagnostics() {
            lines.push(Line::from(diagnostic.clone()));
        }
    }

    let diagnostics = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Diagnostics")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .scroll((app.diagnostics_scroll(), 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(diagnostics, area);
}

fn draw_help(frame: &mut Frame, area: Rect, scroll: u16) {
    frame.render_widget(Clear, area);
    let help = Paragraph::new(vec![
        Line::from(vec![Span::styled(
            "Shortcuts page",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("Scroll with j/k, arrows, PageUp/PageDown, [ and ], or Ctrl-U/Ctrl-D."),
        Line::from("Close with ?/F1/q/Esc."),
        Line::from(""),
        Line::from("App navigation"),
        Line::from("  j/k or Up/Down: move selection"),
        Line::from("  g/G: top/bottom"),
        Line::from("  Space: expand/collapse selected session or window"),
        Line::from("  h/l or Left/Right: collapse/expand"),
        Line::from("  [ and ] or Ctrl-U/Ctrl-D: scroll details and pane content"),
        Line::from(""),
        Line::from("App tmux actions"),
        Line::from("  Enter: switch to target inside tmux, attach outside tmux"),
        Line::from("  n: new session"),
        Line::from("  N: launch session preset (name | start-dir | command)"),
        Line::from("  w: new window in selected session"),
        Line::from("  %: split selected pane left/right"),
        Line::from("  \": split selected pane top/bottom"),
        Line::from("  Cmd-R or r: rename session/window or set pane title"),
        Line::from("  x: kill selected item after confirmation"),
        Line::from("  z: toggle pane zoom"),
        Line::from("  *: toggle favorite/pin"),
        Line::from("  s: send keys to selected pane"),
        Line::from("  y: copy selected pane text to tmux buffer"),
        Line::from("  d: detach current client"),
        Line::from("  :: run arbitrary tmux command without the leading tmux word"),
        Line::from(""),
        Line::from("Native tmux shortcuts, default prefix Ctrl-b"),
        Line::from("  Prefix c: new window"),
        Line::from("  Prefix ,: rename window"),
        Line::from("  Prefix $: rename session"),
        Line::from("  Prefix %: split pane left/right"),
        Line::from("  Prefix \": split pane top/bottom"),
        Line::from("  Prefix x: kill pane"),
        Line::from("  Prefix z: toggle pane zoom"),
        Line::from("  Prefix n/p: next or previous window"),
        Line::from("  Prefix d: detach client"),
        Line::from("  Prefix [: copy mode"),
        Line::from(""),
        Line::from("View"),
        Line::from("  /: fuzzy multi-token filter"),
        Line::from("  f: clear filter"),
        Line::from("  R: refresh now"),
        Line::from("  D: diagnostics"),
        Line::from("  ?/q/Esc: close this shortcuts page"),
        Line::from("  q/Esc/Ctrl-C: quit when the shortcuts page is closed"),
    ])
    .block(
        Block::default()
            .title("Shortcuts")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .scroll((scroll, 0))
    .wrap(Wrap { trim: false });
    frame.render_widget(help, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
