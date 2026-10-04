//! A command result marker composed as a styled span in terminal output.
use ratatui::style::Style;
use ratatui::text::Span;

use crate::TundraTheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    Pending,
    Succeeded,
    Failed,
}

impl CommandStatus {
    pub fn span(self, theme: &TundraTheme) -> Span<'static> {
        let (symbol, style) = match self {
            Self::Pending => ("○", theme.muted_style()),
            Self::Succeeded => (
                "●",
                Style::default().fg(theme.accent_color).bg(theme.background),
            ),
            Self::Failed => ("×", theme.error_style()),
        };
        Span::styled(symbol, style)
    }
}
