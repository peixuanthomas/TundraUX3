//! External input uses the same terminal events as physical input.
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use shell_control::{Action, Button, PROTOCOL_VERSION, Phase, Request, Response, Snapshot, Target};
use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(super) struct ControlRuntime {
    directory: PathBuf,
    server: Option<platform::shell_control::Server>,
    snapshot: Option<Snapshot>,
    number: u64,
    active: Option<u64>,
    events: VecDeque<(Instant, Event)>,
    releases: Vec<Event>,
    awaiting_frame: bool,
    failed: bool,
}

impl ControlRuntime {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            server: None,
            snapshot: None,
            number: 0,
            active: None,
            events: VecDeque::new(),
            releases: Vec::new(),
            awaiting_frame: false,
            failed: false,
        }
    }

    pub fn enabled(&self) -> bool {
        self.server.is_some()
    }

    pub fn synchronize(&mut self, enabled: bool) -> io::Result<()> {
        if !enabled {
            self.close();
            self.snapshot = None;
            self.active = None;
            self.events.clear();
            self.awaiting_frame = false;
            cancel_pointer(&mut self.releases);
            self.failed = false;
        } else if self.server.is_none() && !self.failed {
            match platform::shell_control::Server::start(&self.directory) {
                Ok(server) => self.server = Some(server),
                Err(error) => {
                    self.failed = true;
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    pub fn publish(&mut self, mut snapshot: Snapshot) {
        let Some(server) = &mut self.server else {
            return;
        };
        self.number += 1;
        snapshot.session = server.info.session.clone();
        snapshot.number = self.number;
        if self.awaiting_frame && self.events.is_empty() {
            if let Some(client) = self.active.take() {
                server.respond(client, &Response::success(snapshot.clone()));
            }
            self.awaiting_frame = false;
        }
        self.snapshot = Some(snapshot);
    }

    pub fn invalidate(&mut self) {
        self.snapshot = None;
        self.events.clear();
        cancel_pointer(&mut self.releases);
        self.awaiting_frame = self.active.is_some();
    }

    pub fn close(&mut self) {
        if let Some(mut server) = self.server.take() {
            if let Some(client) = self.active.take() {
                server.respond(client, &Response::closed());
            }
            server.stop_accepting();
            for _ in 0..10 {
                server.poll();
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        self.snapshot = None;
        self.events.clear();
        self.awaiting_frame = false;
        cancel_pointer(&mut self.releases);
    }

    /// Returns at most one scheduled input so a press can be painted before release.
    pub fn poll(&mut self, now: Instant) -> Option<Event> {
        let Some(server) = &mut self.server else {
            return self.releases.pop();
        };
        let requests = server.poll();
        let disconnected = self.active.is_some_and(|id| !server.connected(id));
        if disconnected {
            self.active = None;
            self.events.clear();
            self.awaiting_frame = false;
            cancel_pointer(&mut self.releases);
        }
        for (client, bytes) in requests {
            let response = match serde_json::from_slice::<Request>(&bytes) {
                Err(_) => Some(Response::failure(
                    "invalid_arguments",
                    i18n::tr!("ui-control-invalid-arguments"),
                )),
                Ok(request) if request.protocol != PROTOCOL_VERSION => Some(Response::failure(
                    "unsupported_protocol",
                    i18n::tr!("ui-control-protocol"),
                )),
                Ok(_) if self.snapshot.is_none() => Some(Response::failure(
                    "not_ready",
                    i18n::tr!("ui-control-not-ready"),
                )),
                Ok(request) => {
                    let snapshot = self.snapshot.as_ref().expect("checked snapshot");
                    if matches!(request.action, Action::Snapshot) {
                        Some(Response::success(snapshot.clone()))
                    } else if request
                        .snapshot
                        .is_some_and(|number| number != snapshot.number)
                    {
                        Some(Response::failure(
                            "stale_snapshot",
                            i18n::tr!("ui-control-stale"),
                        ))
                    } else if self.active.is_some() || !self.releases.is_empty() {
                        Some(Response::failure("busy", i18n::tr!("ui-control-busy")))
                    } else {
                        match action_events(&request.action, snapshot) {
                            Ok(events) => {
                                self.active = Some(client);
                                self.events = events
                                    .into_iter()
                                    .map(|(delay, event)| (now + delay, event))
                                    .collect();
                                self.awaiting_frame = false;
                                None
                            }
                            Err(code) => Some(Response::failure(
                                code,
                                i18n::tr!(if code == "invalid_arguments" {
                                    "ui-control-invalid-arguments"
                                } else {
                                    "ui-control-target"
                                }),
                            )),
                        }
                    }
                }
            };
            if let Some(response) = response {
                server.respond(client, &response);
            }
        }
        if disconnected || self.active.is_none() {
            return self.releases.pop();
        }
        if self.events.front().is_none_or(|(at, _)| *at > now) {
            return None;
        }
        let (_, event) = self.events.pop_front().expect("ready event");
        track_release(&event, &mut self.releases);
        if self.events.is_empty() {
            self.awaiting_frame = true;
        }
        Some(event)
    }

    pub fn needs_frame(&self) -> bool {
        self.awaiting_frame
    }
}

impl weathr::DisplayControl for ControlRuntime {
    fn frame(&mut self, buffer: &ratatui::buffer::Buffer) {
        if !self.enabled() {
            return;
        }
        let capture = ui::automation::FrameCapture::begin(buffer.area, "lockscreen");
        let mut snapshot = capture.finish(buffer);
        let area = buffer.area;
        let mut start = shell_control::Node::new(
            "lockscreen/start",
            "button",
            i18n::tr!("ui-control-lockscreen"),
            shell_control::Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: area.height,
            },
        );
        start.actions = vec!["click".into(), "key".into()];
        start.parent = Some("screen".into());
        snapshot.nodes.push(start);
        self.publish(snapshot);
    }
    fn poll(&mut self) -> Option<Event> {
        ControlRuntime::poll(self, Instant::now())
    }
}

fn cancel_pointer(releases: &mut Vec<Event>) {
    if releases
        .iter()
        .any(|event| matches!(event, Event::Mouse(_)))
    {
        releases.retain(|event| !matches!(event, Event::Mouse(_)));
        releases.push(Event::FocusLost);
    }
}

fn track_release(event: &Event, releases: &mut Vec<Event>) {
    match event {
        Event::Key(key) => {
            releases.retain(|event| !matches!(event, Event::Key(held) if held.code == key.code && held.modifiers == key.modifiers));
            if key.kind != KeyEventKind::Release {
                let mut release = *key;
                release.kind = KeyEventKind::Release;
                releases.push(Event::Key(release));
            }
        }
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::Down(button) | MouseEventKind::Drag(button) => {
                releases.retain(|event| !matches!(event, Event::Mouse(held) if held.kind == MouseEventKind::Up(button)));
                let mut release = *mouse;
                release.kind = MouseEventKind::Up(button);
                releases.push(Event::Mouse(release));
            }
            MouseEventKind::Up(button) => {
                releases.retain(|event| !matches!(event, Event::Mouse(held) if held.kind == MouseEventKind::Up(button)));
            }
            _ => {}
        },
        _ => {}
    }
}

fn position(target: &Target, snapshot: &Snapshot) -> Result<(u16, u16), &'static str> {
    match target {
        Target::Position { x, y } if *x < snapshot.width && *y < snapshot.height => Ok((*x, *y)),
        Target::Id { id } => snapshot
            .nodes
            .iter()
            .find(|node| {
                node.id == *id
                    && node.enabled
                    && node.actionable
                    && node.rect.width > 0
                    && node.rect.height > 0
            })
            .map(|node| {
                (
                    node.rect.x + node.rect.width / 2,
                    node.rect.y + node.rect.height / 2,
                )
            })
            .ok_or("target_unavailable"),
        _ => Err("target_unavailable"),
    }
}

fn mouse(point: (u16, u16), kind: MouseEventKind) -> Event {
    Event::Mouse(MouseEvent {
        column: point.0,
        row: point.1,
        kind,
        modifiers: KeyModifiers::NONE,
    })
}

pub(super) fn action_events(
    action: &Action,
    snapshot: &Snapshot,
) -> Result<Vec<(Duration, Event)>, &'static str> {
    if snapshot.page == "terminal_size" {
        return Err("target_unavailable");
    }
    let mut events = Vec::new();
    let button = |button: Button| match button {
        Button::Left => MouseButton::Left,
        Button::Right => MouseButton::Right,
        Button::Middle => MouseButton::Middle,
    };
    let mut add = |ms: u64, event| events.push((Duration::from_millis(ms), event));
    match action {
        Action::Snapshot => {}
        Action::Click {
            target,
            button: b,
            double,
        } => {
            let point = position(target, snapshot)?;
            let b = button(*b);
            add(0, mouse(point, MouseEventKind::Down(b)));
            add(16, mouse(point, MouseEventKind::Up(b)));
            if *double {
                add(80, mouse(point, MouseEventKind::Down(b)));
                add(96, mouse(point, MouseEventKind::Up(b)));
            }
        }
        Action::Move { target } => {
            add(0, mouse(position(target, snapshot)?, MouseEventKind::Moved))
        }
        Action::Drag {
            from,
            to,
            duration_ms,
            button: b,
        } => {
            if !(16..=10000).contains(duration_ms) {
                return Err("invalid_arguments");
            }
            let from = position(from, snapshot)?;
            let to = position(to, snapshot)?;
            let b = button(*b);
            add(0, mouse(from, MouseEventKind::Down(b)));
            let steps = duration_ms.div_ceil(16).min(600);
            for step in 1..=steps {
                let interpolate = |a: u16, b: u16| {
                    (i64::from(a) + (i64::from(b) - i64::from(a)) * step as i64 / steps as i64)
                        as u16
                };
                add(
                    duration_ms * step / steps,
                    mouse(
                        (interpolate(from.0, to.0), interpolate(from.1, to.1)),
                        MouseEventKind::Drag(b),
                    ),
                );
            }
            add(duration_ms + 16, mouse(to, MouseEventKind::Up(b)));
        }
        Action::Scroll {
            target,
            direction,
            count,
        } => {
            if !(1..=1000).contains(count) {
                return Err("invalid_arguments");
            }
            let point = position(target, snapshot)?;
            let kind = match direction.as_str() {
                "up" => MouseEventKind::ScrollUp,
                "down" => MouseEventKind::ScrollDown,
                "left" => MouseEventKind::ScrollLeft,
                "right" => MouseEventKind::ScrollRight,
                _ => return Err("invalid_arguments"),
            };
            for i in 0..*count {
                add(u64::from(i) * 2, mouse(point, kind));
            }
        }
        Action::Key {
            key,
            modifiers,
            phase,
        } => {
            let (code, mut mods) = parse_key(key)?;
            for modifier in modifiers {
                mods |= match modifier.as_str() {
                    "ctrl" | "control" => KeyModifiers::CONTROL,
                    "alt" => KeyModifiers::ALT,
                    "shift" => KeyModifiers::SHIFT,
                    "super" => KeyModifiers::SUPER,
                    "hyper" => KeyModifiers::HYPER,
                    "meta" => KeyModifiers::META,
                    _ => return Err("invalid_arguments"),
                };
            }
            let kind = match phase {
                Phase::Press => KeyEventKind::Press,
                Phase::Repeat => KeyEventKind::Repeat,
                Phase::Release => KeyEventKind::Release,
            };
            add(0, Event::Key(KeyEvent::new_with_kind(code, mods, kind)));
        }
        Action::Text { text } => add(0, Event::Paste(text.clone())),
    }
    Ok(events)
}

fn parse_key(key: &str) -> Result<(KeyCode, KeyModifiers), &'static str> {
    let code = if key.chars().count() == 1 {
        KeyCode::Char(key.chars().next().unwrap())
    } else {
        match key.to_ascii_lowercase().as_str() {
            "enter" => KeyCode::Enter,
            "esc" | "escape" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "backtab" => return Ok((KeyCode::BackTab, KeyModifiers::SHIFT)),
            "backspace" => KeyCode::Backspace,
            "space" => KeyCode::Char(' '),
            "delete" => KeyCode::Delete,
            "insert" => KeyCode::Insert,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            other => KeyCode::F(
                other
                    .strip_prefix('f')
                    .and_then(|v| v.parse::<u8>().ok())
                    .filter(|v| (1..=24).contains(v))
                    .ok_or("invalid_arguments")?,
            ),
        }
    };
    Ok((code, KeyModifiers::NONE))
}

#[cfg(test)]
#[path = "../../tests/unit/session/automation.rs"]
mod tests;
