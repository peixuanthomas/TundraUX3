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

/// Stable identities observed by the shell around a state change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MotionIdentity<'a> {
    pub screen: Option<&'a str>,
    pub focus: Option<&'a str>,
    pub overlay: Option<&'a str>,
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
    let elapsed = frame.now.saturating_sub(changed_at);
    let screen_transition = prior.screen != current.screen && elapsed < MotionTimings::PAGE;
    let focus_transition = prior.focus != current.focus && elapsed < MotionTimings::FOCUS;
    let overlay_transition = prior.overlay != current.overlay
        && elapsed
            < if current.overlay.is_some() {
                MotionTimings::DIALOG
            } else {
                MotionTimings::POPOVER
            };
    let mut remaining = Duration::MAX;
    if screen_transition {
        remaining = remaining.min(MotionTimings::PAGE.saturating_sub(elapsed));
    }
    if focus_transition {
        remaining = remaining.min(MotionTimings::FOCUS.saturating_sub(elapsed));
    }
    if overlay_transition {
        let duration = if current.overlay.is_some() {
            MotionTimings::DIALOG
        } else {
            MotionTimings::POPOVER
        };
        remaining = remaining.min(duration.saturating_sub(elapsed));
    }
    let active = screen_transition || focus_transition || overlay_transition;
    MotionSchedule {
        screen_transition,
        focus_transition,
        overlay_transition,
        active,
        next_redraw_in: if active {
            remaining.min(Duration::from_nanos(16_666_667))
        } else {
            Duration::ZERO
        },
    }
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
            capabilities,
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
