//! Page composition and conversation state. Existing UI components own editing,
//! hit testing, focus rendering and button activation.
use crate::channel::{ClientMessage, PamStyle, ServerMessage};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    widgets::{Paragraph, Wrap},
};
use ui::{
    InputEvent, Key, TundraTheme,
    components::{Button, ComponentEvent, Dialog, DialogAction, TextInput},
};
use unicode_width::UnicodeWidthStr;

const MAX_RESPONSE_BYTES: usize = 4096;

#[derive(Clone, Copy)]
enum Page {
    Waiting,
    Login,
    Prompt { id: u64, style: PamStyle },
    Consent { id: u64 },
    Locked,
    Complete,
}

pub struct Greeter {
    page: Page,
    input: TextInput,
    submit: Button,
    cancel: Button,
    dialog: Dialog,
    title: String,
    description: String,
    focus: usize,
    consent_fits: bool,
    theme: TundraTheme,
}

impl Default for Greeter {
    fn default() -> Self {
        Self {
            page: Page::Waiting,
            input: TextInput::new("response"),
            submit: Button::new("submit", i18n::tr!("greeter-continue")),
            cancel: Button::new("cancel", i18n::tr!("greeter-cancel")),
            dialog: Dialog::new("trusted-dialog", "", "", vec![]),
            title: i18n::tr!("greeter-title"),
            description: i18n::tr!("greeter-waiting"),
            focus: 0,
            consent_fits: false,
            theme: TundraTheme::default(),
        }
    }
}

impl Greeter {
    /// A new service message always discards all editable state and approvals.
    pub fn receive(&mut self, message: ServerMessage) {
        self.input.set_value("");
        self.input.set_secret(false);
        self.focus = 0;
        self.dialog.close();
        self.consent_fits = false;
        self.submit = Button::new("submit", i18n::tr!("greeter-continue"));
        self.cancel = Button::new("cancel", i18n::tr!("greeter-cancel"));
        match message {
            ServerMessage::Login { message } => {
                self.page = Page::Login;
                self.title = i18n::tr!("greeter-title");
                self.description = display_text(message.as_deref().unwrap_or(""));
                self.input.placeholder = i18n::tr!("greeter-username");
                self.submit.label = i18n::tr!("greeter-login");
            }
            ServerMessage::PamPrompt { id, style, text } => {
                self.page = Page::Prompt { id, style };
                self.title = i18n::tr!("greeter-authentication");
                self.description = display_text(&text);
                self.input.placeholder.clear();
                self.input.set_secret(style == PamStyle::EchoOff);
            }
            ServerMessage::Consent {
                id,
                title,
                description,
            } => {
                self.page = Page::Consent { id };
                self.title = display_text(&title);
                self.description = display_text(&description);
                self.dialog = Dialog::new(
                    "consent",
                    self.title.clone(),
                    "",
                    vec![DialogAction::new("cancel", i18n::tr!("greeter-cancel"))],
                );
                self.dialog.open();
            }
            ServerMessage::Locked { username } => {
                self.page = Page::Locked;
                self.title = i18n::tr!("greeter-locked", username = display_text(&username));
                self.description = String::new();
                self.dialog = Dialog::new(
                    "locked",
                    self.title.clone(),
                    "",
                    vec![
                        DialogAction::new("unlock", i18n::tr!("greeter-unlock")),
                        DialogAction::new("logout", i18n::tr!("greeter-logout")),
                    ],
                );
                self.dialog.open();
            }
            ServerMessage::Complete {} => {
                self.page = Page::Complete;
                self.description = i18n::tr!("greeter-complete");
            }
        }
        self.sync_focus();
    }

    pub fn is_complete(&self) -> bool {
        matches!(self.page, Page::Complete)
    }

    fn editable(&self) -> bool {
        matches!(
            self.page,
            Page::Login
                | Page::Prompt {
                    style: PamStyle::EchoOn | PamStyle::EchoOff,
                    ..
                }
        )
    }

    fn sync_focus(&mut self) {
        self.input.set_focused(self.focus == 0 && self.editable());
        self.submit
            .set_focused(self.focus == if self.editable() { 1 } else { 0 });
        self.cancel
            .set_focused(self.focus == if self.editable() { 2 } else { 1 });
    }

    fn submit(&mut self) -> Option<ClientMessage> {
        match self.page {
            Page::Login if self.input.value().trim().is_empty() => None,
            Page::Login => Some(ClientMessage::Login {
                username: self.input.take_value(),
            }),
            Page::Prompt { id, style } => Some(ClientMessage::PamResponse {
                id,
                response: if matches!(style, PamStyle::EchoOn | PamStyle::EchoOff) {
                    self.input.take_value()
                } else {
                    String::new()
                },
            }),
            _ => None,
        }
    }

    pub fn handle(&mut self, event: InputEvent, area: Rect) -> Option<ClientMessage> {
        if matches!(self.page, Page::Waiting | Page::Complete) {
            return None;
        }
        let response = self.handle_active(event, area);
        if response.is_some() {
            self.input.set_value("");
            self.dialog.close();
            self.page = Page::Waiting;
            self.description = i18n::tr!("greeter-waiting");
        }
        response
    }

    fn handle_active(&mut self, event: InputEvent, area: Rect) -> Option<ClientMessage> {
        let areas = page_areas(area);
        if let Page::Consent { id } = self.page {
            self.layout_consent(areas.panel);
            return match self.dialog.handle_event(event, areas.panel) {
                ComponentEvent::Activated(action)
                    if action.as_str() == "confirm" && self.consent_fits =>
                {
                    Some(ClientMessage::Consent { id, approved: true })
                }
                ComponentEvent::Activated(_) | ComponentEvent::Dismissed(_) => {
                    Some(ClientMessage::Consent {
                        id,
                        approved: false,
                    })
                }
                _ => None,
            };
        }
        if matches!(self.page, Page::Locked) {
            return match self.dialog.handle_event(event, areas.panel) {
                ComponentEvent::Activated(action) if action.as_str() == "unlock" => {
                    Some(ClientMessage::Unlock {})
                }
                ComponentEvent::Activated(action) if action.as_str() == "logout" => {
                    Some(ClientMessage::Logout {})
                }
                // Escape must not close a locked view or imply cancellation of the lock.
                ComponentEvent::Dismissed(_) => {
                    self.dialog.open();
                    None
                }
                _ => None,
            };
        }
        if let InputEvent::Key(key) = &event {
            if !key.is_press_like() {
                return None;
            }
            match key.key {
                Key::Escape => return Some(ClientMessage::Cancel {}),
                Key::Tab | Key::BackTab => {
                    let count = if self.editable() { 3 } else { 2 };
                    self.focus =
                        (self.focus + if key.key == Key::Tab { 1 } else { count - 1 }) % count;
                    self.sync_focus();
                    return None;
                }
                Key::Char(_) | Key::Space if self.input.value().len() >= MAX_RESPONSE_BYTES => {
                    return None;
                }
                _ => {}
            }
        }
        if self.editable() {
            match self.input.handle_event(event.clone(), areas.input) {
                ComponentEvent::Activated(_) => return self.submit(),
                ComponentEvent::FocusRequested(_) => {
                    self.focus = 0;
                    self.sync_focus();
                }
                _ => {}
            }
        }
        match self.submit.handle_event(event.clone(), areas.submit) {
            ComponentEvent::Activated(_) => return self.submit(),
            ComponentEvent::FocusRequested(_) => {
                self.focus = if self.editable() { 1 } else { 0 };
                self.sync_focus();
            }
            _ => {}
        }
        match self.cancel.handle_event(event, areas.cancel) {
            ComponentEvent::Activated(_) => Some(ClientMessage::Cancel {}),
            ComponentEvent::FocusRequested(_) => {
                self.focus = if self.editable() { 2 } else { 1 };
                self.sync_focus();
                None
            }
            _ => None,
        }
    }

    fn layout_consent(&mut self, area: Rect) {
        let width = usize::from(area.width.saturating_sub(2)).max(1);
        let lines = textwrap::wrap(&self.description, width);
        let fits = lines.len() <= usize::from(area.height.saturating_sub(3))
            && self.title.width() <= width
            && width >= 32;
        if fits != self.consent_fits {
            self.dialog.actions = if fits {
                vec![
                    DialogAction::new("cancel", i18n::tr!("greeter-cancel")),
                    DialogAction::new("confirm", i18n::tr!("greeter-confirm")),
                ]
            } else {
                vec![DialogAction::new("cancel", i18n::tr!("greeter-cancel"))]
            };
            self.dialog.set_selected_action(Some(0));
        }
        self.consent_fits = fits;
        self.dialog.body = if fits {
            lines.iter().map(ToString::to_string).collect()
        } else {
            vec![i18n::tr!("greeter-resize")]
        };
    }

    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let areas = page_areas(frame.area());
        frame.render_widget(
            Paragraph::new("").style(self.theme.body_style()),
            frame.area(),
        );
        frame.render_widget(
            Paragraph::new(i18n::tr!("greeter-trusted")).style(self.theme.muted_style()),
            areas.footer,
        );
        if matches!(self.page, Page::Consent { .. } | Page::Locked) {
            if matches!(self.page, Page::Consent { .. }) {
                self.layout_consent(areas.panel);
            }
            self.dialog.render_frame(frame, areas.panel, &self.theme);
            return;
        }
        frame.render_widget(
            Paragraph::new(self.title.clone()).style(self.theme.title_style()),
            areas.title,
        );
        frame.render_widget(
            Paragraph::new(self.description.clone())
                .wrap(Wrap { trim: false })
                .style(self.theme.body_style()),
            areas.body,
        );
        if matches!(self.page, Page::Waiting | Page::Complete) {
            return;
        }
        if self.editable() {
            self.input.render_frame(frame, areas.input, &self.theme);
        }
        self.submit.render_frame(frame, areas.submit, &self.theme);
        self.cancel.render_frame(frame, areas.cancel, &self.theme);
    }
}

struct Areas {
    panel: Rect,
    title: Rect,
    body: Rect,
    input: Rect,
    submit: Rect,
    cancel: Rect,
    footer: Rect,
}
fn page_areas(area: Rect) -> Areas {
    let [main, footer] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
    let panel = main.centered(Constraint::Length(90), Constraint::Length(22));
    let [title, body, input, actions] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(3),
        Constraint::Length(3),
    ])
    .areas(panel);
    let [submit, cancel] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(actions);
    Areas {
        panel,
        title,
        body,
        input,
        submit,
        cancel,
        footer,
    }
}

/// Remove terminal controls and bidirectional overrides from service/PAM text.
fn display_text(text: &str) -> String {
    text.chars()
        .filter(|c| {
            (*c == '\n' || !c.is_control())
                && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    const AREA: Rect = Rect::new(0, 0, 100, 30);
    #[test]
    fn consent_defaults_cancel_and_is_consumed_once() {
        let mut ui = Greeter::default();
        ui.receive(ServerMessage::Consent {
            id: 12,
            title: "Reboot".into(),
            description: "End your session and restart.".into(),
        });
        assert!(matches!(
            ui.handle(InputEvent::key(Key::Enter), AREA),
            Some(ClientMessage::Consent {
                id: 12,
                approved: false
            })
        ));
        assert!(ui.handle(InputEvent::key(Key::Enter), AREA).is_none());
    }
    #[test]
    fn approve_requires_selection_and_visible_description() {
        let mut ui = Greeter::default();
        for (area, expected) in [(AREA, true), (Rect::new(0, 0, 20, 6), false)] {
            ui.receive(ServerMessage::Consent {
                id: 6,
                title: "Reboot".into(),
                description: "End your session and restart.".into(),
            });
            ui.handle(InputEvent::key(Key::Tab), area);
            assert!(
                matches!(ui.handle(InputEvent::key(Key::Enter), area), Some(ClientMessage::Consent { id: 6, approved }) if approved == expected)
            );
        }
    }
    #[test]
    fn pam_rounds_reset_secret_and_preserve_prompt_identity() {
        let mut ui = Greeter::default();
        ui.receive(ServerMessage::PamPrompt {
            id: 1,
            style: PamStyle::EchoOff,
            text: "Password:".into(),
        });
        for c in "first".chars() {
            ui.handle(InputEvent::key(Key::Char(c)), AREA);
        }
        assert!(
            matches!(ui.handle(InputEvent::key(Key::Enter), AREA), Some(ClientMessage::PamResponse { id: 1, response }) if response == "first")
        );
        ui.receive(ServerMessage::PamPrompt {
            id: 2,
            style: PamStyle::EchoOff,
            text: "New password:".into(),
        });
        assert!(ui.input.value().is_empty());
        ui.handle(InputEvent::key(Key::Char('新')), AREA);
        assert!(
            matches!(ui.handle(InputEvent::key(Key::Enter), AREA), Some(ClientMessage::PamResponse { id: 2, response }) if response == "新")
        );
    }
    #[test]
    fn informational_prompt_requires_ack_and_locked_escape_does_not_unlock() {
        let mut ui = Greeter::default();
        ui.receive(ServerMessage::PamPrompt {
            id: 9,
            style: PamStyle::Error,
            text: "Password expired".into(),
        });
        assert!(
            matches!(ui.handle(InputEvent::key(Key::Enter), AREA), Some(ClientMessage::PamResponse { id: 9, response }) if response.is_empty())
        );
        ui.receive(ServerMessage::Locked {
            username: "test".into(),
        });
        assert!(ui.handle(InputEvent::key(Key::Escape), AREA).is_none());
        assert!(matches!(
            ui.handle(InputEvent::key(Key::Enter), AREA),
            Some(ClientMessage::Unlock {})
        ));
    }
    #[test]
    fn mouse_confirmation_requires_matching_press_and_release() {
        let mut ui = Greeter::default();
        let message = ServerMessage::Consent {
            id: 22,
            title: "Reboot".into(),
            description: "Restart this system.".into(),
        };
        ui.receive(message.clone());
        let panel = page_areas(AREA).panel;
        ui.layout_consent(panel);
        let (_, confirm) = ui.dialog.action_areas(panel)[1];
        let point = (confirm.x, confirm.y);
        assert!(
            ui.handle(InputEvent::mouse_up(ui::MouseButton::Left, point), AREA)
                .is_none()
        );
        assert!(
            ui.handle(InputEvent::mouse_down(ui::MouseButton::Left, point), AREA)
                .is_none()
        );
        // A replacement request clears any press belonging to the previous one.
        ui.receive(message);
        assert!(
            ui.handle(InputEvent::mouse_up(ui::MouseButton::Left, point), AREA)
                .is_none()
        );
        ui.handle(InputEvent::mouse_down(ui::MouseButton::Left, point), AREA);
        assert!(matches!(
            ui.handle(InputEvent::mouse_up(ui::MouseButton::Left, point), AREA),
            Some(ClientMessage::Consent {
                id: 22,
                approved: true
            })
        ));
    }

    #[test]
    fn secret_is_absent_from_terminal_frame() {
        let mut ui = Greeter::default();
        ui.receive(ServerMessage::PamPrompt {
            id: 4,
            style: PamStyle::EchoOff,
            text: "Password:".into(),
        });
        for c in "hunter2".chars() {
            ui.handle(InputEvent::key(Key::Char(c)), AREA);
        }
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| ui.render(f)).unwrap();
        let output = format!("{:?}", terminal.backend().buffer());
        assert!(!output.contains("hunter2"));
        assert!(output.contains("*******"));
    }
}
