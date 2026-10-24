use super::*;
use std::io::{Read, Write};

fn snapshot() -> Snapshot {
    let mut button = shell_control::Node::new(
        "page/test",
        "button",
        "Test",
        shell_control::Rect {
            x: 12,
            y: 7,
            width: 8,
            height: 3,
        },
    );
    button.actions = vec!["click".into()];
    Snapshot {
        session: "test".into(),
        number: 1,
        page: "Home".into(),
        width: 120,
        height: 40,
        animation_running: false,
        background_tasks_running: false,
        nodes: vec![button],
        lines: vec![String::new(); 40],
    }
}

#[test]
fn click_uses_press_and_release_in_the_painted_rectangle() {
    let snapshot = snapshot();
    let action = Action::Click {
        target: Target::Id {
            id: "page/test".into(),
        },
        button: Button::Left,
        double: false,
    };
    let events = action_events(&action, &snapshot).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[0],
        (
            Duration::ZERO,
            mouse((16, 8), MouseEventKind::Down(MouseButton::Left))
        )
    );
    assert_eq!(
        events[1],
        (
            Duration::from_millis(16),
            mouse((16, 8), MouseEventKind::Up(MouseButton::Left))
        )
    );
    let mut disabled = snapshot.clone();
    disabled.nodes[0].enabled = false;
    assert_eq!(action_events(&action, &disabled), Err("target_unavailable"));
    disabled.nodes[0].enabled = true;
    disabled.nodes[0].actionable = false;
    assert_eq!(action_events(&action, &disabled), Err("target_unavailable"));
}

#[test]
fn double_click_and_drag_preserve_timing_and_intermediate_positions() {
    let double = Action::Click {
        target: Target::Position { x: 3, y: 4 },
        button: Button::Right,
        double: true,
    };
    let events = action_events(&double, &snapshot()).unwrap();
    assert_eq!(events.len(), 4);
    assert!(events.windows(2).all(|pair| pair[0].0 < pair[1].0));
    let drag = Action::Drag {
        from: Target::Position { x: 2, y: 2 },
        to: Target::Position { x: 18, y: 10 },
        duration_ms: 160,
        button: Button::Left,
    };
    let events = action_events(&drag, &snapshot()).unwrap();
    assert_eq!(events.len(), 12);
    assert_eq!(
        events[0].1,
        mouse((2, 2), MouseEventKind::Down(MouseButton::Left))
    );
    assert_eq!(
        events[5].1,
        mouse((10, 6), MouseEventKind::Drag(MouseButton::Left))
    );
    assert_eq!(
        events.last().unwrap().1,
        mouse((18, 10), MouseEventKind::Up(MouseButton::Left))
    );
}

#[test]
fn key_events_preserve_unicode_modifiers_and_release() {
    for phase in [Phase::Press, Phase::Repeat, Phase::Release] {
        let action = Action::Key {
            key: "中".into(),
            modifiers: vec!["ctrl".into(), "shift".into()],
            phase,
        };
        let events = action_events(&action, &snapshot()).unwrap();
        let Event::Key(event) = events[0].1 else {
            panic!("key event")
        };
        assert_eq!(event.code, KeyCode::Char('中'));
        assert_eq!(event.modifiers, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        assert_eq!(
            event.kind,
            match phase {
                Phase::Press => KeyEventKind::Press,
                Phase::Repeat => KeyEventKind::Repeat,
                Phase::Release => KeyEventKind::Release,
            }
        );
    }
    assert_eq!(parse_key("F25"), Err("invalid_arguments"));
    assert_eq!(parse_key("Ctrl+S"), Err("invalid_arguments"));
}

#[test]
fn text_is_one_paste_event_and_coordinates_are_checked() {
    let text = "中文\nsecond line\t🙂";
    assert_eq!(
        action_events(&Action::Text { text: text.into() }, &snapshot()).unwrap(),
        vec![(Duration::ZERO, Event::Paste(text.into()))]
    );
    assert_eq!(
        action_events(
            &Action::Move {
                target: Target::Position { x: 120, y: 0 }
            },
            &snapshot()
        ),
        Err("target_unavailable")
    );
    let mut small = snapshot();
    small.page = "terminal_size".into();
    assert_eq!(
        action_events(
            &Action::Key {
                key: "Enter".into(),
                modifiers: vec![],
                phase: Phase::Press
            },
            &small
        ),
        Err("target_unavailable")
    );
}

#[test]
fn wake_only_sends_input_to_weather_pages() {
    let mut current = snapshot();
    for page in ["Home", "terminal_size"] {
        current.page = page.into();
        assert!(action_events(&Action::Wake, &current).unwrap().is_empty());
    }
    for page in ["lockscreen", "screen_saver"] {
        current.page = page.into();
        let events = action_events(&Action::Wake, &current).unwrap();
        assert_eq!(events.len(), 2);
        for (index, phase) in [KeyEventKind::Press, KeyEventKind::Release]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                events[index],
                (
                    Duration::from_millis(index as u64 * 16),
                    Event::Key(KeyEvent::new_with_kind(
                        KeyCode::Char(' '),
                        KeyModifiers::NONE,
                        phase,
                    )),
                )
            );
        }
    }
}

#[test]
fn disconnect_cancels_pointer_instead_of_activating_a_pressed_button() {
    let mut releases = Vec::new();
    track_release(
        &mouse((16, 8), MouseEventKind::Down(MouseButton::Left)),
        &mut releases,
    );
    track_release(
        &Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
        &mut releases,
    );
    cancel_pointer(&mut releases);
    assert_eq!(releases.pop(), Some(Event::FocusLost));
    assert!(
        releases
            .iter()
            .all(|event| matches!(event, Event::Key(key) if key.kind == KeyEventKind::Release))
    );
}

struct TempDirectory(PathBuf);
impl TempDirectory {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tundra-control-runtime-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        Self(root)
    }
}
impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn local_request_waits_for_a_rendered_frame_and_rejects_stale_actions() {
    let directory = TempDirectory::new();
    let mut control = ControlRuntime::new(directory.0.clone());
    control.synchronize(true).unwrap();
    control.publish(snapshot());
    let info = control.server.as_ref().unwrap().info.clone();
    let mut request = Request::new(Action::Key {
        key: "Enter".into(),
        modifiers: vec![],
        phase: Phase::Press,
    });
    request.snapshot = Some(1);
    let reader = std::thread::spawn(move || {
        platform::shell_control::exchange(&info, &request, Duration::from_secs(5)).unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(event) = control.poll(Instant::now()) {
            assert!(matches!(event, Event::Key(key) if key.code == KeyCode::Enter));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(control.needs_frame());
    assert!(
        !reader.is_finished(),
        "input completion must wait for the next draw"
    );
    control.publish(snapshot());
    while !reader.is_finished() {
        control.poll(Instant::now());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let response = reader.join().unwrap();
    assert!(response.ok);
    assert_eq!(response.snapshot.unwrap().number, 2);
    let info = control.server.as_ref().unwrap().info.clone();
    let mut request = Request::new(Action::Text {
        text: "must not be inserted".into(),
    });
    request.snapshot = Some(1);
    let reader = std::thread::spawn(move || {
        platform::shell_control::exchange(&info, &request, Duration::from_secs(5)).unwrap()
    });
    while !reader.is_finished() {
        let event = control.poll(Instant::now());
        assert!(!matches!(event, Some(Event::Paste(_))));
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(reader.join().unwrap().error.unwrap().code, "stale_snapshot");
    control.synchronize(false).unwrap();
    assert!(!control.enabled());
    assert!(
        platform::shell_control::sessions(&directory.0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn disconnect_does_not_drop_other_requests_received_in_the_same_poll() {
    let directory = TempDirectory::new();
    let mut control = ControlRuntime::new(directory.0.clone());
    control.synchronize(true).unwrap();
    control.publish(snapshot());
    let info = control.server.as_ref().unwrap().info.clone();
    let mut stream = platform::shell_control::connect(&info).unwrap();
    let mut bytes = serde_json::to_vec(&Request::new(Action::Snapshot)).unwrap();
    bytes.push(b'\n');
    stream.write_all(&bytes).unwrap();
    control.active = Some(u64::MAX);
    track_release(
        &mouse((1, 1), MouseEventKind::Down(MouseButton::Left)),
        &mut control.releases,
    );
    assert_eq!(control.poll(Instant::now()), Some(Event::FocusLost));
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut received = Vec::new();
    while received.last() != Some(&b'\n') {
        control.poll(Instant::now());
        let mut bytes = [0; 8192];
        match stream.read(&mut bytes) {
            Ok(0) => panic!("unexpected disconnect"),
            Ok(len) => received.extend_from_slice(&bytes[..len]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("{error}"),
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(serde_json::from_slice::<Response>(&received).unwrap().ok);
}

#[test]
fn disabling_control_finishes_the_request_before_disconnect() {
    let directory = TempDirectory::new();
    let mut control = ControlRuntime::new(directory.0.clone());
    control.synchronize(true).unwrap();
    control.publish(snapshot());
    let info = control.server.as_ref().unwrap().info.clone();
    let request = Request::new(Action::Key {
        key: "Enter".into(),
        modifiers: vec![],
        phase: Phase::Press,
    });
    let reader = std::thread::spawn(move || {
        platform::shell_control::exchange(&info, &request, Duration::from_secs(3)).unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while control.poll(Instant::now()).is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    control.synchronize(false).unwrap();
    let response = reader.join().unwrap();
    assert!(response.ok && response.closed);
    assert!(response.snapshot.is_none());
}

#[test]
fn changing_session_discards_remaining_gesture_coordinates() {
    let mut control = ControlRuntime::new(PathBuf::new());
    control.active = Some(1);
    control.events.push_back((
        Instant::now(),
        mouse((10, 10), MouseEventKind::Drag(MouseButton::Left)),
    ));
    track_release(
        &mouse((1, 1), MouseEventKind::Down(MouseButton::Left)),
        &mut control.releases,
    );
    control.invalidate();
    assert!(control.events.is_empty());
    assert!(control.needs_frame());
    assert_eq!(control.poll(Instant::now()), Some(Event::FocusLost));
}
