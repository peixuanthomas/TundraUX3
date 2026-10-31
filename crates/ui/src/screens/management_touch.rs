use crate::{
    RenderContext,
    components::{Button, Scrollbar, ScrollbarOrientation, Surface, TextInput},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[path = "management_workspace.rs"]
mod workspace;
pub use workspace::ManagementWorkspace;

pub use crate::components::{
    FormChoices as ManagementChoices, FormControl as ManagementFormControl,
    FormDialog as ManagementForm, FormField as ManagementFormField,
};
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagementViewModel {
    pub workspace: Option<ManagementWorkspace>,
    pub scope_id: String,
    pub action_ids: Vec<String>,
    /// Actions before this index belong to the toolbar; the rest sit below details.
    pub detail_action_start: Option<usize>,
    pub title: String,
    pub columns: Vec<String>,
    pub sort: Option<crate::TableSort>,
    pub column_width_limits: Vec<usize>,
    pub rows: Vec<Vec<String>>,
    pub empty_message: String,
    pub selected: usize,
    pub selected_rows: Option<Vec<usize>>,
    pub selection_mode: bool,
    pub scroll: usize,
    pub table_scroll: usize,
    pub details: String,
    pub details_scroll: u16,
    pub details_horizontal_scroll: u16,
    pub details_only: bool,
    pub details_expansion: Option<u16>,
    pub actions: Vec<(String, bool)>,
    pub action_help: Vec<String>,
    pub action_tones: Vec<crate::components::ComponentTone>,
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
    vec![
        (
            ManagementControl::Refresh,
            i18n::tr!("management-control-refresh"),
        ),
        (
            ManagementControl::ApplySearch,
            i18n::tr!("regression-management-search"),
        ),
    ]
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagementScrollTarget {
    Rows,
    Columns,
    Details,
    DetailsColumns,
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
    fn orientation(&self) -> ScrollbarOrientation {
        if self.horizontal {
            ScrollbarOrientation::HorizontalBottom
        } else {
            ScrollbarOrientation::VerticalRight
        }
    }
    pub fn hit_test(&self, point: (u16, u16)) -> bool {
        self.orientation().hit_test(self.track, point)
    }
    pub fn offset_at(&self, point: (u16, u16), grab: u16) -> usize {
        let (coordinate, start, length, thumb) = if self.horizontal {
            (point.0, self.track.x, self.track.width, self.thumb.width)
        } else {
            (point.1, self.track.y, self.track.height, self.thumb.height)
        };
        let travel = usize::from(length.saturating_sub(thumb));
        if travel == 0 {
            // A one-cell track has no room for its thumb to travel. Pressing
            // it must preserve the current position; dragging beyond either
            // end still allows a very small menu to reach its first/last item.
            return if coordinate < start {
                0
            } else if coordinate >= start.saturating_add(length) {
                self.content_len.saturating_sub(self.viewport_len)
            } else {
                self.offset
            };
        }
        let position =
            usize::from(coordinate.saturating_sub(start).saturating_sub(grab)).min(travel);
        position.saturating_mul(self.content_len.saturating_sub(self.viewport_len)) / travel
    }
    pub fn grab_at(&self, point: (u16, u16)) -> u16 {
        if !self.orientation().hit_test(self.thumb, point) {
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
            .orientation(self.orientation())
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
        let mut wrapped = false;
        let original = original
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect::<String>()
            .replace('\t', "    ");
        let span = Span::raw(original);
        for glyph in span.styled_graphemes(Style::default()) {
            let next = glyph.symbol.width();
            let whitespace = glyph.symbol.chars().all(char::is_whitespace);
            if used > 0 && used + next > width && whitespace {
                lines.push(std::mem::take(&mut line));
                used = 0;
                wrapped = true;
                continue;
            }
            if used > 0 && used + next > width {
                wrapped = true;
                // Keep words intact when a whitespace boundary fits the viewport.
                if let Some((boundary, space)) = line
                    .char_indices()
                    .rev()
                    .find(|(index, c)| c.is_whitespace() && !line[..*index].trim().is_empty())
                {
                    let remainder = line.split_off(boundary + space.len_utf8());
                    lines.push(line.trim_end().to_string());
                    line = remainder;
                    used = line.width();
                }
            }
            if used > 0 && used + next > width {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            if line.is_empty() && wrapped && whitespace {
                continue;
            }
            line.push_str(glyph.symbol);
            used += next;
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
fn management_choice_is_menu(form: &ManagementForm) -> bool {
    matches!(form.identity.as_str(), "more-actions" | "config-menu")
}
fn management_choice_width(form: &ManagementForm, choice: &ManagementChoices) -> usize {
    let decoration = if management_choice_is_menu(form) {
        0
    } else {
        4
    };
    choice
        .values
        .iter()
        .map(|value| value.width().saturating_add(decoration))
        .max()
        .unwrap_or(0)
}
fn choice_form(main: Rect, form: &ManagementForm, choice: &ManagementChoices) -> Rect {
    let content_width = management_choice_width(form, choice)
        .saturating_add(1)
        .max(form.title.width())
        .max(i18n::tr!("management-form-close").width() + 2);
    let width = content_width
        .saturating_add(6)
        .clamp(24, 88)
        .min(usize::from(
            main.width.saturating_sub(u16::from(main.width > 8) * 2),
        )) as u16;
    let horizontal = management_choice_width(form, choice) > usize::from(width.saturating_sub(6));
    let height = choice
        .values
        .len()
        .saturating_add(6 + usize::from(horizontal))
        .min(usize::from(
            main.height.saturating_sub(u16::from(main.height > 8) * 2),
        )) as u16;
    Rect::new(
        main.x + main.width.saturating_sub(width) / 2,
        main.y + main.height.saturating_sub(height) / 2,
        width,
        height,
    )
}
fn choice_form_inner(form: Rect) -> Rect {
    let content = inner(form).intersection(form);
    // Keep a blank row and two blank columns between menu items and the border.
    // Small terminals spend these cells on reachable items instead.
    let horizontal = if content.width >= 10 { 2 } else { 0 };
    let vertical = u16::from(content.height >= 5);
    Rect::new(
        content.x + horizontal,
        content.y + vertical,
        content.width.saturating_sub(horizontal * 2),
        content.height.saturating_sub(vertical * 2),
    )
}
fn form_submit_text(form: &ManagementForm) -> String {
    format!(
        "F10 {}",
        form.submit_label
            .clone()
            .unwrap_or_else(|| i18n::tr!("management-form-submit"))
    )
}
fn content_form_inner(form: Rect, spacious: bool) -> Rect {
    if form.height <= 5 {
        return form;
    }
    let content = inner(form).intersection(form);
    let horizontal = if content.width >= 38 {
        2
    } else {
        u16::from(content.width >= 10)
    };
    let vertical = u16::from(spacious && content.height >= 5);
    Rect::new(
        content.x + horizontal,
        content.y + vertical,
        content.width.saturating_sub(horizontal * 2),
        content.height.saturating_sub(vertical * 2),
    )
}
fn form_cancel_text(form: &ManagementForm) -> String {
    if let Some(label) = &form.cancel_label {
        return format!("Esc {label}");
    }
    if form.dismiss_only {
        i18n::tr!("management-form-close")
    } else {
        format!("Esc {}", i18n::tr!("management-form-cancel"))
    }
}
fn field_lines(field: &ManagementFormField, width: u16) -> Vec<String> {
    if field.control == ManagementFormControl::Toggle {
        let value = format!(
            "[{}]",
            i18n::tr!(if field.value == "true" {
                "settings-on"
            } else {
                "settings-off"
            })
        );
        let gap = usize::from(width)
            .saturating_sub(field.label.width() + value.width())
            .max(1);
        return management_wrapped_lines(
            &format!("{}{}{value}", field.label, " ".repeat(gap)),
            width,
        );
    }
    if field.control == ManagementFormControl::Permissions {
        return vec![
            field.label.clone(),
            permission_heading(width),
            i18n::tr!("permissions-owner"),
            i18n::tr!("permissions-group"),
            i18n::tr!("permissions-others"),
        ];
    }
    let mut lines = if field.label.is_empty() {
        vec![]
    } else {
        management_wrapped_lines(&field.label, width)
    };
    if field.input.is_some() || field.is_text() {
        lines.push(String::new());
        return lines;
    }
    let value = if field.control == ManagementFormControl::Checkbox {
        if field.value == "true" {
            "[x]".into()
        } else {
            "[ ]".into()
        }
    } else if field.secret {
        "•".repeat(field.value.chars().count())
    } else {
        field.value.clone()
    };
    lines.extend(management_wrapped_lines(&value, width));
    lines
}

fn permission_heading(width: u16) -> String {
    let (label, cell) = permission_column_widths(width);
    let mut text = " ".repeat(label as usize);
    for title in [
        i18n::tr!("permissions-read"),
        i18n::tr!("permissions-write"),
        i18n::tr!("permissions-execute"),
    ] {
        let title = crate::components::truncate_to_terminal_width(&title, cell as usize);
        let title_width = title.width() as u16;
        let leading = cell / 2 - title_width / 2;
        text.push_str(&" ".repeat(usize::from(leading)));
        text.push_str(&title);
        text.push_str(&" ".repeat(usize::from(cell - leading - title_width)));
    }
    text
}
fn permission_column_widths(width: u16) -> (u16, u16) {
    let label = [
        i18n::tr!("permissions-owner"),
        i18n::tr!("permissions-group"),
        i18n::tr!("permissions-others"),
    ]
    .iter()
    .map(|s| s.width() as u16 + 1)
    .max()
    .unwrap_or(1)
    .min(width.saturating_sub(9));
    (label, width.saturating_sub(label) / 3)
}
pub fn management_permission_cells(
    field: &ManagementFormField,
    area: Rect,
    skip: usize,
) -> Vec<(usize, Rect)> {
    if field.control != ManagementFormControl::Permissions {
        return Vec::new();
    }
    let (label, width) = permission_column_widths(area.width);
    (0..9)
        .filter_map(|index| {
            let row = 2 + index / 3;
            if row < skip || row - skip >= area.height as usize || width < 3 {
                return None;
            }
            Some((
                index,
                Rect::new(
                    area.x + label + index as u16 % 3 * width,
                    area.y + (row - skip) as u16,
                    width,
                    1,
                ),
            ))
        })
        .collect()
}
pub fn management_permission_id(form: &ManagementForm, index: usize, cell: usize) -> String {
    format!("{}.cell.{cell}", management_field_id(form, index))
}
fn form_action_sizes(form: &ManagementForm, width: u16) -> (u16, u16, u16, u16, u16) {
    if !form.buttons.is_empty() {
        return (0, 0, 0, 0, form.action_bar().height(width));
    }
    let submit = if form.dismiss_only {
        0
    } else {
        (form_submit_text(form).width() + 2).min(usize::from(width)) as u16
    };
    let cancel = (form_cancel_text(form).width() + 2).min(usize::from(width)) as u16;
    let submit_height = if submit == 0 {
        0
    } else {
        management_wrapped_lines(&format!("[{}]", form_submit_text(form)), submit)
            .len()
            .min(usize::from(u16::MAX)) as u16
    };
    let cancel_height = management_wrapped_lines(&format!("[{}]", form_cancel_text(form)), cancel)
        .len()
        .min(usize::from(u16::MAX)) as u16;
    let rows = if submit > 0 && submit.saturating_add(1).saturating_add(cancel) > width {
        submit_height.saturating_add(cancel_height)
    } else {
        submit_height.max(cancel_height)
    };
    (submit, cancel, submit_height, cancel_height, rows)
}
fn form_columns(form: &ManagementForm) -> usize {
    form.columns.max(1).min(form.fields.len().max(1))
}
fn form_field_width(width: u16, columns: usize) -> u16 {
    width.saturating_sub(columns as u16) / columns as u16
}
fn content_form(main: Rect, form: &ManagementForm) -> Rect {
    let max_width = main.width.saturating_sub(u16::from(main.width > 40) * 2);
    let longest = form
        .message
        .lines()
        .map(UnicodeWidthStr::width)
        .chain(form.fields.iter().map(|field| {
            if field.control == ManagementFormControl::Toggle {
                // Keep the switch beside its label, including the scrollbar cell.
                field.label.width()
                    + i18n::tr!("settings-on")
                        .width()
                        .max(i18n::tr!("settings-off").width())
                    + 4
            } else {
                field.label.width().max(field.value.width())
            }
        }))
        .max()
        .unwrap_or(0);
    let columns = form_columns(form);
    let longest = longest.saturating_mul(columns).saturating_add(columns - 1);
    let actions = if !form.buttons.is_empty() {
        form.actions()
            .iter()
            .map(|button| button.label().width() + 3)
            .sum()
    } else if form.dismiss_only {
        form_cancel_text(form).width() + 2
    } else {
        form_submit_text(form).width() + form_cancel_text(form).width() + 5
    };
    let width = longest
        .max(form.title.width())
        .max(actions)
        .saturating_add(7)
        .clamp(32, 88)
        .min(usize::from(max_width)) as u16;
    let content_width = content_form_inner(Rect::new(0, 0, width, 12), true).width;
    let message = if form.message.is_empty() {
        0
    } else {
        management_wrapped_lines(&form.message, content_width.saturating_sub(1)).len()
    };
    let field_width = form_field_width(content_width, columns);
    let fields = form
        .fields
        .chunks(columns)
        .map(|row| {
            row.iter()
                .map(|field| field_lines(field, field_width).len())
                .max()
                .unwrap_or(0)
        })
        .sum::<usize>()
        + form.fields.len().div_ceil(columns).saturating_sub(1);
    let (_, _, _, _, footer) = form_action_sizes(form, content_width);
    let height = (message
        + fields
        + usize::from(message > 0 && fields > 0)
        + usize::from(footer)
        + if main.height > 16 { 5 } else { 3 })
    .min(usize::from(
        main.height.saturating_sub(u16::from(main.height > 16) * 2),
    )) as u16;
    crate::centered_rect(main, width, height)
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
    pub workspace: Option<crate::components::SidebarWorkspaceLayout>,
    pub compact_actions: bool,
    pub filter: Rect,
    pub controls: Vec<(ManagementControl, Rect)>,
    pub list: Rect,
    pub list_rows: Rect,
    pub list_capacity: usize,
    pub headers: Vec<(usize, Rect)>,
    pub column_widths: Vec<usize>,
    pub details: Rect,
    pub details_text: Rect,
    pub detail_actions_panel: Rect,
    pub actions_panel: Rect,
    pub actions: Vec<Rect>,
    pub primary_action_bar: crate::components::PageActionBarLayout,
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
    pub form_buttons: Vec<(usize, Rect)>,
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
fn management_action_rects(area: Rect, actions: &[(String, bool)], compact: bool) -> Vec<Rect> {
    let widths = actions
        .iter()
        .map(|(label, _)| {
            (label.width().min(usize::from(u16::MAX)) as u16)
                .saturating_add(2)
                .min(if compact && area.width >= 32 {
                    area.width.saturating_sub(1) / 2
                } else {
                    area.width
                })
        })
        .collect::<Vec<_>>();
    crate::right_aligned_actions(area, &widths)
}
fn management_search_button_label(model: &ManagementViewModel) -> String {
    if model.filtering {
        i18n::tr!("regression-management-search-enter")
    } else {
        i18n::tr!("management-touch-search")
    }
}
pub fn management_layout(main: Rect, model: &ManagementViewModel) -> ManagementLayout {
    if let Some(workspace) = model.workspace.as_ref().filter(|_| !model.terminal) {
        return workspace::layout(main, model, workspace);
    }
    management_content_layout(main, model, main, 0)
}
fn management_content_layout(
    main: Rect,
    model: &ManagementViewModel,
    form_area: Rect,
    workspace_footer: u16,
) -> ManagementLayout {
    // Very short shell content areas keep actionable controls before repeated
    // titles, status text, borders, and the list/detail preview.
    let compact_actions =
        model.detail_action_start.is_some() && main.height < 10 && !model.terminal;
    let header_y = main
        .y
        .saturating_add(u16::from(!compact_actions))
        .min(main.bottom());
    let min_filter_width = management_search_prefix(0).width() as u16 + 3;
    let refresh_width = management_controls()[0].1.width() as u16 + 2;
    let show_refresh = main.width >= refresh_width + 1 + min_filter_width;
    let refresh = Rect::new(
        main.x,
        header_y,
        if show_refresh { refresh_width } else { 0 },
        u16::from(header_y < main.bottom()),
    );
    let filter_x = if show_refresh {
        refresh.right().saturating_add(1)
    } else {
        main.x
    };
    let available = main.right().saturating_sub(filter_x);
    let clear_width = if model.filter.is_empty() || available < 12 + min_filter_width {
        0
    } else {
        12
    };
    let clear = Rect::new(
        main.right().saturating_sub(clear_width),
        header_y,
        clear_width,
        refresh.height,
    );
    let mut controls = Vec::new();
    if show_refresh {
        controls.push((ManagementControl::Refresh, refresh));
    }
    let search_width = if model.workspace.is_some() {
        management_search_button_label(model)
    } else {
        i18n::tr!("regression-management-search-enter")
    }
    .width() as u16
        + 2;
    let show_search = clear.x.saturating_sub(filter_x) >= search_width + min_filter_width;
    let search = Rect::new(
        clear
            .x
            .saturating_sub(if show_search { search_width } else { 0 }),
        header_y,
        if show_search { search_width } else { 0 },
        refresh.height,
    );
    if show_search {
        controls.push((ManagementControl::ApplySearch, search));
    }
    if clear_width > 0 {
        controls.push((ManagementControl::ClearSearch, clear));
    }
    let filter = Rect::new(
        filter_x,
        header_y,
        search.x.saturating_sub(filter_x),
        refresh.height,
    );
    let header_end = header_y.saturating_add(refresh.height).min(main.bottom());
    let status = Rect::new(
        main.x,
        main.bottom()
            .saturating_sub(u16::from(!compact_actions || model.workspace.is_some()))
            .max(main.y),
        main.width,
        u16::from(main.height > 0 && (!compact_actions || model.workspace.is_some())),
    );
    let detail_start = model
        .detail_action_start
        .map(|start| start.min(model.actions.len()));
    let action_capacity = if model.terminal {
        0
    } else {
        detail_start.unwrap_or_else(|| 6.min(model.actions.len()))
    };
    let action_start = if detail_start.is_some() {
        0
    } else {
        model
            .action_scroll
            .unwrap_or_else(|| {
                model
                    .selected_action
                    .saturating_sub(action_capacity.saturating_sub(1))
            })
            .min(model.actions.len().saturating_sub(action_capacity))
    };
    let paging =
        detail_start.is_none() && model.actions.len() > action_capacity && action_capacity > 0;
    let paging_width = if paging { 14.min(main.width) } else { 0 };
    let action_available = main.width.saturating_sub(paging_width);
    let mut actions = Vec::new();
    let mut action_x = main.x;
    let mut action_y = header_end;
    for index in action_start..action_start + action_capacity {
        let width = (model.actions[index].0.width() as u16)
            .saturating_add(2)
            .min(if compact_actions && action_available >= 32 {
                action_available.saturating_sub(1) / 2
            } else {
                action_available
            });
        if action_x != main.x
            && action_x.saturating_add(width) > main.x.saturating_add(action_available)
        {
            action_x = main.x;
            action_y = action_y.saturating_add(1);
        }
        if action_y >= status.y {
            break;
        }
        actions.push(Rect::new(action_x, action_y, width, u16::from(width > 0)));
        action_x = action_x.saturating_add(width).saturating_add(1);
    }
    let action_height = actions
        .iter()
        .map(|rect| rect.bottom())
        .max()
        .unwrap_or(header_end)
        .saturating_sub(header_end);
    let actions_panel = Rect::new(main.x, header_end, main.width, action_height);
    let action_previous = Rect::new(
        main.right().saturating_sub(paging_width),
        header_end,
        paging_width / 2,
        u16::from(paging && header_end < status.y),
    );
    let action_next = Rect::new(
        action_previous.right(),
        header_end,
        paging_width.saturating_sub(action_previous.width),
        action_previous.height,
    );
    let content = Rect::new(
        main.x,
        actions_panel.bottom(),
        main.width,
        status.y.saturating_sub(actions_panel.bottom()),
    );
    let detail_button_rows = |width: u16| {
        if model.workspace.is_some() {
            return workspace::primary_action_bar(
                model,
                &(0..model.actions.len()).collect::<Vec<_>>(),
            )
            .height(width.saturating_sub(if compact_actions { 0 } else { 2 }));
        }
        detail_start.map_or(0, |start| {
            let area = Rect::new(
                0,
                0,
                width.saturating_sub(if compact_actions { 0 } else { 2 }),
                u16::MAX,
            );
            let rects = if model.workspace.is_some() && !compact_actions {
                workspace::action_rects(area, &model.actions[start..])
            } else {
                management_action_rects(area, &model.actions[start..], compact_actions)
            };
            rects.iter().map(|area| area.bottom()).max().unwrap_or(0)
        })
    };
    let (mut list, mut details) = if model.workspace.is_some() {
        crate::components::SidebarWorkspaceLayout::panes(content, compact_actions)
    } else if compact_actions {
        (Rect::default(), content)
    } else if main.width >= 90 {
        let left = content.width * 58 / 100;
        (
            Rect::new(content.x, content.y, left, content.height),
            Rect::new(
                content.x + left,
                content.y,
                content.width.saturating_sub(left),
                content.height,
            ),
        )
    } else {
        let minimum_details = if detail_start.is_some() {
            detail_button_rows(content.width).saturating_add(5)
        } else {
            2
        };
        let list_height =
            (content.height * 3 / 4).min(content.height.saturating_sub(minimum_details));
        (
            Rect::new(content.x, content.y, content.width, list_height),
            Rect::new(
                content.x,
                content.y + list_height,
                content.width,
                content.height.saturating_sub(list_height),
            ),
        )
    };
    if !compact_actions {
        (list, details) = crate::expand_trailing_pane(
            list,
            details,
            model
                .details_expansion
                .unwrap_or(if model.details_only { 1_000 } else { 0 }),
        );
    }
    list.height = list.height.saturating_sub(workspace_footer);
    let mut detail_actions_panel = Rect::default();
    if let Some(start) = detail_start.filter(|_| !model.terminal) {
        let height = detail_button_rows(details.width)
            .saturating_add(if compact_actions { 0 } else { 2 })
            .min(details.height);
        detail_actions_panel = Rect::new(
            details.x,
            details.bottom().saturating_sub(height),
            details.width,
            height,
        );
        details.height = details.height.saturating_sub(height);
        actions.resize(start, Rect::default());
        let area = if compact_actions {
            detail_actions_panel
        } else {
            inner(detail_actions_panel)
        };
        actions.extend(if model.workspace.is_some() && !compact_actions {
            workspace::action_rects(area, &model.actions[start..])
        } else {
            management_action_rects(area, &model.actions[start..], compact_actions)
        });
    }
    let mut column_widths = model
        .columns
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let natural = model
                .rows
                .iter()
                .filter_map(|row| row.get(index))
                .map(|value| value.width())
                .max()
                .unwrap_or(0)
                .max(label.width() + 2)
                .max(4);
            model
                .column_width_limits
                .get(index)
                .map_or(natural, |limit| {
                    natural.min((*limit).max(label.width() + 2))
                })
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
    let mut offset = 0;
    let mut headers = Vec::new();
    for (index, width) in column_widths
        .iter()
        .copied()
        .enumerate()
        .take(model.columns.len())
    {
        let start = offset.max(model.table_scroll);
        let end = (offset + width).min(model.table_scroll + usize::from(list_rows.width));
        if start < end && list_inner.height > 0 {
            headers.push((
                index,
                Rect::new(
                    list_rows.x + (start - model.table_scroll) as u16,
                    list_inner.y,
                    (end - start) as u16,
                    1,
                ),
            ));
        }
        offset += width;
    }
    let list_capacity = usize::from(list_rows.height);
    let mut details_text = text_area(inner(details));
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
    if model.workspace.is_some() && model.details_only {
        let lines = management_wrapped_lines(&model.details, u16::MAX);
        let width = lines.iter().map(|line| line.width()).max().unwrap_or(0);
        if let Some(bar) = management_scrollbar(
            ManagementScrollTarget::DetailsColumns,
            bottom_track(details_text),
            width,
            usize::from(details_text.width),
            usize::from(model.details_horizontal_scroll),
            true,
        ) {
            details_text.height = details_text.height.saturating_sub(1);
            scrollbars.push(bar);
        }
        if let Some(bar) = management_scrollbar(
            ManagementScrollTarget::Details,
            Rect::new(
                details_text.right(),
                details_text.y,
                u16::from(inner(details).width > 0),
                details_text.height,
            ),
            lines.len(),
            usize::from(details_text.height),
            usize::from(model.details_scroll),
            false,
        ) {
            scrollbars.push(bar);
        }
    } else {
        add_text_bar(
            &mut scrollbars,
            ManagementScrollTarget::Details,
            inner(details),
            &model.details,
            usize::from(model.details_scroll),
        );
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
    let form_model = model.form.as_ref();
    let form = form_model
        .map(|form| {
            form.choice.as_ref().map_or_else(
                || content_form(form_area, form),
                |choice| choice_form(form_area, form, choice),
            )
        })
        .unwrap_or_default();
    let form_inner = content_form_inner(form, main.height > 16);
    let message_len = form_model.map_or(0, |form| {
        if form.message.is_empty() {
            0
        } else {
            management_wrapped_lines(&form.message, form_inner.width.saturating_sub(1)).len()
        }
    });
    let has_fields = form_model.is_some_and(|form| !form.fields.is_empty());
    let (submit_width, cancel_width, submit_height, cancel_height, footer_height) =
        form_model.map_or((0, 0, 0, 0, 0), |f| form_action_sizes(f, form_inner.width));
    let footer_height = footer_height.min(form_inner.height);
    let available = form_inner
        .height
        .saturating_sub(footer_height + u16::from(form_inner.height > footer_height));
    let field_min = form_model
        .and_then(|f| {
            f.fields
                .get(f.selected.min(f.fields.len().saturating_sub(1)))
        })
        .map_or(0, |field| {
            field_lines(
                field,
                form_field_width(form_inner.width, form_model.map_or(1, form_columns)),
            )
            .len()
            .min(3) as u16
        });
    let message_height = if has_fields && message_len > 0 {
        (message_len.min(usize::from(
            available
                .saturating_sub(field_min + 1)
                .max(u16::from(available >= 2)),
        ))) as u16
    } else {
        (message_len.min(usize::from(available))) as u16
    };
    let message_gap = u16::from(
        has_fields
            && message_height > 0
            && available > message_height + field_min.min(available.saturating_sub(message_height)),
    );
    let message = Rect::new(form_inner.x, form_inner.y, form_inner.width, message_height);
    let fields_area = Rect::new(
        form_inner.x,
        message.bottom().saturating_add(message_gap),
        form_inner.width,
        available.saturating_sub(message_height + message_gap),
    );
    let columns = form_model.map_or(1, form_columns);
    let field_width = form_field_width(fields_area.width, columns);
    let mut field_ranges = Vec::new();
    let mut field_length = 0;
    if let Some(form) = form_model {
        for row in form.fields.chunks(columns) {
            let heights = row
                .iter()
                .map(|field| field_lines(field, field_width).len())
                .collect::<Vec<_>>();
            let row_height = heights.iter().copied().max().unwrap_or(0);
            for height in heights {
                field_ranges.push((field_length, height));
            }
            field_length += row_height + 1;
        }
        field_length = field_length.saturating_sub(1);
    }
    let mut selected_range = form_model
        .and_then(|form| field_ranges.get(form.selected))
        .copied()
        .unwrap_or((0, 0));
    if let Some(field) = form_model.and_then(|form| form.fields.get(form.selected))
        && field.control == ManagementFormControl::Permissions
    {
        selected_range = (
            selected_range.0 + 2 + field.selected_cell.unwrap_or(0) / 3,
            1,
        );
    }
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
                        fields_area.x + (field_width + 1) * (index % columns) as u16,
                        fields_area.y + (start_y - field_start) as u16,
                        field_width,
                        (end - start_y) as u16,
                    ),
                )
            })
        })
        .collect();
    let mut submit = Rect::new(
        if submit_width.saturating_add(cancel_width).saturating_add(1) > form_inner.width {
            form_inner.right().saturating_sub(submit_width)
        } else {
            form_inner
                .right()
                .saturating_sub(submit_width + cancel_width + u16::from(submit_width > 0))
                .max(form_inner.x)
        },
        form_inner.bottom().saturating_sub(footer_height),
        submit_width,
        submit_height.min(footer_height),
    );
    let mut cancel = Rect::new(
        form_inner.right().saturating_sub(cancel_width),
        form_inner
            .bottom()
            .saturating_sub(cancel_height.min(footer_height))
            .max(form_inner.y),
        cancel_width,
        cancel_height.min(footer_height),
    );
    let form_buttons = form_model.map_or_else(Vec::new, |form| {
        if !form.buttons.is_empty() {
            let buttons = form
                .action_bar()
                .layout(form_inner)
                .actions
                .into_iter()
                .map(|button| (button.index, button.area))
                .collect::<Vec<_>>();
            for (index, area) in &buttons {
                match form.buttons[*index].action {
                    crate::components::FormAction::Submit => submit = *area,
                    crate::components::FormAction::Cancel => cancel = *area,
                    _ => {}
                }
            }
            buttons
        } else if form.dismiss_only {
            vec![(0, cancel)]
        } else {
            vec![(0, submit), (1, cancel)]
        }
    });
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
        let choice_inner = choice_form_inner(form);
        let content_width = management_choice_width(form_model.unwrap(), choice);
        let footer_height = u16::from(choice_inner.height > 0);
        let gap = u16::from(choice_inner.height > 3);
        let available = choice_inner.height.saturating_sub(footer_height + gap);
        let mut vertical = choice.values.len() > usize::from(available);
        let mut horizontal =
            content_width > usize::from(choice_inner.width.saturating_sub(u16::from(vertical)));
        vertical |=
            choice.values.len() > usize::from(available.saturating_sub(u16::from(horizontal)));
        horizontal |=
            content_width > usize::from(choice_inner.width.saturating_sub(u16::from(vertical)));
        let row_height = available.saturating_sub(u16::from(horizontal));
        let rows = Rect::new(
            choice_inner.x,
            choice_inner.y,
            choice_inner.width.saturating_sub(u16::from(vertical)),
            row_height,
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
            choice_inner.right().saturating_sub(
                (i18n::tr!("management-form-close").width() + 2)
                    .min(usize::from(choice_inner.width)) as u16,
            ),
            choice_inner.bottom().saturating_sub(footer_height),
            (i18n::tr!("management-form-close").width() + 2).min(usize::from(choice_inner.width))
                as u16,
            footer_height,
        );
        if let Some(bar) = management_scrollbar(
            ManagementScrollTarget::Choices,
            Rect::new(
                choice_inner.right().saturating_sub(1),
                rows.y,
                u16::from(vertical),
                rows.height,
            ),
            choice.values.len(),
            count,
            start,
            false,
        ) {
            scrollbars.push(bar);
        }
        if let Some(bar) = management_scrollbar(
            ManagementScrollTarget::ChoiceColumns,
            Rect::new(
                rows.x,
                rows.bottom(),
                rows.width,
                u16::from(horizontal && available > 0),
            ),
            content_width,
            usize::from(rows.width),
            choice.columns,
            true,
        ) {
            scrollbars.push(bar);
        }
    }
    ManagementLayout {
        workspace: None,
        primary_action_bar: Default::default(),
        compact_actions,
        filter,
        controls,
        list,
        list_rows,
        list_capacity,
        headers,
        column_widths,
        details,
        details_text,
        detail_actions_panel,
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
        form_buttons,
        choice_panel,
        choice_rows,
        choice_cancel,
        scrollbars,
    }
}

pub fn management_control_id(model: &ManagementViewModel, control: ManagementControl) -> String {
    format!("management.control.{}.{control:?}", model.scope_id)
}
pub fn management_header_id(model: &ManagementViewModel, index: usize) -> String {
    format!(
        "management.sort.{}.{}.{:?}",
        model.scope_id, index, model.columns
    )
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
    if main.is_empty() {
        return Vec::new();
    }
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
                push(
                    management_choice_id(form, *index),
                    *area,
                    form.choice
                        .as_ref()
                        .and_then(|choice| choice.disabled.get(*index))
                        .copied()
                        .unwrap_or(false),
                );
            }
        } else {
            let actions = form.actions();
            for (index, area) in &layout.form_buttons {
                let button = &actions[*index];
                push(
                    management_form_control_id(form, &button.id),
                    *area,
                    button.disabled,
                );
            }
            for (index, area) in &layout.fields {
                let field = &form.fields[*index];
                if matches!(field.control, ManagementFormControl::Number { .. }) {
                    for (suffix, area) in ["decrease", "increase"]
                        .into_iter()
                        .zip(management_number_buttons(&layout, *index, field))
                    {
                        push(
                            format!("{}.{}", management_field_id(form, *index), suffix),
                            area,
                            field.disabled,
                        );
                    }
                }
                if field.control == ManagementFormControl::Permissions {
                    let skip = layout
                        .field_start
                        .saturating_sub(layout.field_ranges[*index].0);
                    for (cell, area) in management_permission_cells(field, *area, skip) {
                        push(management_permission_id(form, *index, cell), area, false);
                    }
                } else {
                    push(management_field_id(form, *index), *area, field.disabled);
                }
            }
        }
        return buttons;
    }
    for (control, area) in &layout.controls {
        push(
            management_control_id(model, *control),
            *area,
            *control == ManagementControl::Details
                && model.rows.is_empty()
                && model.details.is_empty(),
        );
    }
    push(
        format!("management.filter.{}", model.scope_id),
        layout.filter,
        false,
    );
    if !model.terminal {
        for (index, area) in &layout.headers {
            push(management_header_id(model, *index), *area, false);
        }
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
    if main.is_empty() {
        return;
    }
    let mut content_context = context.clone();
    if model.form.is_some() {
        // A form owns the main area's buttons. Keep the covered page visible,
        // but do not register its controls or pass pointer feedback to them.
        content_context.buttons = None;
    }
    let context = &content_context;
    let theme = context.compatibility_theme();
    let layout = management_layout(main, model);
    if model
        .details_expansion
        .is_some_and(|amount| amount > 0 && amount < 1_000)
    {
        for area in [layout.list, layout.details, layout.detail_actions_panel] {
            Surface::new()
                .bordered(true)
                .render_frame(frame, area, context);
        }
        return;
    }
    let selected = theme
        .title_style()
        .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    if !layout.compact_actions {
        let title_area = layout
            .workspace
            .as_ref()
            .map_or(main, |workspace| workspace.content);
        frame.render_widget(
            Paragraph::new(model.title.clone()).style(theme.title_style()),
            Rect::new(
                title_area.x,
                title_area.y,
                title_area.width,
                u16::from(title_area.height > 0),
            ),
        );
    }
    if let (Some(workspace), Some(workspace_layout)) = (&model.workspace, &layout.workspace) {
        let buttons = workspace
            .navigation_actions
            .iter()
            .map(|index| {
                let mut button = Button::new(
                    management_action_id(model, *index),
                    model.actions[*index].0.clone(),
                );
                button.set_disabled(!model.actions[*index].1);
                button.set_focused(
                    model.actions_focused
                        && model.selected_action == *index
                        && model.form.is_none(),
                );
                button
            })
            .collect::<Vec<_>>();
        workspace_layout.render_navigation(frame, &buttons, workspace.active_navigation, context);
    }
    for (control, area) in &layout.controls {
        let label = match control {
            ManagementControl::ClearSearch => "× [Ctrl+U]".into(),
            ManagementControl::ApplySearch => management_search_button_label(model),
            _ => management_controls()[0].1.clone(),
        };
        let mut button = Button::new(management_control_id(model, *control), label);
        button.set_disabled(
            *control == ManagementControl::Details
                && model.rows.is_empty()
                && model.details.is_empty(),
        );
        button.render_borderless_frame(frame, *area, &theme);
    }
    let mut filter = TextInput::new(format!("management.filter.{}", model.scope_id))
        .with_capture_name(i18n::tr!("management-search"))
        .with_cursor_symbol("_")
        .with_horizontal_scroll(true);
    filter.set_value(&model.filter);
    filter.set_focused(model.filtering && model.form.is_none());
    filter.render_borderless_frame_with_prefix(
        frame,
        layout.filter,
        &theme,
        &management_search_prefix(layout.filter.width),
    );
    if model.terminal {
        if let Some(snapshot) = &model.terminal_snapshot {
            super::super::command_line::render_terminal_snapshot(
                frame,
                layout.output_text,
                snapshot,
                &theme,
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
            .focused(
                model.workspace.as_ref().is_some_and(|workspace| {
                    workspace.focus == crate::components::WorkspaceFocus::Details
                }) && model.form.is_none(),
            )
            .titled(i18n::tr!("management-touch-details"))
            .render_frame(frame, layout.details, context);
        if !layout.detail_actions_panel.is_empty() && !layout.compact_actions {
            Surface::new()
                .bordered(true)
                .titled(i18n::tr!("management-touch-actions"))
                .render_frame(frame, layout.detail_actions_panel, context);
        }
        let table_line = |cells: &[String]| {
            cells
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let width = layout.column_widths.get(index).copied().unwrap_or(6);
                    let value = if value.width() > width.saturating_sub(2) {
                        format!(
                            "{}…",
                            crate::components::truncate_to_terminal_width(
                                value,
                                width.saturating_sub(3)
                            )
                        )
                    } else {
                        value.clone()
                    };
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
        if crate::automation::enabled() {
            let _table = crate::automation::scope(
                &format!("management.rows.{}", model.scope_id),
                "table",
                &model.title,
                layout.list,
            );
            for (index, row) in model
                .rows
                .iter()
                .enumerate()
                .skip(model.scroll)
                .take(layout.list_capacity)
            {
                let row_area = Rect::new(
                    layout.list_rows.x,
                    layout.list_rows.y + (index - model.scroll) as u16,
                    layout.list_rows.width,
                    1,
                );
                let _row = crate::automation::scope(&format!("row.{index}"), "row", "", row_area);
                crate::automation::control(
                    "select",
                    "listitem",
                    "",
                    row_area,
                    crate::components::ComponentState {
                        selected: model
                            .selected_rows
                            .as_ref()
                            .map_or(index == model.selected, |rows| rows.contains(&index)),
                        focused: index == model.selected
                            && !model.filtering
                            && !model.actions_focused,
                        ..Default::default()
                    },
                    &["click", "double_click"],
                );
                for (column, header) in &layout.headers {
                    if let Some(value) = row.get(*column) {
                        crate::automation::control(
                            &format!("cell.{column}"),
                            "cell",
                            value,
                            Rect::new(header.x, row_area.y, header.width, 1),
                            Default::default(),
                            &[],
                        );
                    }
                }
            }
        }
        for (index, row) in model
            .rows
            .iter()
            .enumerate()
            .skip(model.scroll)
            .take(layout.list_capacity)
        {
            let mut style = if model
                .selected_rows
                .as_ref()
                .map_or(index == model.selected, |rows| rows.contains(&index))
            {
                selected
            } else {
                theme.body_style()
            };
            if model.selected_rows.is_some()
                && index == model.selected
                && !model.actions_focused
                && !model.filtering
                && theme.keyboard_focus_visible()
            {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            lines.push(Line::styled(table_line(row), style));
        }
        let area = Rect::new(
            layout.list_rows.x,
            layout.list_rows.y.saturating_sub(1),
            layout.list_rows.width,
            layout.list_rows.height.saturating_add(1),
        );
        frame.render_widget(
            Paragraph::new(lines).scroll((0, model.table_scroll.min(u16::MAX as usize) as u16)),
            area.intersection(main),
        );
        if model.rows.is_empty() {
            let message = if model.loading {
                i18n::tr!("management-loading")
            } else if model.empty_message.is_empty() {
                i18n::tr!("management-empty")
            } else {
                model.empty_message.clone()
            };
            render_text(frame, layout.list_rows, &message, 0, context);
        }
        for (index, area) in &layout.headers {
            let label = model.sort.map_or_else(
                || model.columns[*index].clone(),
                |sort| sort.label(*index, &model.columns[*index]),
            );
            let hidden = model
                .table_scroll
                .saturating_sub(layout.column_widths[..*index].iter().sum());
            let mut cells = 0;
            let label = label
                .chars()
                .filter(|c| {
                    let visible = cells >= hidden;
                    cells += c.width().unwrap_or(0);
                    visible
                })
                .collect::<String>();
            Button::new(
                management_header_id(model, *index),
                crate::table_sort::table_header_text(&label, area.width),
            )
            .with_bracketed_label(false)
            .render_borderless_frame(frame, *area, &theme);
        }
        if model.workspace.is_some() && model.details_only {
            let offset = |target| {
                layout
                    .scrollbars
                    .iter()
                    .find(|bar| bar.target == target)
                    .map_or(0, |bar| bar.offset.min(u16::MAX as usize) as u16)
            };
            frame.render_widget(
                Paragraph::new(
                    management_wrapped_lines(&model.details, u16::MAX)
                        .into_iter()
                        .map(Line::from)
                        .collect::<Vec<_>>(),
                )
                .scroll((
                    offset(ManagementScrollTarget::Details),
                    offset(ManagementScrollTarget::DetailsColumns),
                ))
                .style(theme.body_style()),
                layout.details_text,
            );
        } else {
            render_text(
                frame,
                layout.details_text,
                &model.details,
                usize::from(model.details_scroll),
                context,
            );
        }
        if let Some(workspace) = &model.workspace {
            workspace::primary_action_bar(model, &workspace.primary_actions).render(
                frame,
                layout.primary_action_bar.area,
                context,
            );
        }
        for (offset, area) in layout.actions.iter().enumerate() {
            let index = layout.action_start + offset;
            if model.workspace.as_ref().is_some_and(|workspace| {
                workspace.navigation_actions.contains(&index)
                    || workspace.primary_actions.contains(&index)
            }) && layout.workspace.is_some()
            {
                continue;
            }
            if let Some((label, enabled)) = model.actions.get(index) {
                let mut button = if model.workspace.is_some() {
                    Button::new(
                        management_action_id(model, index),
                        management_wrapped_lines(label, area.width).join("\n"),
                    )
                    .with_bracketed_label(false)
                } else {
                    Button::new(management_action_id(model, index), label.clone())
                };
                if model.action_tones.get(index) == Some(&crate::components::ComponentTone::Danger)
                {
                    button = button.with_emphasis(crate::components::ButtonEmphasis::Danger);
                }
                button.set_disabled(!enabled);
                button.state.selected =
                    model.form.is_none() && model.actions_focused && model.selected_action == index;
                button.render_borderless_frame(frame, *area, &theme);
            }
        }
        for (area, next, disabled) in [
            (layout.action_previous, false, layout.action_start == 0),
            (
                layout.action_next,
                true,
                layout.action_start + layout.actions.len() >= model.actions.len(),
            ),
        ] {
            if area.height > 0 {
                let mut button = Button::new(
                    management_action_page_id(model, layout.action_start, next),
                    if next { "Alt+→" } else { "Alt+←" },
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
                    | ManagementScrollTarget::DetailsColumns
                    | ManagementScrollTarget::Actions
            )
        };
        if show {
            bar.render(frame, context);
        }
    }
    frame.render_widget(
        Paragraph::new(model.status.clone()).style(theme.muted_style()),
        layout.status,
    );
}
pub fn render_management_overlay(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &ManagementViewModel,
    context: &RenderContext,
) {
    if main.is_empty() {
        return;
    }
    let Some(form) = &model.form else {
        return;
    };
    let layout = management_layout(main, model);
    let theme = context.compatibility_theme();
    let _scope = crate::automation::scope(
        &format!("management.form.{}", form.identity),
        "dialog",
        &form.title,
        layout.form,
    );
    frame.render_widget(Clear, layout.form);
    Surface::new()
        .bordered(form.choice.is_some() || layout.form.height > 5)
        .raised(true)
        .titled(
            if form
                .choice
                .as_ref()
                .is_some_and(|choice| !choice.search.is_empty())
            {
                format!("{} · {}", form.title, form.choice.as_ref().unwrap().search)
            } else {
                form.title.clone()
            },
        )
        .render_frame(frame, layout.form, context);
    if let Some(choice) = &form.choice {
        for (index, area) in &layout.choice_rows {
            let value = &choice.values[*index];
            let is_menu = management_choice_is_menu(form);
            let text = if is_menu {
                value.clone()
            } else {
                format!(
                    "{} {value}",
                    if *index == choice.selected {
                        "●"
                    } else {
                        "○"
                    }
                )
            };
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
            let mut button = Button::new(management_choice_id(form, *index), text)
                .with_current(!is_menu && *index == choice.selected);
            button.bracketed_label = !is_menu;
            button.set_disabled(choice.disabled.get(*index).copied().unwrap_or(false));
            button.state.selected = *index == choice.selected;
            button.render_borderless_frame(frame, *area, &theme);
        }
        Button::new(
            management_form_control_id(form, "choice.cancel"),
            i18n::tr!("management-form-close"),
        )
        .render_borderless_frame(frame, layout.choice_cancel, &theme);
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
        let lines = field_lines(field, area.width);
        let skip = layout
            .field_start
            .saturating_sub(layout.field_ranges[*index].0);
        if field.input.is_some() || field.is_text() {
            if let Some(buttons) = &theme.buttons {
                buttons.register_region(crate::components::ButtonRegion {
                    id: management_field_id(form, *index).into(),
                    area: *area,
                    disabled: field.disabled,
                });
            }
            let label_rows = lines.len().saturating_sub(1);
            frame.render_widget(
                Paragraph::new(
                    lines
                        .into_iter()
                        .take(label_rows)
                        .skip(skip)
                        .map(Line::from)
                        .collect::<Vec<_>>(),
                )
                .style(if form.selected == *index {
                    theme.title_style()
                } else {
                    theme.body_style()
                }),
                *area,
            );
            if skip <= label_rows && label_rows - skip < usize::from(area.height) {
                let input_area = management_field_input_area(&layout, *index, field);
                let mut input = field
                    .input
                    .clone()
                    .unwrap_or_else(|| {
                        let mut input = TextInput::new(management_field_id(form, *index))
                            .with_horizontal_scroll(true);
                        input.set_value(if field.secret {
                            "•".repeat(field.value.chars().count())
                        } else {
                            field.value.clone()
                        });
                        input
                    })
                    .with_capture_name(&field.label)
                    .with_sensitive(field.secret);
                input.id = management_field_id(form, *index).into();
                if field.secret {
                    let cursor = input.cursor();
                    let anchor = input.selection_anchor();
                    input.set_value("•".repeat(field.value.chars().count()));
                    input.set_cursor(cursor);
                    input.set_selection_anchor(anchor);
                }
                input.state.disabled = field.disabled;
                input.set_focused(form.selected == *index && !field.disabled);
                input.render_borderless_frame(frame, input_area, &theme);
                if matches!(field.control, ManagementFormControl::Number { .. }) {
                    for ((suffix, label), area) in [("decrease", "−"), ("increase", "+")]
                        .into_iter()
                        .zip(management_number_buttons(&layout, *index, field))
                    {
                        let mut button = Button::new(
                            format!("{}.{}", management_field_id(form, *index), suffix),
                            label,
                        )
                        .with_bracketed_label(area.width >= 3);
                        button.set_disabled(field.disabled);
                        button.render_borderless_frame(frame, area, &theme);
                    }
                }
            }
            continue;
        }
        if field.control == ManagementFormControl::Permissions {
            frame.render_widget(
                Paragraph::new(
                    lines
                        .into_iter()
                        .skip(skip)
                        .map(Line::from)
                        .collect::<Vec<_>>(),
                )
                .style(theme.body_style()),
                *area,
            );
            for (cell, area) in management_permission_cells(field, *area, skip) {
                let value = match field.value.as_bytes().get(cell) {
                    Some(b'1') => "[x]",
                    Some(b'0') => "[ ]",
                    _ => "[-]",
                };
                let mut button = Button::new(management_permission_id(form, *index, cell), value)
                    .with_capture_role("checkbox")
                    .with_capture_selected(field.value.as_bytes().get(cell) == Some(&b'1'));
                button.bracketed_label = false;
                button.state.selected =
                    form.selected == *index && field.selected_cell == Some(cell);
                button.render_borderless_frame(frame, area, &theme);
            }
            continue;
        }
        let mut button = Button::new(
            management_field_id(form, *index),
            lines.into_iter().skip(skip).collect::<Vec<_>>().join("\n"),
        )
        .with_current(form.selected == *index)
        .with_capture_name(&field.label)
        .with_sensitive(field.secret)
        .with_capture_role(match field.control {
            ManagementFormControl::Checkbox | ManagementFormControl::Toggle => "checkbox",
            _ if !field.choices.is_empty() => "combobox",
            _ => "textbox",
        });
        if matches!(
            field.control,
            ManagementFormControl::Checkbox | ManagementFormControl::Toggle
        ) {
            button = button.with_capture_selected(field.value == "true");
        }
        button.bracketed_label = false;
        button.state.disabled = field.disabled;
        button.state.selected = form.selected == *index;
        button.state.focused = form.selected == *index;
        button.render_borderless_frame(frame, *area, &theme);
    }
    let actions = form.actions();
    for (index, area) in &layout.form_buttons {
        let action = &actions[*index];
        let mut button = Button::new(
            management_form_control_id(form, &action.id),
            management_wrapped_lines(&format!("[{}]", action.label()), area.width).join("\n"),
        )
        .with_bracketed_label(false);
        button.set_disabled(action.disabled);
        button.set_focused(form.selected == form.fields.len() + index);
        button.render_borderless_frame(frame, *area, &theme);
    }
    for bar in &layout.scrollbars {
        if matches!(
            bar.target,
            ManagementScrollTarget::FormMessage | ManagementScrollTarget::FormFields
        ) {
            bar.render(frame, context);
        }
    }
}

pub fn management_field_input_area(
    layout: &ManagementLayout,
    index: usize,
    field: &ManagementFormField,
) -> Rect {
    let Some((_, area)) = layout.fields.iter().find(|(i, _)| *i == index) else {
        return Rect::default();
    };
    let label_rows = field_lines(field, area.width).len().saturating_sub(1);
    let skip = layout
        .field_start
        .saturating_sub(layout.field_ranges[index].0);
    if skip <= label_rows && label_rows - skip < usize::from(area.height) {
        let input = Rect::new(area.x, area.y + (label_rows - skip) as u16, area.width, 1);
        if matches!(field.control, ManagementFormControl::Number { .. }) {
            Rect::new(
                input
                    .x
                    .saturating_add(if input.width < 10 { 2 } else { 4 })
                    .min(input.right()),
                input.y,
                input
                    .width
                    .saturating_sub(if input.width < 10 { 4 } else { 8 }),
                1,
            )
        } else {
            input
        }
    } else {
        Rect::default()
    }
}

pub fn management_number_buttons(
    layout: &ManagementLayout,
    index: usize,
    field: &ManagementFormField,
) -> [Rect; 2] {
    let Some((_, area)) = layout.fields.iter().find(|(field, _)| *field == index) else {
        return [Rect::default(); 2];
    };
    let input = management_field_input_area(layout, index, field);
    if input.is_empty() {
        return [Rect::default(); 2];
    }
    let width = if area.width < 10 { 1 } else { 3 };
    [
        Rect::new(area.x, input.y, width.min(area.width), 1),
        Rect::new(
            area.right().saturating_sub(width).max(area.x),
            input.y,
            width.min(area.width),
            1,
        ),
    ]
}

pub fn management_search_prefix(width: u16) -> String {
    let label = format!("{} ", i18n::tr!("management-search"));
    if usize::from(width) >= label.width() + 5 {
        label
    } else {
        "⌕ ".into()
    }
}
