use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

/// Parse text containing ANSI SGR escape sequences into styled ratatui `Text`.
/// Non-SGR CSI and OSC sequences are stripped cleanly so terminal previews never
/// show raw escape garbage.
pub fn parse_ansi_text(input: &str, base_style: Style) -> Text<'static> {
    let mut style = base_style;
    let lines: Vec<Line<'static>> = input
        .split('\n')
        .map(|raw_line| {
            parse_ansi_line_stateful(raw_line.trim_end_matches('\r'), &mut style, base_style)
        })
        .collect();
    Text::from(lines)
}

/// Remove all ANSI CSI/OSC/single-character escape sequences from `input`,
/// returning plain text suitable for clipboard and tmux paste buffers.
pub fn strip_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == 0x1b {
            index = skip_escape(bytes, index);
            continue;
        }
        let Some(ch) = input[index..].chars().next() else {
            break;
        };
        out.push(ch);
        index += ch.len_utf8();
    }

    out
}

fn parse_ansi_line_stateful(input: &str, style: &mut Style, base_style: Style) -> Line<'static> {
    let bytes = input.as_bytes();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut chunk = String::new();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == 0x1b {
            if !chunk.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut chunk), *style));
            }
            if index + 1 < bytes.len() && bytes[index + 1] == b'[' {
                let start = index + 2;
                let mut cursor = start;
                while cursor < bytes.len() && !(0x40..=0x7e).contains(&bytes[cursor]) {
                    cursor += 1;
                }
                if cursor < bytes.len() {
                    let final_byte = bytes[cursor];
                    if final_byte == b'm' {
                        let params = &input[start..cursor];
                        apply_sgr(params, style, base_style);
                    }
                    index = cursor + 1;
                    continue;
                }
                break;
            }
            index = skip_escape(bytes, index);
            continue;
        }

        let Some(ch) = input[index..].chars().next() else {
            break;
        };
        if !ch.is_control() || ch == '\t' {
            if ch == '\t' {
                chunk.push_str("    ");
            } else {
                chunk.push(ch);
            }
        }
        index += ch.len_utf8();
    }

    if !chunk.is_empty() {
        spans.push(Span::styled(chunk, *style));
    }

    Line::from(spans)
}

fn skip_escape(bytes: &[u8], start: usize) -> usize {
    if start + 1 >= bytes.len() {
        return bytes.len();
    }

    match bytes[start + 1] {
        // CSI: ESC [ ... final_byte (0x40..=0x7E)
        b'[' => {
            let mut cursor = start + 2;
            while cursor < bytes.len() && !(0x40..=0x7e).contains(&bytes[cursor]) {
                cursor += 1;
            }
            (cursor + 1).min(bytes.len())
        }
        // OSC: ESC ] ... (BEL or ST = ESC \)
        b']' | b'P' | b'_' | b'^' => {
            let mut cursor = start + 2;
            while cursor < bytes.len() {
                if bytes[cursor] == 0x07 {
                    return cursor + 1;
                }
                if bytes[cursor] == 0x1b && cursor + 1 < bytes.len() && bytes[cursor + 1] == b'\\' {
                    return cursor + 2;
                }
                cursor += 1;
            }
            bytes.len()
        }
        // Charset designation etc: ESC ( X
        b'(' | b')' | b'*' | b'+' => (start + 3).min(bytes.len()),
        _ => (start + 2).min(bytes.len()),
    }
}

fn apply_sgr(params: &str, style: &mut Style, base_style: Style) {
    if params.is_empty() {
        *style = base_style;
        return;
    }

    // Support both semicolon (`38;2;r;g;b`) and colon (`38:2::r:g:b`) subparameters.
    let tokens: Vec<&str> = params.split(';').collect();
    let mut index = 0;

    while index < tokens.len() {
        let token = tokens[index];
        if token.contains(':') {
            apply_colon_sgr(token, style, base_style);
            index += 1;
            continue;
        }

        let code = if token.is_empty() {
            0
        } else {
            match token.parse::<u16>() {
                Ok(value) => value,
                Err(_) => {
                    index += 1;
                    continue;
                }
            }
        };

        match code {
            0 => *style = base_style,
            1 => *style = style.add_modifier(Modifier::BOLD),
            2 => *style = style.add_modifier(Modifier::DIM),
            3 => *style = style.add_modifier(Modifier::ITALIC),
            4 => *style = style.add_modifier(Modifier::UNDERLINED),
            7 => *style = style.add_modifier(Modifier::REVERSED),
            9 => *style = style.add_modifier(Modifier::CROSSED_OUT),
            22 => *style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => *style = style.remove_modifier(Modifier::ITALIC),
            24 => *style = style.remove_modifier(Modifier::UNDERLINED),
            27 => *style = style.remove_modifier(Modifier::REVERSED),
            29 => *style = style.remove_modifier(Modifier::CROSSED_OUT),
            30..=37 => *style = style.fg(ansi_standard_color(code - 30)),
            39 => *style = style.fg(base_style.fg.unwrap_or(Color::Reset)),
            40..=47 => *style = style.bg(ansi_standard_color(code - 40)),
            49 => *style = style.bg(base_style.bg.unwrap_or(Color::Reset)),
            90..=97 => *style = style.fg(ansi_bright_color(code - 90)),
            100..=107 => *style = style.bg(ansi_bright_color(code - 100)),
            38 | 48 => {
                let is_fg = code == 38;
                if let Some(mode) = tokens.get(index + 1).and_then(|v| v.parse::<u16>().ok()) {
                    if mode == 5 {
                        if let Some(color_idx) =
                            tokens.get(index + 2).and_then(|v| v.parse::<u8>().ok())
                        {
                            let color = Color::Indexed(color_idx);
                            *style = if is_fg {
                                style.fg(color)
                            } else {
                                style.bg(color)
                            };
                            index += 3;
                            continue;
                        }
                    } else if mode == 2 {
                        let r = tokens.get(index + 2).and_then(|v| v.parse::<u8>().ok());
                        let g = tokens.get(index + 3).and_then(|v| v.parse::<u8>().ok());
                        let b = tokens.get(index + 4).and_then(|v| v.parse::<u8>().ok());
                        if let (Some(r), Some(g), Some(b)) = (r, g, b) {
                            let color = Color::Rgb(r, g, b);
                            *style = if is_fg {
                                style.fg(color)
                            } else {
                                style.bg(color)
                            };
                            index += 5;
                            continue;
                        }
                    }
                }
            }
            _ => {}
        }

        index += 1;
    }
}

fn apply_colon_sgr(token: &str, style: &mut Style, _base_style: Style) {
    let parts: Vec<&str> = token.split(':').collect();
    let Ok(code) = parts[0].parse::<u16>() else {
        return;
    };
    if code != 38 && code != 48 {
        return;
    }
    let is_fg = code == 38;
    let Some(mode) = parts.get(1).and_then(|v| v.parse::<u16>().ok()) else {
        return;
    };
    if mode == 5 {
        if let Some(idx) = parts.get(2).and_then(|v| v.parse::<u8>().ok()) {
            let color = Color::Indexed(idx);
            *style = if is_fg {
                style.fg(color)
            } else {
                style.bg(color)
            };
        }
    } else if mode == 2 {
        // Supports both 38:2:r:g:b and ISO 38:2:colorspace:r:g:b
        let offset = if parts.len() >= 6 { 3 } else { 2 };
        let r = parts.get(offset).and_then(|v| v.parse::<u8>().ok());
        let g = parts.get(offset + 1).and_then(|v| v.parse::<u8>().ok());
        let b = parts.get(offset + 2).and_then(|v| v.parse::<u8>().ok());
        if let (Some(r), Some(g), Some(b)) = (r, g, b) {
            let color = Color::Rgb(r, g, b);
            *style = if is_fg {
                style.fg(color)
            } else {
                style.bg(color)
            };
        }
    }
}

fn ansi_standard_color(index: u16) -> Color {
    match index {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        _ => Color::Gray,
    }
}

fn ansi_bright_color(index: u16) -> Color {
    match index {
        0 => Color::DarkGray,
        1 => Color::LightRed,
        2 => Color::LightGreen,
        3 => Color::LightYellow,
        4 => Color::LightBlue,
        5 => Color::LightMagenta,
        6 => Color::LightCyan,
        _ => Color::White,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_truecolor_and_bold_sgr() {
        let input = "\x1b[1;38;2;0;240;255mhello\x1b[0m world";
        let text = parse_ansi_text(input, Style::default());
        assert_eq!(text.lines.len(), 1);
        assert_eq!(text.lines[0].spans.len(), 2);
        assert_eq!(text.lines[0].spans[0].content, "hello");
        assert_eq!(
            text.lines[0].spans[0].style.fg,
            Some(Color::Rgb(0, 240, 255))
        );
        assert!(text.lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
        assert_eq!(text.lines[0].spans[1].content, " world");
    }

    #[test]
    fn parses_256_color_and_colon_truecolor() {
        let input = "\x1b[38;5;196mred\x1b[38:2::10:20:30mrgb";
        let text = parse_ansi_text(input, Style::default());
        assert_eq!(text.lines[0].spans[0].style.fg, Some(Color::Indexed(196)));
        assert_eq!(
            text.lines[0].spans[1].style.fg,
            Some(Color::Rgb(10, 20, 30))
        );
    }

    #[test]
    fn strips_csi_and_osc_sequences() {
        let input = "\x1b]0;window title\x07\x1b[32mok\x1b[0m\x1b[2K!";
        assert_eq!(strip_ansi(input), "ok!");
    }
}
