use ratatui::{Frame, layout::Rect};

use super::{ButtonRegion, InputEvent, Key, TextInput, contains_point};
use crate::{InputPhase, ManagementLayout, ManagementScrollTarget, RenderContext};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FormControl {
    #[default]
    Text,
    Checkbox,
    Toggle,
    Cycle,
    Permissions,
    Number {
        minimum: u32,
        maximum: u32,
        step: u32,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormField {
    pub id: String,
    pub label: String,
    pub value: String,
    pub secret: bool,
    pub read_only: bool,
    pub disabled: bool,
    pub choices: Vec<String>,
    pub control: FormControl,
    pub selected_cell: Option<usize>,
    pub input: Option<TextInput>,
}

impl FormField {
    pub fn text(id: impl Into<String>, label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value: value.into(),
            ..Default::default()
        }
    }

    pub fn toggle(id: impl Into<String>, label: impl Into<String>, checked: bool) -> Self {
        Self {
            control: FormControl::Toggle,
            ..Self::text(id, label, checked.to_string())
        }
    }

    pub fn is_text(&self) -> bool {
        matches!(self.control, FormControl::Text | FormControl::Number { .. })
            && self.choices.is_empty()
    }

    pub fn edit(&mut self, event: InputEvent, area: Rect) {
        if !self.is_text() || self.read_only || self.disabled {
            return;
        }
        let added = match &event {
            InputEvent::Paste(value) => value.len(),
            InputEvent::Key(key) => match key.key {
                Key::Char(c) => c.len_utf8(),
                Key::Space => 1,
                _ => 0,
            },
            _ => 0,
        };
        if self.value.len().saturating_add(added) > 16 * 1024 {
            return;
        }
        if matches!(self.control, FormControl::Number { .. })
            && matches!(&event, InputEvent::Key(key) if matches!(key.key, Key::Char(c) if !c.is_ascii_digit()) && !key.has_non_shift_modifier())
        {
            return;
        }
        let input = self.input.get_or_insert_with(|| {
            let mut input = TextInput::new(self.id.as_str()).with_horizontal_scroll(true);
            input.set_value(self.value.clone());
            input
        });
        input.set_focused(true);
        if self.secret && matches!(event, InputEvent::Mouse(_)) {
            let mut masked = input.clone();
            masked.set_value("•".repeat(self.value.chars().count()));
            masked.handle_event_borderless(event, area);
            input.set_cursor(masked.cursor());
            input.set_selection_anchor(masked.selection_anchor());
        } else {
            input.handle_event_borderless(event, area);
            self.value = input.value().to_owned();
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormChoices {
    pub search: String,
    pub field: usize,
    pub values: Vec<String>,
    pub selected: usize,
    pub scroll: usize,
    pub columns: usize,
    pub disabled: Vec<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormAction {
    Submit,
    Cancel,
    Custom(String),
    AdjustNumber { field: usize, direction: i8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormButton {
    pub id: String,
    pub label: String,
    pub action: FormAction,
    pub shortcut: Option<Key>,
    pub disabled: bool,
}

impl FormButton {
    pub fn new(id: impl Into<String>, label: impl Into<String>, action: FormAction) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            action,
            shortcut: None,
            disabled: false,
        }
    }

    pub fn label(&self) -> String {
        match &self.shortcut {
            Some(key) => format!("{} {}", key.label(), self.label),
            None => self.label.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormEvent {
    None,
    Changed,
    Action(FormAction),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormInteraction {
    pressed: Option<ButtonRegion>,
    scrollbar: Option<(ManagementScrollTarget, u16)>,
    field_scroll: Option<usize>,
    message_scroll: u16,
}

impl FormInteraction {
    pub fn is_dragging(&self) -> bool {
        self.scrollbar.is_some()
    }

    pub fn cancel_pointer(&mut self) {
        self.pressed = None;
        self.scrollbar = None;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormDialog {
    pub identity: String,
    pub title: String,
    pub message: String,
    pub message_scroll: u16,
    pub fields: Vec<FormField>,
    pub columns: usize,
    pub selected: usize,
    pub field_scroll: Option<usize>,
    pub choice: Option<FormChoices>,
    pub buttons: Vec<FormButton>,
    pub interaction: FormInteraction,
    pub cancel_label: Option<String>,
    pub cancel_disabled: bool,
    pub submit_label: Option<String>,
    pub submit_disabled: bool,
    pub dismiss_only: bool,
}

impl FormDialog {
    pub fn with_interaction(mut self, interaction: FormInteraction) -> Self {
        self.field_scroll = interaction.field_scroll;
        self.message_scroll = interaction.message_scroll;
        self.interaction = interaction;
        self
    }

    pub fn interaction_state(&self) -> FormInteraction {
        FormInteraction {
            field_scroll: self.field_scroll,
            message_scroll: self.message_scroll,
            ..self.interaction.clone()
        }
    }

    pub fn new(
        identity: impl Into<String>,
        title: impl Into<String>,
        fields: Vec<FormField>,
    ) -> Self {
        Self {
            identity: identity.into(),
            title: title.into(),
            fields,
            ..Default::default()
        }
    }

    pub fn actions(&self) -> Vec<FormButton> {
        if !self.buttons.is_empty() {
            return self.buttons.clone();
        }
        let mut buttons = Vec::new();
        if !self.dismiss_only {
            buttons.push(FormButton {
                shortcut: Some(Key::F(10)),
                disabled: self.submit_disabled,
                ..FormButton::new(
                    "confirm",
                    self.submit_label
                        .clone()
                        .unwrap_or_else(|| i18n::tr!("management-form-submit")),
                    FormAction::Submit,
                )
            });
        }
        buttons.push(FormButton {
            shortcut: (!self.dismiss_only).then_some(Key::Escape),
            disabled: self.cancel_disabled,
            ..FormButton::new(
                "cancel",
                self.cancel_label.clone().unwrap_or_else(|| {
                    i18n::tr!(if self.dismiss_only {
                        "management-form-close"
                    } else {
                        "management-form-cancel"
                    })
                }),
                FormAction::Cancel,
            )
        });
        buttons
    }

    pub fn layout(&self, main: Rect) -> ManagementLayout {
        crate::management_layout(
            main,
            &crate::ManagementViewModel {
                form: Some(self.clone()),
                ..Default::default()
            },
        )
    }

    pub fn button_regions(&self, main: Rect) -> Vec<ButtonRegion> {
        crate::management_button_regions(
            main,
            &crate::ManagementViewModel {
                form: Some(self.clone()),
                ..Default::default()
            },
        )
    }

    pub(crate) fn action_bar(&self) -> super::PageActionBar {
        super::PageActionBar::new(self.identity.as_str()).with_actions(
            self.actions()
                .iter()
                .map(|button| {
                    super::PageAction::new(button.id.as_str(), button.label())
                        .disabled(button.disabled)
                })
                .collect(),
        )
    }

    pub fn render_frame(&self, frame: &mut Frame<'_>, main: Rect, context: &RenderContext) {
        crate::render_management_overlay(
            frame,
            main,
            &crate::ManagementViewModel {
                form: Some(self.clone()),
                ..Default::default()
            },
            context,
        );
    }

    pub fn handle_event(&mut self, event: InputEvent, main: Rect) -> FormEvent {
        let layout = self.layout(main);
        if self.choice.is_some() && matches!(event, InputEvent::Key(_) | InputEvent::Paste(_)) {
            self.interaction = Default::default();
            return self.handle_choice(event, &layout);
        }
        match event {
            InputEvent::Key(key) => {
                self.interaction = Default::default();
                if key.phase == InputPhase::Release {
                    return FormEvent::None;
                }
                let actions = self.actions();
                if matches!(key.key, Key::Char('+' | '=' | '-'))
                    && !key.has_non_shift_modifier()
                    && self
                        .fields
                        .get(self.selected)
                        .is_some_and(|field| matches!(field.control, FormControl::Number { .. }))
                {
                    return self.adjust_number(
                        self.selected,
                        if key.key == Key::Char('-') { -1 } else { 1 },
                    );
                }
                if matches!(key.key, Key::Enter | Key::Escape | Key::F(10))
                    && key.phase != InputPhase::Press
                {
                    return FormEvent::None;
                }
                if key.key == Key::Enter && key.modifiers.is_control_only() {
                    return self.submit();
                }
                if let Some(index) = actions
                    .iter()
                    .position(|button| button.shortcut.as_ref() == Some(&key.key))
                {
                    if key.phase == InputPhase::Press && !key.has_non_shift_modifier() {
                        return self.activate_button(index);
                    }
                    return FormEvent::None;
                }
                if key.key == Key::Escape && !key.has_non_shift_modifier() {
                    return self.cancel();
                }
                if !key.has_non_shift_modifier() {
                    if matches!(key.key, Key::Left | Key::Right)
                        && self.fields.get(self.selected).is_some_and(|field| {
                            field.read_only && matches!(field.control, FormControl::Number { .. })
                        })
                    {
                        return self.adjust_number(
                            self.selected,
                            if key.key == Key::Left { -1 } else { 1 },
                        );
                    }
                    match key.key {
                        Key::Tab | Key::BackTab | Key::Up | Key::Down => {
                            let reverse = matches!(key.key, Key::BackTab | Key::Up)
                                || key.key == Key::Tab && key.modifiers.shift;
                            let count = self.fields.len() + actions.len();
                            self.selected =
                                (self.selected + if reverse { count - 1 } else { 1 }) % count;
                            self.field_scroll = None;
                            return FormEvent::Changed;
                        }
                        Key::Enter => {
                            self.field_scroll = None;
                            if self.selected + 1 < self.fields.len() {
                                self.selected += 1;
                                return FormEvent::Changed;
                            }
                            if self.selected < self.fields.len() {
                                return self.submit();
                            }
                            return self.activate_button(self.selected - self.fields.len());
                        }
                        Key::PageUp => {
                            self.message_scroll = self.message_scroll.saturating_sub(3);
                            return FormEvent::Changed;
                        }
                        Key::PageDown => {
                            self.message_scroll = self.message_scroll.saturating_add(3);
                            return FormEvent::Changed;
                        }
                        _ => {}
                    }
                    if self.selected >= self.fields.len() {
                        if matches!(key.key, Key::Left | Key::Right) {
                            let count = self.fields.len() + actions.len();
                            self.selected = (self.selected
                                + if key.key == Key::Left { count - 1 } else { 1 })
                                % count;
                            return FormEvent::Changed;
                        }
                        if matches!(key.key, Key::Space | Key::Char(' '))
                            && key.phase == InputPhase::Press
                        {
                            return self.activate_button(self.selected - self.fields.len());
                        }
                        return FormEvent::None;
                    }
                    let field = &mut self.fields[self.selected];
                    if field.disabled {
                        return FormEvent::None;
                    }
                    if matches!(key.key, Key::Space | Key::Char(' '))
                        && !field.is_text()
                        && key.phase == InputPhase::Press
                    {
                        return self.activate_field(self.selected);
                    }
                    if matches!(key.key, Key::Left | Key::Right) && !field.is_text() {
                        let forward = key.key == Key::Right;
                        if field.control == FormControl::Permissions {
                            let cell = field.selected_cell.unwrap_or(0);
                            field.selected_cell = Some((cell + if forward { 1 } else { 8 }) % 9);
                        } else if !field.choices.is_empty() {
                            let index = field
                                .choices
                                .iter()
                                .position(|value| value == &field.value)
                                .unwrap_or(0);
                            field.value = field.choices[(index
                                + if forward { 1 } else { field.choices.len() - 1 })
                                % field.choices.len()]
                            .clone();
                        }
                        return FormEvent::Changed;
                    }
                }
                self.edit_selected(InputEvent::Key(key), &layout)
            }
            InputEvent::Paste(value) => self.edit_selected(InputEvent::Paste(value), &layout),
            InputEvent::Mouse(mouse) => {
                let point = mouse.coordinates();
                if let Some((target, grab)) = self.interaction.scrollbar {
                    if matches!(
                        mouse.kind,
                        crate::MouseEventKind::Drag(_) | crate::MouseEventKind::Up(_)
                    ) {
                        if let Some(bar) = layout.scrollbars.iter().find(|bar| bar.target == target)
                        {
                            self.scroll(target, bar.offset_at(point, grab));
                        }
                        if matches!(mouse.kind, crate::MouseEventKind::Up(_)) {
                            self.interaction.scrollbar = None;
                        }
                        return FormEvent::Changed;
                    }
                }
                match mouse.kind {
                    crate::MouseEventKind::Down(super::MouseButton::Left) => {
                        if let Some(bar) = layout
                            .scrollbars
                            .iter()
                            .find(|bar| contains_point(bar.track, point.0, point.1))
                        {
                            let grab = bar.grab_at(point);
                            self.scroll(bar.target, bar.offset_at(point, grab));
                            self.interaction.scrollbar = Some((bar.target, grab));
                        } else {
                            self.interaction.pressed =
                                self.button_regions(main).into_iter().find(|button| {
                                    !button.disabled
                                        && contains_point(button.area, point.0, point.1)
                                });
                            if self.choice.is_none()
                                && let Some((index, _)) = layout
                                    .fields
                                    .iter()
                                    .find(|(_, area)| contains_point(*area, point.0, point.1))
                            {
                                self.selected = *index;
                                if self.fields[*index].is_text() {
                                    return self.edit_selected(InputEvent::Mouse(mouse), &layout);
                                }
                            }
                        }
                        FormEvent::Changed
                    }
                    crate::MouseEventKind::Up(super::MouseButton::Left) => {
                        if self.interaction.pressed.take().is_some_and(|pressed| {
                            contains_point(pressed.area, point.0, point.1)
                                && self.button_regions(main).contains(&pressed)
                        }) {
                            if self.choice.is_none()
                                && let Some(field) = self.fields.get(self.selected)
                            {
                                let input = crate::management_field_input_area(
                                    &layout,
                                    self.selected,
                                    field,
                                );
                                if field.is_text() && contains_point(input, point.0, point.1) {
                                    return self.edit_selected(InputEvent::Mouse(mouse), &layout);
                                }
                            }
                            self.activate_at(main, point)
                        } else {
                            FormEvent::None
                        }
                    }
                    crate::MouseEventKind::Drag(super::MouseButton::Left) => {
                        if self
                            .interaction
                            .pressed
                            .as_ref()
                            .is_some_and(|pressed| !contains_point(pressed.area, point.0, point.1))
                        {
                            self.interaction.pressed = None;
                        }
                        if self.choice.is_none() {
                            self.edit_selected(InputEvent::Mouse(mouse), &layout)
                        } else {
                            FormEvent::None
                        }
                    }
                    crate::MouseEventKind::Moved => {
                        if self
                            .interaction
                            .pressed
                            .as_ref()
                            .is_some_and(|pressed| !contains_point(pressed.area, point.0, point.1))
                        {
                            self.interaction.pressed = None;
                        }
                        FormEvent::None
                    }
                    crate::MouseEventKind::Down(_) => {
                        self.interaction = Default::default();
                        FormEvent::None
                    }
                    crate::MouseEventKind::Scroll(direction) => {
                        self.interaction.pressed = None;
                        let target = if self.choice.is_some() {
                            ManagementScrollTarget::Choices
                        } else if contains_point(layout.message, point.0, point.1) {
                            ManagementScrollTarget::FormMessage
                        } else {
                            ManagementScrollTarget::FormFields
                        };
                        let offset = if let Some(choice) = &self.choice {
                            choice.scroll
                        } else if target == ManagementScrollTarget::FormMessage {
                            usize::from(self.message_scroll)
                        } else {
                            layout.field_start
                        };
                        self.scroll(
                            target,
                            if direction == crate::ScrollDirection::Up {
                                offset.saturating_sub(3)
                            } else {
                                offset.saturating_add(3)
                            },
                        );
                        FormEvent::Changed
                    }
                    _ => FormEvent::None,
                }
            }
            InputEvent::FocusLost | InputEvent::Resize { .. } => {
                self.interaction = Default::default();
                FormEvent::None
            }
            _ => FormEvent::None,
        }
    }

    /// Activates a click already checked by the Shell's shared press/release handling.
    pub fn activate_at(&mut self, main: Rect, point: (u16, u16)) -> FormEvent {
        let InputEvent::Mouse(mouse) = InputEvent::mouse_down(super::MouseButton::Left, point)
        else {
            unreachable!()
        };
        self.activate_pointer(main, mouse)
    }

    pub fn activate_pointer(&mut self, main: Rect, mouse: super::MouseInput) -> FormEvent {
        let point = mouse.coordinates();
        let layout = self.layout(main);
        if self.choice.is_some() {
            return self.activate_choice_at(&layout, point);
        }
        for (index, _) in &layout.fields {
            if matches!(self.fields[*index].control, FormControl::Number { .. }) {
                for (direction, area) in [-1, 1].into_iter().zip(crate::management_number_buttons(
                    &layout,
                    *index,
                    &self.fields[*index],
                )) {
                    if contains_point(area, point.0, point.1) {
                        return self.adjust_number(*index, direction);
                    }
                }
            }
        }
        if let Some((index, area)) = layout
            .fields
            .iter()
            .find(|(_, area)| contains_point(*area, point.0, point.1))
        {
            self.selected = *index;
            if self.fields[*index].is_text() {
                let input =
                    crate::management_field_input_area(&layout, *index, &self.fields[*index]);
                self.fields[*index].edit(InputEvent::Mouse(mouse), input);
                return FormEvent::Changed;
            }
            if self.fields[*index].control == FormControl::Permissions {
                let skip = layout
                    .field_start
                    .saturating_sub(layout.field_ranges[*index].0);
                if let Some((cell, _)) =
                    crate::management_permission_cells(&self.fields[*index], *area, skip)
                        .iter()
                        .find(|(_, cell)| contains_point(*cell, point.0, point.1))
                {
                    self.fields[*index].selected_cell = Some(*cell);
                }
            }
            self.activate_field(*index)
        } else if let Some((index, _)) = layout
            .form_buttons
            .iter()
            .find(|(_, area)| contains_point(*area, point.0, point.1))
        {
            self.selected = self.fields.len() + index;
            self.activate_button(*index)
        } else {
            FormEvent::None
        }
    }

    /// For hosts that already check matching pointer presses and releases.
    pub fn handle_confirmed_pointer(&mut self, mouse: super::MouseInput, main: Rect) -> FormEvent {
        if mouse.kind == crate::MouseEventKind::Down(super::MouseButton::Left)
            && !self
                .layout(main)
                .scrollbars
                .iter()
                .any(|bar| contains_point(bar.track, mouse.column(), mouse.row()))
        {
            self.activate_pointer(main, mouse)
        } else {
            self.handle_event(InputEvent::Mouse(mouse), main)
        }
    }

    fn activate_choice_at(&mut self, layout: &ManagementLayout, point: (u16, u16)) -> FormEvent {
        let choice = self.choice.as_ref().unwrap();
        if contains_point(layout.choice_cancel, point.0, point.1) {
            self.choice = None;
        } else if let Some((index, _)) = layout
            .choice_rows
            .iter()
            .find(|(_, area)| contains_point(*area, point.0, point.1))
        {
            if choice.disabled.get(*index).copied().unwrap_or(false) {
                return FormEvent::None;
            }
            self.fields[choice.field].value = choice.values[*index].clone();
            self.choice = None;
        } else {
            return FormEvent::None;
        }
        FormEvent::Changed
    }

    fn edit_selected(&mut self, event: InputEvent, layout: &ManagementLayout) -> FormEvent {
        let Some(field) = self.fields.get_mut(self.selected) else {
            return FormEvent::None;
        };
        let area = crate::management_field_input_area(layout, self.selected, field);
        field.edit(event, area);
        FormEvent::Changed
    }

    fn submit(&mut self) -> FormEvent {
        self.actions()
            .iter()
            .position(|button| button.action == FormAction::Submit)
            .map_or(FormEvent::None, |index| self.activate_button(index))
    }

    fn cancel(&mut self) -> FormEvent {
        self.actions()
            .iter()
            .position(|button| button.action == FormAction::Cancel)
            .map_or(FormEvent::None, |index| self.activate_button(index))
    }

    pub fn activate_button(&mut self, index: usize) -> FormEvent {
        let Some(button) = self
            .actions()
            .get(index)
            .cloned()
            .filter(|button| !button.disabled)
        else {
            return FormEvent::None;
        };
        if let FormAction::AdjustNumber { field, direction } = button.action {
            self.adjust_number(field, direction)
        } else {
            FormEvent::Action(button.action)
        }
    }

    pub fn adjust_number(&mut self, index: usize, direction: i8) -> FormEvent {
        if let Some(field) = self.fields.get_mut(index).filter(|field| !field.disabled) {
            if let FormControl::Number {
                minimum,
                maximum,
                step,
            } = field.control
            {
                let value = field.value.parse::<u32>().unwrap_or(minimum);
                field.value = if direction < 0 {
                    value.saturating_sub(step)
                } else {
                    value.saturating_add(step)
                }
                .clamp(minimum, maximum)
                .to_string();
                field.input = None;
            }
        }
        FormEvent::Changed
    }

    pub fn activate_field(&mut self, index: usize) -> FormEvent {
        let field = &mut self.fields[index];
        if field.disabled {
            return FormEvent::None;
        }
        match field.control {
            FormControl::Checkbox | FormControl::Toggle => {
                field.value = (field.value != "true").to_string()
            }
            FormControl::Permissions => {
                let cell = field.selected_cell.unwrap_or(0).min(8);
                let mut values = field.value.as_bytes().to_vec();
                if let Some(value) = values.get_mut(cell) {
                    *value = if *value == b'1' { b'0' } else { b'1' };
                }
                field.value = String::from_utf8(values).unwrap();
            }
            FormControl::Cycle if !field.choices.is_empty() => {
                let index = field
                    .choices
                    .iter()
                    .position(|value| value == &field.value)
                    .unwrap_or(0);
                field.value = field.choices[(index + 1) % field.choices.len()].clone();
            }
            _ if !field.choices.is_empty() => {
                self.choice = Some(FormChoices {
                    field: index,
                    values: field.choices.clone(),
                    selected: field
                        .choices
                        .iter()
                        .position(|value| value == &field.value)
                        .unwrap_or(0),
                    ..Default::default()
                });
            }
            _ => {}
        }
        FormEvent::Changed
    }

    fn scroll(&mut self, target: ManagementScrollTarget, offset: usize) {
        match target {
            ManagementScrollTarget::FormMessage => {
                self.message_scroll = offset.min(u16::MAX as usize) as u16
            }
            ManagementScrollTarget::FormFields => self.field_scroll = Some(offset),
            ManagementScrollTarget::Choices => {
                if let Some(choice) = &mut self.choice {
                    choice.scroll = offset;
                }
            }
            _ => {}
        }
    }

    fn handle_choice(&mut self, event: InputEvent, layout: &ManagementLayout) -> FormEvent {
        let choice = self.choice.as_mut().unwrap();
        match event {
            InputEvent::Key(key) if key.phase.is_press_like() && !key.has_non_shift_modifier() => {
                if key.phase != InputPhase::Press
                    && matches!(key.key, Key::Enter | Key::Space | Key::Escape)
                {
                    return FormEvent::None;
                }
                match key.key {
                    Key::Escape => {
                        self.choice = None;
                    }
                    Key::Enter | Key::Space => {
                        if !choice
                            .disabled
                            .get(choice.selected)
                            .copied()
                            .unwrap_or(false)
                        {
                            if let Some(value) = choice.values.get(choice.selected) {
                                self.fields[choice.field].value = value.clone();
                            }
                            self.choice = None;
                        }
                    }
                    Key::Up | Key::BackTab => choice.selected = choice.selected.saturating_sub(1),
                    Key::Tab if key.modifiers.shift => {
                        choice.selected = choice.selected.saturating_sub(1)
                    }
                    Key::Down | Key::Tab => {
                        choice.selected =
                            (choice.selected + 1).min(choice.values.len().saturating_sub(1))
                    }
                    Key::Home => choice.selected = 0,
                    Key::End => choice.selected = choice.values.len().saturating_sub(1),
                    Key::PageUp => {
                        choice.selected = choice
                            .selected
                            .saturating_sub(layout.choice_rows.len().max(1))
                    }
                    Key::PageDown => {
                        choice.selected = (choice.selected + layout.choice_rows.len().max(1))
                            .min(choice.values.len().saturating_sub(1))
                    }
                    Key::Char(c) => {
                        choice.search.push(c);
                        self.filter_choices();
                    }
                    Key::Backspace => {
                        choice.search.pop();
                        self.filter_choices();
                    }
                    _ => return FormEvent::None,
                }
            }
            InputEvent::Paste(value) => {
                choice.search.push_str(&value);
                self.filter_choices();
            }
            _ => return FormEvent::None,
        }
        if let Some(choice) = &mut self.choice {
            let capacity = layout.choice_rows.len().max(1);
            if choice.selected < choice.scroll {
                choice.scroll = choice.selected;
            } else if choice.selected >= choice.scroll + capacity {
                choice.scroll = choice.selected + 1 - capacity;
            }
        }
        FormEvent::Changed
    }

    fn filter_choices(&mut self) {
        let choice = self.choice.as_mut().unwrap();
        let query = choice.search.to_lowercase();
        choice.values = self.fields[choice.field]
            .choices
            .iter()
            .filter(|value| value.to_lowercase().contains(&query))
            .cloned()
            .collect();
        choice.selected = 0;
        choice.scroll = 0;
    }
}
