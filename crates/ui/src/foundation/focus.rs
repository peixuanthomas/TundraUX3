use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

use crate::input::{InputEvent, Key, UiId};

pub type ComponentId = UiId;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FocusScope {
    Global,
    Modal(ComponentId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusNode {
    pub id: ComponentId,
    pub scope: FocusScope,
    pub enabled: bool,
}

impl FocusNode {
    pub fn new(id: impl Into<ComponentId>, scope: FocusScope) -> Self {
        Self {
            id: id.into(),
            scope,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Next,
    Previous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusChange {
    pub previous: Option<ComponentId>,
    pub current: Option<ComponentId>,
    pub direction: FocusDirection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusError {
    DuplicateTarget(ComponentId),
    UnknownTarget(ComponentId),
    DisabledTarget(ComponentId),
    EmptyFocusScope,
    TargetOutsideModal {
        modal: ComponentId,
        target: ComponentId,
    },
}

impl fmt::Display for FocusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateTarget(target) => write!(formatter, "duplicate focus target: {target}"),
            Self::UnknownTarget(target) => write!(formatter, "unknown focus target: {target}"),
            Self::DisabledTarget(target) => write!(formatter, "disabled focus target: {target}"),
            Self::EmptyFocusScope => formatter.write_str("focus scope cannot be empty"),
            Self::TargetOutsideModal { modal, target } => {
                write!(formatter, "focus target {target} is outside modal {modal}")
            }
        }
    }
}

impl Error for FocusError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalTrap {
    pub modal: ComponentId,
    pub focus_order: Vec<ComponentId>,
    pub restore_focus: Option<ComponentId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusManager {
    order: Vec<FocusNode>,
    focused: Option<ComponentId>,
    modal_trap: Option<ModalTrap>,
}

impl FocusManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_order<I, T>(ids: I) -> Result<Self, FocusError>
    where
        I: IntoIterator<Item = T>,
        T: Into<ComponentId>,
    {
        let mut manager = Self::new();
        for id in ids {
            manager.register(id)?;
        }
        Ok(manager)
    }

    pub fn register(&mut self, id: impl Into<ComponentId>) -> Result<(), FocusError> {
        self.register_in_scope(id, FocusScope::Global)
    }

    pub fn register_in_scope(
        &mut self,
        id: impl Into<ComponentId>,
        scope: FocusScope,
    ) -> Result<(), FocusError> {
        let node = FocusNode::new(id, scope);
        if self.order.iter().any(|existing| existing.id == node.id) {
            return Err(FocusError::DuplicateTarget(node.id));
        }

        self.order.push(node);
        self.normalize_focus();
        Ok(())
    }

    pub fn unregister(&mut self, id: &ComponentId) -> bool {
        let initial_len = self.order.len();
        self.order.retain(|node| &node.id != id);

        if let Some(trap) = &mut self.modal_trap {
            trap.focus_order.retain(|target| target != id);
        }

        if self.focused.as_ref() == Some(id) {
            self.focused = None;
        }

        self.normalize_focus();
        self.order.len() != initial_len
    }

    pub fn set_enabled(&mut self, id: &ComponentId, enabled: bool) -> Result<(), FocusError> {
        let Some(node) = self.order.iter_mut().find(|node| &node.id == id) else {
            return Err(FocusError::UnknownTarget(id.clone()));
        };

        node.enabled = enabled;
        if self.focused.as_ref() == Some(id) && !enabled {
            self.focused = None;
            self.normalize_focus();
        }
        Ok(())
    }

    pub fn set_focus(&mut self, id: impl Into<ComponentId>) -> Result<(), FocusError> {
        let id = id.into();
        if !self.order.iter().any(|node| node.id == id) {
            return Err(FocusError::UnknownTarget(id));
        }
        if !self.is_enabled(&id) {
            return Err(FocusError::DisabledTarget(id));
        }
        if let Some(modal) = self.modal_id()
            && !self.is_in_active_scope(&id)
        {
            return Err(FocusError::TargetOutsideModal {
                modal: modal.clone(),
                target: id,
            });
        }

        self.focused = Some(id);
        Ok(())
    }

    pub fn focused(&self) -> Option<&ComponentId> {
        self.focused.as_ref()
    }

    pub fn clear_focus(&mut self) {
        self.focused = None;
    }

    pub fn move_focus(&mut self, direction: FocusDirection) -> Option<&ComponentId> {
        let candidates = self.enabled_active_order();
        if candidates.is_empty() {
            self.focused = None;
            return None;
        }

        let next_index = self
            .focused
            .as_ref()
            .and_then(|focused| candidates.iter().position(|candidate| candidate == focused))
            .map(|index| match direction {
                FocusDirection::Next => (index + 1) % candidates.len(),
                FocusDirection::Previous => (index + candidates.len() - 1) % candidates.len(),
            })
            .unwrap_or(0);

        self.focused = Some(candidates[next_index].clone());
        self.focused.as_ref()
    }

    pub fn handle_input(&mut self, event: &InputEvent) -> Option<FocusChange> {
        let InputEvent::Key(key_event) = event else {
            return None;
        };
        if !key_event.is_press_like() {
            return None;
        }
        if key_event.modifiers.ctrl || key_event.modifiers.alt {
            return None;
        }

        let direction = match key_event.key {
            Key::BackTab => FocusDirection::Previous,
            Key::Tab if key_event.modifiers.shift => FocusDirection::Previous,
            Key::Tab => FocusDirection::Next,
            _ => return None,
        };

        let previous = self.focused.clone();
        self.move_focus(direction);
        Some(FocusChange {
            previous,
            current: self.focused.clone(),
            direction,
        })
    }

    pub fn enter_modal(&mut self, modal: impl Into<ComponentId>) -> Result<(), FocusError> {
        let modal = modal.into();
        let focus_order = self
            .order
            .iter()
            .filter(|node| node.scope == FocusScope::Modal(modal.clone()))
            .map(|node| node.id.clone())
            .collect::<Vec<_>>();

        self.trap_modal(modal, focus_order)
    }

    pub fn trap_modal<I, T>(
        &mut self,
        modal: impl Into<ComponentId>,
        focus_order: I,
    ) -> Result<(), FocusError>
    where
        I: IntoIterator<Item = T>,
        T: Into<ComponentId>,
    {
        let focus_order = focus_order.into_iter().map(Into::into).collect::<Vec<_>>();
        validate_focus_scope(&focus_order, &self.order)?;

        self.modal_trap = Some(ModalTrap {
            modal: modal.into(),
            focus_order,
            restore_focus: self.focused.clone(),
        });
        self.normalize_focus();
        Ok(())
    }

    pub fn release_modal_trap(&mut self) -> Option<&ComponentId> {
        let restore_focus = self.modal_trap.take().and_then(|modal| modal.restore_focus);

        self.focused = restore_focus.filter(|target| self.can_focus_without_trap(target));
        self.normalize_focus();
        self.focused.as_ref()
    }

    pub fn exit_modal(&mut self) -> Option<&ComponentId> {
        self.release_modal_trap()
    }

    pub fn modal_trap(&self) -> Option<&ModalTrap> {
        self.modal_trap.as_ref()
    }

    pub fn modal_id(&self) -> Option<&ComponentId> {
        self.modal_trap.as_ref().map(|trap| &trap.modal)
    }

    pub fn is_modal_active(&self) -> bool {
        self.modal_trap.is_some()
    }

    pub fn nodes(&self) -> &[FocusNode] {
        &self.order
    }

    fn normalize_focus(&mut self) {
        if self
            .focused
            .as_ref()
            .is_some_and(|focused| self.is_in_active_scope(focused) && self.is_enabled(focused))
        {
            return;
        }

        self.focused = self.enabled_active_order().into_iter().next();
    }

    fn enabled_active_order(&self) -> Vec<ComponentId> {
        self.active_order()
            .into_iter()
            .filter(|id| self.is_enabled(id))
            .cloned()
            .collect()
    }

    fn active_order(&self) -> Vec<&ComponentId> {
        if let Some(trap) = &self.modal_trap {
            return trap.focus_order.iter().collect();
        }

        self.order
            .iter()
            .filter(|node| node.scope == FocusScope::Global)
            .map(|node| &node.id)
            .collect()
    }

    fn is_in_active_scope(&self, id: &ComponentId) -> bool {
        self.active_order().into_iter().any(|target| target == id)
    }

    fn is_enabled(&self, id: &ComponentId) -> bool {
        self.order.iter().any(|node| &node.id == id && node.enabled)
    }

    fn can_focus_without_trap(&self, id: &ComponentId) -> bool {
        self.order
            .iter()
            .any(|node| &node.id == id && node.scope == FocusScope::Global && node.enabled)
    }
}

fn validate_focus_scope(
    focus_order: &[ComponentId],
    order: &[FocusNode],
) -> Result<(), FocusError> {
    if focus_order.is_empty() {
        return Err(FocusError::EmptyFocusScope);
    }

    let mut seen = BTreeSet::new();
    for target in focus_order {
        if !seen.insert(target.clone()) {
            return Err(FocusError::DuplicateTarget(target.clone()));
        }
    }

    let known = order.iter().map(|node| &node.id).collect::<BTreeSet<_>>();
    let unknown = focus_order
        .iter()
        .filter(|target| !known.contains(target))
        .min()
        .cloned();
    if let Some(target) = unknown {
        return Err(FocusError::UnknownTarget(target));
    }

    Ok(())
}
