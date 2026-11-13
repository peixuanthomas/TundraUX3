use crate::{
    RenderContext,
    components::{
        Button, ButtonEmphasis, DialogLayout, FormAction, PageAction, PageActionBar, Scrollbar,
        ScrollbarOrientation, Surface, TextInput,
    },
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
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
    pub filter_edit: crate::components::TextEditState,
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
fn management_clipped_line(text: &str, hidden: usize, width: u16) -> String {
    let mut cells = 0;
    let span = Span::raw(text);
    let clipped = span
        .styled_graphemes(Style::default())
        .filter_map(|glyph| {
            let start = cells;
            cells += glyph.symbol.width();
            if start >= hidden {
                Some(glyph.symbol.to_string())
            } else if cells > hidden {
                Some(" ".repeat(cells - hidden))
            } else {
                None
            }
        })
        .collect::<String>();
    crate::components::truncate_to_terminal_width(&clipped, usize::from(width))
}
fn pane_inner(area: Rect) -> Rect {
    Surface::new().titled("").inner(area)
}
fn management_choice_width(choice: &ManagementChoices) -> usize {
    choice
        .values
        .iter()
        .map(|value| value.width())
        .max()
        .unwrap_or(0)
}
fn choice_form(main: Rect, form: &ManagementForm, choice: &ManagementChoices) -> Rect {
    let content_width = management_choice_width(choice)
        .saturating_add(1)
        .max(form.title.width() + i18n::tr!("management-form-close").width() + 3);
    let width = content_width
        .saturating_add(6)
        .clamp(24, 88)
        .min(usize::from(
            main.width.saturating_sub(u16::from(main.width > 8) * 2),
        )) as u16;
    let horizontal =
        management_choice_width(choice) > usize::from(DialogLayout::content_width(width));
    let height = choice
        .values
        .len()
        .saturating_add(4 + usize::from(horizontal))
        .min(usize::from(
            main.height.saturating_sub(u16::from(main.height > 8) * 2),
        )) as u16;
    crate::centered_rect(main, width, height)
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
fn form_columns(form: &ManagementForm) -> usize {
    form.columns.max(1).min(form.fields.len().max(1))
}
fn form_field_width(width: u16, columns: usize) -> u16 {
    width.saturating_sub(columns as u16) / columns as u16
}
fn content_form(
    main: Rect,
    form: &ManagementForm,
    bar: &PageActionBar,
    close: Option<&str>,
) -> Rect {
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
    let actions = bar
        .actions
        .iter()
        .map(|button| button.label.width() + 3)
        .sum();
    let width = longest
        .max(form.title.width() + close.map_or(0, |label| label.width() + 3))
        .max(actions)
        .saturating_add(7)
        .clamp(32, 88)
        .min(usize::from(max_width)) as u16;
    let content_width = DialogLayout::content_width(width);
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
    let footer = bar.height(content_width);
    let height = (message
        + fields
        + usize::from(message > 0 && fields > 0)
        + usize::from(footer)
        + if footer > 0 { 6 } else { 4 })
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Final content geometry; `details_text` is its visible intersection.
    pub details_canvas: Rect,
    pub detail_actions_panel: Rect,
    pub detail_actions_canvas: Rect,
    pub actions_panel: Rect,
    pub actions: Vec<Rect>,
    /// Full rectangles indexed alongside `actions`, before viewport clipping.
    pub action_canvases: Vec<Rect>,
    pub primary_action_bar: crate::components::PageActionBarLayout,
    pub action_start: usize,
    pub action_previous: Rect,
    pub action_next: Rect,
    pub status: Rect,
    pub content: Rect,
    pub output_text: Rect,
    pub form: Rect,
    dialog: DialogLayout,
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

impl ManagementLayout {
    pub fn scrollbar_hit_test(&self, bar: &ManagementScrollbar, point: (u16, u16)) -> bool {
        let position = point.into();
        bar.track.contains(position)
            || (bar.hit_test(point)
                && !self
                    .choice_rows
                    .iter()
                    .any(|(_, row)| row.contains(position))
                && !self.choice_cancel.contains(position))
    }
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
fn management_action_rects(area: Rect, actions: &[(String, bool)], _compact: bool) -> Vec<Rect> {
    let actions = actions
        .iter()
        .enumerate()
        .map(|(index, (label, enabled))| {
            crate::components::PageAction::new(index.to_string(), label.clone())
                .with_bracketed_label(false)
                .disabled(!enabled)
        })
        .collect::<Vec<_>>();
    let bar = crate::components::PageActionBar::new("").with_actions(actions);
    let mut rects = vec![Rect::default(); bar.actions.len()];
    for action in bar
        .layout(Rect::new(
            area.x,
            area.y,
            area.width,
            bar.height(area.width).min(area.height),
        ))
        .actions
    {
        rects[action.index] = action.area;
    }
    rects
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
pub fn management_busy_visible(main: Rect, model: &ManagementViewModel) -> bool {
    model.loading
        && model.rows.is_empty()
        && !model.terminal
        && model.form.is_none()
        && !management_layout(main, model).list_rows.is_empty()
}
fn management_content_layout(
    main: Rect,
    model: &ManagementViewModel,
    form_area: Rect,
    workspace_footer: u16,
) -> ManagementLayout {
    // Very short shell content areas keep actionable controls before repeated
    // titles, status text, borders, and the list/detail preview.
    let compact_actions = model.detail_action_start.is_some()
        && main.height
            < if model
                .workspace
                .as_ref()
                .is_some_and(|workspace| workspace.navigation_actions.is_empty())
            {
                5
            } else {
                10
            }
        && !model.terminal;
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
            .height(width);
        }
        detail_start.map_or(0, |start| {
            let area = Rect::new(0, 0, width, u16::MAX);
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
        let available = content.width.saturating_sub(2);
        let left = available * 58 / 100;
        (
            Rect::new(content.x, content.y, left, content.height),
            Rect::new(
                content.x + left + 2,
                content.y,
                available.saturating_sub(left),
                content.height,
            ),
        )
    } else {
        let minimum_details = if detail_start.is_some() {
            detail_button_rows(content.width).saturating_add(5)
        } else {
            2
        };
        let available = content.height.saturating_sub(1);
        let list_height = (available * 3 / 4).min(available.saturating_sub(minimum_details));
        (
            Rect::new(content.x, content.y, content.width, list_height),
            Rect::new(
                content.x,
                content.y + list_height + 1,
                content.width,
                available.saturating_sub(list_height),
            ),
        )
    };
    let mut details_canvas = if compact_actions {
        details
    } else {
        crate::expand_trailing_pane(list, details, if model.details_only { 1_000 } else { 0 }).1
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
    let mut detail_actions_canvas = Rect::default();
    let mut action_canvases = actions.clone();
    if let Some(start) = detail_start.filter(|_| !model.terminal) {
        let canvas_height = detail_button_rows(details_canvas.width).min(details_canvas.height);
        let height = canvas_height.min(details.height);
        details_canvas.height = details_canvas.height.saturating_sub(canvas_height);
        detail_actions_panel = Rect::new(
            details.x,
            details.bottom().saturating_sub(height),
            details.width,
            height,
        );
        detail_actions_canvas = Rect::new(
            details.right().saturating_sub(details_canvas.width),
            details.bottom().saturating_sub(canvas_height),
            details_canvas.width,
            canvas_height,
        );
        details.height = details.height.saturating_sub(height);
        actions.resize(start, Rect::default());
        action_canvases.resize(start, Rect::default());
        let rects = if model.workspace.is_some() && !compact_actions {
            workspace::action_rects(detail_actions_canvas, &model.actions[start..])
        } else {
            management_action_rects(
                detail_actions_canvas,
                &model.actions[start..],
                compact_actions,
            )
        };
        actions.extend(
            rects
                .iter()
                .map(|rect| rect.intersection(detail_actions_panel)),
        );
        action_canvases.extend(rects);
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
    let list_inner = pane_inner(list);
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
    details_canvas = text_area(pane_inner(details_canvas));
    let mut details_text = text_area(pane_inner(details)).intersection(details_canvas);
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
            usize::from(model.details_horizontal_scroll)
                .min(width.saturating_sub(usize::from(details_canvas.width))),
            true,
        ) {
            details_text.height = details_text.height.saturating_sub(1);
            details_canvas.height = details_canvas
                .height
                .saturating_sub(u16::from(width > usize::from(details_canvas.width)));
            scrollbars.push(bar);
        }
        if let Some(bar) = management_scrollbar(
            ManagementScrollTarget::Details,
            Rect::new(
                details_text.right(),
                details_text.y,
                u16::from(pane_inner(details).width > 0),
                details_text.height,
            ),
            lines.len(),
            usize::from(details_text.height),
            usize::from(model.details_scroll).min(
                lines
                    .len()
                    .saturating_sub(usize::from(details_canvas.height)),
            ),
            false,
        ) {
            scrollbars.push(bar);
        }
    } else if let Some(bar) = management_scrollbar(
        ManagementScrollTarget::Details,
        Rect::new(
            details_text.right(),
            details_text.y,
            u16::from(!details_text.is_empty()),
            details_text.height,
        ),
        management_wrapped_lines(&model.details, details_canvas.width).len(),
        usize::from(details_text.height),
        usize::from(model.details_scroll).min(
            management_wrapped_lines(&model.details, details_canvas.width)
                .len()
                .saturating_sub(usize::from(details_canvas.height)),
        ),
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
    let form_model = model.form.as_ref();
    let buttons = form_model.map_or_else(Vec::new, ManagementForm::actions);
    let cancel_index = buttons
        .iter()
        .position(|button| button.action == FormAction::Cancel);
    let close_label = if form_model.is_some_and(|form| form.choice.is_some()) {
        Some(i18n::tr!("management-form-close"))
    } else {
        cancel_index.map(|index| buttons[index].label())
    };
    let footer_indices = (0..buttons.len())
        .filter(|index| Some(*index) != cancel_index)
        .collect::<Vec<_>>();
    let footer = PageActionBar::new("").with_actions(
        footer_indices
            .iter()
            .map(|index| {
                let button = &buttons[*index];
                PageAction::new(button.id.as_str(), button.label()).disabled(button.disabled)
            })
            .collect(),
    );
    let form = form_model
        .map(|form| {
            form.choice.as_ref().map_or_else(
                || content_form(form_area, form, &footer, close_label.as_deref()),
                |choice| choice_form(form_area, form, choice),
            )
        })
        .unwrap_or_default();
    let body_minimum = if form_model.is_some_and(|form| !form.fields.is_empty()) {
        2
    } else {
        1
    };
    let is_choice = form_model.is_some_and(|form| form.choice.is_some());
    let footer_height = if is_choice {
        0
    } else {
        footer
            .height(DialogLayout::content_width(form.width))
            .min(form.height.saturating_sub(1 + body_minimum))
    };
    let dialog = DialogLayout::with_min_body(
        form,
        footer_height,
        close_label.as_deref(),
        if is_choice { 2 } else { body_minimum },
    );
    let form_inner = dialog.body;
    let message_len = form_model.map_or(0, |form| {
        if form.message.is_empty() {
            0
        } else {
            management_wrapped_lines(&form.message, form_inner.width.saturating_sub(1)).len()
        }
    });
    let has_fields = form_model.is_some_and(|form| !form.fields.is_empty());
    let available = form_inner.height;
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
    let mut form_buttons = Vec::new();
    if let Some(form_model) = form_model.filter(|_| !is_choice) {
        let selected = form_model
            .selected
            .checked_sub(form_model.fields.len())
            .and_then(|index| footer_indices.iter().position(|item| *item == index));
        form_buttons.extend(
            dialog
                .action_areas(&footer, selected)
                .into_iter()
                .map(|(index, area)| (footer_indices[index], area)),
        );
        if let Some(index) = cancel_index {
            form_buttons.push((index, dialog.close));
        }
        form_buttons.sort_unstable_by_key(|(index, _)| *index);
    }
    let submit = form_buttons
        .iter()
        .find(|(index, _)| buttons[*index].action == FormAction::Submit)
        .map_or(Rect::default(), |(_, area)| *area);
    let cancel = if cancel_index.is_some() {
        dialog.close
    } else {
        Rect::default()
    };
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
        let choice_inner = dialog.body;
        let content_width = management_choice_width(choice);
        let available = choice_inner.height;
        let mut vertical = choice.values.len() > usize::from(available);
        let mut horizontal =
            content_width > usize::from(choice_inner.width.saturating_sub(u16::from(vertical)));
        vertical |=
            choice.values.len() > usize::from(available.saturating_sub(u16::from(horizontal)));
        horizontal |=
            content_width > usize::from(choice_inner.width.saturating_sub(u16::from(vertical)));
        let row_height = available.saturating_sub(u16::from(horizontal));
        let rows = Rect::new(
            form.x,
            choice_inner.y,
            form.width.saturating_sub(u16::from(vertical)),
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
        choice_cancel = dialog.close;
        if let Some(bar) = management_scrollbar(
            ManagementScrollTarget::Choices,
            Rect::new(
                form.right().saturating_sub(1).max(form.x),
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
                choice_inner.x,
                rows.bottom(),
                choice_inner.width.saturating_sub(u16::from(vertical)),
                u16::from(horizontal && available > 0),
            ),
            content_width,
            usize::from(choice_inner.width.saturating_sub(u16::from(vertical))),
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
        details_canvas,
        detail_actions_panel,
        detail_actions_canvas,
        actions_panel,
        actions,
        action_canvases,
        action_start,
        action_previous,
        action_next,
        status,
        content,
        output_text,
        form,
        dialog,
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
    let list_focused = !model.filtering
        && !model.actions_focused
        && model.form.is_none()
        && model
            .workspace
            .as_ref()
            .is_none_or(|workspace| workspace.focus == crate::components::WorkspaceFocus::List);
    if !layout.compact_actions {
        let title_area = layout
            .workspace
            .as_ref()
            .map_or(main, |workspace| workspace.content);
        frame.render_widget(
            Paragraph::new(model.title.clone())
                .style(theme.body_style().add_modifier(Modifier::BOLD)),
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
    let mut filter = model
        .filter_edit
        .input(
            format!("management.filter.{}", model.scope_id),
            &model.filter,
        )
        .with_capture_name(i18n::tr!("management-search"))
        .with_cursor_symbol("_")
        .with_horizontal_scroll(true);
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
        let details_focused =
            model.workspace.as_ref().is_some_and(|workspace| {
                workspace.focus == crate::components::WorkspaceFocus::Details
            }) && model.form.is_none();
        Surface::new()
            .titled(i18n::tr!("management-touch-items"))
            .render_frame(frame, layout.list, context);
        Surface::new()
            .focused(details_focused)
            .titled(i18n::tr!("management-touch-details"))
            .raised(true)
            .render_frame(frame, layout.details, context);
        let details_style = theme.body_style().bg(context.theme.raised);
        Surface::new()
            .raised(true)
            .render_frame(frame, layout.actions_panel, context);
        Surface::new()
            .raised(true)
            .render_frame(frame, layout.detail_actions_panel, context);
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
            theme.title_style().bg(context.theme.surface),
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
                        focused: index == model.selected && list_focused,
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
            let row_area = Rect::new(
                layout.list_rows.x,
                layout.list_rows.y + (index - model.scroll) as u16,
                layout.list_rows.width,
                1,
            );
            let state = crate::components::ComponentState {
                selected: model
                    .selected_rows
                    .as_ref()
                    .map_or(index == model.selected, |rows| rows.contains(&index)),
                focused: index == model.selected && list_focused,
                hovered: context
                    .buttons
                    .as_ref()
                    .and_then(|buttons| buttons.hover_position)
                    .is_some_and(|(x, y)| crate::components::contains_point(row_area, x, y)),
                ..Default::default()
            };
            let style = crate::components::row_style(state, &theme, context.theme.surface);
            frame.buffer_mut().set_style(
                row_area,
                Style::default().bg(style.bg.unwrap_or(context.theme.surface)),
            );
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
            if model.loading {
                let area = layout.list_rows;
                frame.render_widget(
                    Paragraph::new(crate::BusyIndicator::from_motion(context.motion).label(
                        &i18n::tr!("management-loading"),
                        area.width,
                        context,
                    )),
                    area,
                );
            } else {
                let message = if model.empty_message.is_empty() {
                    i18n::tr!("management-empty")
                } else {
                    model.empty_message.clone()
                };
                frame.render_widget(
                    Paragraph::new(message)
                        .style(Style::default().fg(context.theme.muted))
                        .wrap(ratatui::widgets::Wrap { trim: true }),
                    layout.list_rows,
                );
            }
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
            .with_background(context.theme.surface)
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
            let rows = usize::from(offset(ManagementScrollTarget::Details))
                + usize::from(
                    layout
                        .details_text
                        .y
                        .saturating_sub(layout.details_canvas.y),
                );
            let columns = usize::from(offset(ManagementScrollTarget::DetailsColumns))
                + usize::from(
                    layout
                        .details_text
                        .x
                        .saturating_sub(layout.details_canvas.x),
                );
            frame.render_widget(
                Paragraph::new(
                    management_wrapped_lines(&model.details, u16::MAX)
                        .into_iter()
                        .skip(rows)
                        .take(usize::from(layout.details_text.height))
                        .map(|line| {
                            Line::from(management_clipped_line(
                                &line,
                                columns,
                                layout.details_text.width,
                            ))
                        })
                        .collect::<Vec<_>>(),
                )
                .style(details_style),
                layout.details_text,
            );
        } else {
            let lines = management_wrapped_lines(&model.details, layout.details_canvas.width);
            let offset = usize::from(model.details_scroll).min(
                lines
                    .len()
                    .saturating_sub(usize::from(layout.details_canvas.height)),
            );
            let rows = offset
                + usize::from(
                    layout
                        .details_text
                        .y
                        .saturating_sub(layout.details_canvas.y),
                );
            let columns = usize::from(
                layout
                    .details_text
                    .x
                    .saturating_sub(layout.details_canvas.x),
            );
            frame.render_widget(
                Paragraph::new(
                    lines
                        .into_iter()
                        .skip(rows)
                        .take(usize::from(layout.details_text.height))
                        .map(|line| {
                            Line::from(management_clipped_line(
                                &line,
                                columns,
                                layout.details_text.width,
                            ))
                        })
                        .collect::<Vec<_>>(),
                )
                .style(details_style),
                layout.details_text,
            );
        }
        for (offset, area) in layout.actions.iter().enumerate() {
            let index = layout.action_start + offset;
            if model
                .workspace
                .as_ref()
                .is_some_and(|workspace| workspace.navigation_actions.contains(&index))
                && layout.workspace.is_some()
            {
                continue;
            }
            if let Some((label, enabled)) = model.actions.get(index) {
                let canvas = layout.action_canvases.get(offset).copied().unwrap_or(*area);
                let hidden = usize::from(area.x.saturating_sub(canvas.x));
                let label = management_wrapped_lines(label, canvas.width)
                    .into_iter()
                    .skip(usize::from(area.y.saturating_sub(canvas.y)))
                    .map(|line| management_clipped_line(&line, hidden, area.width))
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut button = Button::new(management_action_id(model, index), label)
                    .with_bracketed_label(false);
                if model.action_tones.get(index) == Some(&crate::components::ComponentTone::Danger)
                {
                    button = button.with_emphasis(crate::components::ButtonEmphasis::Danger);
                } else if model
                    .workspace
                    .as_ref()
                    .is_some_and(|workspace| workspace.primary_actions.first() == Some(&index))
                {
                    button = button.with_emphasis(crate::components::ButtonEmphasis::Primary);
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
    let theme = DialogLayout::body_theme(context);
    let _scope = crate::automation::scope(
        &format!("management.form.{}", form.identity),
        "dialog",
        &form.title,
        layout.form,
    );
    let title = form
        .choice
        .as_ref()
        .filter(|choice| !choice.search.is_empty())
        .map_or_else(
            || form.title.clone(),
            |choice| format!("{} · {}", form.title, choice.search),
        );
    layout.dialog.render_frame(frame, &title, context);
    if let Some(choice) = &form.choice {
        let inset = layout.dialog.body.x - layout.form.x;
        let left = " ".repeat(usize::from(inset));
        let width = layout.dialog.body.width.saturating_sub(u16::from(
            layout
                .scrollbars
                .iter()
                .any(|bar| bar.target == ManagementScrollTarget::Choices),
        ));
        for (index, area) in &layout.choice_rows {
            let value = &choice.values[*index];
            let selected = *index == choice.selected;
            let text = management_clipped_line(value, choice.columns, width);
            let text = if selected && inset > 0 {
                format!("▌{}{text}", " ".repeat(usize::from(inset - 1)))
            } else {
                format!("{left}{text}")
            };
            let disabled = choice.disabled.get(*index).copied().unwrap_or(false);
            let mut button = Button::new(management_choice_id(form, *index), text)
                .with_bracketed_label(false)
                .with_capture_name(value)
                .with_capture_role("listitem")
                .with_capture_selected(selected)
                .with_background(if selected && !disabled {
                    context.theme.selected_row_background()
                } else {
                    context.theme.surface
                });
            button.set_disabled(disabled);
            button.set_focused(selected);
            button.render_borderless_frame(frame, *area, &theme);
        }
        Button::new(
            management_form_control_id(form, "choice.cancel"),
            i18n::tr!("management-form-close"),
        )
        .with_bracketed_label(false)
        .with_background(context.theme.surface)
        .render_borderless_frame(
            frame,
            layout.choice_cancel,
            &DialogLayout::close_theme(context),
        );
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
    let message = text_area(layout.message);
    let message_lines = management_wrapped_lines(&form.message, message.width);
    let offset = usize::from(form.message_scroll).min(
        message_lines
            .len()
            .saturating_sub(usize::from(message.height)),
    );
    frame.render_widget(
        Paragraph::new(
            message_lines
                .into_iter()
                .skip(offset)
                .map(Line::from)
                .collect::<Vec<_>>(),
        )
        .style(Style::default().fg(context.theme.muted)),
        message,
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
                    theme.title_style().fg(theme.accent_color)
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
                        .with_background(context.theme.surface)
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
                    .with_background(context.theme.surface)
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
        .with_background(context.theme.surface)
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
    let footer_theme = DialogLayout::footer_theme(context);
    let close_theme = DialogLayout::close_theme(context);
    for (index, area) in &layout.form_buttons {
        let action = &actions[*index];
        let is_close = action.action == FormAction::Cancel;
        let theme = if is_close {
            &close_theme
        } else {
            &footer_theme
        };
        let label_width = area.width.saturating_sub(u16::from(area.width > 4) * 2);
        let mut button = Button::new(
            management_form_control_id(form, &action.id),
            management_wrapped_lines(&action.label(), label_width).join("\n"),
        )
        .with_background(if is_close {
            theme.background
        } else {
            context.theme.control_background()
        })
        .with_label_alignment(ratatui::layout::HorizontalAlignment::Center)
        .with_bracketed_label(false);
        if action.action == FormAction::Submit {
            button = button.with_emphasis(ButtonEmphasis::Primary);
        }
        button.set_disabled(action.disabled);
        button.set_focused(form.selected == form.fields.len() + index);
        button.render_borderless_frame(frame, *area, theme);
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
