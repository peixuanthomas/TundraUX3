use super::*;

impl Workspace {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::session) fn render_panes(
        &mut self,
        frame: &mut Frame<'_>,
        layout: ui::ShellFrameLayout,
        context: &ui::RenderContext,
        capture: Option<&ui::automation::FrameCapture>,
        controls_blocked: bool,
        picker: Option<&ui::EditorImagePicker>,
        image_caches: &mut std::collections::HashMap<SessionId, ui::TerminalImageCache>,
        image_cleanup: &mut Vec<u8>,
        text_sizing: bool,
    ) {
        let mut cached_images = HashSet::new();
        let controls_blocked = controls_blocked || self.has_overlay();
        if let Some(capture) = capture {
            capture.set_group("workspace", "toolbar", layout.main, controls_blocked);
        }
        if self.layout.blocked {
            let pane = self.blocked_pane();
            if let Some(pane) = &pane {
                self.render_pane_frame(frame, pane, context, controls_blocked);
            }
            ui::render_workspace_resize_notice(
                frame,
                pane.map_or(layout.main, |pane| pane.content),
                Size::new(self.width, self.height),
                self.layout.size_needed,
                context,
            );
        }
        for pane in &self.layout.panes {
            self.render_pane_frame(frame, pane, context, controls_blocked);
        }
        for pane in &self.layout.panes {
            let window = &self.windows[&pane.id];
            let focused = Some(pane.id) == self.selection.focused();
            if let Some(capture) = capture {
                let group = format!("session/{}", pane.id);
                capture.set_group(
                    if focused { "page" } else { &group },
                    "page",
                    pane.content,
                    controls_blocked,
                );
            }
            let placeholder = if window.state == ProcessState::Unresponsive
                || window.status.unresponsive
            {
                Some(WorkspacePanePlaceholder::Unresponsive)
            } else if matches!(window.state, ProcessState::ForceStopping)
                || matches!(
                    window.close,
                    CloseProgress::Cleaning(_) | CloseProgress::Waiting | CloseProgress::Exiting
                )
            {
                Some(WorkspacePanePlaceholder::Closing)
            } else {
                None
            };
            if let Some(placeholder) = placeholder {
                ui::render_workspace_pane_placeholder(frame, pane.content, placeholder, context);
                continue;
            }
            if let Some(rendered) = window.frame.as_ref().filter(|rendered| {
                rendered.size == Size::new(pane.content.width, pane.content.height)
            }) {
                ui::render_workspace_frame(
                    frame,
                    pane.content,
                    rendered,
                    context,
                    !controls_blocked && rendered.layout == self.generation,
                );
                if let Some(picker) = picker
                    && rendered.layout == self.generation
                    && !rendered.modal
                    && !rendered.images.is_empty()
                {
                    image_caches.entry(pane.id).or_default().render_workspace(
                        frame,
                        pane.content,
                        &rendered.images,
                        picker,
                    );
                    cached_images.insert(pane.id);
                }
                if text_sizing && rendered.layout == self.generation && !rendered.modal {
                    ui::components::render_workspace_headings(
                        frame,
                        pane.content,
                        &rendered.headings,
                        context,
                    );
                }
                if !controls_blocked
                    && Some(pane.id) == self.input_focus
                    && rendered.layout == self.generation
                    && let Some(cursor) = rendered.cursor
                {
                    frame.set_cursor_position((
                        pane.content.x + cursor.x,
                        pane.content.y + cursor.y,
                    ));
                }
            } else {
                ui::render_workspace_pane_placeholder(
                    frame,
                    pane.content,
                    WorkspacePanePlaceholder::Starting,
                    context,
                );
            }
        }
        image_caches.retain(|id, cache| {
            if cached_images.contains(id) {
                true
            } else {
                cache.clear();
                image_cleanup.extend(cache.take_cleanup());
                false
            }
        });
    }

    fn render_pane_frame(
        &self,
        frame: &mut Frame<'_>,
        pane: &WorkspacePaneLayout,
        context: &ui::RenderContext,
        controls_blocked: bool,
    ) {
        let interaction = WorkspaceWindowButtonsState {
            hovered: self
                .last_pointer
                .filter(|_| self.pointer_hovered && !controls_blocked)
                .and_then(|point| pane.controls.hit_at(point)),
            pressed: self.capture.and_then(|capture| match capture.target {
                PointerTarget::WindowButton(id, action) if id == pane.id => Some(action),
                _ => None,
            }),
            close_disabled: self.windows[&pane.id].close != CloseProgress::Idle,
            ..Default::default()
        };
        ui::render_workspace_pane_frame(
            frame,
            pane,
            &self.windows[&pane.id].launch.title(),
            self.selection.focused() == Some(pane.id),
            self.selection.maximized() == Some(pane.id),
            interaction,
            context,
        );
    }

    fn taskbar_items(&self) -> Vec<WorkspaceTaskbarItem> {
        let blocked_window = self.blocked_pane().map(|pane| pane.id);
        self.windows
            .iter()
            .map(|(&id, window)| WorkspaceTaskbarItem {
                id,
                title: window.launch.title(),
                focused: self.selection.focused() == Some(id),
                minimized: blocked_window != Some(id)
                    && !self.layout.panes.iter().any(|pane| pane.id == id),
                closing: window.close != CloseProgress::Idle,
                unresponsive: window.state == ProcessState::Unresponsive
                    || window.status.unresponsive,
            })
            .collect()
    }

    pub(in crate::session) fn render_taskbar(
        &mut self,
        frame: &mut Frame<'_>,
        area: Option<Rect>,
        context: &ui::RenderContext,
        capture: Option<&ui::automation::FrameCapture>,
        controls_blocked: bool,
    ) {
        let Some(area) = area else {
            self.taskbar = None;
            return;
        };
        let items = self.taskbar_items();
        let focused = self.selection.focused();
        let mut taskbar = WorkspaceTaskbar::new(area, &items, self.taskbar_scroll);
        if (self.taskbar_focus != focused
            || self
                .taskbar
                .as_ref()
                .is_none_or(|previous| previous.area != area))
            && let Some(id) = focused
            && !taskbar
                .hits
                .iter()
                .any(|(action, _)| *action == WorkspaceTaskbarAction::Activate(id))
            && let Some(index) = items.iter().position(|item| item.id == id)
        {
            taskbar = WorkspaceTaskbar::new(area, &items, index);
        }
        if self
            .taskbar
            .as_ref()
            .is_some_and(|previous| previous.hits != taskbar.hits)
            && self
                .capture
                .is_some_and(|capture| matches!(capture.target, PointerTarget::Taskbar(_)))
        {
            self.cancel_capture();
        }
        self.taskbar_focus = focused;
        self.taskbar_scroll = taskbar.scroll;
        if let Some(capture) = capture {
            capture.set_group("workspace-taskbar", "toolbar", area, controls_blocked);
        }
        let hovered = self
            .last_pointer
            .filter(|_| self.pointer_hovered && !controls_blocked)
            .and_then(|point| taskbar.hit_at(point));
        let pressed = self.capture.and_then(|capture| match capture.target {
            PointerTarget::Taskbar(action) => Some(action),
            _ => None,
        });
        ui::render_workspace_taskbar(frame, &taskbar, &items, hovered, pressed, context);
        self.taskbar = Some(taskbar);
    }

    pub(in crate::session) fn render_overlay(
        &mut self,
        frame: &mut Frame<'_>,
        context: &ui::RenderContext,
        capture: Option<&ui::automation::FrameCapture>,
        global_modal: bool,
    ) {
        if global_modal {
            self.close_split_menu();
            return;
        }
        let available = self.layout.available;
        let hovered = self.last_pointer.filter(|_| self.pointer_hovered);
        let pressed = self.capture.and_then(|capture| match capture.target {
            PointerTarget::Overlay(action) => Some(action),
            _ => None,
        });
        if let Some(id) = self.wait_for
            && let Some(window) = self.windows.get(&id)
        {
            if let Some(capture) = capture {
                capture.set_group("workspace-overlay", "dialog", available, false);
            }
            let wait = WorkspaceCloseWait::new(
                available,
                &window.launch.title(),
                id,
                matches!(window.close, CloseProgress::Confirmation { .. }),
            );
            ui::render_workspace_close_wait(
                frame,
                &wait,
                self.modal_focus,
                hovered.and_then(|point| wait.hit_at(point)),
                pressed,
                context,
            );
            self.panel_hits = wait.hits;
        } else if let Some(model) = &mut self.panel {
            if let Some(capture) = capture {
                capture.set_group("workspace", "dialog", available, false);
            }
            model.windows = self
                .windows
                .iter()
                .map(|(&id, window)| WorkspaceWindowViewModel {
                    id,
                    title: window.launch.title(),
                    minimum_size: window.status.minimum,
                    visible: self.layout.panes.iter().any(|pane| pane.id == id),
                    unresponsive: window.state == ProcessState::Unresponsive
                        || window.status.unresponsive,
                    closing: window.close != CloseProgress::Idle,
                })
                .collect();
            model.focused = self.selection.focused();
            model.mode = self.mode;
            model.size_needed = self.layout.size_needed;
            model.capacity = self.layout.capacity;
            model.blocked = self.layout.blocked;
            let panel = WorkspacePanel::new(available, model);
            self.panel_scroll_rows = usize::from(panel.list.height);
            self.panel_max_scroll = panel.max_scroll;
            model.scroll = panel.scroll;
            model.hovered = hovered.and_then(|point| panel.hit_at(point));
            model.pressed = pressed;
            if self.panel_key_focus {
                ui::render_workspace_panel(frame, &panel, model, context);
            } else {
                let mut pointer = model.clone();
                pointer.action_focus = None;
                ui::render_workspace_panel(frame, &panel, &pointer, context);
            }
            self.panel_hits = panel.hits;
        } else {
            self.panel_hits.clear();
            self.render_split_menu(frame, context, capture);
        }
    }

    fn render_split_menu(
        &mut self,
        frame: &mut Frame<'_>,
        context: &ui::RenderContext,
        capture: Option<&ui::automation::FrameCapture>,
    ) {
        let Some(id) = self.split_window else {
            return;
        };
        let Some(pane) = self
            .layout
            .panes
            .iter()
            .find(|pane| pane.id == id)
            .copied()
            .or_else(|| self.blocked_pane().filter(|pane| pane.id == id))
        else {
            self.close_split_menu();
            return;
        };
        self.split_model.mode = self.mode;
        self.split_model.windows = self
            .taskbar_items()
            .into_iter()
            .filter(|item| item.id != id && item.minimized)
            .collect();
        let menu = WorkspaceSplitMenu::new(
            self.layout.available,
            pane.controls.split,
            &self.split_model,
        );
        if self
            .split_menu
            .as_ref()
            .is_some_and(|previous| previous.hits != menu.hits)
            && self
                .capture
                .is_some_and(|capture| matches!(capture.target, PointerTarget::Overlay(_)))
        {
            self.cancel_capture();
        }
        self.split_model.scroll = menu.scroll;
        self.split_model.hovered = self
            .last_pointer
            .filter(|_| self.pointer_hovered)
            .and_then(|point| menu.hit_at(point));
        self.split_model.pressed = self.capture.and_then(|capture| match capture.target {
            PointerTarget::Overlay(action) => Some(action),
            _ => None,
        });
        if let Some(capture) = capture {
            capture.set_group("workspace-split", "toolbar", menu.area, false);
        }
        if self.panel_key_focus {
            ui::render_workspace_split_menu(frame, &menu, &self.split_model, context);
        } else {
            let mut model = self.split_model.clone();
            model.focused = None;
            ui::render_workspace_split_menu(frame, &menu, &model, context);
        }
        self.split_menu = Some(menu);
    }

    pub(in crate::session) fn has_overlay(&self) -> bool {
        self.panel.is_some() || self.wait_for.is_some() || self.split_window.is_some()
    }
}
