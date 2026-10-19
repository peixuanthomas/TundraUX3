use std::{
    io::{self, IsTerminal, Write},
    sync::Arc,
};

use crossterm::event;
use ui::{
    InputEvent, MouseButton, MouseEventKind, RenderContext, TundraTheme,
    components::{ButtonFrame, ButtonRegion, contains_point},
    ui_test::UiTestGallery,
};

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

struct PreviewPointer {
    hovered: Option<ButtonRegion>,
    pressed: Option<ButtonRegion>,
    position: Option<(u16, u16)>,
    keyboard: bool,
}

impl Default for PreviewPointer {
    fn default() -> Self {
        Self {
            hovered: None,
            pressed: None,
            position: None,
            keyboard: true,
        }
    }
}

impl PreviewPointer {
    fn prepare(&mut self, input: InputEvent, regions: &[ButtonRegion]) -> Option<InputEvent> {
        match input {
            InputEvent::Key(ref key) if key.is_press_like() => {
                self.hovered = None;
                self.pressed = None;
                self.keyboard = true;
            }
            InputEvent::FocusLost | InputEvent::Resize { .. } => {
                self.hovered = None;
                self.pressed = None;
                self.keyboard = false;
            }
            InputEvent::Mouse(mut mouse) => {
                let position = (mouse.column(), mouse.row());
                if mouse.kind == MouseEventKind::Move && self.position == Some(position) {
                    return None;
                }
                self.position = Some(position);
                self.keyboard = false;
                let target = regions
                    .iter()
                    .rev()
                    .find(|region| {
                        !region.disabled && contains_point(region.area, position.0, position.1)
                    })
                    .cloned();
                match mouse.kind {
                    MouseEventKind::Move => self.hovered = target,
                    MouseEventKind::Down(MouseButton::Left) => {
                        self.hovered = target.clone();
                        self.pressed = target;
                        if self.pressed.is_some() {
                            return None;
                        }
                    }
                    MouseEventKind::Drag(MouseButton::Left) => {
                        if self.pressed != target {
                            self.pressed = None;
                        }
                        self.hovered = target;
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        self.hovered = None;
                        let pressed = self.pressed.take();
                        if pressed.is_some() && pressed == target {
                            mouse.kind = MouseEventKind::Click(MouseButton::Left);
                            return Some(InputEvent::Mouse(mouse));
                        }
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        self.pressed = None;
                        self.hovered = None;
                    }
                    _ => {}
                }
                return Some(InputEvent::Mouse(mouse));
            }
            _ => {}
        }
        Some(input)
    }
}
