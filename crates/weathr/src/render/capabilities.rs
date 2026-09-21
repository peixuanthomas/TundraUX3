use crossterm::style::Color;
use std::env;
use std::io::IsTerminal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSupport {
    None,
    Basic,
    Ansi256,
    TrueColor,
}

#[derive(Debug, Clone)]
pub struct TerminalCapabilities {
    pub color_support: ColorSupport,
    #[allow(dead_code)]
    pub is_tty: bool,
}

impl TerminalCapabilities {
    pub fn detect() -> Self {
        let is_tty = std::io::stdout().is_terminal();

        if env::var("NO_COLOR").is_ok() {
            return Self {
                color_support: ColorSupport::None,
                is_tty,
            };
        }

        if env::var("TERM").is_ok_and(|term| term == "dumb") {
            return Self {
                color_support: ColorSupport::None,
                is_tty,
            };
        }

        if !is_tty {
            return Self {
                color_support: ColorSupport::None,
                is_tty,
            };
        }

        let color_support = if let Ok(colorterm) = env::var("COLORTERM") {
            if colorterm == "truecolor" || colorterm == "24bit" {
                ColorSupport::TrueColor
            } else {
                check_term_for_256()
            }
        } else {
            check_term_for_256()
        };

        Self {
            color_support,
            is_tty,
        }
    }

    pub fn adjust_color(&self, color: Color) -> Color {
        if self.color_support == ColorSupport::None {
            return Color::Reset;
        }

        match self.color_support {
            ColorSupport::None => Color::Reset,
            ColorSupport::Basic => match color {
                Color::Rgb { .. } => Color::White,
                _ => color,
            },
            ColorSupport::Ansi256 => color,
            ColorSupport::TrueColor => color,
        }
    }
}

fn check_term_for_256() -> ColorSupport {
    if env::var("TERM").is_ok_and(|term| term.contains("256color")) {
        return ColorSupport::Ansi256;
    }
    ColorSupport::Basic
}
