use crate::{
    RenderContext,
    components::{Button, RainbowText, Surface},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::Text,
    widgets::{Paragraph, Wrap},
};
use std::time::Duration;

pub fn render(frame: &mut Frame<'_>, elapsed: Duration, context: &RenderContext) -> Rect {
    let area = frame.area();
    let theme = context.compatibility_theme();
    Surface::new().render_frame(frame, area, context);
    let [title, _, sample, description, _, exit] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(3),
    ])
    .margin(1)
    .areas(area);
    frame.render_widget(
        Paragraph::new(i18n::tr!("rainbow-text-title"))
            .centered()
            .style(theme.title_style()),
        title,
    );
    frame.render_widget(
        RainbowText::new(
            Text::from(i18n::tr!("rainbow-text-sample")).centered(),
            elapsed,
        ),
        sample,
    );
    frame.render_widget(
        Paragraph::new(i18n::tr!("rainbow-text-description"))
            .centered()
            .wrap(Wrap { trim: true })
            .style(theme.body_style()),
        description,
    );
    let [_, exit, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(20),
        Constraint::Fill(1),
    ])
    .areas(exit);
    let mut button = Button::new("rainbow-text.exit", i18n::tr!("rainbow-text-exit"));
    button.set_focused(true);
    button.render_frame(frame, exit, &theme);
    exit
}
