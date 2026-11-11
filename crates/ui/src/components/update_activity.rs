use super::{DataTable, ProgressGauge, Surface, visible_scrolled_rect};
use crate::RenderContext;
use ratatui::{Frame, layout::Rect};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateMeterViewModel {
    pub percent: Option<u16>,
    /// Animated fill in basis points; real values and labels remain authoritative.
    pub display_basis_points: Option<u16>,
    pub label: i18n::LocalizedText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateActivityViewModel {
    pub show_build_details: bool,
    pub download: UpdateMeterViewModel,
    pub compilation: UpdateMeterViewModel,
    pub output: Vec<String>,
}

impl Default for UpdateActivityViewModel {
    fn default() -> Self {
        Self {
            show_build_details: false,
            download: UpdateMeterViewModel {
                percent: None,
                display_basis_points: None,
                label: i18n::msg!("ui-components-download-waiting").into(),
            },
            compilation: UpdateMeterViewModel {
                percent: None,
                display_basis_points: None,
                label: i18n::msg!("ui-components-compilation-waiting").into(),
            },
            output: Vec::new(),
        }
    }
}

/// Read-only progress and a tail-following output viewport. The containing
/// settings page keeps ownership of focus and page scrolling.
pub struct UpdateActivity<'a>(&'a UpdateActivityViewModel);

impl<'a> UpdateActivity<'a> {
    pub fn new(model: &'a UpdateActivityViewModel) -> Self {
        Self(model)
    }

    pub fn height(&self) -> u16 {
        if self.0.show_build_details { 15 } else { 2 }
    }

    pub fn render_scrolled(
        &self,
        frame: &mut Frame<'_>,
        clip: Rect,
        y: i32,
        context: &RenderContext,
    ) {
        let progress_height = if self.0.show_build_details { 3 } else { 2 };
        Surface::new()
            .titled(i18n::tr!("ui-components-progress-padded"))
            .render_scrolled_frame(frame, clip, y, progress_height, context);
        for (index, meter) in std::iter::once(&self.0.download)
            .chain(self.0.show_build_details.then_some(&self.0.compilation))
            .enumerate()
        {
            if let Some((area, _)) =
                visible_scrolled_rect(clip.x, y + 1 + index as i32, clip.width, 1, clip)
            {
                frame.render_widget(
                    ProgressGauge::new(
                        meter.label.render_current(),
                        f64::from(
                            meter
                                .display_basis_points
                                .unwrap_or(meter.percent.unwrap_or(0).min(100) * 100)
                                .min(10_000),
                        ) / 10_000.0,
                        if meter.percent.is_some_and(|percent| percent >= 100) {
                            context.theme.success
                        } else {
                            context.theme.accent
                        },
                        context.theme.surface,
                        &context.theme,
                    ),
                    area,
                );
            }
        }
        if !self.0.show_build_details {
            return;
        }
        let log_y = y + 4;
        Surface::new()
            .titled(i18n::tr!("ui-components-live-output-padded"))
            .render_scrolled_frame(frame, clip, log_y, 11, context);
        if let Some((area, skipped)) =
            visible_scrolled_rect(clip.x, log_y + 1, clip.width, 10, clip)
        {
            let rows = self
                .0
                .output
                .iter()
                .map(|line| vec![line.clone()])
                .collect::<Vec<_>>();
            let start = rows
                .len()
                .saturating_sub(10)
                .saturating_add(usize::from(skipped));
            let mut table = DataTable::new("settings.update.output", Vec::<String>::new(), rows)
                .show_header(false)
                .bordered(false)
                .with_viewport_start(start);
            table.selected = None;
            table.interactive = false;
            table.render_frame(frame, area, context);
        }
    }
}
