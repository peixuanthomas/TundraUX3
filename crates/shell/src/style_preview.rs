//! Isolated debug playground: a Model/Message/Update loop feeds the existing
//! Ratatui components. Progress is simulated; no jobs or preferences are saved.

use std::{
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

use crossterm::event;
use tachyonfx::{CellFilter, Effect, Interpolation, Motion, fx};
use ui::{
    InputEvent, Key, MotionFrame, RenderCapabilities, RenderContext, TundraTheme,
    components::{
        Button, ComponentEvent, Dialog, DialogAction, List, ListItem, TextInput, contains_point,
    },
    style_preview::{PreviewLayout, PreviewView, UiStyleVersion, preview_layout, render_preview},
};

use crate::{ShellAppConfig, TerminalGuard, crossterm_event_to_input};

const FRAME: Duration = Duration::from_millis(16);
const STAGE: Duration = Duration::from_millis(1200);

/// Runs until Escape/Ctrl-C. TerminalGuard restores modes on normal exit,
/// rendering/input errors and unwinding through the CLI watchdog boundary.
pub fn run_ui_style_preview(
    output: &mut impl Write,
    version: UiStyleVersion,
    appearance: &storage::AppearanceConfig,
) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "view-ui-style requires an interactive terminal (stdin and stdout)",
        ));
    }
    let config = ShellAppConfig::from_appearance(appearance);
    let theme = TundraTheme::default_dark()
        .with_border_shape(config.border_shape)
        .with_border_color(config.border_color)
        .with_accent_color(config.accent_color);
    let reduced = matches!(
        appearance.motion_preference,
        storage::MotionPreference::Reduced
    );
    let speed = f64::from(appearance.normalized_animation_speed_percent()) / 100.0;
    let mut model = PreviewModel::new(version, reduced);
    let mut terminal = TerminalGuard::enter(output)?;
    let origin = Instant::now();
    let mut previous = origin;
    let mut next_frame = origin;
    let mut dirty = true;
    let mut bounds = ratatui::layout::Rect::default();
    let capabilities = preview_capabilities();
    loop {
        let now = Instant::now();
        if dirty || (model.is_running() && now >= next_frame) {
            // A resize or long pause must not destabilize spring integration.
            let delta = now.duration_since(previous).min(Duration::from_millis(50));
            model.update(Message::Tick(delta, speed));
            previous = now;
            let context = RenderContext::from_theme(
                &theme,
                MotionFrame {
                    now: origin.elapsed(),
                    delta,
                    reduced_motion: model.reduced,
                    animation_speed_percent: appearance.normalized_animation_speed_percent(),
                },
                capabilities,
            );
            terminal.terminal_mut().draw(|frame| {
                bounds = frame.area();
                let layout = preview_layout(bounds, model.version);
                if model.reveal {
                    model.effect = if model.reduced || !layout.usable {
                        None
                    } else {
                        Some(reveal_effect(
                            model.version,
                            if model.dialog.open {
                                layout.dialog
                            } else {
                                layout.list.union(layout.details)
                            },
                            &context,
                        ))
                    };
                    model.reveal = false;
                }
                render_preview(frame, &model.view(), &context);
                if let Some(effect) = &mut model.effect {
                    effect.process(delta.mul_f64(speed), frame.buffer_mut(), bounds);
                    if !effect.running() {
                        model.effect = None;
                    }
                }
            })?;
            dirty = false;
            next_frame = Instant::now() + FRAME;
        }
        let timeout = if model.is_running() {
            next_frame.saturating_duration_since(Instant::now())
        } else {
            Duration::from_secs(1)
        };
        if event::poll(timeout)? {
            let input = crossterm_event_to_input(event::read()?);
            if matches!(input, InputEvent::Resize { .. }) {
                model.effect = None;
                // Only render/input geometry changes. Never retain old effect bounds.
                model.reveal = false;
            }
            let layout = preview_layout(bounds, model.version);
            if model.update(Message::Input(input, layout)) {
                break;
            }
            dirty = true;
        }
    }
    terminal.restore()
}

fn preview_capabilities() -> RenderCapabilities {
    // Reuse the production Shell detection without issuing image protocol probes.
    crate::terminal_session::text_render_capabilities()
}

fn reveal_effect(
    version: UiStyleVersion,
    area: ratatui::layout::Rect,
    context: &RenderContext,
) -> Effect {
    let effect = match version {
        UiStyleVersion::Glacier => fx::sweep_in(
            Motion::LeftToRight,
            6,
            0,
            context.theme.canvas,
            (260, Interpolation::QuadOut),
        ),
        UiStyleVersion::Tea => fx::fade_from_fg(context.theme.muted, (180, Interpolation::QuadOut)),
        UiStyleVersion::Spring => crate::spring_style::spring_reveal(area, context.theme, 320),
    };
    effect.with_area(area).with_filter(CellFilter::Text)
}

enum Message {
    Tick(Duration, f64),
    Input(InputEvent, PreviewLayout),
}

struct PreviewModel {
    version: UiStyleVersion,
    list: List,
    input: TextInput,
    replay: Button,
    inspect: Button,
    dialog: Dialog,
    focus: usize,
    reduced: bool,
    progress: AnimatedProgress,
    stage_elapsed: Duration,
    auto: bool,
    effect: Option<Effect>,
    reveal: bool,
}

impl PreviewModel {
    fn new(version: UiStyleVersion, reduced: bool) -> Self {
        let mut model = Self {
            version,
            list: List::new(
                "preview.list",
                vec![
                    ListItem::new("explorer", "Explorer / files"),
                    ListItem::new("launcher", "Launcher / apps"),
                    ListItem::new("settings", "Settings / preferences"),
                    ListItem::new("updates", "Updates / tasks"),
                ],
            )
            .titled(" Components "),
            input: TextInput::new("preview.input")
                .with_placeholder("Type here: Unicode / paste supported"),
            replay: Button::new("preview.replay", "Replay"),
            inspect: Button::new("preview.inspect", "Open dialog"),
            dialog: Dialog::new(
                "preview.dialog",
                "Style details",
                "",
                vec![DialogAction::new("close", "Back to preview")],
            ),
            focus: 0,
            reduced,
            progress: AnimatedProgress::default(),
            stage_elapsed: Duration::ZERO,
            auto: true,
            effect: None,
            reveal: true,
        };
        model.set_focus(0);
        model.progress.retarget(0.25);
        model
    }

    fn view(&self) -> PreviewView<'_> {
        PreviewView {
            version: self.version,
            list: &self.list,
            input: &self.input,
            replay: &self.replay,
            inspect: &self.inspect,
            dialog: &self.dialog,
            displayed: self.progress.motion.value().clamp(0.0, 1.0),
            target: self.progress.motion.target(),
            running: self.auto || self.progress.active(),
        }
    }

    fn is_running(&self) -> bool {
        self.auto || self.progress.active() || self.effect.is_some() || self.reveal
    }

    fn set_focus(&mut self, focus: usize) {
        self.focus = focus;
        self.list.set_focused(focus == 0);
        self.input.set_focused(focus == 1);
        self.replay.set_focused(focus == 2);
        self.inspect.set_focused(focus == 3);
    }

    fn replay(&mut self) {
        self.progress = AnimatedProgress::default();
        self.progress.retarget(0.25);
        self.stage_elapsed = Duration::ZERO;
        self.auto = true;
        self.reveal = true;
        self.effect = None;
    }

    /// Returns true only for an exit message. Components retain ownership of
    /// editing, selection, scrolling and button activation behavior.
    fn update(&mut self, message: Message) -> bool {
        let (input, layout) = match message {
            Message::Tick(delta, speed) => {
                if self.auto {
                    self.stage_elapsed += delta;
                    if self.stage_elapsed >= STAGE {
                        self.stage_elapsed -= STAGE;
                        if self.progress.motion.target() >= 1.0 {
                            self.auto = false;
                        } else {
                            self.progress
                                .retarget((self.progress.motion.target() + 0.25).min(1.0));
                        }
                    }
                }
                self.progress
                    .advance(delta.as_secs_f64() * speed, self.version, self.reduced);
                return false;
            }
            Message::Input(input, layout) => (input, layout),
        };
        if let InputEvent::Key(key) = &input {
            if !key.is_press_like() {
                return false;
            }
            if key.key == Key::Char('c') && (key.modifiers.control || key.modifiers.ctrl) {
                return true;
            }
            if key.key == Key::Escape && (!self.dialog.open || !layout.usable) {
                return true;
            }
            if !self.dialog.open {
                match key.key {
                    Key::F(number @ 1..=3) => {
                        self.version = UiStyleVersion::ALL[usize::from(number - 1)];
                        self.replay();
                        return false;
                    }
                    Key::F(4) => {
                        self.reduced = !self.reduced;
                        if self.reduced {
                            self.effect = None;
                        }
                        return false;
                    }
                    Key::F(5) => {
                        self.replay();
                        return false;
                    }
                    Key::Tab if layout.usable => {
                        self.set_focus((self.focus + 1) % 4);
                        return false;
                    }
                    Key::BackTab if layout.usable => {
                        self.set_focus((self.focus + 3) % 4);
                        return false;
                    }
                    _ => {}
                }
            }
        }
        if !layout.usable {
            return false;
        }
        if self.dialog.open {
            let event = self.dialog.handle_event(input, layout.dialog);
            if matches!(
                event,
                ComponentEvent::Activated(_) | ComponentEvent::Dismissed(_)
            ) {
                self.dialog.close();
                self.effect = None;
                self.reveal = false;
                self.set_focus(self.focus);
            }
            return false;
        }
        match &input {
            InputEvent::FocusGained => {
                self.set_focus(self.focus);
                return false;
            }
            InputEvent::FocusLost => {
                self.list.set_focused(false);
                self.input.set_focused(false);
                self.replay.set_focused(false);
                self.inspect.set_focused(false);
                return false;
            }
            InputEvent::Paste(text) => {
                // Normalize bracketed paste for the existing single-line editor;
                // the component still owns insertion and cursor movement.
                if self.focus == 1 {
                    for character in text.chars().filter(|character| !character.is_control()) {
                        self.input
                            .handle_event(InputEvent::key(Key::Char(character)), layout.input);
                    }
                }
                return false;
            }
            _ => {}
        }
        if let InputEvent::Mouse(mouse) = &input {
            if matches!(
                mouse.kind,
                ui::MouseEventKind::Down(_) | ui::MouseEventKind::Click(_)
            ) {
                let areas = [layout.list, layout.input, layout.replay, layout.inspect];
                if let Some(index) = areas
                    .iter()
                    .position(|area| contains_point(*area, mouse.column(), mouse.row()))
                {
                    self.set_focus(index);
                }
            }
        }
        let previous_selection = self.list.selected_index();
        let borderless = self.version == UiStyleVersion::Tea;
        let list_event = if borderless {
            self.list
                .handle_event_borderless(input.clone(), layout.list)
        } else {
            self.list.handle_event(input.clone(), layout.list)
        };
        if previous_selection != self.list.selected_index()
            || matches!(list_event, ComponentEvent::Activated(_))
        {
            self.auto = false;
            self.progress
                .retarget((self.list.selected_index().unwrap_or(0) + 1) as f64 / 4.0);
        }
        if borderless {
            self.input
                .handle_event_borderless(input.clone(), layout.input);
        } else {
            self.input.handle_event(input.clone(), layout.input);
        }
        if matches!(
            self.replay.handle_event(input.clone(), layout.replay),
            ComponentEvent::Activated(_)
        ) {
            self.replay();
        }
        if matches!(
            self.inspect.handle_event(input, layout.inspect),
            ComponentEvent::Activated(_)
        ) {
            self.dialog.body = vec![
                self.version.title().into(),
                match self.version {
                    UiStyleVersion::Glacier => "Native widgets + tachyonfx sweep.",
                    UiStyleVersion::Tea => "Model -> Message -> Update -> View.",
                    UiStyleVersion::Spring => "A spring retains position, velocity and target.",
                }
                .into(),
                format!("Your text: {}", self.input.value()),
                "Rust / Ratatui preview. No preferences are saved.".into(),
            ];
            self.dialog.open();
            self.effect = None;
            self.reveal = true;
        }
        false
    }
}

#[derive(Default)]
struct AnimatedProgress {
    motion: ui::SpringValue,
    start: f64,
    elapsed: f64,
}

impl AnimatedProgress {
    fn retarget(&mut self, target: f64) {
        self.start = self.motion.value();
        self.motion.retarget(target);
        self.elapsed = 0.0;
    }

    fn active(&self) -> bool {
        self.motion.is_running()
    }

    fn advance(&mut self, delta: f64, version: UiStyleVersion, reduced: bool) {
        if reduced || version == UiStyleVersion::Glacier {
            self.motion.set_value(self.motion.target());
        } else if version == UiStyleVersion::Tea {
            self.elapsed += delta;
            let t = (self.elapsed / 0.45).clamp(0.0, 1.0);
            self.motion.set_value(
                self.start + (self.motion.target() - self.start) * (1.0 - (1.0 - t).powi(3)),
            );
        } else {
            self.motion.advance(Duration::from_secs_f64(delta), false);
        }
    }
}
