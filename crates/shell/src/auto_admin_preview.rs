//! Isolated visual demo: no authorization, subprocess, or system changes.
use crate::{TerminalGuard, crossterm_event_to_input};
use crossterm::event;
use ratatui::{
    layout::{Margin, Rect},
    style::{Color, Style},
    widgets::{Block, Paragraph},
};
use std::{
    io::{self, IsTerminal, Write},
    sync::Arc,
    time::Duration,
};
use ui::{
    AutoAdminPreviewStyle, AutoAdminViewModel, CommandLineTerminalSnapshot, InputEvent, Key,
    MouseButton, MouseEventKind, RenderContext,
};

pub fn run_auto_admin_style_preview(
    output: &mut impl Write,
    style: AutoAdminPreviewStyle,
) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "test-aa-style requires an interactive terminal (stdin and stdout)",
        ));
    }
    let mut terminal = TerminalGuard::enter(output)?;
    let mut model = preview_model();
    let mut pressed = None;
    let mut bounds = Rect::default();
    let mut visible = true;
    loop {
        terminal.terminal_mut().draw(|frame| {
            bounds = frame.area();
            let main = match ui::compute_shell_layout(bounds) {
                ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
            };
            frame.render_widget(
                Block::bordered()
                    .title(i18n::tr!("aa-preview-page"))
                    .style(Style::default().fg(Color::White).bg(Color::Rgb(26, 39, 55))),
                main,
            );
            frame.render_widget(
                Paragraph::new(i18n::tr!("aa-preview-page-content"))
                    .style(Style::default().fg(Color::White)),
                main.inner(Margin::new(2, 2)),
            );
            frame.render_widget(
                Paragraph::new(i18n::tr!("aa-preview-controls")),
                Rect::new(bounds.x, bounds.y, bounds.width, bounds.height.min(1)),
            );
            if visible {
                ui::render_auto_admin_preview(
                    frame,
                    bounds,
                    &model,
                    style,
                    &RenderContext::default(),
                );
            }
            if bounds.height > 1 {
                frame.render_widget(
                    Paragraph::new(i18n::tr!("aa-preview-safe")),
                    Rect::new(bounds.x, bounds.bottom() - 1, bounds.width, 1),
                );
            }
        })?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let input = crossterm_event_to_input(event::read()?);
        match input {
            InputEvent::Key(key) if key.phase == ui::InputPhase::Press => {
                pressed = None;
                if key.key == Key::Escape || key.is_ctrl_c() {
                    break;
                }
                let count = if model.confirming {
                    2
                } else if model.finished {
                    1
                } else {
                    3
                };
                let selected = if model.confirming {
                    usize::from(!model.approve_selected)
                } else {
                    model.button_focus.unwrap_or(0)
                };
                match key.key {
                    Key::Char('b' | 'B') => visible = !visible,
                    Key::Char('c' | 'C') => set_phase(&mut model, 0),
                    Key::Char('r' | 'R') => set_phase(&mut model, 1),
                    Key::Char('f' | 'F') => set_phase(&mut model, 2),
                    Key::Tab | Key::Right | Key::Left | Key::BackTab => {
                        let backwards =
                            matches!(key.key, Key::Left | Key::BackTab) || key.modifiers.shift;
                        let next = (selected + if backwards { count - 1 } else { 1 }) % count;
                        model.approve_selected = next == 0;
                        model.button_focus = Some(next);
                    }
                    Key::Enter | Key::Char(' ') => {
                        if activate(&mut model, selected) {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            InputEvent::Mouse(mouse) => {
                if !visible {
                    continue;
                }
                let (layout, _) = ui::auto_admin_preview_layout(bounds, &model, style);
                let hit = layout
                    .buttons
                    .iter()
                    .position(|area| area.contains(mouse.coordinates().into()));
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => pressed = hit,
                    MouseEventKind::Up(MouseButton::Left) => {
                        if let Some(index) = pressed.take().filter(|index| Some(*index) == hit)
                            && activate(&mut model, index)
                        {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            InputEvent::Resize { .. } => pressed = None,
            _ => {}
        }
    }
    Ok(())
}

fn preview_model() -> AutoAdminViewModel {
    let mut snapshot = CommandLineTerminalSnapshot::blank(76, 6);
    for (row, text) in [
        "[DEMO] Request administrator authorization",
        "[DEMO] Disable login for sample-user",
        "[DEMO] No command is executed. No files are changed.",
    ]
    .iter()
    .enumerate()
    {
        for (column, character) in text.chars().enumerate() {
            snapshot.cells[row * 76 + column].symbol = character.to_string();
        }
    }
    AutoAdminViewModel {
        description: i18n::tr!("aa-preview-operation"),
        status: i18n::tr!("aa-request"),
        confirming: true,
        finished: false,
        approve_selected: false,
        button_focus: None,
        scroll: 0,
        input: Some(i18n::tr!("aa-preview-safe")),
        terminal: Arc::new(snapshot),
    }
}

fn set_phase(model: &mut AutoAdminViewModel, phase: u8) {
    model.confirming = phase == 0;
    model.finished = phase == 2;
    model.approve_selected = false;
    model.button_focus = Some(0);
    model.status = i18n::tr!(match phase {
        0 => "aa-request",
        1 => "aa-running",
        _ => "aa-completed",
    });
}

fn activate(model: &mut AutoAdminViewModel, index: usize) -> bool {
    if model.finished {
        return true;
    }
    if model.confirming {
        set_phase(model, if index == 0 { 1 } else { 2 });
        if index != 0 {
            model.status = i18n::tr!("aa-rejected");
        }
    } else {
        set_phase(model, 2);
    }
    false
}
