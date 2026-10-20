use super::pointer::PreviewPointer;
use crate::{TerminalGuard, crossterm_event_to_input};
use crossterm::event;
use std::{
    io::{self, IsTerminal, Write},
    sync::Arc,
    time::{Duration, Instant},
};
use ui::{
    InputEvent, Key, MouseButton, MouseEventKind, RenderContext, TundraTheme,
    components::{ButtonFrame, contains_point},
};

pub fn run_rainbow_text_preview(output: &mut impl Write) -> io::Result<()> {
    let _language = i18n::enter_snapshot(Arc::new(i18n::LanguageSnapshot::embedded(1)));
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(i18n::tr!(
            "rainbow-text-terminal-required"
        )));
    }
    let theme = TundraTheme::default_dark();
    let mut terminal = TerminalGuard::enter(output)?;
    let mut pointer = PreviewPointer::default();
    let origin = Instant::now();
    loop {
        let mut buttons =
            ButtonFrame::new(pointer.hovered.clone(), pointer.pressed.clone(), &theme);
        buttons.keyboard_focus_visible = pointer.keyboard;
        let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        context.buttons = Some(buttons.clone());
        let mut exit = Default::default();
        terminal.terminal_mut().draw(|frame| {
            exit = ui::rainbow_text_preview::render(frame, origin.elapsed(), &context);
        })?;
        if !event::poll(Duration::from_millis(33))? {
            continue;
        }
        let Some(input) =
            pointer.prepare(crossterm_event_to_input(event::read()?), &buttons.regions())
        else {
            continue;
        };
        match input {
            InputEvent::Key(key)
                if key.is_press_like()
                    && (key.key == Key::Escape
                        || key.key == Key::Enter
                        || key.key == Key::Space
                        || (key.key == Key::Char('c') && key.modifiers.control)) =>
            {
                break;
            }
            InputEvent::Mouse(mouse)
                if mouse.kind == MouseEventKind::Click(MouseButton::Left)
                    && contains_point(exit, mouse.column(), mouse.row()) =>
            {
                break;
            }
            _ => {}
        }
    }
    terminal.restore()
}
