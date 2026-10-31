use super::*;
use crate::components::{
    PageAction, PageActionBar, Scrollbar, ScrollbarOrientation, contains_point, terminal_width,
};
use crate::{
    EditorHitTarget, EditorLayout, EditorScrollbarLayout, editor_layout, right_aligned_actions,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptHitTarget {
    Editor(EditorHitTarget),
    BreakpointLine(usize),
    Pane(ScriptPane),
    Tab(ScriptPane),
    Row(ScriptPane, usize),
    Action(ScriptAction),
    ExpressionInput,
    Scrollbar(ScriptPane),
}

#[derive(Debug, Clone)]
pub struct ScriptLayout {
    pub area: Rect,
    pub wide: bool,
    pub editor_area: Rect,
    pub editor: EditorLayout,
    pub panes: Vec<(ScriptPane, Rect)>,
    pub output_area: Rect,
    pub expression_input: Rect,
    pub status: Rect,
    pub actions: Vec<(ScriptAction, Rect)>,
    pub tabs: Vec<(ScriptPane, Rect)>,
    pub rows: Vec<(ScriptPane, usize, Rect)>,
    pub scrollbars: Vec<(ScriptPane, EditorScrollbarLayout)>,
    pub breakpoint_lines: Vec<(usize, Rect)>,
    pub expression_lines: Vec<String>,
}

impl ScriptLayout {
    pub fn pane(&self, pane: ScriptPane) -> Rect {
        self.panes
            .iter()
            .find(|(candidate, _)| *candidate == pane)
            .map_or(Rect::default(), |(_, area)| *area)
    }

    pub fn hit_test(&self, x: u16, y: u16) -> Option<ScriptHitTarget> {
        if self.editor.menu_popup.is_some()
            || self.editor.settings.is_some()
            || self.editor.quick_menu_popup.is_some()
        {
            return self.editor.hit_test(x, y).map(ScriptHitTarget::Editor);
        }
        for (action, area) in &self.actions {
            if contains_point(*area, x, y) {
                return Some(ScriptHitTarget::Action(*action));
            }
        }
        for (pane, area) in &self.tabs {
            if contains_point(*area, x, y) {
                return Some(ScriptHitTarget::Tab(*pane));
            }
        }
        if contains_point(self.expression_input, x, y) {
            return Some(ScriptHitTarget::ExpressionInput);
        }
        for (pane, scrollbar) in &self.scrollbars {
            if ScrollbarOrientation::VerticalRight.hit_test(scrollbar.track, (x, y)) {
                return Some(ScriptHitTarget::Scrollbar(*pane));
            }
        }
        for (line, area) in &self.breakpoint_lines {
            if contains_point(*area, x, y) {
                return Some(ScriptHitTarget::BreakpointLine(*line));
            }
        }
        for (pane, index, area) in &self.rows {
            if contains_point(*area, x, y) {
                return Some(ScriptHitTarget::Row(*pane, *index));
            }
        }
        if contains_point(self.editor_area, x, y) {
            return self
                .editor
                .hit_test(x, y)
                .map(ScriptHitTarget::Editor)
                .or(Some(ScriptHitTarget::Pane(ScriptPane::Source)));
        }
        self.panes
            .iter()
            .find(|(_, area)| contains_point(*area, x, y))
            .map(|(pane, _)| ScriptHitTarget::Pane(*pane))
    }
}

pub(super) fn inset(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(u16::from(area.width > 1)),
        area.y.saturating_add(u16::from(area.height > 1)),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

pub(super) fn primary_actions(status: ScriptStatus) -> &'static [ScriptAction] {
    match status {
        ScriptStatus::Idle | ScriptStatus::Finished | ScriptStatus::Failed => &[
            ScriptAction::Run,
            ScriptAction::Debug,
            ScriptAction::Settings,
        ],
        ScriptStatus::Paused => &[
            ScriptAction::Continue,
            ScriptAction::Next,
            ScriptAction::Step,
            ScriptAction::Finish,
            ScriptAction::Stop,
        ],
        _ => &[ScriptAction::Stop],
    }
}

pub(super) fn primary_action_bar(status: ScriptStatus, dirty: bool) -> PageActionBar {
    PageActionBar::new("script").with_actions(
        primary_actions(status)
            .iter()
            .map(|action| {
                let button = PageAction::new(format!("action.{action:?}"), action.label(dirty))
                    .disabled(status == ScriptStatus::Stopping);
                match action {
                    ScriptAction::Run | ScriptAction::Debug | ScriptAction::Continue => {
                        button.primary()
                    }
                    ScriptAction::Stop => button.danger(),
                    _ => button,
                }
            })
            .collect(),
    )
}

pub(super) fn workspace_areas(
    area: Rect,
    status: ScriptStatus,
    dirty: bool,
    detail: ScriptPane,
    detail_expansion: Option<u16>,
) -> (Rect, Vec<(ScriptPane, Rect)>, Rect, Rect, Rect) {
    let action_height = area
        .height
        .min(primary_action_bar(status, dirty).height(area.width));
    let actions = Rect::new(
        area.x,
        area.bottom().saturating_sub(action_height),
        area.width,
        action_height,
    );
    let status = Rect::new(
        area.x,
        actions.y.saturating_sub(1).max(area.y),
        area.width,
        u16::from(area.height > action_height),
    );
    let body = Rect::new(area.x, area.y, area.width, status.y.saturating_sub(area.y));
    let wide = area.width >= 140 && area.height >= 36;
    if wide {
        let tabs = Rect::new(body.x, body.y, body.width, u16::from(body.height > 0));
        let content = Rect::new(
            body.x,
            tabs.bottom(),
            body.width,
            body.height.saturating_sub(tabs.height),
        );
        let show_detail = !matches!(detail, ScriptPane::Source | ScriptPane::Output);
        let (left, right) = crate::expand_trailing_pane(
            content,
            Rect::new(content.right(), content.y, 0, content.height),
            if show_detail {
                detail_expansion.unwrap_or(1_000) / 3
            } else {
                0
            },
        );
        let upper = content.height * 2 / 3;
        let source = Rect::new(content.x, content.y, left.width, upper);
        let output = Rect::new(
            content.x,
            source.bottom(),
            left.width,
            content.height - upper,
        );
        let mut panes = vec![(ScriptPane::Output, output)];
        if show_detail && !right.is_empty() {
            panes.push((detail, right));
        }
        (source, panes, status, actions, tabs)
    } else {
        let source_height = (body.height * 3 / 5).max(body.height.min(5));
        let source = Rect::new(body.x, body.y, body.width, source_height);
        let tabs = Rect::new(
            body.x,
            source.bottom(),
            body.width,
            u16::from(source.bottom() < body.bottom()),
        );
        let detail_area = Rect::new(
            body.x,
            tabs.bottom(),
            body.width,
            body.bottom().saturating_sub(tabs.bottom()),
        );
        (
            source,
            vec![(
                if matches!(detail, ScriptPane::Source) {
                    ScriptPane::Output
                } else {
                    detail
                },
                detail_area,
            )],
            status,
            actions,
            tabs,
        )
    }
}

pub fn script_layout(area: Rect, model: &ScriptViewModel) -> ScriptLayout {
    let (editor_area, panes, status, action_area, tab_area) = workspace_areas(
        area,
        model.state.status,
        model.editor.dirty,
        model.detail,
        model.detail_expansion,
    );
    let wide = area.width >= 140 && area.height >= 36;
    let editor_model = model.decorated_editor();
    let mut editor = editor_layout(editor_area, &editor_model);
    editor.settings = editor_layout(area, &editor_model).settings;
    let mut actions = Vec::new();
    let primary = primary_actions(model.state.status);
    for action in primary_action_bar(model.state.status, model.editor.dirty)
        .layout(action_area)
        .actions
    {
        if !action.area.is_empty() {
            actions.push((primary[action.index], action.area));
        }
    }
    let mut tabs = Vec::new();
    let mut x = tab_area.x;
    for pane in ScriptPane::ALL.into_iter().skip(1) {
        let width = (terminal_width(&pane.title()) + 2)
            .min(usize::from(tab_area.right().saturating_sub(x))) as u16;
        if tab_area.height > 0 && width > 0 {
            tabs.push((pane, Rect::new(x, tab_area.y, width, 1)));
        }
        x = x.saturating_add(width);
    }
    let mut rows = Vec::new();
    let mut scrollbars = Vec::new();
    let mut output_area = Rect::default();
    let mut expression_input = Rect::default();
    let mut expression_lines = Vec::new();
    for (pane, rect) in &panes {
        let mut inner = inset(*rect);
        match pane {
            ScriptPane::Output => {
                output_area = Rect::new(
                    inner.x,
                    inner.y,
                    inner.width.saturating_sub(1),
                    inner.height,
                );
            }
            ScriptPane::Expression => {
                expression_lines = model
                    .state
                    .expression_log
                    .iter()
                    .flat_map(|entry| {
                        crate::management_wrapped_lines(entry, inner.width.saturating_sub(1))
                    })
                    .map(|line| crate::terminal_safe_text(&line).into_owned())
                    .collect();
                let buttons = [ScriptAction::Examine, ScriptAction::Evaluate];
                let lane = Rect::new(
                    inner.x,
                    inner.bottom().saturating_sub(1).max(inner.y),
                    inner.width,
                    u16::from(inner.height > 0),
                );
                let widths =
                    buttons.map(|action| (terminal_width(&action.label(false)) + 2) as u16);
                actions.extend(
                    buttons
                        .into_iter()
                        .zip(right_aligned_actions(lane, &widths)),
                );
                expression_input = Rect::new(
                    inner.x,
                    lane.y.saturating_sub(1).max(inner.y),
                    inner.width,
                    u16::from(inner.height > 1),
                );
                inner.height = expression_input.y.saturating_sub(inner.y);
            }
            ScriptPane::Variables => {
                let lane = Rect::new(
                    inner.x,
                    inner.bottom().saturating_sub(1).max(inner.y),
                    inner.width,
                    u16::from(inner.height > 0),
                );
                actions.push((ScriptAction::InspectVariable, lane));
                inner.height = inner.height.saturating_sub(1);
            }
            ScriptPane::Breakpoints => {
                let buttons = [
                    ScriptAction::LocateBreakpoint,
                    ScriptAction::ToggleBreakpoint,
                    ScriptAction::DeleteBreakpoint,
                ];
                let lane = Rect::new(
                    inner.x,
                    inner.bottom().saturating_sub(2).max(inner.y),
                    inner.width,
                    inner.height.min(2),
                );
                let widths =
                    buttons.map(|action| (terminal_width(&action.label(false)) + 2) as u16);
                actions.extend(
                    buttons
                        .into_iter()
                        .zip(right_aligned_actions(lane, &widths))
                        .filter(|(_, area)| !area.is_empty()),
                );
                inner.height = lane.y.saturating_sub(inner.y);
            }
            _ => {}
        }
        let content_len = match pane {
            ScriptPane::Variables => model.variable_rows().len(),
            ScriptPane::Stack => model.state.stack.len(),
            ScriptPane::Breakpoints => model.state.breakpoints.len(),
            ScriptPane::Expression => expression_lines.len(),
            ScriptPane::Output => model.output.as_ref().map_or(0, |output| {
                output.scrollback_rows + usize::from(output.rows)
            }),
            ScriptPane::Source => 0,
        };
        let requested_offset = if *pane == ScriptPane::Output {
            model.output.as_ref().map_or(0, |output| {
                output
                    .scrollback_rows
                    .saturating_sub(output.scrollback_offset)
            })
        } else {
            model.scrolls[pane.index()]
        };
        let offset = requested_offset.min(content_len.saturating_sub(usize::from(inner.height)));
        if *pane != ScriptPane::Output {
            for relative in 0..usize::from(inner.height).min(content_len.saturating_sub(offset)) {
                rows.push((
                    *pane,
                    offset + relative,
                    Rect::new(
                        inner.x,
                        inner.y + relative as u16,
                        inner.width.saturating_sub(1),
                        1,
                    ),
                ));
            }
        }
        let scroll = Scrollbar::new(content_len, usize::from(inner.height), offset);
        let (start, length) = scroll.thumb_range(inner);
        if length > 0 && inner.width > 0 {
            let track = Rect::new(inner.right() - 1, inner.y, 1, inner.height);
            scrollbars.push((
                *pane,
                EditorScrollbarLayout {
                    track,
                    thumb: Rect::new(track.x, track.y + start, 1, length),
                },
            ));
        }
    }
    let breakpoint_lines = if editor_model.vim_terminal.is_none() {
        editor
            .line_areas
            .iter()
            .filter_map(|line| {
                let wrapped = editor_model
                    .wrap_layout
                    .as_ref()
                    .and_then(|wrap| wrap.row(line.document_line));
                if wrapped.as_ref().is_some_and(|row| row.continuation) {
                    return None;
                }
                let logical = wrapped.map_or(line.document_line, |row| row.logical_line) + 1;
                Some((
                    logical,
                    Rect::new(editor.gutter.x, line.area.y, editor.gutter.width.min(2), 1),
                ))
            })
            .collect()
    } else {
        Vec::new()
    };
    ScriptLayout {
        area,
        wide,
        editor_area,
        editor,
        panes,
        output_area,
        expression_input,
        status,
        actions,
        tabs,
        rows,
        scrollbars,
        breakpoint_lines,
        expression_lines,
    }
}
