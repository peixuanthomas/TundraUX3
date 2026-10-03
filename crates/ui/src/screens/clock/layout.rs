use ratatui::layout::Rect;

use super::model::{ClockEntryViewModel, ClockViewModel};

const CLOCK_ANALOG_MIN_WIDTH: u16 = 76;
const CLOCK_ANALOG_MIN_HEIGHT: u16 = 18;
const CLOCK_PANEL_MIN_WIDTH: u16 = 28;
const CLOCK_PANEL_MAX_WIDTH: u16 = 34;
const CLOCK_COLUMN_GAP: u16 = 1;
const CLOCK_CREATE_DIALOG_WIDTH: u16 = 58;
const CLOCK_CREATE_DIALOG_HEIGHT: u16 = 11;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockPageMode {
    Analog,
    DigitalOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockEntryKind {
    Alarm,
    Countdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockEntryRowLayout {
    pub id: u64,
    pub kind: ClockEntryKind,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockCreateDialogLayout {
    pub dialog: Rect,
    pub prompt: Rect,
    pub input: Rect,
    pub labels: [Rect; 3],
    pub increments: [Rect; 3],
    pub values: [Rect; 3],
    pub decrements: [Rect; 3],
    pub error: Rect,
    pub create_alarm: Rect,
    pub create_countdown: Rect,
    pub cancel: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockPageLayout {
    pub mode: ClockPageMode,
    /// Outer area of the clock block.
    pub clock: Rect,
    /// ASCII face canvas; absent in the narrow digital-only layout.
    pub analog: Option<Rect>,
    /// Exact date and digital-time content area.
    pub digital: Rect,
    /// Outer area of the alarms and countdowns block.
    pub panel: Rect,
    pub new_button: Rect,
    pub manage_button: Rect,
    pub help: Rect,
    pub scrollbar: Option<Rect>,
    pub alarms_heading: Rect,
    pub countdowns_heading: Rect,
    pub entry_rows: Vec<ClockEntryRowLayout>,
    /// Effective offset into alarms followed by countdowns.
    pub entry_window_start: usize,
    pub entry_capacity: usize,
    pub create_dialog: Option<ClockCreateDialogLayout>,
}

/// Computes every interactive rectangle used by the Clock page.
///
/// Callers should pass the `main` rectangle from [`compute_shell_layout`]. The
/// renderer and input routing can then share this value without duplicating
/// geometry or visible-entry window calculations.
pub fn clock_page_layout(main: Rect, model: &ClockViewModel) -> ClockPageLayout {
    let mode = if main.width >= CLOCK_ANALOG_MIN_WIDTH && main.height >= CLOCK_ANALOG_MIN_HEIGHT {
        ClockPageMode::Analog
    } else {
        ClockPageMode::DigitalOnly
    };

    let (clock, analog, digital, panel) = match mode {
        ClockPageMode::Analog => {
            let panel_width = (main.width / 3)
                .clamp(CLOCK_PANEL_MIN_WIDTH, CLOCK_PANEL_MAX_WIDTH)
                .min(main.width.saturating_sub(CLOCK_COLUMN_GAP));
            let clock_width = main
                .width
                .saturating_sub(panel_width)
                .saturating_sub(CLOCK_COLUMN_GAP);
            let clock = Rect::new(main.x, main.y, clock_width, main.height);
            let panel = Rect::new(
                main.x
                    .saturating_add(clock_width)
                    .saturating_add(CLOCK_COLUMN_GAP),
                main.y,
                panel_width,
                main.height,
            );
            let inner = inset_rect(clock, 1);
            let digital_height = 2.min(inner.height);
            let digital = Rect::new(
                inner.x,
                inner
                    .y
                    .saturating_add(inner.height.saturating_sub(digital_height)),
                inner.width,
                digital_height,
            );
            let face_height = inner
                .height
                .saturating_sub(digital_height)
                .saturating_sub(1);
            let analog = (inner.width > 0 && face_height > 0).then_some(Rect::new(
                inner.x,
                inner.y,
                inner.width,
                face_height,
            ));
            (clock, analog, digital, panel)
        }
        ClockPageMode::DigitalOnly => {
            if main.width < 50 {
                let clock = Rect::new(main.x, main.y, main.width, main.height.min(2));
                let panel = Rect::new(
                    main.x,
                    clock.bottom(),
                    main.width,
                    main.height.saturating_sub(clock.height),
                );
                (clock, None, clock, panel)
            } else {
                let panel_width = (main.width / 2)
                    .clamp(CLOCK_PANEL_MIN_WIDTH, CLOCK_PANEL_MAX_WIDTH)
                    .min(main.width.saturating_sub(17));
                let clock_width = main
                    .width
                    .saturating_sub(panel_width)
                    .saturating_sub(CLOCK_COLUMN_GAP);
                let clock = Rect::new(main.x, main.y, clock_width, main.height);
                let panel = Rect::new(
                    main.x
                        .saturating_add(clock_width)
                        .saturating_add(CLOCK_COLUMN_GAP),
                    main.y,
                    panel_width,
                    main.height,
                );
                let digital = inset_rect(clock, 1);
                (clock, None, digital, panel)
            }
        }
    };

    let mut panel_inner = inset_rect(panel, 1);
    let initial_capacity =
        usize::from(panel_inner.height.saturating_sub(if model.is_read_only() {
            2
        } else if panel_inner.height < 7 {
            3
        } else {
            4
        }));
    let scrollbar = (model.alarms.len().saturating_add(model.countdowns.len()) > initial_capacity
        && initial_capacity > 0
        && panel_inner.width > 1
        && panel_inner.height > 1)
        .then(|| {
            Rect::new(
                panel_inner.right() - 1,
                panel_inner.y + u16::from(!model.is_read_only()),
                1,
                panel_inner.height - u16::from(!model.is_read_only()),
            )
        });
    panel_inner.width = panel_inner
        .width
        .saturating_sub(u16::from(scrollbar.is_some()));
    let new_button = if model.is_read_only() {
        Rect::new(panel_inner.x, panel_inner.y, 0, 0)
    } else {
        Rect::new(panel_inner.x, panel_inner.y, panel_inner.width / 2, 1)
    };
    let condensed_panel = panel_inner.height < 7;
    let reserved_lines = if model.is_read_only() {
        2
    } else if condensed_panel {
        3
    } else {
        4
    };
    let manage_button = if model.is_read_only() {
        Rect::default()
    } else {
        Rect::new(
            new_button.right(),
            new_button.y,
            panel_inner.width.saturating_sub(new_button.width),
            new_button.height,
        )
    };
    let entry_capacity = usize::from(panel_inner.height.saturating_sub(reserved_lines));
    let help = if !condensed_panel && !model.is_read_only() {
        line_in_rect(panel_inner, panel_inner.y + 1)
    } else {
        Rect::default()
    };
    let total_entries = model.alarms.len().saturating_add(model.countdowns.len());
    let entry_window_start = model
        .entry_window_start
        .min(total_entries.saturating_sub(entry_capacity));
    let visible = flattened_clock_entries(model)
        .into_iter()
        .skip(entry_window_start)
        .take(entry_capacity)
        .collect::<Vec<_>>();
    let visible_alarm_count = visible
        .iter()
        .filter(|(kind, _)| *kind == ClockEntryKind::Alarm)
        .count();

    let alarms_heading = line_in_rect(
        panel_inner,
        panel_inner.y.saturating_add(if model.is_read_only() {
            0
        } else if condensed_panel {
            1
        } else {
            2
        }),
    );
    let alarm_rows_y = alarms_heading.y.saturating_add(alarms_heading.height);
    let countdowns_heading = line_in_rect(
        panel_inner,
        alarm_rows_y.saturating_add(usize_to_u16(visible_alarm_count)),
    );
    let countdown_rows_y = countdowns_heading
        .y
        .saturating_add(countdowns_heading.height);
    let mut alarm_row = 0_u16;
    let mut countdown_row = 0_u16;
    let entry_rows = visible
        .into_iter()
        .filter_map(|(kind, entry)| {
            let y = match kind {
                ClockEntryKind::Alarm => {
                    let y = alarm_rows_y.saturating_add(alarm_row);
                    alarm_row = alarm_row.saturating_add(1);
                    y
                }
                ClockEntryKind::Countdown => {
                    let y = countdown_rows_y.saturating_add(countdown_row);
                    countdown_row = countdown_row.saturating_add(1);
                    y
                }
            };
            let area = line_in_rect(panel_inner, y);
            (area.width > 0 && area.height > 0).then_some(ClockEntryRowLayout {
                id: entry.id,
                kind,
                area,
            })
        })
        .collect();

    ClockPageLayout {
        mode,
        clock,
        analog,
        digital,
        panel,
        new_button,
        manage_button,
        help,
        scrollbar,
        alarms_heading,
        countdowns_heading,
        entry_rows,
        entry_window_start,
        entry_capacity,
        create_dialog: (!model.is_read_only())
            .then_some(model.create_dialog.as_ref())
            .flatten()
            .map(|_| clock_create_dialog_layout(main)),
    }
}

fn flattened_clock_entries(model: &ClockViewModel) -> Vec<(ClockEntryKind, &ClockEntryViewModel)> {
    model
        .alarms
        .iter()
        .map(|entry| (ClockEntryKind::Alarm, entry))
        .chain(
            model
                .countdowns
                .iter()
                .map(|entry| (ClockEntryKind::Countdown, entry)),
        )
        .collect()
}

fn clock_create_dialog_layout(area: Rect) -> ClockCreateDialogLayout {
    let dialog = centered_rect(
        area,
        area.width.min(CLOCK_CREATE_DIALOG_WIDTH),
        area.height.min(CLOCK_CREATE_DIALOG_HEIGHT),
    );
    let inner = inset_rect(dialog, 1);
    let prompt_height = u16::from(inner.height >= 7);
    let prompt = Rect::new(inner.x, inner.y, inner.width, prompt_height);
    let input_y = inner.y.saturating_add(prompt_height);
    let button_y = inner.bottom().saturating_sub(1);
    let separate_labels = inner.height >= 6;
    let input_height = (if separate_labels { 4 } else { 3 }).min(button_y.saturating_sub(input_y));
    let input = Rect::new(inner.x, input_y, inner.width, input_height);
    let field_row = |row: u16| {
        std::array::from_fn(|index| {
            let start = inner.width * index as u16 / 3;
            let end = inner.width * (index as u16 + 1) / 3;
            let column = Rect::new(inner.x + start, input.y, end - start, input.height);
            line_in_rect(column, input.y.saturating_add(row))
        })
    };
    let labels = if separate_labels {
        field_row(0)
    } else {
        [Rect::default(); 3]
    };
    let offset = u16::from(separate_labels);
    let increments = field_row(offset);
    let values = field_row(offset + 1);
    let decrements = field_row(offset + 2);
    let error = if input.bottom() < button_y {
        line_in_rect(inner, input.bottom())
    } else {
        Rect::new(inner.x, button_y, 0, 0)
    };
    let buttons_width = inner.width;
    let alarm_width = buttons_width / 3;
    let countdown_width = alarm_width;
    let create_alarm = Rect::new(inner.x, button_y, alarm_width, u16::from(inner.height > 0));
    let create_countdown = Rect::new(
        inner.x.saturating_add(alarm_width),
        button_y,
        countdown_width,
        u16::from(inner.height > 0),
    );
    let cancel = Rect::new(
        create_countdown.right(),
        button_y,
        buttons_width.saturating_sub(alarm_width + countdown_width),
        create_alarm.height,
    );

    ClockCreateDialogLayout {
        dialog,
        prompt,
        input,
        labels,
        increments,
        values,
        decrements,
        error,
        create_alarm,
        create_countdown,
        cancel,
    }
}

fn inset_rect(area: Rect, margin: u16) -> Rect {
    let doubled = margin.saturating_mul(2);
    Rect::new(
        area.x.saturating_add(margin.min(area.width)),
        area.y.saturating_add(margin.min(area.height)),
        area.width.saturating_sub(doubled),
        area.height.saturating_sub(doubled),
    )
}

fn line_in_rect(area: Rect, y: u16) -> Rect {
    if area.width == 0 || area.height == 0 || y < area.y || y >= area.y.saturating_add(area.height)
    {
        return Rect::new(area.x, area.y.saturating_add(area.height), 0, 0);
    }
    Rect::new(area.x, y, area.width, 1)
}

fn usize_to_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}
