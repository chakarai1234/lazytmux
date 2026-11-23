use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;

use crate::app::{App, FocusPane, LayoutRegions, TreeItem, TreeItemKind};
use crate::metrics::format_bytes_compact;
use crate::theme::Theme;
use crate::ui::truncate_text;

pub fn draw_tree(frame: &mut Frame, area: Rect, app: &App, regions: &mut LayoutRegions) {
    let theme = app.theme();
    let focused = app.focus() == FocusPane::Tree;

    let mut title_spans = vec![
        Span::raw(" "),
        Span::styled(
            if focused { "◆ " } else { "◇ " },
            Style::default().fg(if focused {
                theme.accent
            } else {
                theme.text_dim
            }),
        ),
        Span::styled(
            "01 // TOPOLOGY",
            Style::default()
                .fg(if focused { theme.accent } else { theme.text })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        theme.outline_tag(format!("⇅ {}", app.sort_mode().label()), theme.text_muted),
        Span::raw(" "),
    ];

    if app.favorites_only() {
        title_spans.push(theme.outline_tag("★", theme.warning));
        title_spans.push(Span::raw(" "));
    }

    let bottom_counter = if app.items().is_empty() {
        " 0 / 0 ".to_string()
    } else {
        format!(" {} / {} ", app.selected() + 1, app.items().len())
    };

    let block = theme
        .panel_block(Line::from(title_spans), focused)
        .title_bottom(
            Line::from(vec![
                Span::styled(" e:exp  c:col  /:flt ", Style::default().fg(theme.text_dim)),
                Span::styled(
                    bottom_counter,
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
            ])
            .alignment(Alignment::Right),
        );

    let inner = block.inner(area);
    regions.tree_inner = inner;

    if app.items().is_empty() {
        frame.render_widget(block, area);
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "◈ NO ACTIVE TMUX TARGETS",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                theme.keycap("n"),
                Span::styled(" Create session   ", Style::default().fg(theme.text_muted)),
                theme.keycap("N"),
                Span::styled(" Launch preset", Style::default().fg(theme.text_muted)),
            ]),
            Line::from(""),
            Line::from(vec![
                theme.keycap("D"),
                Span::styled(
                    " Socket diagnostics   ",
                    Style::default().fg(theme.text_muted),
                ),
                theme.keycap("f"),
                Span::styled(" Clear filter", Style::default().fg(theme.text_muted)),
            ]),
        ])
        .alignment(Alignment::Center);
        frame.render_widget(empty, inner);
        return;
    }

    let row_width = inner.width.saturating_sub(2) as usize;
    let list_items: Vec<ListItem> = app
        .items()
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            render_tree_row(
                item,
                idx == app.selected(),
                row_width,
                &theme,
                app.cpu_metrics_ready(),
            )
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected()));

    let list = List::new(list_items).block(block).highlight_style(
        Style::default()
            .bg(theme.selection_bg)
            .fg(theme.selection_fg)
            .add_modifier(Modifier::BOLD),
    );

    frame.render_stateful_widget(list, area, &mut state);
    regions.tree_scroll_offset = state.offset();

    // Futuristic vertical scrollbar along right panel border
    if app.items().len() > inner.height as usize {
        let mut scrollbar_state = ScrollbarState::new(app.items().len()).position(app.selected());
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_symbol("┃")
            .track_symbol(Some("│"))
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::default().fg(theme.accent))
            .track_style(Style::default().fg(theme.border_dim));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

fn render_tree_row<'a>(
    item: &TreeItem,
    selected: bool,
    row_width: usize,
    theme: &Theme,
    cpu_ready: bool,
) -> ListItem<'a> {
    let mut left_spans: Vec<Span<'static>> = Vec::new();

    // Glowing selection indicator bar
    left_spans.push(Span::styled(
        if selected { "▎" } else { " " },
        Style::default().fg(if selected {
            theme.accent
        } else {
            theme.border_dim
        }),
    ));

    // Hierarchy connector guides
    match item.depth {
        0 => {}
        1 => {
            left_spans.push(Span::styled("├─", Style::default().fg(theme.border_dim)));
        }
        _ => {
            left_spans.push(Span::styled("│ └─", Style::default().fg(theme.border_dim)));
        }
    }

    // Expansion chevron
    let chevron = match item.expanded {
        Some(true) => "▾ ",
        Some(false) => "▸ ",
        None => "",
    };
    if !chevron.is_empty() {
        left_spans.push(Span::styled(
            chevron,
            Style::default().fg(if item.expanded == Some(true) {
                theme.accent
            } else {
                theme.text_dim
            }),
        ));
    }

    // Favorite star
    if item.favorite {
        left_spans.push(Span::styled("★ ", Style::default().fg(theme.warning)));
    }

    // Kind geometric glyph + styled name
    let (glyph, glyph_color) = match item.kind {
        TreeItemKind::Session => (
            "◆ ",
            if item.active {
                theme.success
            } else {
                theme.accent
            },
        ),
        TreeItemKind::Window => (
            "◇ ",
            if item.active {
                theme.info
            } else {
                theme.text_muted
            },
        ),
        TreeItemKind::Pane => {
            if item.dead {
                ("✕ ", theme.danger)
            } else if item.active {
                ("▣ ", theme.success)
            } else {
                ("▪ ", theme.text_dim)
            }
        }
    };

    left_spans.push(Span::styled(glyph, Style::default().fg(glyph_color)));

    // Build right-aligned telemetry & badges string
    let mut badges: Vec<(String, Style)> = Vec::new();
    if item.synchronized {
        badges.push((
            "⇄".into(),
            Style::default()
                .fg(theme.accent_alt)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if item.zoomed {
        badges.push((
            "⊕".into(),
            Style::default()
                .fg(theme.accent_soft)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if item.marked {
        badges.push((
            "✦".into(),
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if item.in_mode {
        badges.push((
            "COPY".into(),
            Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
        ));
    }

    if row_width >= 34 {
        if let Some(cpu) = item.cpu_percent {
            if cpu_ready {
                badges.push((
                    format!("{cpu:>4.1}%"),
                    Style::default().fg(theme.heat_color(cpu)),
                ));
            }
        }
        if item.memory_bytes > 0 {
            badges.push((
                format!("{:>4}", format_bytes_compact(item.memory_bytes)),
                Style::default().fg(theme.text_muted),
            ));
        }
    }

    let right_len: usize = badges
        .iter()
        .map(|(text, _)| text.chars().count() + 1)
        .sum();
    let prefix_len: usize = left_spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum();

    let available_for_title = row_width
        .saturating_sub(prefix_len)
        .saturating_sub(right_len)
        .saturating_sub(1);

    let name_style = if item.dead {
        Style::default().fg(theme.danger)
    } else if item.favorite {
        Style::default()
            .fg(theme.warning)
            .add_modifier(Modifier::BOLD)
    } else if item.active {
        Style::default()
            .fg(if selected {
                theme.selection_fg
            } else {
                theme.text
            })
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text)
    };

    let title_budget = available_for_title.min(item.name.chars().count().max(8));
    let shown_name = truncate_text(&item.name, title_budget.max(4));
    let shown_name_len = shown_name.chars().count();
    left_spans.push(Span::styled(shown_name, name_style));

    let remaining_for_sub = available_for_title.saturating_sub(shown_name_len);
    let mut sub_used = 0usize;
    if remaining_for_sub >= 6 && !item.subtitle.is_empty() {
        let sub_text = truncate_text(&item.subtitle, remaining_for_sub.saturating_sub(2));
        sub_used = sub_text.chars().count() + 2;
        left_spans.push(Span::raw("  "));
        left_spans.push(Span::styled(sub_text, Style::default().fg(theme.text_dim)));
    }

    // Pad between left content and right-aligned telemetry
    let used_left = prefix_len + shown_name_len + sub_used;
    let pad = row_width
        .saturating_sub(used_left)
        .saturating_sub(right_len);
    if pad > 0 {
        left_spans.push(Span::raw(" ".repeat(pad)));
    }

    for (badge_text, style) in badges {
        left_spans.push(Span::raw(" "));
        left_spans.push(Span::styled(badge_text, style));
    }

    ListItem::new(Line::from(left_spans))
}
