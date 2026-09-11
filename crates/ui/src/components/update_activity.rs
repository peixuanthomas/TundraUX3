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
    pub download: UpdateMeterViewModel,
    pub compilation: UpdateMeterViewModel,
    pub output: Vec<String>,
}

impl Default for UpdateActivityViewModel {
    fn default() -> Self {
        Self {
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
    pub const HEIGHT: u16 = 17;
    pub fn new(model: &'a UpdateActivityViewModel) -> Self {
        Self(model)
    }

    pub fn render_scrolled(
        &self,
        frame: &mut Frame<'_>,
        clip: Rect,
        y: i32,
        context: &RenderContext,
    ) {
        if let Some((area, _)) = visible_scrolled_rect(clip.x, y, clip.width, 4, clip) {
            Surface::new()
                .titled(i18n::tr!("ui-components-progress-padded"))
                .bordered(true)
                .raised(true)
                .render_frame(frame, area, context);
        }
        for (index, meter) in [&self.0.download, &self.0.compilation]
            .into_iter()
            .enumerate()
        {
            if let Some((area, _)) = visible_scrolled_rect(
                clip.x.saturating_add(1),
                y + 1 + index as i32,
                clip.width.saturating_sub(2),
                1,
                clip,
            ) {
                frame.render_widget(
                    ProgressGauge::new(
                        meter.label.render_current(),
                        f64::from(
                            meter
                                .display_basis_points
                                .unwrap_or(meter.percent.unwrap_or(0).min(100) * 100)
                                .min(10_000),
                        ) / 10_000.0,
                        context.theme.accent,
                        context.theme.surface,
                        &context.theme,
                    ),
                    area,
                );
            }
        }
        let log_y = y + 5;
        if let Some((area, _)) = visible_scrolled_rect(clip.x, log_y, clip.width, 12, clip) {
            Surface::new()
                .titled(i18n::tr!("ui-components-live-output-padded"))
                .bordered(true)
                .raised(true)
                .render_frame(frame, area, context);
        }
        if let Some((area, skipped)) = visible_scrolled_rect(
            clip.x.saturating_add(1),
            log_y + 1,
            clip.width.saturating_sub(2),
            10,
            clip,
        ) {
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
            table.render_frame(frame, area, context);
        }
    }
}
