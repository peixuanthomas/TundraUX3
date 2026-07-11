use crate::{
    KeyBinding, KeyInput, ShellCommand, ShellScreen, ShellShortcut, ShortcutConflict, ShortcutScope,
};

pub fn default_shell_shortcuts() -> Vec<ShellShortcut> {
    vec![
        ShellShortcut {
            scope: ShortcutScope::Global,
            binding: KeyBinding::from(&KeyInput::from_label("Ctrl+C")),
            command: ShellCommand::Shutdown,
        },
        ShellShortcut {
            scope: ShortcutScope::Global,
            binding: KeyBinding::from(&KeyInput::from_label("Tab")),
            command: ShellCommand::FocusNext,
        },
        ShellShortcut {
            scope: ShortcutScope::Global,
            binding: KeyBinding::from(&KeyInput::from_label("Shift+Tab")),
            command: ShellCommand::FocusPrevious,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::Home),
            binding: KeyBinding::from(&KeyInput::from_label("q")),
            command: ShellCommand::RequestExit,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::Home),
            binding: KeyBinding::from(&KeyInput::from_label("Esc")),
            command: ShellCommand::RequestExit,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::Home),
            binding: KeyBinding::from(&KeyInput::from_label("L")),
            command: ShellCommand::Logout,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::Login),
            binding: KeyBinding::from(&KeyInput::from_label("F2")),
            command: ShellCommand::ToggleLoginPasswordVisibility,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::ExitConfirm),
            binding: KeyBinding::from(&KeyInput::from_label("y")),
            command: ShellCommand::ConfirmExit,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::ExitConfirm),
            binding: KeyBinding::from(&KeyInput::from_label("Y")),
            command: ShellCommand::ConfirmExit,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::ExitConfirm),
            binding: KeyBinding::from(&KeyInput::from_label("Enter")),
            command: ShellCommand::ConfirmExit,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::ExitConfirm),
            binding: KeyBinding::from(&KeyInput::from_label("n")),
            command: ShellCommand::CancelExit,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::ExitConfirm),
            binding: KeyBinding::from(&KeyInput::from_label("N")),
            command: ShellCommand::CancelExit,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::ExitConfirm),
            binding: KeyBinding::from(&KeyInput::from_label("Esc")),
            command: ShellCommand::CancelExit,
        },
    ]
}

pub fn detect_shortcut_conflicts(shortcuts: &[ShellShortcut]) -> Vec<ShortcutConflict> {
    let mut conflicts = Vec::new();

    for (index, first) in shortcuts.iter().enumerate() {
        for second in shortcuts.iter().skip(index + 1) {
            if first.scope == second.scope
                && first.binding == second.binding
                && first.command != second.command
            {
                conflicts.push(ShortcutConflict {
                    scope: first.scope.clone(),
                    binding: first.binding.clone(),
                    first: first.command.clone(),
                    second: second.command.clone(),
                });
            }
        }
    }

    conflicts
}
