use std::{
    io::{self, IsTerminal, Write},
    sync::Arc,
};

use crossterm::event;
use ui::{RenderContext, TundraTheme, components::ButtonFrame, ui_test::UiTestGallery};

use super::pointer::PreviewPointer;
use crate::{TerminalGuard, crossterm_event_to_input};

pub fn run_ui_test(output: &mut impl Write) -> io::Result<()> {
    let _language = i18n::enter_snapshot(Arc::new(i18n::LanguageSnapshot::embedded(1)));
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(i18n::tr!("ui-test-terminal-required")));
    }
    let theme = TundraTheme::default_dark();
    let mut terminal = TerminalGuard::enter(output)?;
    let mut gallery = UiTestGallery::default();
    let mut pointer = PreviewPointer::default();
    loop {
        let mut layout = gallery.layout(Default::default());
        let mut buttons =
            ButtonFrame::new(pointer.hovered.clone(), pointer.pressed.clone(), &theme);
        buttons.keyboard_focus_visible = pointer.keyboard;
        let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        context.buttons = Some(buttons.clone());
        terminal.terminal_mut().draw(|frame| {
            layout = gallery.render(frame, &context);
        })?;
        let input = crossterm_event_to_input(event::read()?);
        let page = gallery.page;
        if let Some(input) = pointer.prepare(input, &buttons.regions()) {
            if gallery.handle_event(input, &layout) {
                break;
            }
        }
        if gallery.page != page {
            pointer.hovered = None;
            pointer.pressed = None;
        }
    }
    terminal.restore()
}
