use super::*;
use ui::components::{ComponentEvent, ComponentTone, ContextMenu, ContextMenuItem};

pub(super) struct TaskbarMenu {
    pub(super) window: SessionId,
    pub(super) area: Rect,
    pub(super) menu: ContextMenu,
}

impl Workspace {
    pub(super) fn open_taskbar_menu(
        &mut self,
        id: SessionId,
        point: CellPosition,
        state: &ShellSession,
    ) {
        let Some(window) = self.windows.get(&id) else {
            return;
        };
        let entry = window.launch.launcher_id();
        let stored = entry.and_then(|id| state.current_launcher_entry(id));
        let mut items = vec![
            ContextMenuItem::new("open", i18n::tr!("ui-launcher-open")),
            ContextMenuItem::new("new-session", i18n::tr!("ui-workspace-new-session")).disabled(
                entry.is_some()
                    && (!state.can_execute_command_line()
                        || !stored.is_some_and(|entry| {
                            entry.custom.is_some() || entry.terminal.is_some()
                        })),
            ),
            ContextMenuItem::new("move", i18n::tr!("shell-move"))
                .disabled(self.taskbar_order.len() < 2),
        ];
        if stored.is_some() && state.can_manage_launcher() {
            items.push(
                ContextMenuItem::new("delete", i18n::tr!("shell-delete"))
                    .disabled(window.close != CloseProgress::Idle)
                    .tone(ComponentTone::Danger),
            );
        }
        self.set_taskbar_menu(id, point, items);
    }

    fn set_taskbar_menu(
        &mut self,
        window: SessionId,
        anchor: CellPosition,
        items: Vec<ContextMenuItem>,
    ) {
        let mut menu = ContextMenu::new("workspace.taskbar.menu", items);
        let area = menu.preferred_area(anchor.0, anchor.1, self.layout.available);
        menu.open();
        self.taskbar_menu = Some(TaskbarMenu { window, area, menu });
    }

    pub(super) fn handle_taskbar_menu(
        &mut self,
        input: &InputEvent,
        state: &mut ShellSession,
    ) -> io::Result<()> {
        let menu = self.taskbar_menu.as_mut().expect("open taskbar menu");
        if let InputEvent::Mouse(mouse) = input {
            self.last_pointer = Some(mouse.coordinates());
            self.pointer_hovered = false;
        }
        let mut input = input.clone();
        if let InputEvent::Key(key) = &mut input {
            if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
                return Ok(());
            }
            match key.key {
                InputKey::Tab => key.key = InputKey::Down,
                InputKey::BackTab => key.key = InputKey::Up,
                _ => {}
            }
        }
        let event = menu.menu.handle_event(input, menu.area);
        if !menu.menu.open {
            let menu = self.taskbar_menu.take().expect("open taskbar menu");
            if let ComponentEvent::Activated(action) = event {
                self.activate_taskbar_menu(menu.window, menu.area.x, action.as_str(), state)?;
            }
        }
        Ok(())
    }

    fn activate_taskbar_menu(
        &mut self,
        id: SessionId,
        anchor_x: u16,
        action: &str,
        state: &mut ShellSession,
    ) -> io::Result<()> {
        let Some(window) = self.windows.get(&id) else {
            return Ok(());
        };
        match action {
            "open" => self.selection.restore(id, self.layout.capacity),
            "new-session" if self.pending_global.is_none() => {
                if let Some(entry) = window.launch.launcher_id() {
                    state.open_taskbar_launcher_session(
                        LauncherCommand::RequestLaunch(entry.to_owned()),
                        platform::native_platform().as_ref(),
                    );
                } else {
                    self.open_requested(
                        window.launch.for_new_session(),
                        true,
                        state.auth_session(),
                    )?;
                }
            }
            "move" => {
                let Some(index) = self.taskbar_order.iter().position(|&item| item == id) else {
                    return Ok(());
                };
                self.set_taskbar_menu(
                    id,
                    (anchor_x, self.layout.available.bottom()),
                    vec![
                        ContextMenuItem::new("left", i18n::tr!("ui-workspace-move-left"))
                            .disabled(index == 0),
                        ContextMenuItem::new("right", i18n::tr!("ui-workspace-move-right"))
                            .disabled(index + 1 == self.taskbar_order.len()),
                    ],
                );
            }
            "left" | "right" => {
                let Some(index) = self.taskbar_order.iter().position(|&item| item == id) else {
                    return Ok(());
                };
                let destination = if action == "left" {
                    index.checked_sub(1)
                } else {
                    Some(index + 1)
                };
                if let Some(destination) =
                    destination.filter(|&index| index < self.taskbar_order.len())
                {
                    self.taskbar_order.swap(index, destination);
                    self.taskbar_scroll = self.taskbar_scroll.min(destination);
                }
            }
            "delete" => {
                if let Some(entry_id) = window.launch.launcher_id() {
                    state.confirm_taskbar_launcher_removal(id, entry_id);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
