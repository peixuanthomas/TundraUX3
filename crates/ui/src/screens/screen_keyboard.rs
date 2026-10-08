//! Standalone English keyboard demo. The host owns input and temporary text.
use std::collections::VecDeque;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    RenderContext,
    components::{Button, ButtonRegion, Surface},
};

pub const SCREEN_KEYBOARD_LETTERS: &str = "qwertyuiopasdfghjklzxcvbnm";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScreenKeyboardModifiers {
    pub shift: bool,
    pub caps_lock: bool,
    pub left_ctrl: bool,
    pub right_ctrl: bool,
    pub alt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenKeyboardAction {
    Letter(char),
    Character(char),
    Function(u8),
    Escape,
    Tab,
    CapsLock,
    Backspace,
    Enter,
    Shift,
    Space,
    LeftCtrl,
    RightCtrl,
    Alt,
    TestAutoAdmin,
    ToggleKeyboard,
    Copy,
    Paste,
    Clear,
    Exit,
}

impl ScreenKeyboardAction {
    pub fn button_id(self) -> String {
        match self {
            Self::Letter(letter) => format!("screen-keyboard.letter.{letter}"),
            Self::Character(character) => {
                format!("screen-keyboard.character.{:02x}", character as u32)
            }
            Self::Function(number) => format!("screen-keyboard.function.{number}"),
            action => format!(
                "screen-keyboard.{}",
                match action {
                    Self::Escape => "escape",
                    Self::Tab => "tab",
                    Self::CapsLock => "caps-lock",
                    Self::Backspace => "backspace",
                    Self::Enter => "enter",
                    Self::Shift => "shift",
                    Self::Space => "space",
                    Self::LeftCtrl => "left-ctrl",
                    Self::RightCtrl => "right-ctrl",
                    Self::Alt => "alt",
                    Self::TestAutoAdmin => "test-aa",
                    Self::ToggleKeyboard => "toggle",
                    Self::Copy => "copy",
                    Self::Paste => "paste",
                    Self::Clear => "clear",
                    Self::Exit => "exit",
                    _ => unreachable!(),
                }
            ),
        }
    }

    /// Maps a keycap to text. Ctrl and Alt combinations are handled by the host.
    pub fn character(self, modifiers: ScreenKeyboardModifiers) -> Option<char> {
        match self {
            Self::Letter(letter) => Some(if modifiers.shift ^ modifiers.caps_lock {
                letter.to_ascii_uppercase()
            } else {
                letter.to_ascii_lowercase()
            }),
            Self::Character(character) => Some(if modifiers.shift {
                match character {
                    '\u{0060}' => '~',
                    '1' => '!',
                    '2' => '@',
                    '3' => '#',
                    '4' => '$',
                    '5' => '%',
                    '6' => '^',
                    '7' => '&',
                    '8' => '*',
                    '9' => '(',
                    '0' => ')',
                    '-' => '_',
                    '=' => '+',
                    '[' => '{',
                    ']' => '}',
                    '\\' => '|',
                    ';' => ':',
                    '\'' => '"',
                    ',' => '<',
                    '.' => '>',
                    '/' => '?',
                    other => other,
                }
            } else {
                character
            }),
            Self::Space => Some(' '),
            Self::Enter => Some('\n'),
            Self::Tab => Some('\t'),
            _ => None,
        }
    }

    fn label(self, modifiers: ScreenKeyboardModifiers, collapsed: bool) -> String {
        match self {
            Self::Letter(_) | Self::Character(_) => {
                return self.character(modifiers).unwrap().to_string();
            }
            Self::Function(number) => return format!("F{number}"),
            Self::Escape => "Esc".to_owned(),
            Self::Tab => "Tab".to_owned(),
            Self::CapsLock => "CapsLock".to_owned(),
            Self::Backspace => i18n::tr!("screen-keyboard-backspace"),
            Self::Enter => "Enter".to_owned(),
            Self::Shift => "Shift".to_owned(),
            Self::Space => i18n::tr!("screen-keyboard-space"),
            Self::LeftCtrl => "Ctrl".to_owned(),
            Self::RightCtrl => "RCtrl".to_owned(),
            Self::Alt => "Alt".to_owned(),
            Self::TestAutoAdmin => i18n::tr!("screen-keyboard-test-aa"),
            Self::ToggleKeyboard => i18n::tr!(if collapsed {
                "screen-keyboard-show"
            } else {
                "screen-keyboard-hide"
            }),
            Self::Copy => i18n::tr!("screen-keyboard-copy"),
            Self::Paste => i18n::tr!("screen-keyboard-paste"),
            Self::Clear => i18n::tr!("screen-keyboard-clear"),
            Self::Exit => i18n::tr!("screen-keyboard-exit"),
        }
    }

    pub fn is_latched(self, modifiers: ScreenKeyboardModifiers) -> bool {
        match self {
            Self::Shift => modifiers.shift,
            Self::CapsLock => modifiers.caps_lock,
            Self::LeftCtrl => modifiers.left_ctrl,
            Self::RightCtrl => modifiers.right_ctrl,
            Self::Alt => modifiers.alt,
            _ => false,
        }
    }

    fn is_toolbar(self) -> bool {
        matches!(
            self,
            Self::ToggleKeyboard
                | Self::Copy
                | Self::Paste
                | Self::Clear
                | Self::Exit
                | Self::TestAutoAdmin
        )
    }

    fn minimum_width(self, bordered: bool) -> u16 {
        let latched = ScreenKeyboardModifiers {
            shift: true,
            caps_lock: true,
            left_ctrl: true,
            right_ctrl: true,
            alt: true,
        };
        let visible = self.label(latched, false);
        let collapsed = self.label(latched, true);
        UnicodeWidthStr::width(visible.as_str()).max(UnicodeWidthStr::width(collapsed.as_str()))
            as u16
            + if bordered { 2 } else { 0 }
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
    pub status: Rect,
    pub hint: Rect,
    pub keyboard: Rect,
    pub buttons: Vec<ScreenKeyboardButtonLayout>,
    pub bordered_buttons: bool,
    pub usable: bool,
    pub collapsed: bool,
    /// Nonempty only in the AA test. It is always above the keyboard panel.
    pub aa_dialog: Rect,
}

pub struct ScreenKeyboardViewModel<'a> {
    pub text: &'a str,
    pub focus: ScreenKeyboardAction,
    pub modifiers: ScreenKeyboardModifiers,
    /// Physical feedback is separate from modifiers latched by clicking.
    pub physical_pressed: &'a [ScreenKeyboardAction],
    pub last_key: &'a str,
    pub message: &'a str,
}

fn append_row(
    buttons: &mut Vec<ScreenKeyboardButtonLayout>,
    area: Rect,
    keys: &[(ScreenKeyboardAction, u16)],
    bordered: bool,
) {
    if area.is_empty() || keys.is_empty() {
        return;
    }
    let gap = u16::from(area.width >= keys.len() as u16 * 2 - 1);
    let available = area.width.saturating_sub(gap * (keys.len() as u16 - 1));
    let minimum: Vec<u16> = keys
        .iter()
        .map(|(action, _)| action.minimum_width(bordered))
        .collect();
    let required: u16 = minimum.iter().sum();
    let mut widths = vec![None; keys.len()];
    if required <= available {
        let mut remaining = u32::from(available);
        let mut remaining_weight: u32 = keys.iter().map(|(_, weight)| u32::from(*weight)).sum();
        loop {
            // Reserve only keys whose proportional share would clip the label.
            // The others keep their physical width relative to each other.
            let constrained: Vec<_> = keys
                .iter()
                .enumerate()
                .filter_map(|(index, (_, weight))| {
                    (widths[index].is_none()
                        && remaining * u32::from(*weight)
                            < u32::from(minimum[index]) * remaining_weight)
                        .then_some(index)
                })
                .collect();
            if constrained.is_empty() {
                break;
            }
            for index in constrained {
                widths[index] = Some(minimum[index]);
                remaining -= u32::from(minimum[index]);
                remaining_weight -= u32::from(keys[index].1);
            }
        }
        let mut accumulated_weight = 0_u32;
        let mut allocated = 0_u32;
        for (index, (_, weight)) in keys.iter().enumerate() {
            if widths[index].is_none() {
                accumulated_weight += u32::from(*weight);
                let next = remaining * accumulated_weight / remaining_weight;
                widths[index] = Some((next - allocated) as u16);
                allocated = next;
            }
        }
    } else {
        // Tiny toolbars keep their actions reachable even when labels clip.
        for (index, width) in widths.iter_mut().enumerate() {
            *width = Some(
                ((index + 1) as u32 * u32::from(available) / keys.len() as u32) as u16
                    - (index as u32 * u32::from(available) / keys.len() as u32) as u16,
            );
        }
    }
    let mut x = area.x;
    for (index, (action, _)) in keys.iter().enumerate() {
        let width = widths[index].unwrap();
        if width > 0 {
            buttons.push(ScreenKeyboardButtonLayout {
                action: *action,
                area: Rect::new(x, area.y, width, area.height),
            });
        }
        x = x.saturating_add(width + gap);
    }
}

fn toolbar(width: u16) -> Vec<(ScreenKeyboardAction, u16)> {
    use ScreenKeyboardAction::*;
    match width {
        0 => vec![],
        1 => vec![(Exit, 1)],
        2 => vec![(ToggleKeyboard, 1), (Exit, 1)],
        3 => vec![(ToggleKeyboard, 1), (Copy, 1), (Exit, 1)],
        4 => vec![(ToggleKeyboard, 1), (Copy, 1), (Paste, 1), (Exit, 1)],
        5 => vec![
            (ToggleKeyboard, 1),
            (Copy, 1),
            (Paste, 1),
            (Clear, 1),
            (Exit, 1),
        ],
        _ => vec![
            (ToggleKeyboard, 3),
            (Copy, 2),
            (Paste, 2),
            (Clear, 2),
            (TestAutoAdmin, 2),
            (Exit, 2),
        ],
    }
}

/// Rendering and input both consume these exact button rectangles.
pub fn screen_keyboard_layout(bounds: Rect, collapsed: bool) -> ScreenKeyboardLayout {
    screen_keyboard_layout_with_visibility(bounds, collapsed, if collapsed { 0 } else { 1_000 })
}

/// Slides key rows while keeping the toolbar stationary and sharing hit regions.
/// Visibility is in thousandths; the host supplies the animated value.
pub fn screen_keyboard_layout_with_visibility(
    bounds: Rect,
    collapsed: bool,
    visibility: u16,
) -> ScreenKeyboardLayout {
    keyboard_layout(
        bounds,
        collapsed,
        visibility,
        bounds.height / 2,
        bounds.width >= 60 && bounds.height >= 14,
    )
}

fn keyboard_layout(
    bounds: Rect,
    collapsed: bool,
    visibility: u16,
    upper_height: u16,
    usable: bool,
) -> ScreenKeyboardLayout {
    use ScreenKeyboardAction::*;
    let expanded_y = bounds.y + upper_height;
    let toolbar_y = bounds.bottom().saturating_sub(u16::from(bounds.height > 0));
    let slide = (u32::from(toolbar_y.saturating_sub(expanded_y))
        * u32::from(1_000 - visibility.min(1_000))
        / 1_000) as u16;
    let keyboard_y = expanded_y + slide;
    let keyboard = Rect::new(
        bounds.x,
        keyboard_y,
        bounds.width,
        bounds.bottom().saturating_sub(keyboard_y),
    );
    let upper_height = keyboard_y.saturating_sub(bounds.y);
    let keys_height = toolbar_y.saturating_sub(expanded_y);
    let button_height = (keys_height / 6).clamp(1, 5);
    let bordered_buttons = bounds.width >= 104 && button_height >= 3;
    let mut row_heights = [button_height; 6];
    let preferred_character_height = if bordered_buttons { 4 } else { 2 };
    // Give character rows more height when there is room for comfortable keys.
    if button_height < preferred_character_height
        && keys_height >= button_height * 2 + preferred_character_height * 4
    {
        row_heights[1..5].fill(preferred_character_height);
    }
    let mut layout = ScreenKeyboardLayout {
        title: Rect::new(
            bounds.x,
            bounds.y,
            bounds.width,
            u16::from(upper_height > 0),
        ),
        text: Rect::new(
            bounds.x,
            bounds.y + u16::from(upper_height > 0),
            bounds.width,
            upper_height.saturating_sub(3),
        ),
        status: Rect::new(
            bounds.x,
            keyboard_y.saturating_sub(2).max(bounds.y),
            bounds.width,
            u16::from(upper_height >= 3),
        ),
        hint: Rect::new(
            bounds.x,
            keyboard_y.saturating_sub(1).max(bounds.y),
            bounds.width,
            u16::from(upper_height >= 2),
        ),
        keyboard,
        buttons: Vec::with_capacity(74),
        bordered_buttons,
        usable,
        collapsed,
        aa_dialog: Rect::default(),
    };
    if keyboard.is_empty() {
        return layout;
    }
    if visibility == 0 || !usable {
        append_row(
            &mut layout.buttons,
            Rect::new(bounds.x, bounds.bottom() - 1, bounds.width, 1),
            &toolbar(bounds.width),
            false,
        );
        return layout;
    }

    let mut function_row = vec![(Escape, 2)];
    function_row.extend((1..=12).map(|number| (Function(number), 1)));
    let mut number_row: Vec<_> = "\u{0060}1234567890-="
        .chars()
        .map(|character| (Character(character), 1))
        .collect();
    number_row.push((Backspace, 3));
    let mut qwerty_row = vec![(Tab, 2)];
    qwerty_row.extend("qwertyuiop".chars().map(|letter| (Letter(letter), 1)));
    qwerty_row.extend("[]\\".chars().map(|character| (Character(character), 1)));
    let mut home_row = vec![(CapsLock, 2)];
    home_row.extend("asdfghjkl".chars().map(|letter| (Letter(letter), 1)));
    home_row.extend(";'".chars().map(|character| (Character(character), 1)));
    home_row.push((Enter, 2));
    let mut shift_row = vec![(Shift, 2)];
    shift_row.extend("zxcvbnm".chars().map(|letter| (Letter(letter), 1)));
    shift_row.extend(",./".chars().map(|character| (Character(character), 1)));
    let rows = [
        function_row,
        number_row,
        qwerty_row,
        home_row,
        shift_row,
        vec![(LeftCtrl, 2), (Alt, 2), (Space, 8), (RightCtrl, 2)],
    ];
    let visible_keys = Rect::new(bounds.x, keyboard_y, bounds.width, toolbar_y - keyboard_y);
    let spare = keys_height.saturating_sub(row_heights.iter().sum());
    let mut preceding_height = 0_u32;
    for (index, keys) in rows.iter().enumerate() {
        let y = u32::from(keyboard.y) + preceding_height + index as u32 * u32::from(spare) / 5;
        let height = row_heights[index];
        preceding_height += u32::from(height);
        if y >= u32::from(toolbar_y) {
            continue;
        }
        append_row(
            &mut layout.buttons,
            Rect::new(bounds.x, y as u16, bounds.width, height).intersection(visible_keys),
            keys,
            bordered_buttons,
        );
    }
    append_row(
        &mut layout.buttons,
        Rect::new(bounds.x, toolbar_y, bounds.width, 1),
        &toolbar(bounds.width),
        false,
    );
    layout
}

/// The final compositor reserves the dialog first and gives the remaining bottom
/// rows to the keyboard. A terminal too small for both keeps the dialog usable.
pub fn screen_keyboard_aa_layout(
    bounds: Rect,
    collapsed: bool,
    visibility: u16,
    context: &RenderContext,
) -> ScreenKeyboardLayout {
    let shell = crate::ShellFrameLayout::new(bounds, None, context);
    let top = if shell.is_compact() {
        bounds.height.min(1)
    } else {
        3
    };
    let height = (bounds.height / 2).min(bounds.height.saturating_sub(top + 8));
    let usable = bounds.width >= 60 && height >= 7;
    let mut layout = keyboard_layout(
        bounds,
        collapsed,
        visibility,
        bounds.height.saturating_sub(height),
        usable,
    );
    layout.title = Rect::default();
    if !usable || visibility == 0 {
        layout.keyboard = Rect::new(bounds.x, bounds.bottom(), bounds.width, 0);
        layout.buttons.clear();
    } else {
        layout.buttons.retain(|button| !button.action.is_toolbar());
        append_row(
            &mut layout.buttons,
            Rect::new(bounds.x, bounds.bottom() - 1, bounds.width, 1),
            &[
                (ScreenKeyboardAction::Copy, 1),
                (ScreenKeyboardAction::Paste, 1),
                (ScreenKeyboardAction::Clear, 1),
            ],
            false,
        );
    }
    // Reserve chrome only when no keyboard covers it. While sliding, use the
    // actual visible panel edge for both modal placement and hit testing.
    let available = if layout.keyboard.is_empty() {
        shell.main
    } else {
        Rect::new(
            bounds.x,
            bounds.y + top,
            bounds.width,
            layout.keyboard.y.saturating_sub(bounds.y + top),
        )
    };
    let width = available
        .width
        .saturating_sub(if available.width >= 64 { 4 } else { 0 })
        .min(76);
    let height = available.height.min(14);
    layout.aa_dialog = Rect::new(
        available.x + available.width.saturating_sub(width) / 2,
        available.y + available.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let inner = layout.aa_dialog.inner(ratatui::layout::Margin::new(1, 1));
    let rows = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(u16::from(inner.height >= 6)),
        ratatui::layout::Constraint::Min(0),
        ratatui::layout::Constraint::Length(u16::from(inner.height >= 4)),
        ratatui::layout::Constraint::Length(u16::from(inner.height >= 3)),
        ratatui::layout::Constraint::Length(u16::from(inner.height > 0)),
    ])
    .split(inner);
    layout.title = rows[0];
    layout.text = rows[1];
    layout.status = rows[2];
    layout.hint = rows[3];
    // The same action IDs work in the popup and in the keyboard toolbar, with
    // distinct rectangles to prevent a moving popup from completing a click.
    append_row(
        &mut layout.buttons,
        rows[4],
        &[
            (ScreenKeyboardAction::ToggleKeyboard, 3),
            (ScreenKeyboardAction::TestAutoAdmin, 2),
            (ScreenKeyboardAction::Exit, 1),
        ],
        false,
    );
    layout
}

/// Wrap complete graphemes and retain the newest screenful, including empty lines.
fn newest_text_lines(text: &str, width: u16, height: u16) -> Vec<Line<'static>> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let mut lines = VecDeque::with_capacity(height as usize);
    let mut push_line = |line: String| {
        if lines.len() == height as usize {
            lines.pop_front();
        }
        lines.push_back(Line::raw(line));
    };
    for source_line in text.split('\n') {
        let mut current = String::new();
        let mut columns = 0_usize;
        // Ratatui omits control characters from styled_graphemes, so expand tabs
        // before asking it for the printable graphemes in each segment.
        for (index, segment) in source_line.trim_end_matches('\r').split('\t').enumerate() {
            if index > 0 {
                let spaces = 4 - columns % 4;
                for _ in 0..spaces {
                    if columns == width as usize {
                        push_line(std::mem::take(&mut current));
                        columns = 0;
                    }
                    current.push(' ');
                    columns += 1;
                }
            }
            let span = Span::raw(segment);
            for grapheme in span.styled_graphemes(Style::default()) {
                let glyph_width = UnicodeWidthStr::width(grapheme.symbol);
                if glyph_width > width as usize {
                    continue;
                }
                if columns + glyph_width > width as usize {
                    push_line(std::mem::take(&mut current));
                    columns = 0;
                }
                current.push_str(grapheme.symbol);
                columns += glyph_width;
            }
        }
        push_line(current);
    }
    lines.into_iter().collect()
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
    if !layout.text.is_empty() {
        let surface = Surface::new()
            .bordered(layout.text.width >= 3 && layout.text.height >= 3)
            .titled(i18n::tr!("screen-keyboard-text"));
        surface.render_frame(frame, layout.text, context);
        let inner = surface.inner(layout.text);
        let placeholder;
        let text = if model.text.is_empty() {
            placeholder = i18n::tr!("screen-keyboard-placeholder");
            &placeholder
        } else {
            model.text
        };
        frame.render_widget(
            Paragraph::new(newest_text_lines(text, inner.width, inner.height))
                .style(body.bg(context.theme.surface)),
            inner,
        );
    }
    let status = if model.last_key.is_empty() {
        model.message.to_owned()
    } else {
        format!(
            "{}: {}  {}",
            i18n::tr!("screen-keyboard-last-key"),
            model.last_key,
            model.message
        )
    };
    frame.render_widget(Paragraph::new(status).style(body), layout.status);
    frame.render_widget(
        Paragraph::new(i18n::tr!(if layout.usable {
            "screen-keyboard-hint"
        } else {
            "screen-keyboard-small"
        }))
        .style(body.fg(context.theme.muted)),
        layout.hint,
    );
    render_screen_keyboard_keys(frame, layout, model, context);
}

/// Only the bottom panel; the compositor calls this after page, chrome and AA.
pub fn render_screen_keyboard_panel(
    frame: &mut Frame<'_>,
    layout: &ScreenKeyboardLayout,
    model: &ScreenKeyboardViewModel<'_>,
    context: &RenderContext,
) {
    frame.render_widget(ratatui::widgets::Clear, layout.keyboard);
    frame.render_widget(
        Block::default().style(context.compatibility_theme().body_style()),
        layout.keyboard,
    );
    render_screen_keyboard_keys(frame, layout, model, context);
}

fn render_screen_keyboard_keys(
    frame: &mut Frame<'_>,
    layout: &ScreenKeyboardLayout,
    model: &ScreenKeyboardViewModel<'_>,
    context: &RenderContext,
) {
    let theme = context.compatibility_theme();
    for target in &layout.buttons {
        if !layout.aa_dialog.is_empty()
            && !layout
                .keyboard
                .contains((target.area.x, target.area.y).into())
        {
            continue;
        }
        let mut button = Button::new(
            target.action.button_id(),
            target.action.label(model.modifiers, layout.collapsed),
        )
        .with_bracketed_label(false)
        .with_centered_label(true)
        .with_latched(
            target.action.is_latched(model.modifiers)
                || model.physical_pressed.contains(&target.action),
        );
        button.set_focused(target.action == model.focus);
        if layout.bordered_buttons && !target.action.is_toolbar() {
            button.render_frame(frame, target.area, &theme);
        } else {
            button.render_borderless_frame(frame, target.area, &theme);
        }
    }
}

/// A harmless AA input exercise using the same AA frame as real requests.
pub fn render_screen_keyboard_aa_test(
    frame: &mut Frame<'_>,
    layout: &ScreenKeyboardLayout,
    model: &ScreenKeyboardViewModel<'_>,
    context: &RenderContext,
) {
    let aa = crate::AutoAdminLayout {
        dialog: layout.aa_dialog,
        description: layout.title,
        terminal: Rect::default(),
        status: layout.status,
        input: layout.text,
        buttons: [Rect::default(); 4],
    };
    super::auto_admin_preview::render_auto_admin_frame_in_area(
        frame,
        layout.aa_dialog,
        &aa,
        Rect::default(),
        crate::AutoAdminPreviewStyle::Authorization,
        Some(" AutoAdmin (AA) "),
    );
    let theme = context.compatibility_theme();
    frame.render_widget(
        Paragraph::new(i18n::tr!("screen-keyboard-aa-safe")).style(theme.body_style()),
        layout.title,
    );
    let surface = Surface::new()
        .bordered(layout.text.height >= 3)
        .titled(i18n::tr!("screen-keyboard-text"));
    surface.render_frame(frame, layout.text, context);
    let input = surface.inner(layout.text);
    frame.render_widget(
        Paragraph::new(newest_text_lines(model.text, input.width, input.height))
            .style(theme.body_style()),
        input,
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{}: {}  {}",
            i18n::tr!("screen-keyboard-last-key"),
            model.last_key,
            model.message
        ))
        .style(theme.body_style()),
        layout.status,
    );
    frame.render_widget(
        Paragraph::new(i18n::tr!(if layout.usable {
            "screen-keyboard-aa-hint"
        } else {
            "screen-keyboard-aa-small"
        }))
        .style(theme.muted_style()),
        layout.hint,
    );
    for target in &layout.buttons {
        if layout
            .aa_dialog
            .contains((target.area.x, target.area.y).into())
        {
            let label = if target.action == ScreenKeyboardAction::TestAutoAdmin {
                i18n::tr!("screen-keyboard-aa-back")
            } else {
                target.action.label(model.modifiers, layout.collapsed)
            };
            let mut button = Button::new(target.action.button_id(), label);
            button.set_focused(target.action == model.focus);
            button.render_borderless_frame(frame, target.area, &theme);
        }
    }
}
