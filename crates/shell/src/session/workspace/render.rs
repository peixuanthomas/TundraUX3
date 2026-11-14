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
        if self.layout.blocked {
            ui::render_workspace_resize_notice(
                frame,
                layout.main,
                Size::new(self.width, self.height),
                self.layout.size_needed,
                context,
            );
        }
        let controls_blocked = controls_blocked || self.panel.is_some() || self.wait_for.is_some();
        if let Some(capture) = capture {
            capture.set_group("workspace", "toolbar", layout.main, controls_blocked);
        }
        for pane in &self.layout.panes {
            let window = &self.windows[&pane.id];
            let focused = Some(pane.id) == self.selection.focused();
            ui::render_workspace_pane_title(frame, pane, &window.launch.title(), focused, context);
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

    pub(in crate::session) fn render_chrome(
        &self,
        frame: &mut Frame<'_>,
        layout: &ui::ShellFrameLayout,
        context: &ui::RenderContext,
    ) {
        let state = ui::components::ComponentState {
            hovered: self.last_pointer.is_some_and(|point| {
                layout
                    .windows_button
                    .is_some_and(|area| area.contains(point.into()))
            }),
            active: self.capture.is_some_and(|capture| {
                capture.target == PointerTarget::Chrome(ChromeAction::Windows)
            }),
            ..Default::default()
        };
        ui::render_workspace_chrome_button(frame, layout, context, state);
    }

    pub(in crate::session) fn render_overlay(
        &mut self,
        frame: &mut Frame<'_>,
        context: &ui::RenderContext,
        capture: Option<&ui::automation::FrameCapture>,
    ) {
        let available = self.layout.available;
        let hovered = self.last_pointer;
        let pressed = self.capture.and_then(|capture| match capture.target {
            PointerTarget::Overlay(action) => Some(action),
            _ => None,
        });
        if let Some(id) = self.wait_for
            && let Some(window) = self.windows.get(&id)
        {
            if let Some(capture) = capture {
                capture.set_group("workspace", "dialog", available, false);
            }
            let wait = WorkspaceCloseWait::new(available, &window.launch.title(), id);
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
        }
    }

    pub(in crate::session) fn has_overlay(&self) -> bool {
        self.panel.is_some() || self.wait_for.is_some()
    }
}
