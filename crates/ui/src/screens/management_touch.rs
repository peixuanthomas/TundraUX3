use crate::{
    RenderContext,
    components::{Button, Scrollbar, ScrollbarOrientation, Surface, TextInput},
};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::Modifier,
    text::Line,
    widgets::{Clear, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagementFormField {
    pub id: String,
    pub label: String,
    pub value: String,
    pub secret: bool,
    pub choices: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagementChoices {
    pub field: usize,
    pub values: Vec<String>,
    pub selected: usize,
    pub scroll: usize,
    pub columns: usize,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagementForm {
    pub identity: String,
    pub title: String,
    pub message: String,
    pub message_scroll: u16,
    pub fields: Vec<ManagementFormField>,
    pub selected: usize,
    pub field_scroll: Option<usize>,
    pub choice: Option<ManagementChoices>,
    pub cancel_disabled: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagementViewModel {
    pub scope_id: String,
    pub action_ids: Vec<String>,
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub selected: usize,
    pub scroll: usize,
    pub table_scroll: usize,
    pub details: String,
    pub details_scroll: u16,
    pub details_only: bool,
    pub actions: Vec<(String, bool)>,
    pub action_help: Vec<String>,
    pub selected_action: usize,
    pub action_scroll: Option<usize>,
    pub actions_focused: bool,
    pub filter: String,
    pub filtering: bool,
    pub status: String,
    pub loading: bool,
    pub running: bool,
    pub output: String,
    pub output_scroll: u16,
    pub terminal: bool,
    pub terminal_snapshot: Option<std::sync::Arc<crate::CommandLineTerminalSnapshot>>,
    pub form: Option<ManagementForm>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagementControl {
    Refresh,
    Search,
    ApplySearch,
    ClearSearch,
    Details,
    Terminal,
}
pub fn management_controls() -> Vec<(ManagementControl, String)> {
    [
        (ManagementControl::Refresh, "refresh"),
        (ManagementControl::Search, "search"),
        (ManagementControl::ApplySearch, "apply"),
        (ManagementControl::ClearSearch, "clear"),
        (ManagementControl::Details, "details"),
        (ManagementControl::Terminal, "terminal"),
    ]
    .into_iter()
    .map(|(control, id)| (control, i18n::tr!(format!("management-touch-{id}"))))
    .collect()
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagementScrollTarget {
    Rows,
    Columns,
    Details,
    Actions,
    Output,
    FormMessage,
    FormFields,
    Choices,
    ChoiceColumns,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagementScrollbar {
    pub target: ManagementScrollTarget,
    pub track: Rect,
    pub thumb: Rect,
    pub content_len: usize,
    pub viewport_len: usize,
    pub offset: usize,
    pub horizontal: bool,
}
impl ManagementScrollbar {
    pub fn offset_at(&self, point: (u16, u16), grab: u16) -> usize {
        let (coordinate, start, length, thumb) = if self.horizontal {
            (point.0, self.track.x, self.track.width, self.thumb.width)
        } else {
            (point.1, self.track.y, self.track.height, self.thumb.height)
        };
        let travel = usize::from(length.saturating_sub(thumb));
        if travel == 0 {
            return 0;
        }
        let position =
            usize::from(coordinate.saturating_sub(start).saturating_sub(grab)).min(travel);
        position.saturating_mul(self.content_len.saturating_sub(self.viewport_len)) / travel
    }
    pub fn grab_at(&self, point: (u16, u16)) -> u16 {
        if !self.thumb.contains(Position::from(point)) {
            return 0;
        }
        if self.horizontal {
            point.0.saturating_sub(self.thumb.x)
        } else {
            point.1.saturating_sub(self.thumb.y)
        }
    }
    pub fn render(&self, frame: &mut Frame<'_>, context: &RenderContext) {
        Scrollbar::new(self.content_len, self.viewport_len, self.offset)
            .orientation(if self.horizontal {
                ScrollbarOrientation::HorizontalBottom
            } else {
                ScrollbarOrientation::VerticalRight
            })
            .render_frame(frame, self.track, context);
    }
}
pub fn management_scrollbar(
    target: ManagementScrollTarget,
    track: Rect,
    content: usize,
    viewport: usize,
    offset: usize,
    horizontal: bool,
) -> Option<ManagementScrollbar> {
    if content <= viewport || viewport == 0 || track.width == 0 || track.height == 0 {
        return None;
    }
    let offset = offset.min(content.saturating_sub(viewport));
    let orientation = if horizontal {
        ScrollbarOrientation::HorizontalBottom
    } else {
        ScrollbarOrientation::VerticalRight
    };
    let (start, length) = Scrollbar::new(content, viewport, offset)
        .orientation(orientation)
        .thumb_range(track);
    let thumb = if horizontal {
        Rect::new(track.x + start, track.y, length, track.height)
    } else {
        Rect::new(track.x, track.y + start, track.width, length)
    };
    Some(ManagementScrollbar {
        target,
        track,
        thumb,
        content_len: content,
        viewport_len: viewport,
        offset,
        horizontal,
    })
}
pub fn management_wrapped_lines(text: &str, width: u16) -> Vec<String> {
    let width = usize::from(width.max(1));
    let mut lines = Vec::new();
    for original in text.split('\n') {
        let mut line = String::new();
        let mut used = 0;
        for c in original.chars().filter(|c| !c.is_control() || *c == '\t') {
            let value = if c == '\t' {
                "    ".to_string()
            } else {
                c.to_string()
            };
            for c in value.chars() {
                let next = c.width().unwrap_or(0);
                if used > 0 && used + next > width {
                    lines.push(std::mem::take(&mut line));
                    used = 0;
                }
                line.push(c);
                used += next;
            }
        }
        lines.push(line);
    }
    lines
}
fn inner(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}
fn right_track(area: Rect) -> Rect {
    Rect::new(
        area.right().saturating_sub(1),
        area.y,
        u16::from(area.width > 0),
        area.height,
    )
}
fn bottom_track(area: Rect) -> Rect {
    Rect::new(
        area.x,
        area.bottom().saturating_sub(1),
        area.width,
        u16::from(area.height > 0),
    )
}
fn text_area(area: Rect) -> Rect {
    Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height)
}
pub fn management_toolbar(
    area: Rect,
    controls: &[(ManagementControl, String)],
) -> Vec<(ManagementControl, Rect)> {
    let mut x = area.x;
    let mut y = area.y;
    let mut result = Vec::new();
    for (target, label) in controls {
        let width = (label.width() as u16).saturating_add(2).min(area.width);
        if x != area.x && x.saturating_add(width) > area.right() {
            x = area.x;
            y = y.saturating_add(1);
        }
        let height = u16::from(y < area.bottom());
        result.push((*target, Rect::new(x, y.min(area.bottom()), width, height)));
        x = x.saturating_add(width).saturating_add(1);
    }
    result
}
#[derive(Debug, Clone)]
pub struct ManagementLayout {
    pub filter: Rect,
    pub controls: Vec<(ManagementControl, Rect)>,
    pub list: Rect,
    pub list_rows: Rect,
    pub list_capacity: usize,
    pub column_widths: Vec<usize>,
    pub details: Rect,
    pub details_text: Rect,
    pub actions_panel: Rect,
    pub actions: Vec<Rect>,
    pub action_start: usize,
    pub action_previous: Rect,
    pub action_next: Rect,
    pub status: Rect,
    pub content: Rect,
    pub output_text: Rect,
    pub form: Rect,
    pub message: Rect,
    pub fields_area: Rect,
    pub fields: Vec<(usize, Rect)>,
    pub field_start: usize,
    pub field_ranges: Vec<(usize, usize)>,
    pub submit: Rect,
    pub cancel: Rect,
    pub choice_panel: Option<Rect>,
    pub choice_rows: Vec<(usize, Rect)>,
    pub choice_cancel: Rect,
    pub scrollbars: Vec<ManagementScrollbar>,
}
fn add_text_bar(
    bars: &mut Vec<ManagementScrollbar>,
    target: ManagementScrollTarget,
    area: Rect,
    text: &str,
    offset: usize,
) {
    let length = management_wrapped_lines(text, area.width.saturating_sub(1)).len();
    if let Some(bar) = management_scrollbar(
        target,
        right_track(area),
        length,
        usize::from(area.height),
        offset,
        false,
    ) {
        bars.push(bar);
    }
}
pub fn management_layout(main: Rect, model: &ManagementViewModel) -> ManagementLayout {
    let controls = management_toolbar(
        Rect::new(
            main.x,
            main.y.saturating_add(1),
            main.width,
            main.height.saturating_sub(1),
        ),
        &management_controls(),
    );
    let header_end = controls
        .iter()
        .map(|(_, area)| area.bottom())
        .max()
        .unwrap_or(main.y);
    let filter = Rect::new(
        main.x,
        header_end,
        main.width,
        u16::from(header_end < main.bottom()),
    );
    let status = Rect::new(
        main.x,
        main.bottom().saturating_sub(2),
        main.width,
        main.height.min(2),
    );
    let content = Rect::new(
        main.x,
        filter.bottom(),
        main.width,
        status.y.saturating_sub(filter.bottom()),
    );
    let (list, details, actions_panel) = if model.details_only {
        (Rect::default(), content, Rect::default())
    } else if main.width >= 90 {
        let left = content.width * 58 / 100;
        let right = content.width.saturating_sub(left);
        let details_height = (content.height * 3 / 5)
            .max(2)
            .min(content.height.saturating_sub(2));
        (
            Rect::new(content.x, content.y, left, content.height),
            Rect::new(content.x + left, content.y, right, details_height),
            Rect::new(
                content.x + left,
                content.y + details_height,
                right,
                content.height.saturating_sub(details_height),
            ),
        )
    } else {
        let actions_height = (content.height / 3).max(2).min(content.height);
        let rest = content.height.saturating_sub(actions_height);
        let list_height = (rest * 3 / 5).max(4).min(rest);
        (
            Rect::new(content.x, content.y, content.width, list_height),
            Rect::new(
                content.x,
                content.y + list_height,
                content.width,
                rest.saturating_sub(list_height),
            ),
            Rect::new(
                content.x,
                content.bottom().saturating_sub(actions_height),
                content.width,
                actions_height,
            ),
        )
    };
    let mut column_widths = model
        .columns
        .iter()
        .enumerate()
        .map(|(index, label)| {
            model
                .rows
                .iter()
                .filter_map(|row| row.get(index))
                .map(|value| value.width())
                .max()
                .unwrap_or(0)
                .max(label.width())
                .max(4)
                + 2
        })
        .collect::<Vec<_>>();
    if column_widths.is_empty() {
        column_widths.push(8);
    }
    let total_width = column_widths.iter().sum::<usize>();
    let list_inner = inner(list);
    let horizontal = total_width > usize::from(list_inner.width.saturating_sub(1));
    let list_rows = Rect::new(
        list_inner.x,
        list_inner.y.saturating_add(1),
        list_inner.width.saturating_sub(1),
        list_inner.height.saturating_sub(1 + u16::from(horizontal)),
    );
    let list_capacity = usize::from(list_rows.height);
    let details_text = text_area(inner(details));
    let action_inner = if actions_panel.height >= 4 {
        inner(actions_panel)
    } else {
        actions_panel
    };
    let action_capacity = usize::from(action_inner.height) / 2;
    let action_start = model
        .action_scroll
        .unwrap_or_else(|| {
            model
                .selected_action
                .saturating_sub(action_capacity.saturating_sub(1))
        })
        .min(model.actions.len().saturating_sub(action_capacity));
    let paging = model.actions.len() > action_capacity && action_inner.height >= 2;
    let paging_width = if paging {
        (i18n::tr!("management-touch-previous-action")
            .width()
            .max(i18n::tr!("management-touch-next-action").width()) as u16
            + 2)
        .min(action_inner.width / 2)
    } else {
        0
    };
    let action_previous = Rect::new(
        action_inner.right().saturating_sub(paging_width + 1),
        action_inner.y,
        paging_width,
        u16::from(paging),
    );
    let action_next = Rect::new(
        action_previous.x,
        action_previous.y.saturating_add(1),
        paging_width,
        u16::from(paging),
    );
    let actions = (0..action_capacity.min(model.actions.len().saturating_sub(action_start)))
        .map(|offset| {
            Rect::new(
                action_inner.x,
                action_inner.y + offset as u16 * 2,
                action_inner
                    .width
                    .saturating_sub(1 + paging_width + u16::from(paging)),
                2,
            )
        })
        .collect::<Vec<_>>();
    let output_text = text_area(content);
    let mut scrollbars = Vec::new();
    if let Some(bar) = management_scrollbar(
        ManagementScrollTarget::Rows,
        Rect::new(
            list_inner.right().saturating_sub(1),
            list_rows.y,
            u16::from(list_inner.width > 0),
            list_rows.height,
        ),
        model.rows.len(),
        list_capacity,
        model.scroll,
        false,
    ) {
        scrollbars.push(bar);
    }
    if horizontal {
        if let Some(bar) = management_scrollbar(
            ManagementScrollTarget::Columns,
            bottom_track(list_inner),
            total_width,
            usize::from(list_rows.width),
            model.table_scroll,
            true,
        ) {
            scrollbars.push(bar);
        }
    }
    add_text_bar(
        &mut scrollbars,
        ManagementScrollTarget::Details,
        inner(details),
        &model.details,
        usize::from(model.details_scroll),
    );
    if let Some(bar) = management_scrollbar(
        ManagementScrollTarget::Actions,
        right_track(action_inner),
        model.actions.len(),
        action_capacity,
        action_start,
        false,
    ) {
        scrollbars.push(bar);
    }
    if model.terminal {
        if let Some(snapshot) = &model.terminal_snapshot {
            let total = snapshot.scrollback_rows + usize::from(snapshot.rows);
            let offset = snapshot
                .scrollback_rows
                .saturating_sub(snapshot.scrollback_offset);
            if let Some(bar) = management_scrollbar(
                ManagementScrollTarget::Output,
                right_track(content),
                total,
                usize::from(snapshot.rows),
                offset,
                false,
            ) {
                scrollbars.push(bar);
            }
        } else {
            add_text_bar(
                &mut scrollbars,
                ManagementScrollTarget::Output,
                content,
                &model.output,
                usize::from(model.output_scroll),
            );
        }
    }
    let form_width = main.width.saturating_sub(2).min(88);
    let form_height = main.height.saturating_sub(2).max(1);
    let form = Rect::new(
        main.x + main.width.saturating_sub(form_width) / 2,
        main.y + main.height.saturating_sub(form_height) / 2,
        form_width,
        form_height,
    );
    let form_inner = inner(form);
    let form_model = model.form.as_ref();
    let message_len = form_model.map_or(0, |form| {
        management_wrapped_lines(&form.message, form_inner.width.saturating_sub(1)).len()
    });
    let has_fields = form_model.is_some_and(|form| !form.fields.is_empty());
    let available = form_inner.height.saturating_sub(3);
    let message_height = if has_fields {
        (available / 2)
            .max(1)
            .min(available.saturating_sub(2))
            .min(message_len as u16)
    } else {
        available
    };
    let message = Rect::new(form_inner.x, form_inner.y, form_inner.width, message_height);
    let fields_area = Rect::new(
        form_inner.x,
        message.bottom(),
        form_inner.width,
        available.saturating_sub(message_height),
    );
    let mut field_ranges = Vec::new();
    let mut field_length = 0;
    if let Some(form_model) = form_model {
        for field in &form_model.fields {
            let height =
                management_wrapped_lines(&field.label, fields_area.width.saturating_sub(1)).len()
                    + management_wrapped_lines(&field.value, fields_area.width.saturating_sub(1))
                        .len()
                    + 1;
            field_ranges.push((field_length, height));
            field_length += height;
        }
    }
    let selected_range = form_model
        .and_then(|form| field_ranges.get(form.selected))
        .copied()
        .unwrap_or((0, 0));
    let field_start = form_model
        .and_then(|form| form.field_scroll)
        .unwrap_or_else(|| {
            selected_range
                .0
                .saturating_add(selected_range.1)
                .saturating_sub(usize::from(fields_area.height))
        })
        .min(field_length.saturating_sub(usize::from(fields_area.height)));
    let fields = field_ranges
        .iter()
        .enumerate()
        .filter_map(|(index, (start, length))| {
            let start_y = (*start).max(field_start);
            let end = (*start + *length).min(field_start + usize::from(fields_area.height));
            (end > start_y).then(|| {
                (
                    index,
                    Rect::new(
                        fields_area.x,
                        fields_area.y + (start_y - field_start) as u16,
                        fields_area.width.saturating_sub(1),
                        (end - start_y) as u16,
                    ),
                )
            })
        })
        .collect();
    let submit = Rect::new(
        form_inner.x,
        form_inner.bottom().saturating_sub(1),
        form_inner.width / 2,
        1,
    );
    let cancel = Rect::new(
        submit.right(),
        submit.y,
        form_inner.width.saturating_sub(submit.width),
        1,
    );
    if let Some(form_model) = form_model {
        add_text_bar(
            &mut scrollbars,
            ManagementScrollTarget::FormMessage,
            message,
            &form_model.message,
            usize::from(form_model.message_scroll),
        );
        if let Some(bar) = management_scrollbar(
            ManagementScrollTarget::FormFields,
            right_track(fields_area),
            field_length,
            usize::from(fields_area.height),
            field_start,
            false,
        ) {
            scrollbars.push(bar);
        }
    }
    let mut choice_panel = None;
    let mut choice_rows = Vec::new();
    let mut choice_cancel = Rect::default();
    if let Some(choice) = form_model.and_then(|form| form.choice.as_ref()) {
        choice_panel = Some(form);
        let rows = Rect::new(
            form_inner.x,
            form_inner.y,
            form_inner.width.saturating_sub(1),
            form_inner.height.saturating_sub(3),
        );
        let count = usize::from(rows.height);
        let start = choice.scroll.min(choice.values.len().saturating_sub(count));
        choice_rows = (start..choice.values.len())
            .take(count)
            .enumerate()
            .map(|(offset, index)| {
                (
                    index,
                    Rect::new(rows.x, rows.y + offset as u16, rows.width, 1),
                )
            })
            .collect();
        choice_cancel = Rect::new(
            form_inner.x,
            form_inner.bottom().saturating_sub(1),
            form_inner.width,
            1,
        );
        if let Some(bar) = management_scrollbar(
            ManagementScrollTarget::Choices,
            right_track(form_inner),
            choice.values.len(),
            count,
            start,
            false,
        ) {
            scrollbars.push(bar);
        }
        let width = choice
            .values
            .iter()
            .map(|value| value.width() + 4)
            .max()
            .unwrap_or(0);
        if let Some(bar) = management_scrollbar(
            ManagementScrollTarget::ChoiceColumns,
            Rect::new(rows.x, rows.bottom(), rows.width, 1),
            width,
            usize::from(rows.width),
            choice.columns,
            true,
        ) {
            scrollbars.push(bar);
        }
    }
    ManagementLayout {
        filter,
        controls,
        list,
        list_rows,
        list_capacity,
        column_widths,
        details,
        details_text,
        actions_panel,
        actions,
        action_start,
        action_previous,
        action_next,
        status,
        content,
        output_text,
        form,
        message,
        fields_area,
        fields,
        field_start,
        field_ranges,
        submit,
        cancel,
        choice_panel,
        choice_rows,
        choice_cancel,
        scrollbars,
    }
}

pub fn management_control_id(model: &ManagementViewModel, control: ManagementControl) -> String {
    format!("management.control.{}.{control:?}", model.scope_id)
}
pub fn management_action_id(model: &ManagementViewModel, index: usize) -> String {
    model
        .action_ids
        .get(index)
        .cloned()
        .unwrap_or_else(|| format!("management.action.{}.{index}", model.scope_id))
}
pub fn management_form_control_id(form: &ManagementForm, control: &str) -> String {
    format!("management.{control}.{}", form.identity)
}
pub fn management_field_id(form: &ManagementForm, index: usize) -> String {
    format!(
        "management.field.{}.{}",
        form.identity,
        form.fields.get(index).map_or("", |field| field.id.as_str())
    )
}
pub fn management_choice_id(form: &ManagementForm, index: usize) -> String {
    let choice = form.choice.as_ref();
    format!(
        "management.choice.{}.{}.{index}.{}",
        form.identity,
        choice.map_or(0, |choice| choice.field),
        choice
            .and_then(|choice| choice.values.get(index))
            .map_or("", String::as_str)
    )
}
pub fn management_action_page_id(model: &ManagementViewModel, start: usize, next: bool) -> String {
    format!("management.action-page.{}.{start}.{next}", model.scope_id)
}
pub fn management_button_regions(
    main: Rect,
    model: &ManagementViewModel,
) -> Vec<crate::components::ButtonRegion> {
    let layout = management_layout(main, model);
    let mut buttons = Vec::new();
    let mut push = |id: String, area: Rect, disabled: bool| {
        if area.width > 0 && area.height > 0 {
            buttons.push(crate::components::ButtonRegion {
                id: id.into(),
                area,
                disabled,
            });
        }
    };
    if let Some(form) = &model.form {
        if form.choice.is_some() {
            push(
                management_form_control_id(form, "choice.cancel"),
                layout.choice_cancel,
                false,
            );
            for (index, area) in &layout.choice_rows {
                push(management_choice_id(form, *index), *area, false);
            }
        } else {
            push(
                management_form_control_id(form, "confirm"),
                layout.submit,
                false,
            );
            push(
                management_form_control_id(form, "cancel"),
                layout.cancel,
                form.cancel_disabled,
            );
            for (index, area) in &layout.fields {
                push(management_field_id(form, *index), *area, false);
            }
        }
        return buttons;
    }
    for (control, area) in &layout.controls {
        push(
            management_control_id(model, *control),
            *area,
            *control == ManagementControl::Details && model.rows.is_empty(),
        );
    }
    push(
        format!("management.filter.{}", model.scope_id),
        layout.filter,
        false,
    );
    if !model.terminal {
        for (offset, area) in layout.actions.iter().enumerate() {
            let index = layout.action_start + offset;
            push(
                management_action_id(model, index),
                *area,
                !model.actions[index].1,
            );
        }
        push(
            management_action_page_id(model, layout.action_start, false),
            layout.action_previous,
            layout.action_start == 0,
        );
        push(
            management_action_page_id(model, layout.action_start, true),
            layout.action_next,
            layout.action_start + layout.actions.len() >= model.actions.len(),
        );
    }
    buttons
}
fn render_text(
    frame: &mut Frame<'_>,
    area: Rect,
    text: &str,
    offset: usize,
    context: &RenderContext,
) {
    let lines = management_wrapped_lines(text, area.width);
    let offset = offset.min(lines.len().saturating_sub(usize::from(area.height)));
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(offset)
                .map(Line::from)
                .collect::<Vec<_>>(),
        )
        .style(context.compatibility_theme().body_style()),
        area,
    );
}
pub fn render_management_content(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &ManagementViewModel,
    context: &RenderContext,
) {
    let theme = context.compatibility_theme();
    let layout = management_layout(main, model);
    let selected = theme
        .title_style()
        .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    frame.render_widget(
        Paragraph::new(format!(
            "{} · {}",
            model.title,
            i18n::tr!("management-touch-guide")
        ))
        .style(theme.title_style()),
        Rect::new(main.x, main.y, main.width, u16::from(main.height > 0)),
    );
    for ((control, area), (_, label)) in layout.controls.iter().zip(management_controls()) {
        let mut button = Button::new(management_control_id(model, *control), label);
        button.set_disabled(*control == ManagementControl::Details && model.rows.is_empty());
        button.render_borderless_frame(frame, *area, &theme);
    }
    let mut filter = TextInput::new(format!("management.filter.{}", model.scope_id))
        .with_cursor_symbol("_")
        .with_horizontal_scroll(true);
    filter.set_value(&model.filter);
    filter.set_focused(model.filtering);
    filter.render_borderless_frame_with_prefix(
        frame,
        layout.filter,
        &theme,
        &format!("{} ", i18n::tr!("management-search")),
    );
    if model.terminal {
        if let Some(snapshot) = &model.terminal_snapshot {
            super::super::command_line::render_terminal_snapshot(
                frame,
                layout.output_text,
                snapshot,
            );
        } else {
            render_text(
                frame,
                layout.output_text,
                &model.output,
                usize::from(model.output_scroll),
                context,
            );
        }
    } else {
        Surface::new()
            .bordered(true)
            .titled(i18n::tr!("management-touch-items"))
            .render_frame(frame, layout.list, context);
        Surface::new()
            .bordered(true)
            .titled(i18n::tr!("management-touch-details"))
            .render_frame(frame, layout.details, context);
        if layout.actions_panel.height >= 4 {
            Surface::new()
                .bordered(true)
                .titled(i18n::tr!("management-touch-actions"))
                .render_frame(frame, layout.actions_panel, context);
        }
        let table_line = |cells: &[String]| {
            cells
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let width = layout.column_widths.get(index).copied().unwrap_or(6);
                    format!(
                        "{}{}",
                        value,
                        " ".repeat(width.saturating_sub(value.width()))
                    )
                })
                .collect::<String>()
        };
        let mut lines = vec![Line::styled(
            table_line(&model.columns),
            theme.title_style(),
        )];
        for (index, row) in model
            .rows
            .iter()
            .enumerate()
            .skip(model.scroll)
            .take(layout.list_capacity)
        {
            lines.push(Line::styled(
                table_line(row),
                if index == model.selected && !model.actions_focused {
                    selected
                } else {
                    theme.body_style()
                },
            ));
        }
        if model.rows.is_empty() {
            lines.push(Line::from(if model.loading {
                i18n::tr!("management-loading")
            } else {
                i18n::tr!("management-empty")
            }));
        }
        let area = Rect::new(
            layout.list_rows.x,
            layout.list_rows.y.saturating_sub(1),
            layout.list_rows.width,
            layout.list_rows.height.saturating_add(1),
        );
        frame.render_widget(
            Paragraph::new(lines).scroll((0, model.table_scroll.min(u16::MAX as usize) as u16)),
            area,
        );
        render_text(
            frame,
            layout.details_text,
            &model.details,
            usize::from(model.details_scroll),
            context,
        );
        for (offset, area) in layout.actions.iter().enumerate() {
            let index = layout.action_start + offset;
            if let Some((label, enabled)) = model.actions.get(index) {
                let number = if index == 9 {
                    "0".into()
                } else {
                    (index + 1).to_string()
                };
                let mut button = Button::new(
                    management_action_id(model, index),
                    format!("{number}. {label}"),
                );
                button.set_disabled(!enabled);
                button.render_borderless_frame(frame, *area, &theme);
                frame.render_widget(
                    Paragraph::new(model.action_help.get(index).cloned().unwrap_or_default())
                        .style(theme.muted_style()),
                    Rect::new(area.x, area.y + 1, area.width, 1),
                );
            }
        }
        for (area, next, key, disabled) in [
            (
                layout.action_previous,
                false,
                "management-touch-previous-action",
                layout.action_start == 0,
            ),
            (
                layout.action_next,
                true,
                "management-touch-next-action",
                layout.action_start + layout.actions.len() >= model.actions.len(),
            ),
        ] {
            if area.height > 0 {
                let mut button = Button::new(
                    management_action_page_id(model, layout.action_start, next),
                    i18n::tr!(key),
                );
                button.set_disabled(disabled);
                button.render_borderless_frame(frame, area, &theme);
            }
        }
    }
    for bar in &layout.scrollbars {
        let show = if model.terminal {
            bar.target == ManagementScrollTarget::Output
        } else {
            matches!(
                bar.target,
                ManagementScrollTarget::Rows
                    | ManagementScrollTarget::Columns
                    | ManagementScrollTarget::Details
                    | ManagementScrollTarget::Actions
            )
        };
        if show {
            bar.render(frame, context);
        }
    }
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(model.status.clone()),
            Line::from(i18n::tr!("management-touch-guide")),
        ])
        .style(theme.muted_style()),
        layout.status,
    );
}
pub fn render_management_overlay(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &ManagementViewModel,
    context: &RenderContext,
) {
    let Some(form) = &model.form else {
        return;
    };
    let layout = management_layout(main, model);
    let theme = context.compatibility_theme();
    frame.render_widget(Clear, layout.form);
    Surface::new()
        .bordered(true)
        .raised(true)
        .titled(form.title.clone())
        .render_frame(frame, layout.form, context);
    if let Some(choice) = &form.choice {
        for (index, area) in &layout.choice_rows {
            let value = &choice.values[*index];
            let text = format!(
                "{} {value}",
                if *index == choice.selected {
                    "●"
                } else {
                    "○"
                }
            );
            let mut columns = 0;
            let text = text
                .chars()
                .skip_while(|c| {
                    if columns < choice.columns {
                        columns += c.width().unwrap_or(0);
                        true
                    } else {
                        false
                    }
                })
                .collect::<String>();
            let mut button = Button::new(management_choice_id(form, *index), text);
            button.state.selected = *index == choice.selected;
            button.render_borderless_frame(frame, *area, &theme);
        }
        Button::new(
            management_form_control_id(form, "choice.cancel"),
            i18n::tr!("management-touch-close-choices"),
        )
        .render_borderless_frame(frame, layout.choice_cancel, &theme);
        if layout.choice_cancel.height > 0 && layout.choice_cancel.y > layout.form.y {
            frame.render_widget(
                Paragraph::new(i18n::tr!("management-choice-hint")).style(theme.muted_style()),
                Rect::new(
                    layout.choice_cancel.x,
                    layout.choice_cancel.y - 1,
                    layout.choice_cancel.width,
                    1,
                ),
            );
        }
        for bar in &layout.scrollbars {
            if matches!(
                bar.target,
                ManagementScrollTarget::Choices | ManagementScrollTarget::ChoiceColumns
            ) {
                bar.render(frame, context);
            }
        }
        return;
    }
    render_text(
        frame,
        text_area(layout.message),
        &form.message,
        usize::from(form.message_scroll),
        context,
    );
    for (index, area) in &layout.fields {
        let field = &form.fields[*index];
        let value = if field.secret {
            "•".repeat(field.value.chars().count())
        } else {
            field.value.clone()
        };
        let mut lines = management_wrapped_lines(&field.label, area.width);
        lines.extend(management_wrapped_lines(&value, area.width));
        lines.push(if field.choices.is_empty() {
            i18n::tr!("management-touch-focus-field")
        } else {
            i18n::tr!("management-touch-choose-field")
        });
        let skip = layout
            .field_start
            .saturating_sub(layout.field_ranges[*index].0);
        let mut button = Button::new(
            management_field_id(form, *index),
            lines.into_iter().skip(skip).collect::<Vec<_>>().join("\n"),
        );
        button.state.selected = form.selected == *index;
        button.render_borderless_frame(frame, *area, &theme);
    }
    Button::new(
        management_form_control_id(form, "confirm"),
        i18n::tr!("management-confirm"),
    )
    .render_borderless_frame(frame, layout.submit, &theme);
    let mut cancel = Button::new(
        management_form_control_id(form, "cancel"),
        i18n::tr!("management-cancel"),
    );
    cancel.set_disabled(form.cancel_disabled);
    cancel.render_borderless_frame(frame, layout.cancel, &theme);
    frame.render_widget(
        Paragraph::new(i18n::tr!("management-touch-form-guide")).style(theme.muted_style()),
        Rect::new(
            layout.submit.x,
            layout.submit.y.saturating_sub(1),
            layout.fields_area.width,
            1,
        ),
    );
    for bar in &layout.scrollbars {
        if matches!(
            bar.target,
            ManagementScrollTarget::FormMessage | ManagementScrollTarget::FormFields
        ) {
            bar.render(frame, context);
        }
    }
}
