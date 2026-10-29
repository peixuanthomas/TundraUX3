use super::*;
use crate::components::{ButtonEmphasis, TextInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherEntryAction {
    Name,
    Command,
    Icon,
    WaitForEnter,
    Remove,
}

impl LauncherEntryAction {
    pub const ALL: [Self; 5] = [
        Self::Name,
        Self::Command,
        Self::Icon,
        Self::WaitForEnter,
        Self::Remove,
    ];

    pub fn label(self) -> String {
        match self {
            Self::Name => i18n::tr!("ui-launcher-change-name"),
            Self::Command => i18n::tr!("ui-launcher-change-command"),
            Self::Icon => i18n::tr!("ui-launcher-change-icon"),
            Self::WaitForEnter => i18n::tr!("ui-launcher-keep-screen"),
            Self::Remove => i18n::tr!("ui-launcher-delete-icon"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherContextMenu {
    pub anchor: (u16, u16),
    pub selected: usize,
    pub command_enabled: bool,
    pub wait_for_enter: Option<bool>,
}

impl LauncherContextMenu {
    pub fn actions(&self) -> impl Iterator<Item = LauncherEntryAction> + '_ {
        LauncherEntryAction::ALL.into_iter().filter(|action| {
            *action != LauncherEntryAction::WaitForEnter || self.wait_for_enter.is_some()
        })
    }

    fn button(&self, index: usize, action: LauncherEntryAction) -> Button {
        let label = action.label();
        let mut button = Button::new(format!("launcher.menu.{index}"), label.clone());
        if action == LauncherEntryAction::WaitForEnter {
            let checked = self.wait_for_enter.unwrap_or(false);
            button = Button::new(
                format!("launcher.menu.{index}"),
                format!(
                    "{label} [{}]",
                    i18n::tr!(if checked {
                        "settings-on"
                    } else {
                        "settings-off"
                    })
                ),
            )
            .with_bracketed_label(false)
            .with_capture_name(label)
            .with_capture_role("checkbox")
            .with_capture_selected(checked);
        }
        button.set_focused(self.selected == index);
        button.set_disabled(action == LauncherEntryAction::Command && !self.command_enabled);
        if action == LauncherEntryAction::Remove {
            button = button.with_emphasis(ButtonEmphasis::Danger);
        }
        button
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherEntryEditor {
    pub id: String,
    pub action: LauncherEntryAction,
    pub inputs: Vec<TextInput>,
    pub selected: usize,
    pub error: Option<String>,
}

impl LauncherEntryEditor {
    pub fn form(&self) -> crate::ManagementForm {
        crate::ManagementForm {
            identity: format!("launcher-edit:{}:{:?}", self.id, self.action),
            title: self.action.label(),
            message: self.error.clone().unwrap_or_else(|| match self.action {
                LauncherEntryAction::Command => i18n::tr!("ui-launcher-command-help"),
                LauncherEntryAction::Icon => i18n::tr!("ui-launcher-icon-help"),
                _ => String::new(),
            }),
            fields: self
                .inputs
                .iter()
                .enumerate()
                .map(|(index, input)| crate::ManagementFormField {
                    id: format!("launcher.edit.{index}"),
                    label: self.action.label(),
                    value: input.value().to_owned(),
                    input: Some(input.clone()),
                    ..Default::default()
                })
                .collect(),
            selected: self.selected,
            submit_label: Some(i18n::tr!("shell-save")),
            cancel_label: Some(i18n::tr!("shell-cancel")),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherEntryEditorLayout {
    pub area: Rect,
    pub canvas: Option<Rect>,
    pub inputs: Vec<Rect>,
    pub submit: Rect,
    pub cancel: Rect,
}

pub fn launcher_context_menu_layout(main: Rect, menu: &LauncherContextMenu) -> (Rect, Vec<Rect>) {
    let width = menu
        .actions()
        .enumerate()
        .map(|(index, action)| menu.button(index, action).rendered_label_width())
        .max()
        .unwrap_or(0)
        .saturating_add(6)
        .min(usize::from(main.width)) as u16;
    let height = (menu.actions().count() as u16 + 4).min(main.height);
    let area = Rect::new(
        menu.anchor
            .0
            .min(main.right().saturating_sub(width))
            .max(main.x),
        menu.anchor
            .1
            .min(main.bottom().saturating_sub(height))
            .max(main.y),
        width,
        height,
    );
    let rows = (0..menu.actions().count() as u16)
        .map(|index| {
            Rect::new(
                area.x.saturating_add(3),
                area.y.saturating_add(2 + index),
                area.width.saturating_sub(6),
                u16::from(2 + index < area.height.saturating_sub(1)),
            )
        })
        .collect();
    (area, rows)
}

pub fn launcher_entry_editor_layout(
    main: Rect,
    editor: &LauncherEntryEditor,
) -> LauncherEntryEditorLayout {
    if editor.action != LauncherEntryAction::Icon {
        let form = editor.form();
        let layout = crate::management_layout(
            main,
            &crate::ManagementViewModel {
                form: Some(form.clone()),
                ..Default::default()
            },
        );
        return LauncherEntryEditorLayout {
            area: layout.form,
            canvas: None,
            inputs: form
                .fields
                .iter()
                .enumerate()
                .map(|(index, field)| crate::management_field_input_area(&layout, index, field))
                .collect(),
            submit: layout.submit,
            cancel: layout.cancel,
        };
    }
    let area = crate::screens::shell::centered_rect(main, 58.min(main.width), 11.min(main.height));
    let width = (app::launcher::LAUNCHER_ICON_WIDTH as u16 + 2).min(area.width);
    let canvas = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y.saturating_add(3),
        width,
        (app::launcher::LAUNCHER_ICON_HEIGHT as u16 + 2).min(area.height.saturating_sub(5)),
    );
    let inner = inset(canvas, 1);
    let inputs = (0..app::launcher::LAUNCHER_ICON_HEIGHT)
        .map(|row| {
            Rect::new(
                inner.x,
                inner.y.saturating_add(row as u16),
                inner.width,
                u16::from(row < usize::from(inner.height)),
            )
        })
        .collect();
    let button_width = ((area.width.saturating_sub(4)) / 2).min(20);
    let buttons_y = area.bottom().saturating_sub(2);
    LauncherEntryEditorLayout {
        area,
        canvas: Some(canvas),
        inputs,
        submit: Rect::new(area.x.saturating_add(1), buttons_y, button_width, 1),
        cancel: Rect::new(
            area.right().saturating_sub(button_width + 1),
            buttons_y,
            button_width,
            1,
        ),
    }
}

pub fn launcher_entry_overlay_area(main: Rect, model: &LauncherViewModel) -> Option<Rect> {
    model
        .entry_editor
        .as_ref()
        .map(|editor| launcher_entry_editor_layout(main, editor).area)
        .or_else(|| {
            model
                .context_menu
                .as_ref()
                .map(|menu| launcher_context_menu_layout(main, menu).0)
        })
}

pub(super) fn render_entry_overlay(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &LauncherViewModel,
    context: &RenderContext,
) -> bool {
    let theme = context.compatibility_theme();
    if let Some(editor) = &model.entry_editor {
        if editor.action != LauncherEntryAction::Icon {
            crate::render_management_overlay(
                frame,
                main,
                &crate::ManagementViewModel {
                    form: Some(editor.form()),
                    ..Default::default()
                },
                context,
            );
            return true;
        }
        let layout = launcher_entry_editor_layout(main, editor);
        frame.render_widget(Clear, layout.area);
        Surface::new()
            .bordered(true)
            .titled(editor.action.label())
            .render_frame(frame, layout.area, context);
        let message = editor.form().message;
        frame.render_widget(
            Paragraph::new(message)
                .style(theme.body_style())
                .wrap(Wrap { trim: false }),
            Rect::new(
                layout.area.x + 1,
                layout.area.y + 1,
                layout.area.width.saturating_sub(2),
                2.min(layout.area.height.saturating_sub(2)),
            ),
        );
        Surface::new()
            .bordered(true)
            .focused(editor.selected < editor.inputs.len())
            .render_frame(frame, layout.canvas.unwrap(), context);
        for (index, (input, area)) in editor.inputs.iter().zip(&layout.inputs).enumerate() {
            let mut input = input.clone();
            input.set_focused(editor.selected == index);
            input.render_borderless_frame(frame, *area, &theme);
        }
        let mut save = Button::new(
            "launcher.edit.save",
            format!("F10 {}", i18n::tr!("shell-save")),
        )
        .with_emphasis(ButtonEmphasis::Primary);
        save.set_focused(editor.selected == editor.inputs.len());
        save.render_borderless_frame(frame, layout.submit, &theme);
        let mut cancel = Button::new(
            "launcher.edit.cancel",
            format!("Esc {}", i18n::tr!("shell-cancel")),
        );
        cancel.set_focused(editor.selected == editor.inputs.len() + 1);
        cancel.render_borderless_frame(frame, layout.cancel, &theme);
        return true;
    }
    if let Some(menu) = &model.context_menu {
        let (area, rows) = launcher_context_menu_layout(main, menu);
        frame.render_widget(Clear, area);
        Surface::new()
            .bordered(true)
            .raised(true)
            .render_frame(frame, area, context);
        for (index, (action, row)) in menu.actions().zip(rows).enumerate() {
            menu.button(index, action)
                .render_borderless_frame(frame, row, &theme);
        }
        return true;
    }
    false
}

pub(super) fn render_custom_icon(
    frame: &mut Frame<'_>,
    area: Rect,
    icon: &str,
    context: &RenderContext,
    style: Style,
) {
    let width = (app::launcher::LAUNCHER_ICON_WIDTH as u16 + 2).min(area.width);
    let canvas = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y,
        width,
        (app::launcher::LAUNCHER_ICON_HEIGHT as u16 + 2).min(area.height),
    );
    Surface::new()
        .bordered(true)
        .render_frame(frame, canvas, context);
    let inner = inset(canvas, 1);
    for (row, line) in icon.split('\n').take(usize::from(inner.height)).enumerate() {
        frame.render_widget(
            Paragraph::new(truncate_to_terminal_width(line, usize::from(inner.width))).style(style),
            Rect::new(inner.x, inner.y + row as u16, inner.width, 1),
        );
    }
}
