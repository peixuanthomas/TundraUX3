//! Glacier Night visual tokens and the compatibility theme used by existing
//! screens. New components consume [`ThemeTokens`]; the small `TundraTheme`
//! facade keeps older view models source-compatible while they are migrated.

use std::time::Duration;

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType};

/// Terminal colour fidelity detected by the shell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorCapability {
    #[default]
    TrueColor,
    Ansi,
}

/// Rendering features that affect visual choices without changing interaction
/// semantics. More capabilities can be added without changing component APIs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderCapabilities {
    pub color: ColorCapability,
    pub image_protocol: bool,
}

impl RenderCapabilities {
    pub const fn ansi() -> Self {
        Self {
            color: ColorCapability::Ansi,
            image_protocol: false,
        }
    }
}

/// Complete Glacier Night colour vocabulary.
///
/// The names are semantic rather than widget-specific, allowing a screen to
/// state intent without inventing another local palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeTokens {
    pub border_shape: BorderShape,
    pub canvas: Color,
    pub surface: Color,
    pub raised: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub accent_strong: Color,
    pub focus: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub shadow: Color,
}

impl ThemeTokens {
    /// The fixed Glacier Night base palette.
    pub const fn glacier_night() -> Self {
        Self {
            border_shape: BorderShape::Rounded,
            canvas: Color::Rgb(0x07, 0x11, 0x16),
            surface: Color::Rgb(0x0D, 0x1B, 0x22),
            raised: Color::Rgb(0x13, 0x26, 0x2F),
            border: Color::Rgb(0x29, 0x43, 0x4E),
            text: Color::Rgb(0xE6, 0xF1, 0xF4),
            muted: Color::Rgb(0x8E, 0xA7, 0xB0),
            accent: Color::Rgb(0x63, 0xD3, 0xE5),
            accent_soft: Color::Rgb(0x15, 0x3B, 0x46),
            accent_strong: Color::Rgb(0x76, 0xE1, 0xF1),
            focus: Color::Rgb(0xA4, 0xF1, 0xFA),
            success: Color::Rgb(0x79, 0xD6, 0x9B),
            warning: Color::Rgb(0xEB, 0xCB, 0x78),
            danger: Color::Rgb(0xF2, 0x7D, 0x89),
            shadow: Color::Rgb(0x02, 0x06, 0x08),
        }
    }

    /// Applies a user accent while deriving its dependent roles. The neutral
    /// Glacier colours remain fixed, so a preference cannot turn ordinary
    /// cards into large accent-coloured fields.
    pub fn with_accent(mut self, accent: Color) -> Self {
        self.accent = accent;
        self.accent_soft = mix(accent, self.surface, 30);
        self.accent_strong = lighten(accent, 18);
        self.focus = lighten(accent, 42);
        self
    }

    /// Uses the explicitly limited ANSI palette when true colour is not
    /// available. The mapping intentionally stays stable across terminals:
    /// black/dark-gray surfaces, white/gray text, cyan/light-cyan focus,
    /// green success, yellow warning, and light-red danger.
    pub fn for_capability(self, capability: ColorCapability) -> Self {
        if capability == ColorCapability::TrueColor {
            return self;
        }

        let accent = ansi_accent(self.accent);
        Self {
            border_shape: self.border_shape,
            canvas: Color::Black,
            surface: Color::Black,
            raised: Color::DarkGray,
            border: Color::DarkGray,
            text: Color::White,
            muted: Color::Gray,
            accent,
            accent_soft: Color::DarkGray,
            accent_strong: accent,
            focus: ansi_focus(accent),
            success: Color::Green,
            warning: Color::Yellow,
            danger: Color::LightRed,
            shadow: Color::Black,
        }
    }
}

impl Default for ThemeTokens {
    fn default() -> Self {
        Self::glacier_night()
    }
}

/// A per-frame description for terminal animation. `now` is supplied by the
/// host, which makes transition tests deterministic and avoids hidden clocks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MotionFrame {
    pub now: Duration,
    pub delta: Duration,
    pub reduced_motion: bool,
}

impl MotionFrame {
    pub const fn reduced(now: Duration) -> Self {
        Self {
            now,
            delta: Duration::ZERO,
            reduced_motion: true,
        }
    }
}

/// Fixed Frost Motion durations. A reduced frame resolves every duration to
/// zero while retaining the caller's ordinary event-driven renders.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MotionTimings;

impl MotionTimings {
    pub const FOCUS: Duration = Duration::from_millis(120);
    pub const POPOVER: Duration = Duration::from_millis(160);
    pub const DIALOG: Duration = Duration::from_millis(180);
    pub const PAGE: Duration = Duration::from_millis(220);
    pub const TOAST_ENTER: Duration = Duration::from_millis(200);
    pub const TOAST_EXIT: Duration = Duration::from_millis(150);

    pub const fn resolve(frame: MotionFrame, duration: Duration) -> Duration {
        if frame.reduced_motion {
            Duration::ZERO
        } else {
            duration
        }
    }
}

/// A small transition tracker for hosts that request redraws. It reports an
/// active transition only until its deadline; callers therefore schedule at
/// most 60 FPS while active and do not keep an idle timer alive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrostMotion {
    started_at: Option<Duration>,
    duration: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionOverlayKind {
    Dialog,
    Popover,
    Toast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionOverlayIdentity<'a> {
    pub kind: MotionOverlayKind,
    pub id: &'a str,
}

/// Stable identities observed by the shell around a state change. Overlay
/// identities retain their semantic type so a popover never inherits dialog
/// timing merely because both happen to be overlays.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MotionIdentity<'a> {
    pub screen: Option<&'a str>,
    pub focus: Option<&'a str>,
    pub overlay: Option<MotionOverlayIdentity<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionTransitionKind {
    Page,
    Focus,
    Dialog,
    Popover,
    Toast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionDirection {
    Entering,
    Exiting,
    Replacing,
}

/// A render-ready transition. `progress` is the visible amount in thousandths:
/// entering/replacing transitions advance from 0 to 1000 and exits recede from
/// 1000 to 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionTransition {
    pub kind: MotionTransitionKind,
    pub direction: MotionDirection,
    pub progress: u16,
    /// Progress through this transition's own interval, independent of the
    /// visible value's start and target range.
    pub phase_progress: u16,
    pub active: bool,
    pub next_redraw_in: Duration,
}

impl MotionTransition {
    pub const fn interaction_ready(self) -> bool {
        !self.active || matches!(self.direction, MotionDirection::Exiting) || self.progress >= 500
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MotionTransitions {
    pub screen: Option<MotionTransition>,
    pub focus: Option<MotionTransition>,
    pub overlay: Option<MotionTransition>,
}

/// Pure redraw decision for one transition interval. The caller owns the
/// change timestamp, so interruption and reversal restart deterministically
/// without a hidden clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MotionSchedule {
    pub screen_transition: bool,
    pub focus_transition: bool,
    pub overlay_transition: bool,
    pub active: bool,
    pub next_redraw_in: Duration,
    pub transitions: MotionTransitions,
}

pub fn schedule_motion(
    prior: MotionIdentity<'_>,
    current: MotionIdentity<'_>,
    changed_at: Duration,
    frame: MotionFrame,
) -> MotionSchedule {
    if frame.reduced_motion {
        return MotionSchedule::default();
    }
    let screen = (prior.screen != current.screen).then(|| {
        schedule_motion_range(
            MotionTransitionKind::Page,
            MotionDirection::Replacing,
            0,
            1_000,
            changed_at,
            frame,
        )
    });
    let focus = (prior.focus != current.focus).then(|| {
        schedule_motion_range(
            MotionTransitionKind::Focus,
            MotionDirection::Replacing,
            0,
            1_000,
            changed_at,
            frame,
        )
    });
    let overlay = (prior.overlay != current.overlay).then(|| {
        let (overlay, direction) = match (prior.overlay, current.overlay) {
            (None, Some(current)) => (current, MotionDirection::Entering),
            (Some(prior), None) => (prior, MotionDirection::Exiting),
            (Some(_), Some(current)) => (current, MotionDirection::Replacing),
            (None, None) => unreachable!("unchanged overlays do not schedule motion"),
        };
        schedule_motion_range(
            match overlay.kind {
                MotionOverlayKind::Dialog => MotionTransitionKind::Dialog,
                MotionOverlayKind::Popover => MotionTransitionKind::Popover,
                MotionOverlayKind::Toast => MotionTransitionKind::Toast,
            },
            direction,
            if matches!(direction, MotionDirection::Exiting) {
                1_000
            } else {
                0
            },
            if matches!(direction, MotionDirection::Exiting) {
                0
            } else {
                1_000
            },
            changed_at,
            frame,
        )
    });
    let transitions = MotionTransitions {
        screen,
        focus,
        overlay,
    };
    let screen_transition = screen.is_some_and(|transition| transition.active);
    let focus_transition = focus.is_some_and(|transition| transition.active);
    let overlay_transition = overlay.is_some_and(|transition| transition.active);
    let active = screen_transition || focus_transition || overlay_transition;
    let next_redraw_in = [screen, focus, overlay]
        .into_iter()
        .flatten()
        .filter(|transition| transition.active)
        .map(|transition| transition.next_redraw_in)
        .min()
        .unwrap_or(Duration::ZERO);
    MotionSchedule {
        screen_transition,
        focus_transition,
        overlay_transition,
        active,
        next_redraw_in,
        transitions,
    }
}

pub fn schedule_motion_range(
    kind: MotionTransitionKind,
    direction: MotionDirection,
    start_progress: u16,
    target_progress: u16,
    changed_at: Duration,
    frame: MotionFrame,
) -> MotionTransition {
    if frame.reduced_motion {
        return MotionTransition {
            kind,
            direction,
            progress: target_progress.min(1_000),
            phase_progress: 1_000,
            active: false,
            next_redraw_in: Duration::ZERO,
        };
    }
    let full_duration = match (kind, direction) {
        (MotionTransitionKind::Page, _) => MotionTimings::PAGE,
        (MotionTransitionKind::Focus, _) => MotionTimings::FOCUS,
        (MotionTransitionKind::Dialog, _) => MotionTimings::DIALOG,
        (MotionTransitionKind::Popover, _) => MotionTimings::POPOVER,
        (MotionTransitionKind::Toast, MotionDirection::Exiting) => MotionTimings::TOAST_EXIT,
        (MotionTransitionKind::Toast, _) => MotionTimings::TOAST_ENTER,
    };
    let distance = start_progress.abs_diff(target_progress);
    let duration = full_duration.mul_f64(f64::from(distance) / 1_000.0);
    let elapsed = frame.now.saturating_sub(changed_at);
    let active = elapsed < duration;
    let normalized = if duration.is_zero() {
        1_000
    } else {
        (elapsed.as_millis().saturating_mul(1_000) / duration.as_millis().max(1)).min(1_000) as u16
    };
    let phase_progress = match direction {
        MotionDirection::Entering | MotionDirection::Replacing => ease_out_cubic(normalized),
        MotionDirection::Exiting => ease_in_cubic(normalized),
    };
    let progress = interpolate_progress(start_progress, target_progress, phase_progress);
    MotionTransition {
        kind,
        direction,
        progress,
        phase_progress,
        active,
        next_redraw_in: if active {
            duration
                .saturating_sub(elapsed)
                .min(Duration::from_nanos(16_666_667))
        } else {
            Duration::ZERO
        },
    }
}

fn interpolate_progress(start: u16, target: u16, phase: u16) -> u16 {
    let start = i32::from(start.min(1_000));
    let delta = i32::from(target.min(1_000)) - start;
    (start + delta * i32::from(phase.min(1_000)) / 1_000).clamp(0, 1_000) as u16
}

impl FrostMotion {
    pub fn begin(&mut self, frame: MotionFrame, duration: Duration) {
        let duration = MotionTimings::resolve(frame, duration);
        self.started_at = (!duration.is_zero()).then_some(frame.now);
        self.duration = duration;
    }

    pub fn cancel(&mut self) {
        self.started_at = None;
        self.duration = Duration::ZERO;
    }

    pub fn is_active(&self, frame: MotionFrame) -> bool {
        self.started_at
            .is_some_and(|started_at| frame.now.saturating_sub(started_at) < self.duration)
    }

    /// Returns whether the host should request another animation redraw. It
    /// becomes false immediately for Reduced Motion and after the end frame.
    pub fn requests_redraw(&self, frame: MotionFrame) -> bool {
        !frame.reduced_motion && self.is_active(frame)
    }

    pub fn progress(&self, frame: MotionFrame, entering: bool) -> u16 {
        let Some(started_at) = self.started_at else {
            return if entering { 1_000 } else { 0 };
        };
        if self.duration.is_zero() {
            return if entering { 1_000 } else { 0 };
        }
        let elapsed = frame.now.saturating_sub(started_at).as_millis();
        let duration = self.duration.as_millis().max(1);
        let normalized = (elapsed.saturating_mul(1_000) / duration).min(1_000) as u16;
        if entering {
            ease_out_cubic(normalized)
        } else {
            1_000_u16.saturating_sub(ease_in_cubic(normalized))
        }
    }
}

/// Cubic easing in thousandths, avoiding floating point and preserving stable
/// visual tests across platforms.
pub const fn ease_out_cubic(value: u16) -> u16 {
    let inverse = 1_000_u32.saturating_sub(value as u32);
    let cubed = inverse.saturating_mul(inverse).saturating_mul(inverse) / 1_000_000;
    1_000_u16.saturating_sub(cubed as u16)
}

pub const fn ease_in_cubic(value: u16) -> u16 {
    let value = value as u32;
    (value.saturating_mul(value).saturating_mul(value) / 1_000_000) as u16
}

/// State used by new components. It maps directly to the legacy
/// `ComponentState` but names `pressed` rather than an implementation detail.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ComponentVisualState {
    pub focused: bool,
    pub selected: bool,
    pub pressed: bool,
    pub disabled: bool,
}

/// Inputs shared by every Glacier component render. Components accept the
/// legacy theme overloads too, so screen migration can be incremental.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderContext {
    pub theme: ThemeTokens,
    pub motion: MotionFrame,
    pub transitions: MotionTransitions,
    pub capabilities: RenderCapabilities,
}

impl RenderContext {
    pub fn from_theme(
        theme: &TundraTheme,
        motion: MotionFrame,
        capabilities: RenderCapabilities,
    ) -> Self {
        Self {
            theme: theme.tokens().for_capability(capabilities.color),
            motion,
            transitions: MotionTransitions::default(),
            capabilities,
        }
    }

    pub fn from_theme_with_transitions(
        theme: &TundraTheme,
        motion: MotionFrame,
        transitions: MotionTransitions,
        capabilities: RenderCapabilities,
    ) -> Self {
        let mut context = Self::from_theme(theme, motion, capabilities);
        context.transitions = transitions;
        context.apply_transition_colors();
        context
    }

    /// Moves an entering page by one terminal row for the first half of its
    /// transition. The shell uses this same projection for its hit map.
    pub fn page_area(self, area: ratatui::layout::Rect) -> ratatui::layout::Rect {
        let entering_page = self.transitions.screen.is_some_and(|transition| {
            transition.active
                && !matches!(transition.direction, MotionDirection::Exiting)
                && transition.progress < 500
        });
        let exiting_overlay = self.transitions.overlay.is_some_and(|transition| {
            transition.active
                && matches!(transition.direction, MotionDirection::Exiting)
                && matches!(
                    transition.kind,
                    MotionTransitionKind::Dialog | MotionTransitionKind::Popover
                )
                && (250..750).contains(&transition.phase_progress)
        });
        let shifted = entering_page || exiting_overlay;
        if shifted && area.height > 0 {
            ratatui::layout::Rect::new(
                area.x,
                area.y.saturating_add(1),
                area.width,
                area.height.saturating_sub(1),
            )
        } else {
            area
        }
    }

    pub fn overlay_interaction_ready(self) -> bool {
        self.transitions
            .overlay
            .is_none_or(MotionTransition::interaction_ready)
    }

    fn apply_transition_colors(&mut self) {
        let Some(transition) = self
            .transitions
            .overlay
            .filter(|transition| {
                transition.active && transition.direction != MotionDirection::Exiting
            })
            .or_else(|| {
                self.transitions
                    .focus
                    .filter(|transition| transition.active)
            })
        else {
            return;
        };
        let progress = transition.progress;
        self.theme.border = interpolate_color(self.theme.canvas, self.theme.border, progress);
        self.theme.focus = interpolate_color(self.theme.border, self.theme.focus, progress);
        if !matches!(transition.kind, MotionTransitionKind::Focus) {
            self.theme.raised = interpolate_color(self.theme.surface, self.theme.raised, progress);
            self.theme.accent_strong =
                interpolate_color(self.theme.border, self.theme.accent_strong, progress);
        }
    }

    /// Convenience boundary for hosts that persist a Full/Reduced preference
    /// outside the UI crate. Keeping this as a boolean avoids making the UI
    /// depend directly on the storage format.
    pub fn from_theme_with_motion_preference(
        theme: &TundraTheme,
        now: Duration,
        reduced_motion: bool,
        capabilities: RenderCapabilities,
    ) -> Self {
        Self::from_theme(
            theme,
            MotionFrame {
                now,
                delta: Duration::ZERO,
                reduced_motion,
            },
            capabilities,
        )
    }

    /// Compatibility theme derived exclusively from this frame's resolved
    /// tokens. This lets legacy renderers participate in context-aware paths
    /// without losing ANSI capability resolution or user colours.
    pub const fn compatibility_theme(self) -> TundraTheme {
        TundraTheme {
            background: self.theme.canvas,
            foreground: self.theme.text,
            accent_color: self.theme.accent,
            muted: self.theme.muted,
            error: self.theme.danger,
            border_color: self.theme.border,
            border_shape: self.theme.border_shape,
        }
    }
}

fn interpolate_color(from: Color, to: Color, progress: u16) -> Color {
    let progress = progress.min(1_000);
    let (Color::Rgb(fr, fg, fb), Color::Rgb(tr, tg, tb)) = (from, to) else {
        return if progress < 500 { from } else { to };
    };
    let channel = |from: u8, to: u8| {
        let from = i32::from(from);
        let delta = i32::from(to) - from;
        (from + delta * i32::from(progress) / 1_000).clamp(0, 255) as u8
    };
    Color::Rgb(channel(fr, tr), channel(fg, tg), channel(fb, tb))
}

impl Default for RenderContext {
    fn default() -> Self {
        Self::from_theme(
            &TundraTheme::default(),
            MotionFrame::default(),
            RenderCapabilities::default(),
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BorderShape {
    #[default]
    Rounded,
    Square,
}

impl BorderShape {
    pub const fn border_type(self) -> BorderType {
        match self {
            Self::Rounded => BorderType::Rounded,
            Self::Square => BorderType::Plain,
        }
    }
}

/// Compatibility facade for existing screen models. Its default values are
/// Glacier Night, and all richer token roles are available through `tokens()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TundraTheme {
    pub background: Color,
    pub foreground: Color,
    /// Color used for selected items, focus affordances, and other emphasis.
    pub accent_color: Color,
    pub muted: Color,
    pub error: Color,
    pub border_color: Color,
    pub border_shape: BorderShape,
}

impl TundraTheme {
    pub fn default_dark() -> Self {
        let tokens = ThemeTokens::glacier_night();
        Self {
            background: tokens.canvas,
            foreground: tokens.text,
            accent_color: tokens.accent,
            muted: tokens.muted,
            error: tokens.danger,
            border_color: tokens.border,
            border_shape: BorderShape::Rounded,
        }
    }

    pub fn with_border_shape(mut self, border_shape: BorderShape) -> Self {
        self.border_shape = border_shape;
        self
    }

    pub fn with_border_color(mut self, border_color: Color) -> Self {
        self.border_color = border_color;
        self
    }

    pub fn with_accent_color(mut self, accent_color: Color) -> Self {
        self.accent_color = accent_color;
        self
    }

    pub fn tokens(&self) -> ThemeTokens {
        let mut tokens = ThemeTokens::glacier_night().with_accent(self.accent_color);
        tokens.border_shape = self.border_shape;
        tokens.canvas = self.background;
        tokens.text = self.foreground;
        tokens.muted = self.muted;
        tokens.border = self.border_color;
        tokens.danger = self.error;
        tokens
    }

    pub const fn border_type(&self) -> BorderType {
        self.border_shape.border_type()
    }

    pub fn block(&self) -> Block<'static> {
        Block::default()
            .border_type(self.border_type())
            .border_style(self.border_style())
    }

    pub fn border_style(&self) -> Style {
        solid_border_style(Style::default().fg(self.border_color).bg(self.background))
    }

    pub fn selectable_border_style(&self, selected: bool) -> Style {
        let tokens = self.tokens();
        let color = if selected {
            tokens.focus
        } else {
            tokens.border
        };
        solid_border_style(Style::default().fg(color).bg(tokens.surface))
    }

    pub fn title_style(&self) -> Style {
        let tokens = self.tokens();
        Style::default()
            .fg(tokens.accent)
            .bg(tokens.canvas)
            .add_modifier(Modifier::BOLD)
    }

    pub fn body_style(&self) -> Style {
        Style::default().fg(self.foreground).bg(self.background)
    }

    pub fn surface_style(&self) -> Style {
        let tokens = self.tokens();
        Style::default().fg(tokens.text).bg(tokens.surface)
    }

    pub fn raised_style(&self) -> Style {
        let tokens = self.tokens();
        Style::default().fg(tokens.text).bg(tokens.raised)
    }

    pub fn muted_style(&self) -> Style {
        Style::default().fg(self.muted).bg(self.background)
    }

    pub fn error_style(&self) -> Style {
        Style::default().fg(self.error).bg(self.background)
    }
}

/// Keep box-drawing glyphs at regular weight. Some terminal fonts render bold
/// vertical borders with gaps between rows, which makes a solid border look dashed.
pub(crate) fn solid_border_style(style: Style) -> Style {
    style.remove_modifier(Modifier::BOLD)
}

impl Default for TundraTheme {
    fn default() -> Self {
        Self::default_dark()
    }
}

fn mix(foreground: Color, background: Color, foreground_percent: u8) -> Color {
    let (Color::Rgb(fr, fg, fb), Color::Rgb(br, bg, bb)) = (foreground, background) else {
        return Color::DarkGray;
    };
    let foreground_percent = u16::from(foreground_percent.min(100));
    let background_percent = 100_u16.saturating_sub(foreground_percent);
    let mix_channel = |foreground: u8, background: u8| {
        ((u16::from(foreground) * foreground_percent + u16::from(background) * background_percent)
            / 100) as u8
    };
    Color::Rgb(
        mix_channel(fr, br),
        mix_channel(fg, bg),
        mix_channel(fb, bb),
    )
}

fn lighten(color: Color, amount: u8) -> Color {
    let Color::Rgb(red, green, blue) = color else {
        return match color {
            Color::Cyan => Color::LightCyan,
            Color::Red => Color::LightRed,
            Color::Green => Color::LightGreen,
            Color::Yellow => Color::LightYellow,
            Color::Blue => Color::LightBlue,
            Color::Magenta => Color::LightMagenta,
            other => other,
        };
    };
    let lift = |channel: u8| {
        channel.saturating_add(((255_u16 - u16::from(channel)) * u16::from(amount) / 100) as u8)
    };
    Color::Rgb(lift(red), lift(green), lift(blue))
}

fn ansi_accent(color: Color) -> Color {
    match color {
        Color::Green | Color::LightGreen => Color::Green,
        Color::Yellow | Color::LightYellow => Color::Yellow,
        Color::Red | Color::LightRed => Color::LightRed,
        Color::Rgb(red, green, blue) if red > green.saturating_add(32) && red > blue => {
            Color::LightRed
        }
        Color::Rgb(red, green, blue)
            if green > red.saturating_add(24) && green > blue.saturating_add(24) =>
        {
            Color::Green
        }
        Color::Rgb(red, green, blue) if red.saturating_add(green) > blue.saturating_add(90) => {
            Color::Yellow
        }
        _ => Color::Cyan,
    }
}

fn ansi_focus(accent: Color) -> Color {
    match accent {
        Color::Cyan => Color::LightCyan,
        Color::Green => Color::LightGreen,
        Color::Yellow => Color::LightYellow,
        Color::LightRed => Color::LightRed,
        other => other,
    }
}
