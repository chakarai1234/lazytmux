mod hud;
mod inspector;
mod overlays;
mod tree;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::Block;
use ratatui::Frame;

use crate::app::{App, LayoutRegions};

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let theme = app.theme();

    // Paint the entire terminal background for crisp futuristic contrast.
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.bg).fg(theme.text)),
        area,
    );

    if area.width < 16 || area.height < 8 {
        return;
    }

    let header_height = if area.height >= 20 { 4 } else { 3 };
    let footer_height = 3;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(4),
            Constraint::Length(footer_height),
        ])
        .split(area);

    let mut regions = LayoutRegions::default();

    hud::draw_header(frame, chunks[0], app, &mut regions);
    draw_body(frame, chunks[1], app, &mut regions);
    hud::draw_footer(frame, chunks[2], app);

    app.set_regions(regions);

    // Overlays rendered in strict z-order priority.
    if app.show_help() {
        overlays::draw_help(frame, area, app);
    }

    if app.show_diagnostics() {
        overlays::draw_diagnostics(frame, area, app);
    }

    if let Some(picker) = app.layout_picker() {
        overlays::draw_layout_picker(frame, area, app, picker);
    }

    if let Some(jump) = app.quick_jump() {
        overlays::draw_quick_jump(frame, area, app, jump);
    }

    if let Some(palette) = app.palette() {
        overlays::draw_palette(frame, area, app, palette);
    }

    if let Some(confirm) = app.confirm() {
        overlays::draw_confirm(frame, area, app, &confirm.message);
    }

    if let Some(prompt) = app.prompt() {
        overlays::draw_prompt(frame, area, app, prompt);
    }
}

fn draw_body(frame: &mut Frame, area: Rect, app: &App, regions: &mut LayoutRegions) {
    if app.zen_mode() || area.width < 56 {
        regions.inspector_area = area;
        inspector::draw_inspector(frame, area, app, regions);
        return;
    }

    let left_pct = if area.width >= 120 { 40 } else { 44 };
    let right_pct = 100 - left_pct;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(left_pct),
            Constraint::Percentage(right_pct),
        ])
        .split(area);

    tree::draw_tree(frame, chunks[0], app, regions);
    regions.inspector_area = chunks[1];
    inspector::draw_inspector(frame, chunks[1], app, regions);
}

pub(crate) fn truncate_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let mut out: String = text.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}

pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
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
