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
pub struct ShortcutBinding {
    pub scope: ShortcutScope,
    pub key: KeyStroke,
    pub command: Command,
}

impl ShortcutBinding {
    pub fn new(scope: ShortcutScope, key: KeyStroke, command: Command) -> Self {
        Self {
            scope,
            key,
            command,
        }
    }

    pub fn global(key: KeyStroke, command: Command) -> Self {
        Self::new(ShortcutScope::Global, key, command)
    }

    pub fn local(scope: impl Into<UiId>, key: KeyStroke, command: Command) -> Self {
        Self::new(ShortcutScope::local(scope), key, command)
    }

    pub fn overlay(scope: impl Into<String>, key: KeyStroke, command: Command) -> Self {
        Self::new(ShortcutScope::overlay(scope), key, command)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShortcutConflictKind {
    DuplicateBinding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutConflict {
    pub kind: ShortcutConflictKind,
    pub scope: ShortcutScope,
    pub key: KeyStroke,
    pub existing: Command,
    pub attempted: Command,
}

pub type ShortcutConflictError = ShortcutConflict;

impl fmt::Display for ShortcutConflict {
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

impl Error for ShortcutConflict {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShortcutRegistry {
    bindings: Vec<ShortcutBinding>,
}

impl ShortcutRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_bindings<I>(bindings: I) -> Result<Self, ShortcutConflict>
    where
        I: IntoIterator<Item = ShortcutBinding>,
    {
        let mut registry = Self::new();
        for binding in bindings {
            registry.register(binding)?;
        }
        Ok(registry)
    }

    pub fn register(&mut self, binding: ShortcutBinding) -> Result<(), ShortcutConflict> {
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
        self.bindings.sort();
        Ok(())
    }

    pub fn command_for(&self, scopes: &[ShortcutScope], key: &KeyStroke) -> Option<&Command> {
        scopes.iter().find_map(|scope| {
            self.bindings
                .iter()
                .find(|binding| &binding.scope == scope && &binding.key == key)
                .map(|binding| &binding.command)
        })
    }

    pub fn resolve(&self, local_scope: Option<&UiId>, event: &InputEvent) -> Option<&Command> {
        self.resolve_binding(local_scope, event)
            .map(|binding| &binding.command)
    }

    pub fn resolve_binding(
        &self,
        local_scope: Option<&UiId>,
        event: &InputEvent,
    ) -> Option<&ShortcutBinding> {
        let InputEvent::Key(key_event) = event else {
            return None;
        };
        let key = key_event.stroke();

        if let Some(scope) = local_scope {
            if let Some(binding) = self.existing_binding(&ShortcutScope::Local(scope.clone()), &key)
            {
                return Some(binding);
            }
        }

        self.existing_binding(&ShortcutScope::Global, &key)
    }

    pub fn bindings(&self) -> &[ShortcutBinding] {
        &self.bindings
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    fn existing_binding(&self, scope: &ShortcutScope, key: &KeyStroke) -> Option<&ShortcutBinding> {
        self.bindings
            .iter()
            .find(|binding| &binding.scope == scope && &binding.key == key)
    }
}
