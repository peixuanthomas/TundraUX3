//! Standalone English keyboard demo. The host owns input and temporary text.
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Paragraph},
};

use crate::{
    RenderContext,
    components::{Button, ButtonRegion, Surface},
};

pub const SCREEN_KEYBOARD_LETTERS: &str = "qwertyuiopasdfghjklzxcvbnm";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenKeyboardAction {
    Letter(char),
    Backspace,
    Clear,
    Exit,
}

impl ScreenKeyboardAction {
    pub fn button_id(self) -> String {
        match self {
            Self::Letter(letter) => format!("screen-keyboard.letter.{letter}"),
            Self::Backspace => "screen-keyboard.backspace".to_owned(),
            Self::Clear => "screen-keyboard.clear".to_owned(),
            Self::Exit => "screen-keyboard.exit".to_owned(),
        }
    }

    fn label(self) -> String {
        match self {
            Self::Letter(letter) => letter.to_ascii_uppercase().to_string(),
            Self::Backspace => i18n::tr!("screen-keyboard-backspace"),
            Self::Clear => i18n::tr!("screen-keyboard-clear"),
            Self::Exit => i18n::tr!("screen-keyboard-exit"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenKeyboardButtonLayout {
    pub action: ScreenKeyboardAction,
    pub area: Rect,
}

impl ScreenKeyboardButtonLayout {
    pub fn region(&self) -> ButtonRegion {
        ButtonRegion {
            id: self.action.button_id().into(),
            area: self.area,
            disabled: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenKeyboardLayout {
    pub title: Rect,
    pub text: Rect,
    pub hint: Rect,
    pub buttons: Vec<ScreenKeyboardButtonLayout>,
    pub bordered_buttons: bool,
    pub usable: bool,
}

pub struct ScreenKeyboardViewModel<'a> {
    pub text: &'a str,
    pub focus: ScreenKeyboardAction,
}

/// Rendering and input both consume these exact button rectangles.
pub fn screen_keyboard_layout(bounds: Rect) -> ScreenKeyboardLayout {
    let usable = bounds.width >= 39 && bounds.height >= 10;
    if !usable {
        return ScreenKeyboardLayout {
            title: Rect::new(bounds.x, bounds.y, bounds.width, bounds.height.min(1)),
            text: Rect::default(),
            hint: Rect::new(
                bounds.x,
                bounds.y.saturating_add(1),
                bounds.width,
                bounds.height.saturating_sub(2),
            ),
            buttons: if bounds.width > 0 && bounds.height > 1 {
                vec![ScreenKeyboardButtonLayout {
                    action: ScreenKeyboardAction::Exit,
                    area: Rect::new(bounds.x, bounds.bottom() - 1, bounds.width, 1),
                }]
            } else {
                Vec::new()
            },
            bordered_buttons: false,
            usable,
        };
    }

    let bordered_buttons = bounds.width >= 59 && bounds.height >= 18;
    let button_height = if bordered_buttons { 3 } else { 1 };
    let width = bounds.width.min(79);
    let height = 6 + 4 * button_height;
    let x = bounds.x + (bounds.width - width) / 2;
    let y = bounds.y + (bounds.height - height) / 2;
    let key_width = ((width - 9) / 10).min(7);
    let step = key_width + 1;
    let mut buttons = Vec::with_capacity(29);
    for (row, letters) in ["qwertyuiop", "asdfghjkl", "zxcvbnm"].iter().enumerate() {
        let row_width = letters.len() as u16 * step - 1;
        let row_x = x + (width - row_width) / 2;
        for (column, letter) in letters.chars().enumerate() {
            buttons.push(ScreenKeyboardButtonLayout {
                action: ScreenKeyboardAction::Letter(letter),
                area: Rect::new(
                    row_x + column as u16 * step,
                    y + 5 + row as u16 * button_height,
                    key_width,
                    button_height,
                ),
            });
        }
    }
    let action_width = (width - 2) / 3;
    for (column, action) in [
        ScreenKeyboardAction::Backspace,
        ScreenKeyboardAction::Clear,
        ScreenKeyboardAction::Exit,
    ]
    .into_iter()
    .enumerate()
    {
        buttons.push(ScreenKeyboardButtonLayout {
            action,
            area: Rect::new(
                x + column as u16 * (action_width + 1),
                y + 5 + 3 * button_height,
                action_width,
                button_height,
            ),
        });
    }
    ScreenKeyboardLayout {
        title: Rect::new(x, y, width, 1),
        text: Rect::new(x, y + 1, width, 3),
        hint: Rect::new(x, y + height - 1, width, 1),
        buttons,
        bordered_buttons,
        usable,
    }
}

pub fn render_screen_keyboard(
    frame: &mut Frame<'_>,
    bounds: Rect,
    layout: &ScreenKeyboardLayout,
    model: &ScreenKeyboardViewModel<'_>,
    context: &RenderContext,
) {
    let body = Style::default()
        .fg(context.theme.text)
        .bg(context.theme.canvas);
    frame.render_widget(Block::default().style(body), bounds);
    frame.render_widget(
        Paragraph::new(i18n::tr!("screen-keyboard-title"))
            .style(body.fg(context.theme.accent).add_modifier(Modifier::BOLD)),
        layout.title,
    );
    if layout.usable {
        let surface = Surface::new()
            .bordered(true)
            .titled(i18n::tr!("screen-keyboard-text"));
        surface.render_frame(frame, layout.text, context);
        let inner = surface.inner(layout.text);
        // Keep the newest letters visible instead of letting long input clip.
        let start = model
            .text
            .char_indices()
            .rev()
            .nth(inner.width.saturating_sub(1) as usize)
            .map_or(0, |(index, _)| index);
        let visible = &model.text[start..];
        frame.render_widget(
            Paragraph::new(if model.text.is_empty() {
                i18n::tr!("screen-keyboard-placeholder")
            } else {
                visible.to_owned()
            })
            .style(body.bg(context.theme.surface)),
            inner,
        );
    }
    frame.render_widget(
        Paragraph::new(i18n::tr!(if layout.usable {
            "screen-keyboard-hint"
        } else {
            "screen-keyboard-small"
        }))
        .style(body.fg(context.theme.muted)),
        layout.hint,
    );
    let theme = context.compatibility_theme();
    for target in &layout.buttons {
        let mut button = Button::new(target.action.button_id(), target.action.label());
        button.set_focused(target.action == model.focus);
        if layout.bordered_buttons {
            button.render_frame(frame, target.area, &theme);
        } else {
            button.render_borderless_frame(frame, target.area, &theme);
        }
    }
}
