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
    /// Color used for selected items, focus affordances, and other emphasis.
    pub accent_color: Color,
    pub muted: Color,
    pub error: Color,
    pub border_color: Color,
    pub border_shape: BorderShape,
}

impl TundraTheme {
    pub fn default_dark() -> Self {
        Self {
            background: Color::Black,
            foreground: Color::Gray,
            accent_color: Color::Cyan,
            muted: Color::DarkGray,
            error: Color::Red,
            border_color: Color::White,
            border_shape: BorderShape::Rounded,
        }
    }

    pub fn with_border_shape(mut self, border_shape: BorderShape) -> Self {
        self.border_shape = border_shape;
        self
    }

    pub fn with_border_color(mut self, border_color: Color) -> Self {
        self.border_color = border_color;
        self
    }

    pub fn with_accent_color(mut self, accent_color: Color) -> Self {
        self.accent_color = accent_color;
        self
    }

    pub const fn border_type(&self) -> BorderType {
        self.border_shape.border_type()
    }

    pub fn block(&self) -> Block<'static> {
        Block::default()
            .border_type(self.border_type())
            .border_style(self.border_style())
    }

    pub fn border_style(&self) -> Style {
        solid_border_style(Style::default().fg(self.border_color).bg(self.background))
    }

    /// Border style for a selectable control. A selected control is outlined
    /// with the accent color; every other state keeps the configured border.
    pub fn selectable_border_style(&self, selected: bool) -> Style {
        let color = if selected {
            self.accent_color
        } else {
            self.border_color
        };
        solid_border_style(Style::default().fg(color).bg(self.background))
    }

    pub fn title_style(&self) -> Style {
        Style::default()
            .fg(self.accent_color)
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
