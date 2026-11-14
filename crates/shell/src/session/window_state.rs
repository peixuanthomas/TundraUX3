use crate::{InputKey, InputPhase, KeyInput};
use workspace_protocol::SessionId;

#[derive(Default)]
pub(super) struct WindowSelection {
    requested: Vec<SessionId>,
    focused: Option<SessionId>,
    maximized: Option<SessionId>,
    adding: bool,
}

impl WindowSelection {
    pub(super) fn focused(&self) -> Option<SessionId> {
        self.focused
    }

    pub(super) fn includes(&self, id: SessionId) -> bool {
        self.requested.contains(&id)
    }

    pub(super) fn visible(&self, capacity: usize) -> Vec<SessionId> {
        if capacity == 0 || self.adding {
            return Vec::new();
        }
        if let Some(id) = self.maximized {
            return vec![id];
        }
        let mut visible = self
            .requested
            .iter()
            .copied()
            .take(capacity)
            .collect::<Vec<_>>();
        if let Some(focused) = self.focused
            && !visible.contains(&focused)
        {
            visible.pop();
            visible.push(focused);
        }
        visible
    }

    pub(super) fn open(&mut self, id: SessionId) {
        if self.adding {
            self.show(id);
        } else {
            self.switch(id);
        }
    }

    pub(super) fn switch(&mut self, id: SessionId) {
        self.adding = false;
        if !self.includes(id) {
            if let Some(slot) = self
                .requested
                .iter_mut()
                .find(|slot| Some(**slot) == self.focused)
            {
                *slot = id;
            } else {
                self.requested.push(id);
            }
        }
        if self.maximized.is_some() {
            self.maximized = Some(id);
        }
        self.focused = Some(id);
    }

    pub(super) fn focus(&mut self, id: SessionId) {
        if self.includes(id) {
            self.focused = Some(id);
        }
    }

    pub(super) fn show(&mut self, id: SessionId) {
        self.adding = false;
        self.maximized = None;
        if !self.includes(id) {
            self.requested.push(id);
        }
        self.focused = Some(id);
    }

    pub(super) fn add_from_home(&mut self) {
        self.adding = true;
    }

    pub(super) fn cancel_add(&mut self) -> bool {
        std::mem::take(&mut self.adding)
    }

    pub(super) fn background(&mut self, id: SessionId) {
        let Some(index) = self.requested.iter().position(|candidate| *candidate == id) else {
            return;
        };
        self.requested.remove(index);
        if self.maximized == Some(id) {
            self.maximized = None;
        }
        if self.focused == Some(id) {
            self.focused = self
                .requested
                .get(index)
                .or_else(|| self.requested.last())
                .copied();
        }
    }

    pub(super) fn toggle_maximized(&mut self, id: SessionId) {
        if self.maximized == Some(id) {
            self.maximized = None;
        } else {
            self.switch(id);
            self.maximized = Some(id);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum WindowShortcut {
    Pass,
    Consume,
    SendPrefix(KeyInput),
    Background,
    Panel,
    Layout,
    Maximize,
    MoveFocus(i16, i16),
    Close,
    Kill,
}

#[derive(Default)]
pub(super) struct WindowPrefix(Option<KeyInput>);

impl WindowPrefix {
    pub(super) fn clear(&mut self) {
        self.0 = None;
    }

    pub(super) fn route(&mut self, key: &KeyInput) -> WindowShortcut {
        let prefix =
            key.modifiers.is_control_only() && matches!(key.key, InputKey::Char('b' | 'B'));
        if key.phase != InputPhase::Press {
            return if prefix || self.0.is_some() {
                WindowShortcut::Consume
            } else {
                WindowShortcut::Pass
            };
        }
        let Some(first) = self.0.take() else {
            if prefix {
                self.0 = Some(key.clone());
                return WindowShortcut::Consume;
            }
            return WindowShortcut::Pass;
        };
        if prefix {
            return WindowShortcut::SendPrefix(first);
        }
        if key.has_non_shift_modifier() {
            return WindowShortcut::Pass;
        }
        match key.key {
            InputKey::Char('b' | 'B') => WindowShortcut::Background,
            InputKey::Char('w' | 'W') => WindowShortcut::Panel,
            InputKey::Char('l' | 'L') => WindowShortcut::Layout,
            InputKey::Char('z' | 'Z') => WindowShortcut::Maximize,
            InputKey::Char('q' | 'Q') => WindowShortcut::Close,
            InputKey::Char('X') => WindowShortcut::Kill,
            InputKey::Char('x') if key.modifiers.shift => WindowShortcut::Kill,
            InputKey::Left => WindowShortcut::MoveFocus(-1, 0),
            InputKey::Right => WindowShortcut::MoveFocus(1, 0),
            InputKey::Up => WindowShortcut::MoveFocus(0, -1),
            InputKey::Down => WindowShortcut::MoveFocus(0, 1),
            InputKey::Escape => WindowShortcut::Consume,
            _ => WindowShortcut::Pass,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session/window_state.rs"]
mod tests;
