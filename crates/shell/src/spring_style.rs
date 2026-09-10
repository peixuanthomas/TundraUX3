use ratatui::layout::{Margin, Rect};
use tachyonfx::{CellFilter, Effect, Interpolation, Motion, fx};

/// The selected version-three reveal, shared by production and the debug gallery.
pub(crate) fn spring_reveal(area: Rect, theme: ui::ThemeTokens, millis: u32) -> Effect {
    fx::parallel(&[
        fx::sweep_in(
            Motion::UpToDown,
            4,
            0,
            theme.surface,
            (millis, Interpolation::QuadOut),
        ),
        fx::fade_from_fg(theme.accent_soft, (millis, Interpolation::QuadOut)),
    ])
    .with_area(area)
    .with_filter(CellFilter::AnyOf(vec![
        CellFilter::Text,
        CellFilter::Outer(Margin::new(1, 1)),
    ]))
}
