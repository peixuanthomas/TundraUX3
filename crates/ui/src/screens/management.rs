//! Shared presentation for five independently opened Linux management applications.
use crate::RenderContext;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagementFormField {
    pub label: String,
    pub value: String,
    pub secret: bool,
    pub choices: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagementForm {
    pub title: String,
    pub message: String,
    pub message_scroll: u16,
    pub fields: Vec<ManagementFormField>,
    pub selected: usize,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagementViewModel {
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub selected: usize,
    pub scroll: usize,
    pub details: String,
    pub details_scroll: u16,
    pub actions: Vec<(String, bool)>,
    pub selected_action: usize,
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

#[derive(Debug, Clone)]
pub struct ManagementLayout {
    pub filter: Rect,
    pub list: Rect,
    pub details: Rect,
    pub actions: Vec<Rect>,
    pub action_start: usize,
    pub status: Rect,
    pub content: Rect,
    pub form: Rect,
    pub message: Rect,
    pub fields: Vec<(usize, Rect)>,
    pub submit: Rect,
    pub cancel: Rect,
}
pub fn management_layout(main: Rect, model: &ManagementViewModel) -> ManagementLayout {
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(main);
    let body = Layout::default()
        .direction(if main.width >= 90 {
            Direction::Horizontal
        } else {
            Direction::Vertical
        })
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(parts[1]);
    let action_count = model
        .actions
        .len()
        .min(body[1].height.saturating_sub(3) as usize);
    let first_action = model
        .selected_action
        .saturating_sub(action_count.saturating_sub(1));
    let action_start = body[1].bottom().saturating_sub(action_count as u16);
    let actions = (0..action_count)
        .map(|i| Rect::new(body[1].x, action_start + i as u16, body[1].width, 1))
        .collect();
    let details = Rect::new(
        body[1].x,
        body[1].y,
        body[1].width,
        action_start.saturating_sub(body[1].y),
    );
    let width = main.width.saturating_sub(2).min(88);
    let height = main
        .height
        .saturating_sub(2)
        .min(model.form.as_ref().map_or(8, |f| {
            (f.fields.len() as u16 * 2)
                .saturating_add(f.message.lines().count().min(20) as u16)
                .saturating_add(7)
        }))
        .max(1);
    let form = Rect::new(
        main.x + main.width.saturating_sub(width) / 2,
        main.y + main.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let message_height = model.form.as_ref().map_or(2, |f| {
        f.message
            .lines()
            .count()
            .max(2)
            .min(form.height.saturating_sub(8).max(2) as usize) as u16
    });
    let message = Rect::new(
        form.x + 1,
        form.y + 1,
        form.width.saturating_sub(2),
        message_height,
    );
    let capacity = form.height.saturating_sub(message_height + 4) as usize / 2;
    let field_start = model
        .form
        .as_ref()
        .map_or(0, |f| f.selected.saturating_sub(capacity.saturating_sub(1)));
    let field_count = model.form.as_ref().map_or(0, |f| f.fields.len());
    let fields = (field_start..field_count)
        .take(capacity)
        .enumerate()
        .map(|(offset, index)| {
            (
                index,
                Rect::new(
                    form.x + 1,
                    message.bottom() + offset as u16 * 2,
                    form.width.saturating_sub(2),
                    2,
                ),
            )
        })
        .collect();
    let submit = Rect::new(
        form.x + 1,
        form.bottom().saturating_sub(2),
        form.width.saturating_sub(2) / 2,
        1,
    );
    let cancel = Rect::new(
        submit.right(),
        submit.y,
        form.width.saturating_sub(2).saturating_sub(submit.width),
        1,
    );
    ManagementLayout {
        filter: parts[0],
        list: body[0],
        details,
        actions,
        action_start: first_action,
        status: parts[2],
        content: parts[1],
        form,
        message,
        fields,
        submit,
        cancel,
    }
}

fn clean(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

pub fn render_management_content(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &ManagementViewModel,
    context: &RenderContext,
) {
    let theme = context.compatibility_theme();
    let layout = management_layout(main, model);
    let selected_style = theme
        .title_style()
        .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    frame.render_widget(
        Paragraph::new(format!(
            "{}  {}{}",
            model.title,
            i18n::tr!("management-search"),
            clean(&model.filter)
        ))
        .style(if model.filtering {
            selected_style
        } else {
            theme.title_style()
        }),
        layout.filter,
    );
    if model.terminal {
        if let Some(snapshot) = &model.terminal_snapshot {
            super::command_line::render_terminal_snapshot(
                frame,
                layout.content,
                snapshot,
                None,
                theme.accent_color,
            );
        } else {
            frame.render_widget(
                Paragraph::new(clean(&model.output))
                    .scroll((model.output_scroll, 0))
                    .style(theme.body_style())
                    .wrap(Wrap { trim: false }),
                layout.content,
            );
        }
    } else {
        let count = model.columns.len().max(1);
        let cell_width = layout.list.width.saturating_sub(2) as usize / count;
        let line = |cells: &[String]| -> String {
            cells
                .iter()
                .map(|cell| {
                    use unicode_width::UnicodeWidthChar;
                    let mut width = 0;
                    let mut value = String::new();
                    for character in clean(cell).chars() {
                        let next = character.width().unwrap_or(0);
                        if width + next > cell_width.saturating_sub(1) {
                            break;
                        }
                        width += next;
                        value.push(character);
                    }
                    value.push_str(&" ".repeat(cell_width.saturating_sub(width)));
                    value
                })
                .collect::<String>()
        };
        let mut lines = vec![Line::styled(line(&model.columns), theme.title_style())];
        for (index, row) in model
            .rows
            .iter()
            .enumerate()
            .skip(model.scroll)
            .take(layout.list.height.saturating_sub(3) as usize)
        {
            lines.push(Line::styled(
                line(row),
                if index == model.selected && !model.actions_focused {
                    selected_style
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
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme.border_style()),
            ),
            layout.list,
        );
        frame.render_widget(
            Paragraph::new(clean(&model.details))
                .scroll((model.details_scroll, 0))
                .style(theme.body_style())
                .wrap(Wrap { trim: false }),
            layout.details,
        );
        for (offset, (i, (label, enabled))) in model
            .actions
            .iter()
            .enumerate()
            .skip(layout.action_start)
            .take(layout.actions.len())
            .enumerate()
        {
            frame.render_widget(
                Paragraph::new(format!("{}. {}", i + 1, clean(label))).style(if !enabled {
                    theme.muted_style()
                } else if model.actions_focused && i == model.selected_action {
                    selected_style
                } else {
                    theme.title_style()
                }),
                layout.actions[offset],
            );
        }
    }
    let hint = if model.terminal {
        i18n::tr!("management-terminal-hint")
    } else {
        i18n::tr!("management-hint")
    };
    frame.render_widget(
        Paragraph::new(vec![Line::from(clean(&model.status)), Line::from(hint)])
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
    let selected = theme.title_style().add_modifier(Modifier::REVERSED);
    frame.render_widget(Clear, layout.form);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(clean(&form.title))
            .style(theme.surface_style())
            .border_style(theme.border_style()),
        layout.form,
    );
    frame.render_widget(
        Paragraph::new(clean(&form.message))
            .style(theme.muted_style())
            .wrap(Wrap { trim: false })
            .scroll((form.message_scroll, 0)),
        layout.message,
    );
    for (index, area) in &layout.fields {
        let field = &form.fields[*index];
        let value = if field.secret {
            "•".repeat(field.value.chars().count())
        } else {
            clean(&field.value)
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(clean(&field.label), theme.title_style()),
                    Span::styled(format!(" {}", field.choices), theme.muted_style()),
                ]),
                Line::from(value),
            ])
            .style(if form.selected == *index {
                selected
            } else {
                theme.body_style()
            }),
            *area,
        );
    }
    frame.render_widget(
        Paragraph::new(i18n::tr!("management-confirm")).style(
            if form.selected >= form.fields.len() {
                selected
            } else {
                theme.title_style()
            },
        ),
        layout.submit,
    );
    frame.render_widget(
        Paragraph::new(i18n::tr!("management-cancel")).style(theme.muted_style()),
        layout.cancel,
    );
    frame.render_widget(
        Paragraph::new(i18n::tr!("management-form-hint")).style(theme.muted_style()),
        Rect::new(
            layout.form.x + 1,
            layout.form.bottom().saturating_sub(3),
            layout.form.width.saturating_sub(2),
            1,
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn narrow_form_keeps_confirmation_in_bounds() {
        let model = ManagementViewModel {
            form: Some(ManagementForm {
                fields: (0..12).map(|_| ManagementFormField::default()).collect(),
                selected: 11,
                ..Default::default()
            }),
            ..Default::default()
        };
        let main = Rect::new(0, 0, 64, 14);
        let layout = management_layout(main, &model);
        assert!(layout.submit.bottom() <= main.bottom());
        assert!(layout.fields.iter().any(|(index, _)| *index == 11));
        assert!(
            layout
                .fields
                .iter()
                .all(|(_, area)| area.bottom() <= layout.submit.y)
        );
    }
    #[test]
    fn long_action_list_keeps_selected_action_visible() {
        let model = ManagementViewModel {
            actions: (0..20).map(|i| (format!("Action {i}"), true)).collect(),
            selected_action: 19,
            ..Default::default()
        };
        let layout = management_layout(Rect::new(0, 0, 80, 24), &model);
        assert!(layout.action_start > 0);
        assert_eq!(layout.action_start + layout.actions.len(), 20);
    }
    #[test]
    fn package_confirmation_has_room_for_dependency_list() {
        let model = ManagementViewModel {
            form: Some(ManagementForm {
                message: (0..50).map(|i| format!("package-{i}\n")).collect(),
                fields: vec![ManagementFormField::default()],
                ..Default::default()
            }),
            ..Default::default()
        };
        let layout = management_layout(Rect::new(0, 0, 120, 40), &model);
        assert!(layout.message.height >= 15);
        assert!(layout.fields[0].1.bottom() <= layout.submit.y);
    }
}
