use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BorderShape {
    #[default]
    Rounded,
    Square,
}

impl BorderShape {
    pub const fn border_type(self) -> BorderType {
        match self {
            Self::Rounded => BorderType::Rounded,
            Self::Square => BorderType::Plain,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TundraTheme {
    pub background: Color,
    pub foreground: Color,
    pub accent: Color,
    pub muted: Color,
    pub error: Color,
    pub border_shape: BorderShape,
}

impl TundraTheme {
    pub fn default_dark() -> Self {
        Self {
            background: Color::Black,
            foreground: Color::Gray,
            accent: Color::Cyan,
            muted: Color::DarkGray,
            error: Color::Red,
            border_shape: BorderShape::Rounded,
        }
    }

    pub fn with_border_shape(mut self, border_shape: BorderShape) -> Self {
        self.border_shape = border_shape;
        self
    }

    pub const fn border_type(&self) -> BorderType {
        self.border_shape.border_type()
    }

    pub fn block(&self) -> Block<'static> {
        Block::default().border_type(self.border_type())
    }

    pub fn title_style(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .bg(self.background)
            .add_modifier(Modifier::BOLD)
    }

    pub fn body_style(&self) -> Style {
        Style::default().fg(self.foreground).bg(self.background)
    }

    pub fn muted_style(&self) -> Style {
        Style::default().fg(self.muted).bg(self.background)
    }

    pub fn error_style(&self) -> Style {
        Style::default().fg(self.error).bg(self.background)
    }
}

/// Keep box-drawing glyphs at regular weight. Some terminal fonts render bold
/// vertical borders with gaps between rows, which makes a solid border look dashed.
pub(crate) fn solid_border_style(style: Style) -> Style {
    style.remove_modifier(Modifier::BOLD)
}

impl Default for TundraTheme {
    fn default() -> Self {
        Self::default_dark()
    }
}
