mod assets;
pub mod automation;
mod foundation;
mod screens;
pub(crate) use assets::home_icons;
pub(crate) use foundation::input;
pub use screens::timezone_map;
mod editor_media;
mod terminal_images;
pub use terminal_images::TerminalImageCache;
mod motion;
mod surface_motion;
pub use surface_motion::morph_surface;
mod spring;
pub use motion::{
    MotionPhases, PanelMotion, RevealOrigin, ease_in_out_cruise, expand_trailing_pane, reveal_rect,
};
mod table_sort;
mod theme;
pub use spring::SpringValue;
pub use table_sort::{
    TableSort, compare_table_cells, equal_table_widths, render_table_headers,
    right_aligned_actions, table_header_areas,
};

pub mod components;
pub mod rainbow_text_preview;
pub mod style_preview;
pub mod ui_test;

/// Animation primitives and macros with support for Ratatui types.
///
/// ```
/// use std::time::Duration;
/// use ratatui::layout::Rect;
/// use ui::animate::{self, Clock};
///
/// #[animate::animate]
/// struct Panel {
///     #[tween(duration = 180)]
///     area: Rect,
///     #[tween(duration = 180)]
///     progress: f32,
/// }
///
/// let mut panel = Panel::new(Rect::new(0, 0, 10, 4), 0.0);
/// let target = Rect::new(10, 4, 30, 12);
/// panel.area.to(target);
/// panel.progress.to(1.0);
/// let mut clock = Clock::new();
/// panel.advance(clock.advance(Duration::ZERO));
/// assert!(panel.advance(clock.advance(Duration::from_millis(90))).running());
/// assert_eq!(*panel.area, Rect::new(5, 2, 20, 8));
/// assert_eq!(*panel.progress, 0.5);
/// assert!(panel.advance(clock.advance(Duration::from_millis(90))).finished());
/// assert_eq!(*panel.area, target);
/// assert_eq!(*panel.progress, 1.0);
/// ```
pub use animate;

pub use assets::*;
pub use editor_media::{
    EDITOR_IMAGE_MAX_PIXELS, EditorGraphicsProtocol, EditorImagePicker, EditorMediaError,
    PreparedEditorImage, TerminalGraphicsProbe, TerminalGraphicsProbeStatus,
};
pub use foundation::*;
pub use screens::timezone_map::{
    TimezoneBoundary, TimezoneBoundaryIndex, TimezoneCoordinate, TimezoneMapCity,
    TimezoneMapColors, TimezoneMapError, TimezoneMapInput, TimezoneMapRasterCache,
    TimezoneMapWidget, TimezonePolygon, boundary_id_for_timezone, timezone_boundaries,
    timezone_boundary_index,
};
pub use screens::*;
pub use theme::{
    BorderShape, ColorCapability, ComponentVisualState, FrostMotion, MotionDirection, MotionFrame,
    MotionIdentity, MotionOverlayIdentity, MotionOverlayKind, MotionSchedule, MotionTimings,
    MotionTransition, MotionTransitionKind, MotionTransitions, RenderCapabilities, RenderContext,
    SpringStyle, TerminalColors, ThemeTokens, TundraTheme, ease_in_cubic, ease_out_cubic,
    schedule_motion, schedule_motion_range,
};

pub use screens::launcher::launcher_sort_headers;
