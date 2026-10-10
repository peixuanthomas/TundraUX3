use crate::session::overlays::ResolvedExplorerOverlay;
use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn route_explorer_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::Explorer);

        // Repeated action keys must not submit a dialog twice or open a file again while held.
        // Character/backspace repeats remain useful in name and search inputs.
        if key.phase != InputPhase::Press && matches!(key.key, InputKey::Enter | InputKey::Escape) {
            return (target, ShellCommand::CaptureOverlayInput);
        }

        if self.resolved_explorer_overlay().is_some()
            && let Some(command) = explorer_overlay_navigation_command(key)
        {
            return (target, command);
        }

        match self.resolved_explorer_overlay() {
            Some(ResolvedExplorerOverlay::RestoreConflict) => {
                if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
                    return (target, ShellCommand::CaptureOverlayInput);
                }
                return match &key.key {
                    InputKey::Enter | InputKey::Char(' ') if key.is_unmodified_action_key() => {
                        (target, ShellCommand::ExplorerOverlayActivate)
                    }
                    InputKey::Char('k' | 'K') => (target, ShellCommand::ExplorerRestoreKeepBoth),
                    InputKey::Char('r' | 'R') => (target, ShellCommand::ExplorerRestoreReplace),
                    InputKey::Escape | InputKey::Char('n' | 'N') => {
                        (target, ShellCommand::ExplorerRestoreCancel)
                    }
                    _ => (target, ShellCommand::CaptureOverlayInput),
                };
            }
            Some(ResolvedExplorerOverlay::OperationConflict) => {
                if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
                    return (target, ShellCommand::CaptureOverlayInput);
                }
                return match &key.key {
                    InputKey::Enter | InputKey::Char(' ') if key.is_unmodified_action_key() => {
                        (target, ShellCommand::ExplorerOverlayActivate)
                    }
                    InputKey::Char('k' | 'K') => (target, ShellCommand::ExplorerConflictKeepBoth),
                    InputKey::Char('r' | 'R') => (target, ShellCommand::ExplorerConflictReplace),
                    InputKey::Char('s' | 'S') => (target, ShellCommand::ExplorerConflictSkip),
                    InputKey::Char('a' | 'A') => {
                        (target, ShellCommand::ExplorerConflictToggleApplyToRemaining)
                    }
                    InputKey::Escape | InputKey::Char('n' | 'N') => {
                        (target, ShellCommand::ExplorerConflictCancel)
                    }
                    _ => (target, ShellCommand::CaptureOverlayInput),
                };
            }
            Some(ResolvedExplorerOverlay::Input(_)) => {
                return match &key.key {
                    InputKey::Escape => (target, ShellCommand::CancelExplorerInput),
                    InputKey::Enter if self.explorer_overlay_selection == 0 => {
                        (target, ShellCommand::SubmitExplorerInput)
                    }
                    InputKey::Enter | InputKey::Char(' ')
                        if self.explorer_overlay_selection != 0
                            && key.phase == InputPhase::Press
                            && key.is_unmodified_action_key() =>
                    {
                        (target, ShellCommand::ExplorerOverlayActivate)
                    }
                    InputKey::Backspace | InputKey::Delete
                        if self.explorer_overlay_selection == 0 =>
                    {
                        (target, ShellCommand::ExplorerBackspace)
                    }
                    InputKey::Char(character)
                        if self.explorer_overlay_selection == 0
                            && !key.has_non_shift_modifier() =>
                    {
                        (target, ShellCommand::AppendExplorerChar(*character))
                    }
                    _ => (target, ShellCommand::RecordInput),
                };
            }
            Some(ResolvedExplorerOverlay::Semantic(_)) => {
                return self.route_explorer_overlay_key(key);
            }
            Some(ResolvedExplorerOverlay::PendingDialog(kind)) => {
                let confirm = match kind {
                    app::explorer::ExplorerDialogKind::DeleteToTrash => {
                        ShellCommand::ExplorerConfirmDelete
                    }
                    app::explorer::ExplorerDialogKind::DumpTrash => {
                        ShellCommand::ExplorerConfirmDumpTrash
                    }
                };
                if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
                    return (target, ShellCommand::CaptureOverlayInput);
                }
                return match &key.key {
                    InputKey::Enter | InputKey::Char(' ') if key.is_unmodified_action_key() => (
                        target,
                        if self.explorer_overlay_selection == 0 {
                            confirm.clone()
                        } else {
                            ShellCommand::CancelExplorerInput
                        },
                    ),
                    InputKey::Escape if key.is_unmodified_action_key() => {
                        (target, ShellCommand::CancelExplorerInput)
                    }
                    InputKey::Char('y' | 'Y') => (target, confirm),
                    InputKey::Char('n' | 'N') => (target, ShellCommand::CancelExplorerInput),
                    _ => (target, ShellCommand::CaptureOverlayInput),
                };
            }
            None => {}
        }

        if matches!(
            self.explorer_input_mode,
            ExplorerInputMode::Address | ExplorerInputMode::Search
        ) {
            return match &key.key {
                InputKey::Escape => (target, ShellCommand::CancelExplorerInput),
                InputKey::Enter => (target, ShellCommand::SubmitExplorerInput),
                InputKey::Left
                | InputKey::Right
                | InputKey::Home
                | InputKey::End
                | InputKey::Delete
                    if self.explorer_input_mode == ExplorerInputMode::Address
                        && !key.has_non_shift_modifier() =>
                {
                    (
                        target,
                        ShellCommand::ExplorerEditAddress(InputEvent::Key(key.clone())),
                    )
                }
                InputKey::Char('a' | 'A')
                    if self.explorer_input_mode == ExplorerInputMode::Address
                        && (key.modifiers.control || key.modifiers.super_key)
                        && !key.modifiers.alt =>
                {
                    (
                        target,
                        ShellCommand::ExplorerEditAddress(InputEvent::Key(key.clone())),
                    )
                }
                InputKey::Backspace | InputKey::Delete => (target, ShellCommand::ExplorerBackspace),
                InputKey::Char(character) if !key.has_non_shift_modifier() => {
                    (target, ShellCommand::AppendExplorerChar(*character))
                }
                _ => (target, ShellCommand::RecordInput),
            };
        }

        let is_trash = self
            .app
            .explorer_state()
            .is_some_and(|state| state.current_location.is_trash());
        let quick_locations_visible = || {
            let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
            let ui::ShellLayout::Full { main, .. } = self.shell_layout_for(area) else {
                return false;
            };
            ui::explorer_layout(main, &self.to_explorer_view_model())
                .mode
                .shows_sidebar()
        };
        match &key.key {
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseExplorer),
            InputKey::BackTab if !key.has_non_shift_modifier() && quick_locations_visible() => {
                (target, ShellCommand::ExplorerPreviousQuickLocation)
            }
            InputKey::Tab
                if !key.has_non_shift_modifier()
                    && key.modifiers.shift
                    && quick_locations_visible() =>
            {
                (target, ShellCommand::ExplorerPreviousQuickLocation)
            }
            InputKey::Tab if !key.has_non_shift_modifier() && quick_locations_visible() => {
                (target, ShellCommand::ExplorerNextQuickLocation)
            }
            InputKey::Char('l' | 'L') if key.modifiers.control || key.modifiers.super_key => {
                (target, ShellCommand::BeginExplorerAddress)
            }
            InputKey::Left if key.is_unmodified_action_key() || key.modifiers.alt => {
                (target, ShellCommand::ExplorerOpenBack)
            }
            InputKey::Right if key.is_unmodified_action_key() || key.modifiers.alt => {
                (target, ShellCommand::ExplorerOpenForward)
            }
            InputKey::Up
            | InputKey::Down
            | InputKey::Home
            | InputKey::End
            | InputKey::PageUp
            | InputKey::PageDown
                if !key.modifiers.alt
                    && (key.modifiers.control
                        || key.modifiers.super_key
                        || matches!(
                            key.key,
                            InputKey::Home | InputKey::End | InputKey::PageUp | InputKey::PageDown
                        )) =>
            {
                use app::explorer::ExplorerSelectionMode;
                let toggle = key.modifiers.control || key.modifiers.super_key;
                let mode = match (key.modifiers.shift, toggle) {
                    (true, true) => ExplorerSelectionMode::AddRange,
                    (true, false) => ExplorerSelectionMode::Range,
                    (false, true) => ExplorerSelectionMode::FocusOnly,
                    (false, false) => ExplorerSelectionMode::Replace,
                };
                (
                    target,
                    ShellCommand::ExplorerNavigateSelection(key.key.clone(), mode),
                )
            }
            InputKey::F(10) if key.modifiers.shift && !key.has_non_shift_modifier() => {
                (target, ShellCommand::ExplorerContextMenu)
            }
            InputKey::Up if key.modifiers.alt => (target, ShellCommand::ExplorerOpenParent),
            InputKey::Up if key.modifiers.shift => (target, ShellCommand::ExplorerPreviousExtend),
            InputKey::Down if key.modifiers.shift => (target, ShellCommand::ExplorerNextExtend),
            InputKey::Up => (target, ShellCommand::ExplorerPrevious),
            InputKey::Down => (target, ShellCommand::ExplorerNext),
            InputKey::Enter if !is_trash => (target, ShellCommand::ExplorerOpenSelected),
            InputKey::Backspace => (target, ShellCommand::ExplorerOpenParent),
            InputKey::Delete if !is_trash => (target, ShellCommand::ExplorerDelete),
            InputKey::F(2) if !is_trash => (target, ShellCommand::BeginExplorerRename),
            InputKey::F(5) if key.is_unmodified_action_key() => (
                target,
                ShellCommand::ExplorerToolbarShortcut(ui::ExplorerToolbarAction::Refresh),
            ),
            InputKey::Delete if is_trash && key.is_unmodified_action_key() => (
                target,
                ShellCommand::ExplorerToolbarShortcut(ui::ExplorerToolbarAction::DumpTrash),
            ),
            InputKey::F(6) if key.phase == InputPhase::Press && key.is_unmodified_action_key() => (
                target,
                ShellCommand::ExplorerToolbarShortcut(ui::ExplorerToolbarAction::Sort),
            ),
            InputKey::Char('o' | 'O') if !key.has_non_shift_modifier() => (
                target,
                ShellCommand::ExplorerToolbarShortcut(ui::ExplorerToolbarAction::Options),
            ),
            InputKey::Char('a' | 'A')
                if key.modifiers.shift && (key.modifiers.control || key.modifiers.super_key) =>
            {
                (target, ShellCommand::ExplorerClearSelection)
            }
            InputKey::Char('i' | 'I') if key.modifiers.control || key.modifiers.super_key => {
                (target, ShellCommand::ExplorerInvertSelection)
            }
            InputKey::Char('a' | 'A') if key.modifiers.control || key.modifiers.super_key => {
                (target, ShellCommand::ExplorerSelectAll)
            }
            InputKey::Char('a' | 'A')
                if !is_trash && self.can_manage_launcher() && !key.has_non_shift_modifier() =>
            {
                (target, ShellCommand::ExplorerAddToLauncher)
            }
            InputKey::Char(' ') => (target, ShellCommand::ExplorerToggleFocused),
            InputKey::Char('f' | 'F') if key.modifiers.control || key.modifiers.super_key => {
                (target, ShellCommand::BeginExplorerSearch)
            }
            InputKey::Char('h' | 'H') if !key.has_non_shift_modifier() => {
                (target, ShellCommand::ExplorerToggleHidden)
            }
            InputKey::Char('r' | 'R') if is_trash && !key.has_non_shift_modifier() => {
                (target, ShellCommand::ExplorerRestore)
            }
            InputKey::Char('c' | 'C')
                if !is_trash
                    && !key.modifiers.alt
                    && !key.modifiers.hyper
                    && !key.modifiers.meta =>
            {
                (target, ShellCommand::ExplorerCopy)
            }
            InputKey::Char('x' | 'X')
                if !is_trash
                    && !key.modifiers.alt
                    && !key.modifiers.hyper
                    && !key.modifiers.meta =>
            {
                (target, ShellCommand::ExplorerCut)
            }
            InputKey::Char('v' | 'V')
                if !is_trash
                    && !key.modifiers.alt
                    && !key.modifiers.hyper
                    && !key.modifiers.meta =>
            {
                (target, ShellCommand::ExplorerPaste)
            }
            InputKey::Char('d' | 'D') if !is_trash && !key.has_non_shift_modifier() => {
                (target, ShellCommand::ExplorerDelete)
            }
            InputKey::Char('n' | 'N' | 'f' | 'F') if !is_trash && !key.has_non_shift_modifier() => {
                (target, ShellCommand::BeginExplorerNewFolder)
            }
            InputKey::Char('t' | 'T') if !is_trash && !key.has_non_shift_modifier() => {
                (target, ShellCommand::BeginExplorerNewTextFile)
            }
            InputKey::Char('r' | 'R') if !is_trash && !key.has_non_shift_modifier() => {
                (target, ShellCommand::BeginExplorerRename)
            }
            InputKey::Char('s' | 'S' | '/') if !key.has_non_shift_modifier() => {
                (target, ShellCommand::BeginExplorerSearch)
            }
            _ => (target, ShellCommand::RecordInput),
        }
    }

    pub(in crate::session) fn route_explorer_overlay_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::Explorer);
        if key.phase == InputPhase::Press
            && !key.modifiers.alt
            && let Some(ui::ExplorerOverlayViewModel::ContextMenu(menu)) =
                self.to_explorer_view_model().overlay
        {
            let command_modifier = key.modifiers.control || key.modifiers.super_key;
            let id = match key.key {
                InputKey::Char('a' | 'A') if command_modifier && key.modifiers.shift => {
                    Some("clear-selection")
                }
                InputKey::Char('a' | 'A') if command_modifier => Some("select-all"),
                InputKey::Char('i' | 'I') if command_modifier => Some("invert-selection"),
                InputKey::Char('c' | 'C') => Some("copy"),
                InputKey::Char('x' | 'X') => Some("cut"),
                InputKey::Char('v' | 'V') => Some("paste"),
                InputKey::Char('d' | 'D') if !key.has_non_shift_modifier() => Some("delete"),
                InputKey::F(2) if key.is_unmodified_action_key() => Some("rename"),
                InputKey::F(5) if key.is_unmodified_action_key() => Some("refresh"),
                InputKey::F(6) if key.is_unmodified_action_key() => Some("sort"),
                InputKey::Char('n' | 'N') if !key.has_non_shift_modifier() => Some("new-folder"),
                InputKey::Char('t' | 'T') if !key.has_non_shift_modifier() => Some("new-text"),
                InputKey::Char('o' | 'O') if !key.has_non_shift_modifier() => Some("options"),
                InputKey::Char('r' | 'R') if !key.has_non_shift_modifier() => Some("restore"),
                InputKey::Delete if key.is_unmodified_action_key() => {
                    Some(if menu.items.iter().any(|item| item.id == "dump-trash") {
                        "dump-trash"
                    } else {
                        "delete"
                    })
                }
                _ => None,
            };
            if let Some(index) = id.and_then(|id| {
                menu.items
                    .iter()
                    .position(|item| item.id == id && item.enabled)
            }) {
                return (target, ShellCommand::ExplorerContextItem(index));
            }
        }
        if let Some(command) = explorer_overlay_navigation_command(key) {
            return (target, command);
        }
        if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
            return (target, ShellCommand::CaptureOverlayInput);
        }
        let can_add_to_launcher = matches!(
            self.to_explorer_view_model().overlay.as_ref(),
            Some(ui::ExplorerOverlayViewModel::ContextMenu(menu))
                if menu.items.iter().any(|item| {
                    item.id == "add-to-launcher" && item.enabled
                })
        );
        match &key.key {
            InputKey::Escape => (target, ShellCommand::ClosePopup),
            InputKey::Enter | InputKey::Char(' ') => {
                (target, ShellCommand::ExplorerOverlayActivate)
            }
            InputKey::Char('a' | 'A') if can_add_to_launcher => {
                (target, ShellCommand::ExplorerAddToLauncher)
            }
            _ => (target, ShellCommand::CaptureOverlayInput),
        }
    }

    pub(in crate::session) fn clear_explorer_pointer_capture(&mut self) {
        let _ = self.update_explorer_state(|state| {
            state.drag = None;
        });
        self.clear_explorer_scrollbar_drag();
        self.last_click = None;
    }

    pub(in crate::session) fn route_explorer_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
        received_at: Instant,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();
        let target = RoutedTarget::Component(ShellComponent::Explorer);

        if matches!(mouse.kind, ui::MouseEventKind::Down(PointerButton::Left))
            && matches!(
                self.resolved_explorer_overlay(),
                Some(ResolvedExplorerOverlay::Semantic(
                    ExplorerOverlayMode::ContextMenu { .. }
                ))
            )
            && self.explorer_hit_target_at(coordinates).is_none()
        {
            self.clear_explorer_pointer_capture();
            return (target, ShellCommand::ClosePopup);
        }

        if hit_target != Some(ShellComponent::Explorer) {
            if matches!(
                mouse.kind,
                ui::MouseEventKind::Down(_)
                    | ui::MouseEventKind::Up(_)
                    | ui::MouseEventKind::Drag(_)
            ) {
                self.clear_explorer_pointer_capture();
            }
            return match mouse.kind {
                ui::MouseEventKind::Moved => {
                    (target_route(hit_target), ShellCommand::Hover(hit_target))
                }
                ui::MouseEventKind::Down(_)
                | ui::MouseEventKind::Up(_)
                | ui::MouseEventKind::Click(_)
                | ui::MouseEventKind::DoubleClick(_)
                | ui::MouseEventKind::Drag(_)
                | ui::MouseEventKind::Scroll(_) => {
                    (target_route(hit_target), ShellCommand::CaptureOverlayInput)
                }
            };
        }

        match mouse.kind {
            ui::MouseEventKind::Moved => {
                (target_route(hit_target), ShellCommand::Hover(hit_target))
            }
            ui::MouseEventKind::Down(PointerButton::Right) => {
                self.last_click = None;
                (
                    target,
                    ShellCommand::OpenContextMenu {
                        target: Some(ShellComponent::Explorer),
                        coordinates,
                    },
                )
            }
            ui::MouseEventKind::Down(PointerButton::Left) => {
                let click = self.register_click(
                    Some(ShellComponent::Explorer),
                    coordinates,
                    PointerButton::Left,
                    received_at,
                );
                (
                    target,
                    ShellCommand::ExplorerPointerDown(coordinates, click, mouse.modifiers),
                )
            }
            ui::MouseEventKind::Drag(PointerButton::Left) => (
                target,
                ShellCommand::ExplorerDragUpdate(coordinates, mouse.modifiers),
            ),
            ui::MouseEventKind::Up(PointerButton::Left) => (
                target,
                ShellCommand::ExplorerDrop(coordinates, mouse.modifiers),
            ),
            ui::MouseEventKind::Scroll(ScrollDirection::Up) => {
                (target, ShellCommand::ExplorerScroll(-3))
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Down) => {
                (target, ShellCommand::ExplorerScroll(3))
            }
            ui::MouseEventKind::Down(_)
            | ui::MouseEventKind::Up(_)
            | ui::MouseEventKind::Click(_)
            | ui::MouseEventKind::DoubleClick(_)
            | ui::MouseEventKind::Drag(_)
            | ui::MouseEventKind::Scroll(_) => (target, ShellCommand::RecordInput),
        }
    }
}

fn explorer_overlay_navigation_command(key: &KeyInput) -> Option<ShellCommand> {
    if key.phase == InputPhase::Release || key.has_non_shift_modifier() {
        return None;
    }
    match key.key {
        InputKey::BackTab => Some(ShellCommand::ExplorerOverlayPrevious),
        InputKey::Tab if key.modifiers.shift => Some(ShellCommand::ExplorerOverlayPrevious),
        InputKey::Tab => Some(ShellCommand::ExplorerOverlayNext),
        InputKey::Left | InputKey::Up if key.is_unmodified_action_key() => {
            Some(ShellCommand::ExplorerOverlayPrevious)
        }
        InputKey::Right | InputKey::Down if key.is_unmodified_action_key() => {
            Some(ShellCommand::ExplorerOverlayNext)
        }
        _ => None,
    }
}
