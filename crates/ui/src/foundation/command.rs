use std::error::Error;
use std::fmt;

use crate::input::{InputEvent, KeyStroke, UiId};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Command {
    Noop,
    Confirm,
    Cancel,
    Open,
    Close,
    Back,
    Select,
    FocusNext,
    FocusPrevious,
    ContextMenu,
    OpenCommandPalette,
    OpenExitConfirm,
    Copy,
    Cut,
    Paste,
    Delete,
    Rename,
    NewFile,
    NewFolder,
    Save,
    SaveAs,
    Shutdown,
    Quit,
    Custom(String),
}

impl Command {
    pub fn custom(name: impl Into<String>) -> Self {
        Self::Custom(name.into())
    }
}

pub type KeyChord = KeyStroke;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShortcutScope {
    Global,
    Local(UiId),
    Overlay(String),
}

impl ShortcutScope {
    pub fn local(scope: impl Into<UiId>) -> Self {
        Self::Local(scope.into())
    }

    pub fn overlay(scope: impl Into<String>) -> Self {
        Self::Overlay(scope.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShortcutBinding<T = Command> {
    pub scope: ShortcutScope,
    pub key: KeyStroke,
    pub command: T,
}

impl<T> ShortcutBinding<T> {
    pub fn new(scope: ShortcutScope, key: KeyStroke, command: T) -> Self {
        Self {
            scope,
            key,
            command,
        }
    }

    pub fn global(key: KeyStroke, command: T) -> Self {
        Self::new(ShortcutScope::Global, key, command)
    }

    pub fn local(scope: impl Into<UiId>, key: KeyStroke, command: T) -> Self {
        Self::new(ShortcutScope::local(scope), key, command)
    }

    pub fn overlay(scope: impl Into<String>, key: KeyStroke, command: T) -> Self {
        Self::new(ShortcutScope::overlay(scope), key, command)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShortcutConflictKind {
    DuplicateBinding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutConflict<T = Command> {
    pub kind: ShortcutConflictKind,
    pub scope: ShortcutScope,
    pub key: KeyStroke,
    pub existing: T,
    pub attempted: T,
}

pub type ShortcutConflictError<T = Command> = ShortcutConflict<T>;

impl<T: fmt::Debug> fmt::Display for ShortcutConflict<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "shortcut conflict for {} in {:?}: {:?} already registered, attempted {:?}",
            self.key.label(),
            self.scope,
            self.existing,
            self.attempted
        )
    }
}

impl<T: fmt::Debug> Error for ShortcutConflict<T> {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutRegistry<T = Command> {
    bindings: Vec<ShortcutBinding<T>>,
}

impl<T> Default for ShortcutRegistry<T> {
    fn default() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }
}

impl<T> ShortcutRegistry<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_bindings<I>(bindings: I) -> Result<Self, ShortcutConflict<T>>
    where
        I: IntoIterator<Item = ShortcutBinding<T>>,
        T: Clone,
    {
        let mut registry = Self::new();
        for binding in bindings {
            registry.register(binding)?;
        }
        Ok(registry)
    }

    pub fn register(&mut self, binding: ShortcutBinding<T>) -> Result<(), ShortcutConflict<T>>
    where
        T: Clone,
    {
        if let Some(existing) = self.existing_binding(&binding.scope, &binding.key) {
            return Err(ShortcutConflict {
                kind: ShortcutConflictKind::DuplicateBinding,
                scope: binding.scope,
                key: binding.key,
                existing: existing.command.clone(),
                attempted: binding.command,
            });
        }

        self.bindings.push(binding);
        self.bindings
            .sort_by(|left, right| left.scope.cmp(&right.scope).then(left.key.cmp(&right.key)));
        Ok(())
    }

    pub fn command_for(&self, scopes: &[ShortcutScope], key: &KeyStroke) -> Option<&T> {
        scopes.iter().find_map(|scope| {
            self.bindings
                .iter()
                .find(|binding| &binding.scope == scope && &binding.key == key)
                .map(|binding| &binding.command)
        })
    }

    pub fn resolve(&self, local_scope: Option<&UiId>, event: &InputEvent) -> Option<&T> {
        self.resolve_binding(local_scope, event)
            .map(|binding| &binding.command)
    }

    pub fn resolve_binding(
        &self,
        local_scope: Option<&UiId>,
        event: &InputEvent,
    ) -> Option<&ShortcutBinding<T>> {
        let InputEvent::Key(key_event) = event else {
            return None;
        };
        if !key_event.is_press_like() {
            return None;
        }
        let key = key_event.stroke();

        if let Some(scope) = local_scope
            && let Some(binding) = self.existing_binding(&ShortcutScope::Local(scope.clone()), &key)
        {
            return Some(binding);
        }

        self.existing_binding(&ShortcutScope::Global, &key)
    }

    pub fn bindings(&self) -> &[ShortcutBinding<T>] {
        &self.bindings
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    fn existing_binding(
        &self,
        scope: &ShortcutScope,
        key: &KeyStroke,
    ) -> Option<&ShortcutBinding<T>> {
        self.bindings
            .iter()
            .find(|binding| &binding.scope == scope && &binding.key == key)
    }
}
