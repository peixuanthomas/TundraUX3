//! Bounded acquisition of already parsed Crossterm events. No byte parsing here.
use crossterm::event;
use std::{io, time::Duration};

const MAX_READY_TERMINAL_EVENTS_PER_FRAME: usize = 4_096;

pub(crate) fn read_ready_terminal_event_batch(
    first: event::Event,
) -> io::Result<Vec<event::Event>> {
    collect_ready_terminal_event_batch(first, || event::poll(Duration::ZERO), event::read)
}

fn collect_ready_terminal_event_batch(
    first: event::Event,
    mut poll_ready: impl FnMut() -> io::Result<bool>,
    mut read_ready: impl FnMut() -> io::Result<event::Event>,
) -> io::Result<Vec<event::Event>> {
    // Crossterm's all-motion mode can enqueue many coordinates while one
    // Ratatui frame is being built, especially through a WSL ConPTY bridge.
    // Drain that ready backlog before drawing again and retain only the newest
    // point in each uninterrupted motion/resize run. Semantic boundaries such
    // as clicks, releases, wheel steps, and focus stay ordered. A key or paste
    // ends the batch immediately so a continuously-ready WSL mouse stream
    // cannot delay keyboard input after Crossterm has already decoded it.
    let mut events = Vec::new();
    let first_requires_dispatch = terminal_event_requires_immediate_dispatch(&first);
    push_coalesced_terminal_event(&mut events, first);
    if first_requires_dispatch {
        return Ok(events);
    }

    let mut raw_event_count = 1;
    while raw_event_count < MAX_READY_TERMINAL_EVENTS_PER_FRAME && poll_ready()? {
        let next = read_ready()?;
        let requires_dispatch = terminal_event_requires_immediate_dispatch(&next);
        push_coalesced_terminal_event(&mut events, next);
        raw_event_count += 1;
        if requires_dispatch {
            break;
        }
    }
    Ok(events)
}

fn terminal_event_requires_immediate_dispatch(event: &event::Event) -> bool {
    matches!(event, event::Event::Key(_) | event::Event::Paste(_))
}

fn push_coalesced_terminal_event(events: &mut Vec<event::Event>, next: event::Event) {
    if events
        .last()
        .is_some_and(|previous| terminal_event_can_replace(previous, &next))
    {
        *events
            .last_mut()
            .expect("the previous event was checked above") = next;
    } else {
        events.push(next);
    }
}

fn terminal_event_can_replace(previous: &event::Event, next: &event::Event) -> bool {
    match (previous, next) {
        (event::Event::Mouse(previous), event::Event::Mouse(next))
            if previous.modifiers == next.modifiers =>
        {
            match (&previous.kind, &next.kind) {
                (event::MouseEventKind::Moved, event::MouseEventKind::Moved) => true,
                (
                    event::MouseEventKind::Drag(previous_button),
                    event::MouseEventKind::Drag(next_button),
                ) => previous_button == next_button,
                _ => false,
            }
        }
        (event::Event::Resize(_, _), event::Event::Resize(_, _)) => true,
        _ => false,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/input/terminal_batch.rs"]
mod tests;
