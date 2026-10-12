use super::*;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

fn mouse_event(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers,
    })
}

fn collect_one_test_batch(source: &Rc<RefCell<VecDeque<Event>>>) -> Vec<Event> {
    let first = source
        .borrow_mut()
        .pop_front()
        .expect("test event source must not be empty");
    let poll_source = Rc::clone(source);
    let read_source = Rc::clone(source);
    collect_ready_terminal_event_batch(
        first,
        move || Ok(!poll_source.borrow().is_empty()),
        move || {
            read_source
                .borrow_mut()
                .pop_front()
                .ok_or_else(|| io::Error::other("test event source was exhausted"))
        },
    )
    .expect("collect terminal event batch")
}

#[test]
fn mouse_motion_flood_is_consumed_in_a_few_render_batches() {
    let source = Rc::new(RefCell::new(
        (0..10_000)
            .map(|index| {
                mouse_event(
                    MouseEventKind::Moved,
                    (index % 200) as u16,
                    (index % 80) as u16,
                    KeyModifiers::NONE,
                )
            })
            .collect::<VecDeque<_>>(),
    ));
    let mut rendered_events = Vec::new();
    let mut batch_count = 0;

    while !source.borrow().is_empty() {
        rendered_events.extend(collect_one_test_batch(&source));
        batch_count += 1;
    }

    assert_eq!(batch_count, 3);
    assert_eq!(rendered_events.len(), batch_count);
    assert_eq!(
        rendered_events.last(),
        Some(&mouse_event(
            MouseEventKind::Moved,
            (9_999 % 200) as u16,
            (9_999 % 80) as u16,
            KeyModifiers::NONE,
        ))
    );
}

#[test]
fn mouse_coalescing_preserves_semantic_event_boundaries() {
    let source = Rc::new(RefCell::new(VecDeque::from([
        mouse_event(MouseEventKind::Moved, 1, 1, KeyModifiers::NONE),
        mouse_event(MouseEventKind::Moved, 2, 2, KeyModifiers::NONE),
        mouse_event(MouseEventKind::Moved, 3, 3, KeyModifiers::SHIFT),
        Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)),
        mouse_event(MouseEventKind::Moved, 4, 4, KeyModifiers::NONE),
        mouse_event(MouseEventKind::Moved, 5, 5, KeyModifiers::NONE),
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            5,
            5,
            KeyModifiers::NONE,
        ),
        mouse_event(
            MouseEventKind::Drag(MouseButton::Left),
            6,
            6,
            KeyModifiers::NONE,
        ),
        mouse_event(
            MouseEventKind::Drag(MouseButton::Left),
            7,
            7,
            KeyModifiers::NONE,
        ),
        mouse_event(MouseEventKind::ScrollDown, 7, 7, KeyModifiers::NONE),
        mouse_event(MouseEventKind::ScrollDown, 7, 7, KeyModifiers::NONE),
        mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            7,
            7,
            KeyModifiers::NONE,
        ),
        Event::Paste("preserve me".to_string()),
        Event::FocusGained,
        Event::Resize(100, 40),
        Event::Resize(120, 50),
    ])));

    let first_batch = collect_one_test_batch(&source);
    assert_eq!(
        first_batch,
        vec![
            mouse_event(MouseEventKind::Moved, 2, 2, KeyModifiers::NONE),
            mouse_event(MouseEventKind::Moved, 3, 3, KeyModifiers::SHIFT),
            Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL,)),
        ]
    );

    let second_batch = collect_one_test_batch(&source);
    assert_eq!(
        second_batch,
        vec![
            mouse_event(MouseEventKind::Moved, 5, 5, KeyModifiers::NONE),
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                5,
                5,
                KeyModifiers::NONE,
            ),
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                7,
                7,
                KeyModifiers::NONE,
            ),
            mouse_event(MouseEventKind::ScrollDown, 7, 7, KeyModifiers::NONE,),
            mouse_event(MouseEventKind::ScrollDown, 7, 7, KeyModifiers::NONE,),
            mouse_event(
                MouseEventKind::Up(MouseButton::Left),
                7,
                7,
                KeyModifiers::NONE,
            ),
            Event::Paste("preserve me".to_string()),
        ]
    );

    let third_batch = collect_one_test_batch(&source);
    assert!(source.borrow().is_empty());
    assert_eq!(
        third_batch,
        vec![Event::FocusGained, Event::Resize(120, 50),]
    );
}

#[test]
fn a_ready_key_is_dispatched_without_polling_for_more_events() {
    let polls = Cell::new(0_usize);
    let batch = collect_ready_terminal_event_batch(
        Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        || {
            polls.set(polls.get() + 1);
            Ok(true)
        },
        || panic!("a key batch must not read a later event"),
    )
    .expect("key batch");

    assert_eq!(polls.get(), 0);
    assert_eq!(
        batch,
        vec![Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ))]
    );
}

#[test]
fn ready_event_drain_is_bounded_when_the_source_never_goes_idle() {
    let reads = Cell::new(0_usize);
    let batch = collect_ready_terminal_event_batch(
        mouse_event(MouseEventKind::Moved, 0, 0, KeyModifiers::NONE),
        || Ok(true),
        || {
            let next = reads.get() + 1;
            reads.set(next);
            Ok(mouse_event(
                MouseEventKind::Moved,
                (next % 200) as u16,
                (next % 80) as u16,
                KeyModifiers::NONE,
            ))
        },
    )
    .expect("bounded batch");

    assert_eq!(reads.get() + 1, MAX_READY_TERMINAL_EVENTS_PER_FRAME);
    assert_eq!(batch.len(), 1);
}
