use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSliderLayout {
    pub field: SettingsField,
    pub area: Rect,
    pub options: Vec<Rect>,
    skipped_rows: u16,
}

impl SettingsSliderLayout {
    pub(super) fn new(viewport: Rect, y: i32, item: &SettingsItemViewModel) -> Option<Self> {
        let option_width = item
            .options
            .iter()
            .map(|label| terminal_width(label))
            .max()?
            + 4;
        let width = viewport
            .width
            .saturating_sub(2)
            .min(u16::try_from(option_width * item.options.len()).unwrap_or(u16::MAX));
        let (area, skipped_rows) = visible_scrolled_rect(viewport.x + 1, y, width, 2, viewport)?;
        let options = Layout::horizontal(vec![Constraint::Fill(1); item.options.len()])
            .split(area)
            .to_vec();
        Some(Self {
            field: item.field,
            area,
            options,
            skipped_rows,
        })
    }

    pub fn option_at(&self, point: (u16, u16)) -> Option<usize> {
        self.options.iter().position(|area| contains(*area, point))
    }

    pub(super) fn render(
        &self,
        frame: &mut Frame<'_>,
        item: &SettingsItemViewModel,
        focused: bool,
        theme: &TundraTheme,
    ) {
        let SettingsControlKind::Slider { selected } = item.kind else {
            return;
        };
        for (index, (area, label)) in self.options.iter().zip(&item.options).enumerate() {
            let label = truncate_to_terminal_width(label, usize::from(area.width));
            let mut button = Button::new(
                format!("settings.slider.{:?}.{index}", self.field).to_ascii_lowercase(),
                if self.skipped_rows == 0 {
                    format!("\n{label}")
                } else {
                    label
                },
            )
            .with_bracketed_label(false)
            .with_current(index == selected)
            .with_capture_role("radio")
            .with_capture_name(&item.options[index])
            .with_capture_selected(index == selected);
            button.set_focused(focused && index == selected);
            button.set_disabled(!item.enabled);
            button.render_borderless_frame(frame, *area, theme);
        }
        if self.skipped_rows == 0 && !self.area.is_empty() {
            let centers: Vec<_> = self
                .options
                .iter()
                .map(|area| area.x + area.width / 2)
                .collect();
            let buffer = frame.buffer_mut();
            for x in centers[0]..=centers[centers.len() - 1] {
                buffer[(x, self.area.y)].set_symbol("─");
            }
            for (index, x) in centers.into_iter().enumerate() {
                buffer[(x, self.area.y)].set_symbol(if index == selected { "●" } else { "○" });
            }
        }
    }
}
