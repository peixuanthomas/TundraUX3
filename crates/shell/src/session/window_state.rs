use crate::{InputKey, InputPhase, KeyInput};
use std::collections::HashSet;
use workspace_protocol::SessionId;

#[derive(Default)]
pub(super) struct WindowSelection {
    requested: Vec<SessionId>,
    minimized: HashSet<SessionId>,
    focused: Option<SessionId>,
    maximized: Option<SessionId>,
    adding: bool,
}

impl WindowSelection {
    pub(super) fn focused(&self) -> Option<SessionId> {
        self.focused
    }

    pub(super) fn includes(&self, id: SessionId) -> bool {
        self.requested.contains(&id) && !self.minimized.contains(&id)
    }

    pub(super) fn maximized(&self) -> Option<SessionId> {
        self.maximized
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
            .filter(|id| !self.minimized.contains(id))
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
            if self.minimized.remove(&id) {
                self.requested.retain(|&window| window != id);
            }
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

    pub(super) fn activate(&mut self, id: SessionId, capacity: usize) {
        if self.requested.len() - self.minimized.len() < capacity.max(1) {
            self.minimized.remove(&id);
        }
        self.switch(id);
    }

    pub(super) fn restore(&mut self, id: SessionId, capacity: usize) {
        if self.adding {
            self.show(id);
        } else {
            self.activate(id, capacity);
        }
    }

    pub(super) fn focus(&mut self, id: SessionId) {
        if self.includes(id) {
            self.focused = Some(id);
        }
    }

    pub(super) fn show(&mut self, id: SessionId) {
        self.adding = false;
        self.maximized = None;
        self.minimized.remove(&id);
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

    pub(super) fn minimize(&mut self, id: SessionId) {
        let Some(index) = self.requested.iter().position(|candidate| *candidate == id) else {
            return;
        };
        self.minimized.insert(id);
        if self.maximized == Some(id) {
            self.maximized = None;
        }
        if self.focused == Some(id) {
            self.focused = self.requested[index + 1..]
                .iter()
                .chain(self.requested[..index].iter().rev())
                .copied()
                .find(|candidate| !self.minimized.contains(candidate));
        }
    }

    pub(super) fn remove(&mut self, id: SessionId) {
        self.minimize(id);
        self.minimized.remove(&id);
        self.requested.retain(|&window| window != id);
    }

    pub(super) fn restore_split(&mut self) {
        self.maximized = None;
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
