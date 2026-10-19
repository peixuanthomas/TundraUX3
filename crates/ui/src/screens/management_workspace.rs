use super::*;
use crate::components::{SidebarWorkspaceLayout, WorkspaceFocus};

/// Action indices keep the shared form and task router independent of layout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagementWorkspace {
    pub navigation_actions: Vec<usize>,
    pub active_navigation: usize,
    pub primary_actions: Vec<usize>,
    pub advanced_toggle: usize,
    pub advanced_actions: Vec<usize>,
    pub advanced_open: bool,
    pub focus: WorkspaceFocus,
}

impl ManagementWorkspace {
    pub fn action_group(&self, focus: WorkspaceFocus) -> Vec<usize> {
        match focus {
            WorkspaceFocus::Navigation => self.navigation_actions.clone(),
            WorkspaceFocus::Actions => self.primary_actions.clone(),
            WorkspaceFocus::Advanced => std::iter::once(self.advanced_toggle)
                .chain(
                    self.advanced_actions
                        .iter()
                        .copied()
                        .filter(|_| self.advanced_open),
                )
                .collect(),
            _ => Vec::new(),
        }
    }
}

pub(super) fn action_rects(area: Rect, actions: &[(String, bool)]) -> Vec<Rect> {
    let mut y = area.y;
    actions
        .iter()
        .map(|(label, _)| {
            let height = management_wrapped_lines(label, area.width)
                .len()
                .min(usize::from(u16::MAX)) as u16;
            let rect = Rect::new(
                area.x,
                y.min(area.bottom()),
                area.width,
                height.min(area.bottom().saturating_sub(y)),
            );
            y = y.saturating_add(height);
            rect
        })
        .collect()
}

pub(super) fn layout(
    main: Rect,
    model: &ManagementViewModel,
    workspace: &ManagementWorkspace,
) -> ManagementLayout {
    let navigation = SidebarWorkspaceLayout::new(
        main,
        &workspace
            .navigation_actions
            .iter()
            .map(|index| model.actions[*index].0.clone())
            .collect::<Vec<_>>(),
    );
    let body = navigation.content;
    let footer_in_list = body.height >= 10 && !model.details_only;
    let footer_width = if footer_in_list {
        SidebarWorkspaceLayout::panes(body, false).0.width
    } else {
        body.width
    };
    let advanced_indices = std::iter::once(workspace.advanced_toggle)
        .chain(
            workspace
                .advanced_actions
                .iter()
                .copied()
                .filter(|_| workspace.advanced_open),
        )
        .collect::<Vec<_>>();
    let advanced_actions = advanced_indices
        .iter()
        .map(|index| model.actions[*index].clone())
        .collect::<Vec<_>>();
    let compact = body.height < 10;
    let measure_area = Rect::new(0, 0, footer_width, u16::MAX);
    let measure = if compact {
        management_action_rects(measure_area, &advanced_actions, false)
    } else {
        action_rects(measure_area, &advanced_actions)
    };
    let height = measure
        .iter()
        .map(|rect| rect.bottom())
        .max()
        .unwrap_or(0)
        .min(body.height.saturating_sub(2));
    let mut content_model = model.clone();
    content_model.actions = workspace
        .primary_actions
        .iter()
        .map(|index| model.actions[*index].clone())
        .collect();
    content_model.detail_action_start = Some(0);
    content_model.selected_action = workspace
        .primary_actions
        .iter()
        .position(|index| *index == model.selected_action)
        .unwrap_or(0);
    let mut layout = management_content_layout(
        Rect::new(
            body.x,
            body.y,
            body.width,
            body.height
                .saturating_sub(if footer_in_list { 0 } else { height }),
        ),
        &content_model,
        main,
        if footer_in_list { height } else { 0 },
    );
    let advanced = if footer_in_list {
        Rect::new(
            layout.list.x,
            layout.list.bottom(),
            layout.list.width,
            height,
        )
    } else {
        Rect::new(
            body.x,
            body.bottom().saturating_sub(height + 1).max(body.y),
            body.width,
            height,
        )
    };
    let mut actions = vec![Rect::default(); model.actions.len()];
    for (index, rect) in workspace.primary_actions.iter().zip(&layout.actions) {
        actions[*index] = *rect;
    }
    for (index, rect) in workspace
        .navigation_actions
        .iter()
        .zip(&navigation.navigation)
    {
        actions[*index] = *rect;
    }
    for (index, rect) in advanced_indices.iter().zip(if compact {
        management_action_rects(advanced, &advanced_actions, false)
    } else {
        action_rects(advanced, &advanced_actions)
    }) {
        actions[*index] = rect;
    }
    layout.actions = actions;
    layout.action_start = 0;
    layout.actions_panel = advanced;
    layout.status = Rect::new(
        body.x,
        body.bottom().saturating_sub(1).max(body.y),
        body.width,
        u16::from(body.height > 0),
    );
    layout.workspace = Some(navigation);
    layout
}
