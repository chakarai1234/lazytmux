use std::fs;
use std::path::PathBuf;

use clap::ValueEnum;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum ThemeId {
    #[default]
    Cyber,
    Aurora,
    Solaris,
    Matrix,
    Frost,
}

impl ThemeId {
    pub const ALL: [ThemeId; 5] = [
        ThemeId::Cyber,
        ThemeId::Aurora,
        ThemeId::Solaris,
        ThemeId::Matrix,
        ThemeId::Frost,
    ];

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Cyber => "CYBER NEON",
            Self::Aurora => "AURORA PULSE",
            Self::Solaris => "SOLARIS AMBER",
            Self::Matrix => "MATRIX GRID",
            Self::Frost => "FROST MINIMAL",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Cyber => "cyber",
            Self::Aurora => "aurora",
            Self::Solaris => "solaris",
            Self::Matrix => "matrix",
            Self::Frost => "frost",
        }
    }

    pub fn from_slug(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "cyber" | "cyber-neon" | "neon" => Some(Self::Cyber),
            "aurora" | "aurora-pulse" => Some(Self::Aurora),
            "solaris" | "solaris-amber" | "amber" => Some(Self::Solaris),
            "matrix" | "matrix-grid" => Some(Self::Matrix),
            "frost" | "frost-minimal" | "minimal" => Some(Self::Frost),
            _ => None,
        }
    }

    pub fn palette(self) -> Theme {
        match self {
            Self::Cyber => Theme {
                id: self,
                bg: Color::Rgb(7, 11, 20),
                surface: Color::Rgb(12, 19, 34),
                surface_alt: Color::Rgb(17, 27, 48),
                overlay_bg: Color::Rgb(10, 16, 30),
                border: Color::Rgb(42, 66, 108),
                border_focus: Color::Rgb(0, 240, 255),
                border_dim: Color::Rgb(28, 42, 70),
                accent: Color::Rgb(0, 240, 255),
                accent_alt: Color::Rgb(255, 46, 151),
                accent_soft: Color::Rgb(157, 78, 221),
                text: Color::Rgb(226, 236, 255),
                text_muted: Color::Rgb(138, 160, 198),
                text_dim: Color::Rgb(84, 104, 140),
                success: Color::Rgb(0, 255, 157),
                warning: Color::Rgb(255, 184, 0),
                danger: Color::Rgb(255, 68, 102),
                info: Color::Rgb(76, 201, 240),
                selection_bg: Color::Rgb(22, 46, 78),
                selection_fg: Color::Rgb(245, 252, 255),
                badge_fg: Color::Rgb(7, 11, 20),
                keycap_bg: Color::Rgb(22, 36, 62),
                keycap_fg: Color::Rgb(0, 240, 255),
            },
            Self::Aurora => Theme {
                id: self,
                bg: Color::Rgb(6, 14, 24),
                surface: Color::Rgb(11, 23, 38),
                surface_alt: Color::Rgb(16, 33, 54),
                overlay_bg: Color::Rgb(9, 19, 32),
                border: Color::Rgb(42, 82, 106),
                border_focus: Color::Rgb(44, 234, 163),
                border_dim: Color::Rgb(26, 52, 70),
                accent: Color::Rgb(44, 234, 163),
                accent_alt: Color::Rgb(168, 85, 247),
                accent_soft: Color::Rgb(56, 189, 248),
                text: Color::Rgb(230, 246, 250),
                text_muted: Color::Rgb(136, 178, 196),
                text_dim: Color::Rgb(78, 114, 134),
                success: Color::Rgb(52, 211, 153),
                warning: Color::Rgb(251, 191, 36),
                danger: Color::Rgb(251, 113, 133),
                info: Color::Rgb(56, 189, 248),
                selection_bg: Color::Rgb(18, 52, 68),
                selection_fg: Color::Rgb(244, 255, 252),
                badge_fg: Color::Rgb(6, 14, 24),
                keycap_bg: Color::Rgb(18, 42, 62),
                keycap_fg: Color::Rgb(44, 234, 163),
            },
            Self::Solaris => Theme {
                id: self,
                bg: Color::Rgb(13, 10, 7),
                surface: Color::Rgb(22, 17, 12),
                surface_alt: Color::Rgb(33, 24, 16),
                overlay_bg: Color::Rgb(18, 14, 10),
                border: Color::Rgb(96, 68, 36),
                border_focus: Color::Rgb(255, 176, 0),
                border_dim: Color::Rgb(60, 44, 24),
                accent: Color::Rgb(255, 176, 0),
                accent_alt: Color::Rgb(255, 107, 0),
                accent_soft: Color::Rgb(245, 210, 120),
                text: Color::Rgb(246, 234, 212),
                text_muted: Color::Rgb(188, 158, 118),
                text_dim: Color::Rgb(126, 98, 66),
                success: Color::Rgb(74, 222, 128),
                warning: Color::Rgb(255, 196, 46),
                danger: Color::Rgb(255, 77, 77),
                info: Color::Rgb(255, 145, 77),
                selection_bg: Color::Rgb(58, 38, 16),
                selection_fg: Color::Rgb(255, 248, 232),
                badge_fg: Color::Rgb(13, 10, 7),
                keycap_bg: Color::Rgb(48, 34, 18),
                keycap_fg: Color::Rgb(255, 176, 0),
            },
            Self::Matrix => Theme {
                id: self,
                bg: Color::Rgb(4, 10, 6),
                surface: Color::Rgb(8, 18, 11),
                surface_alt: Color::Rgb(13, 28, 18),
                overlay_bg: Color::Rgb(6, 14, 9),
                border: Color::Rgb(30, 86, 50),
                border_focus: Color::Rgb(0, 255, 102),
                border_dim: Color::Rgb(18, 52, 30),
                accent: Color::Rgb(0, 255, 102),
                accent_alt: Color::Rgb(163, 255, 18),
                accent_soft: Color::Rgb(0, 204, 136),
                text: Color::Rgb(212, 255, 226),
                text_muted: Color::Rgb(112, 186, 138),
                text_dim: Color::Rgb(58, 116, 78),
                success: Color::Rgb(0, 255, 102),
                warning: Color::Rgb(217, 249, 157),
                danger: Color::Rgb(255, 51, 85),
                info: Color::Rgb(45, 212, 191),
                selection_bg: Color::Rgb(14, 54, 28),
                selection_fg: Color::Rgb(236, 255, 242),
                badge_fg: Color::Rgb(4, 10, 6),
                keycap_bg: Color::Rgb(14, 42, 24),
                keycap_fg: Color::Rgb(0, 255, 102),
            },
            Self::Frost => Theme {
                id: self,
                bg: Color::Rgb(11, 15, 23),
                surface: Color::Rgb(17, 23, 35),
                surface_alt: Color::Rgb(24, 33, 49),
                overlay_bg: Color::Rgb(14, 19, 29),
                border: Color::Rgb(58, 74, 98),
                border_focus: Color::Rgb(125, 211, 252),
                border_dim: Color::Rgb(38, 49, 68),
                accent: Color::Rgb(125, 211, 252),
                accent_alt: Color::Rgb(192, 132, 252),
                accent_soft: Color::Rgb(148, 163, 184),
                text: Color::Rgb(241, 245, 249),
                text_muted: Color::Rgb(148, 163, 184),
                text_dim: Color::Rgb(92, 108, 132),
                success: Color::Rgb(74, 222, 128),
                warning: Color::Rgb(250, 204, 21),
                danger: Color::Rgb(248, 113, 113),
                info: Color::Rgb(125, 211, 252),
                selection_bg: Color::Rgb(30, 48, 72),
                selection_fg: Color::Rgb(248, 250, 252),
                badge_fg: Color::Rgb(11, 15, 23),
                keycap_bg: Color::Rgb(30, 41, 59),
                keycap_fg: Color::Rgb(186, 230, 253),
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub id: ThemeId,
    pub bg: Color,
    pub surface: Color,
    pub surface_alt: Color,
    pub overlay_bg: Color,
    pub border: Color,
    pub border_focus: Color,
    pub border_dim: Color,
    pub accent: Color,
    pub accent_alt: Color,
    pub accent_soft: Color,
    pub text: Color,
    pub text_muted: Color,
    pub text_dim: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub info: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub badge_fg: Color,
    pub keycap_bg: Color,
    pub keycap_fg: Color,
}

impl Theme {
    pub fn heat_color(&self, percent: f32) -> Color {
        if percent >= 80.0 {
            self.danger
        } else if percent >= 50.0 {
            self.warning
        } else if percent >= 20.0 {
            self.accent
        } else {
            self.success
        }
    }

    pub fn pill(&self, label: impl Into<String>, color: Color) -> Span<'static> {
        Span::styled(
            format!(" {} ", label.into()),
            Style::default()
                .bg(color)
                .fg(self.badge_fg)
                .add_modifier(Modifier::BOLD),
        )
    }

    pub fn outline_tag(&self, label: impl Into<String>, color: Color) -> Span<'static> {
        Span::styled(
            format!("[{}]", label.into()),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )
    }

    pub fn keycap(&self, key: impl Into<String>) -> Span<'static> {
        Span::styled(
            format!(" {} ", key.into()),
            Style::default()
                .bg(self.keycap_bg)
                .fg(self.keycap_fg)
                .add_modifier(Modifier::BOLD),
        )
    }

    pub fn panel_block<'a>(&self, title: Line<'a>, focused: bool) -> Block<'a> {
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_type(if focused {
                BorderType::Thick
            } else {
                BorderType::Rounded
            })
            .border_style(Style::default().fg(if focused {
                self.border_focus
            } else {
                self.border
            }))
            .style(Style::default().bg(self.surface).fg(self.text))
    }

    pub fn modal_block<'a>(&self, title: Line<'a>, border_color: Color) -> Block<'a> {
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(self.overlay_bg).fg(self.text))
    }
}

pub fn render_meter_bar(ratio: f64, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let clamped = ratio.clamp(0.0, 1.0);
    let filled = (clamped * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

pub fn load_saved_theme() -> ThemeId {
    let Some(path) = theme_path() else {
        return ThemeId::default();
    };
    let Ok(content) = fs::read_to_string(path) else {
        return ThemeId::default();
    };
    ThemeId::from_slug(&content).unwrap_or_default()
}

pub fn save_theme(theme: ThemeId) -> anyhow::Result<()> {
    let Some(path) = theme_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, theme.slug())?;
    Ok(())
}

fn theme_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(path).join("lazytmux/theme"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state/lazytmux/theme"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_through_all_themes() {
        let mut current = ThemeId::Cyber;
        let mut seen = Vec::new();
        for _ in 0..ThemeId::ALL.len() {
            seen.push(current);
            current = current.next();
        }
        assert_eq!(seen.len(), 5);
        assert_eq!(current, ThemeId::Cyber);
    }

    #[test]
    fn parses_theme_slugs_case_insensitively() {
        assert_eq!(ThemeId::from_slug("CYBER"), Some(ThemeId::Cyber));
        assert_eq!(ThemeId::from_slug("aurora-pulse"), Some(ThemeId::Aurora));
        assert_eq!(ThemeId::from_slug("solaris"), Some(ThemeId::Solaris));
        assert_eq!(ThemeId::from_slug("matrix"), Some(ThemeId::Matrix));
        assert_eq!(ThemeId::from_slug("frost"), Some(ThemeId::Frost));
        assert_eq!(ThemeId::from_slug("unknown"), None);
    }

    #[test]
    fn renders_meter_bar_clamped() {
        assert_eq!(render_meter_bar(0.0, 4), "░░░░");
        assert_eq!(render_meter_bar(0.5, 4), "██░░");
        assert_eq!(render_meter_bar(1.5, 4), "████");
    }
}
