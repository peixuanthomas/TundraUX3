use super::*;
use crate::components::{ButtonEmphasis, TextInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherEntryAction {
    Toolbar(LauncherToolbarAction),
    NewSession,
    Move,
    Name,
    Command,
    Icon,
    WaitForEnter,
    Remove,
}

impl LauncherEntryAction {
    pub fn label(self) -> String {
        match self {
            Self::Toolbar(action) => action.label(),
            Self::NewSession => i18n::tr!("ui-workspace-new-session"),
            Self::Move => i18n::tr!("shell-move"),
            Self::Name => i18n::tr!("ui-launcher-change-name"),
            Self::Command => i18n::tr!("ui-launcher-change-command"),
            Self::Icon => i18n::tr!("ui-launcher-change-icon"),
            Self::WaitForEnter => i18n::tr!("ui-launcher-keep-screen"),
            Self::Remove => i18n::tr!("shell-delete"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherContextMenu {
    pub editable: bool,
    pub new_session: bool,
    pub move_item: Option<bool>,
    pub toolbar: Vec<LauncherToolbarButtonViewModel>,
    pub anchor: (u16, u16),
    pub selected: usize,
    pub command_enabled: bool,
    pub wait_for_enter: Option<bool>,
}

impl LauncherContextMenu {
    pub fn actions(&self) -> impl Iterator<Item = LauncherEntryAction> + '_ {
        use LauncherEntryAction::*;
        self.toolbar
            .iter()
            .map(|button| Toolbar(button.action))
            .chain(self.new_session.then_some(NewSession))
            .chain(self.move_item.map(|_| Move))
            .chain(self.editable.then_some(Remove))
            .chain([Name, Command, Icon].into_iter().filter(|_| self.editable))
            .chain((self.editable && self.wait_for_enter.is_some()).then_some(WaitForEnter))
    }

    pub fn enabled(&self, action: LauncherEntryAction) -> bool {
        match action {
            LauncherEntryAction::Toolbar(action) => self
                .toolbar
                .iter()
                .any(|button| button.action == action && button.enabled),
            LauncherEntryAction::NewSession => {
                self.new_session
                    && self.enabled(LauncherEntryAction::Toolbar(LauncherToolbarAction::Open))
            }
            LauncherEntryAction::Move => self.move_item == Some(true),
            LauncherEntryAction::Command => self.command_enabled,
            _ => self.editable,
        }
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
        button.set_disabled(!self.enabled(action));
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
    let height = (menu.actions().count() as u16 + 2).min(main.height);
    let first = menu
        .selected
        .saturating_sub(usize::from(height.saturating_sub(2)).saturating_sub(1));
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
    let rows = (0..menu.actions().count())
        .map(|index| {
            let Some(row) = index
                .checked_sub(first)
                .filter(|&row| row < usize::from(height.saturating_sub(2)))
            else {
                return Rect::default();
            };
            Rect::new(area.x, area.y + 1 + row as u16, area.width, 1).intersection(area)
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
    let area = crate::screens::shell::centered_rect(main, 58.min(main.width), 15.min(main.height));
    let chrome = launcher_icon_editor_chrome(area);
    let width = (app::launcher::LAUNCHER_ICON_WIDTH as u16 + 2).min(chrome.body.width);
    let canvas_height = (app::launcher::LAUNCHER_ICON_HEIGHT as u16 + 2).min(chrome.body.height);
    let canvas = Rect::new(
        chrome.body.x + chrome.body.width.saturating_sub(width) / 2,
        chrome.body.bottom().saturating_sub(canvas_height),
        width,
        canvas_height,
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
    let widths = [
        format!("Esc {}", i18n::tr!("shell-cancel")),
        format!("F10 {}", i18n::tr!("shell-save")),
    ]
    .map(|label| {
        (terminal_width(&label) as u16 + 2).min(chrome.footer.width.saturating_sub(1) / 2)
    });
    let actions = crate::right_aligned_actions(chrome.footer, &widths);
    LauncherEntryEditorLayout {
        area,
        canvas: Some(canvas),
        inputs,
        submit: actions[1],
        cancel: actions[0],
    }
}

fn launcher_icon_editor_chrome(area: Rect) -> DialogLayout {
    DialogLayout::with_min_body(
        area,
        1,
        None,
        app::launcher::LAUNCHER_ICON_HEIGHT as u16 + 2,
    )
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
        let chrome = launcher_icon_editor_chrome(layout.area);
        chrome.render_frame(frame, &editor.action.label(), context);
        let message = editor.form().message;
        let canvas = layout.canvas.unwrap();
        frame.render_widget(
            Paragraph::new(message).wrap(Wrap { trim: false }),
            Rect::new(
                chrome.body.x,
                chrome.body.y,
                chrome.body.width,
                canvas.y.saturating_sub(chrome.body.y),
            ),
        );
        Surface::new()
            .bordered(true)
            .focused(editor.selected < editor.inputs.len())
            .render_frame(frame, canvas, context);
        let theme = DialogLayout::body_theme(context);
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
        let footer = DialogLayout::footer_theme(context);
        save.render_inline_frame(frame, layout.submit, &footer);
        let mut cancel = Button::new(
            "launcher.edit.cancel",
            format!("Esc {}", i18n::tr!("shell-cancel")),
        );
        cancel.set_focused(editor.selected == editor.inputs.len() + 1);
        cancel.render_inline_frame(frame, layout.cancel, &footer);
        return true;
    }
    if let Some(menu) = &model.context_menu {
        let (area, rows) = launcher_context_menu_layout(main, menu);
        frame.render_widget(ratatui::widgets::Clear, area);
        Surface::new()
            .raised(true)
            .render_frame(frame, area, context);
        let theme = DialogLayout::footer_theme(context);
        for (index, (action, row)) in menu.actions().zip(rows).enumerate() {
            menu.button(index, action)
                .with_background(context.theme.raised)
                .with_current(index == menu.selected)
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
