use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::{
    ExplorerDialogViewModel, ExplorerEntryViewModel, ExplorerLayout, ExplorerOverlayControl,
    ExplorerOverlayLayout, ExplorerOverlayViewModel, ExplorerSearchViewModel, ExplorerSortColumn,
    ExplorerToolbarAction, ExplorerViewModel, explorer_layout,
};
use crate::components::{
    Button, ComponentTone, DataTable, List, ListItem, Panel, Scrollbar, Surface, TextInput,
    terminal_width,
};
use crate::screens::shell::{
    ShellChromeViewModel, ShellLayout, compute_shell_layout, fit_cell, render_compact_home,
    render_status, render_top,
};
use crate::{RuntimeAsciiAssets, TundraTheme};

const EXPLORER_HELP_LINE: &str = "Enter: open    Backspace: parent    N: folder    T: text file    R: rename    X/Delete: delete    C: copy    V: paste    /: search    H: hidden    Tab/Shift+Tab: quick access    Esc: back";

pub fn render_explorer(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &ExplorerViewModel,
    theme: &TundraTheme,
) {
    let context = crate::RenderContext::from_theme(theme, Default::default(), Default::default());
    render_explorer_with_context(frame, area, chrome, model, &context);
}

pub fn render_explorer_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &ExplorerViewModel,
    context: &crate::RenderContext,
) {
    let theme = context.compatibility_theme();
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, &theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, &theme);
            render_explorer_main(frame, main, model, context, &theme);
            render_status(frame, status, chrome, &theme);
            render_explorer_overlay(frame, main, model, context, &theme);
        }
    }
}

fn render_explorer_main(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExplorerViewModel,
    context: &crate::RenderContext,
    theme: &TundraTheme,
) {
    Panel::new("Explorer").render_frame(frame, area, context);

    let layout = explorer_layout(area, model);
    let Some(assets) = model.ascii_assets.as_ref() else {
        frame.render_widget(
            Paragraph::new("Explorer ASCII assets are unavailable")
                .style(theme.error_style())
                .alignment(HorizontalAlignment::Center),
            layout.table,
        );
        return;
    };

    render_explorer_toolbar(frame, &layout, model, assets, theme);
    render_explorer_path_bar(frame, &layout, model, theme);
    render_explorer_sidebar(frame, &layout, model, assets, context, theme);
    render_explorer_table(frame, &layout, model, assets, context);
    render_explorer_footer(frame, &layout, model, assets, theme);
}

fn render_explorer_toolbar(
    frame: &mut Frame<'_>,
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    assets: &RuntimeAsciiAssets,
    theme: &TundraTheme,
) {
    for button_layout in &layout.toolbar_buttons {
        let Some(button) = model
            .toolbar
            .buttons
            .iter()
            .find(|button| button.action == button_layout.action)
        else {
            continue;
        };
        let icon_key = if button.action == ExplorerToolbarAction::Sort {
            super::explorer_sort_direction_icon_key(model.sort_direction)
        } else {
            button.icon_key.as_str()
        };
        let icon = explorer_icon_line(assets, icon_key);
        let text = if button_layout.show_label {
            format!("{icon} {}", button.label)
        } else {
            icon
        };
        render_explorer_button(
            frame,
            button_layout.area,
            format!("explorer.toolbar.{}", button.action.label()),
            fit_cell(&text, usize::from(button_layout.area.width)),
            button.active,
            button.enabled,
            theme,
        );
    }
}

fn render_explorer_path_bar(
    frame: &mut Frame<'_>,
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    theme: &TundraTheme,
) {
    render_explorer_button(
        frame,
        layout.address_button,
        "explorer.address.edit",
        fit_cell("[Edit]", usize::from(layout.address_button.width)),
        model.address_editing,
        true,
        theme,
    );
    if model.address_editing || model.breadcrumbs.is_empty() {
        let mut input = TextInput::new("explorer.address.input").with_cursor_symbol("_");
        input.set_value(&model.address_value);
        input.set_focused(model.address_editing);
        input.state.hovered = model.address_editing;
        input.render_borderless_frame_with_prefix(
            frame,
            layout.address_input,
            theme,
            if model.address_editing { "> " } else { "" },
        );
    }
    for crumb_layout in &layout.breadcrumbs {
        let Some(crumb) = model.breadcrumbs.get(crumb_layout.index) else {
            continue;
        };
        let suffix = if crumb_layout.index + 1 < model.breadcrumbs.len() {
            " > "
        } else {
            ""
        };
        render_explorer_button(
            frame,
            crumb_layout.area,
            format!("explorer.breadcrumb.{}", crumb.id),
            fit_cell(
                &format!("{}{suffix}", crumb.label),
                usize::from(crumb_layout.area.width),
            ),
            crumb.drop_target,
            crumb.enabled,
            theme,
        );
    }

    render_explorer_search(frame, layout.search, model.search.as_ref(), theme);
}

fn render_explorer_sidebar(
    frame: &mut Frame<'_>,
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    assets: &RuntimeAsciiAssets,
    context: &crate::RenderContext,
    theme: &TundraTheme,
) {
    if let Some(header) = layout.sidebar_header {
        frame.render_widget(
            Paragraph::new("Quick access")
                .alignment(HorizontalAlignment::Left)
                .style(theme.title_style()),
            header,
        );
    }
    let (Some(first), Some(last)) = (
        layout.quick_locations.first(),
        layout.quick_locations.last(),
    ) else {
        return;
    };
    let items = layout
        .quick_locations
        .iter()
        .map(|location_layout| {
            let Some(location) = model.quick_locations.get(location_layout.index) else {
                return ListItem::new("explorer.quick.missing", "").tone(ComponentTone::Muted);
            };
            let icon = explorer_icon_line(assets, &location.icon_key);
            let text = fit_cell(
                &format!("{icon} {}", location.label),
                usize::from(location_layout.area.width),
            );
            let tone = if location.current || location.drop_target {
                ComponentTone::Accent
            } else if location.enabled {
                ComponentTone::Default
            } else {
                ComponentTone::Muted
            };
            ListItem::new(format!("explorer.quick.{}", location_layout.index), text)
                .disabled(!location.enabled)
                .tone(tone)
        })
        .collect();
    let mut list = List::new("explorer.quick", items).with_highlight_symbol(None::<String>);
    list.set_selected(
        model
            .quick_locations
            .iter()
            .position(|location| location.current)
            .and_then(|index| index.checked_sub(layout.quick_location_visible_start)),
    );
    list.render_borderless_with_context(
        Rect::new(
            first.area.x,
            first.area.y,
            first.area.width,
            last.area.bottom().saturating_sub(first.area.y),
        ),
        frame.buffer_mut(),
        context,
    );
}

fn explorer_table_header(
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    assets: &RuntimeAsciiAssets,
) -> Vec<String> {
    let last_column = layout.columns.last().map(|column| column.column);
    let cells = layout.columns.iter().map(|column| {
        let mut label = column.column.label().to_string();
        if model.sort_column == column.column {
            label.push(' ');
            label.push_str(&explorer_icon_line(
                assets,
                super::explorer_sort_direction_icon_key(model.sort_direction),
            ));
        }
        explorer_table_cell(
            &label,
            column.area.width,
            Some(column.column) != last_column,
        )
    });
    cells.collect()
}

fn explorer_table_row(
    row: &super::ExplorerRowLayout,
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    assets: &RuntimeAsciiAssets,
) -> Option<(Vec<String>, ComponentTone)> {
    let entry = model.entries.get(row.index)?;
    let presentation = model.entry_presentation(row.index);
    let icon_key = presentation
        .map(|presentation| presentation.icon_key.as_str())
        .unwrap_or_else(|| legacy_explorer_icon_key(entry));
    let icon = explorer_icon_line(assets, icon_key);
    let selected = presentation
        .map(|presentation| presentation.selected)
        .unwrap_or(entry.selected);
    let focused = presentation
        .map(|presentation| presentation.focused)
        .unwrap_or(model.selected_index == Some(row.index));
    let cut = presentation.is_some_and(|presentation| presentation.cut);
    let drop_target = presentation.is_some_and(|presentation| presentation.drop_target);
    let marker = if selected { "* " } else { "  " };
    let name = format!("{marker}{icon} {}", entry.name);
    let values = [
        (ExplorerSortColumn::Name, name),
        (ExplorerSortColumn::Type, entry.kind.clone()),
        (
            ExplorerSortColumn::Size,
            entry.size.clone().unwrap_or_else(|| "--".to_string()),
        ),
        (
            ExplorerSortColumn::Modified,
            entry.modified.clone().unwrap_or_else(|| "--".to_string()),
        ),
    ];
    let tone = if cut {
        ComponentTone::Muted
    } else if focused || drop_target {
        ComponentTone::Accent
    } else {
        ComponentTone::Default
    };
    let cells = layout
        .columns
        .iter()
        .enumerate()
        .map(|(column_index, column)| {
            let value = values
                .iter()
                .find_map(|(candidate, value)| (*candidate == column.column).then_some(value))
                .map(String::as_str)
                .unwrap_or("");
            explorer_table_cell(
                value,
                column.area.width,
                column_index + 1 < layout.columns.len(),
            )
        });
    Some((cells.collect(), tone))
}

fn render_explorer_table(
    frame: &mut Frame<'_>,
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    assets: &RuntimeAsciiAssets,
    context: &crate::RenderContext,
) {
    if let (Some(first), Some(last)) = (layout.columns.first(), layout.columns.last()) {
        let widths = layout
            .columns
            .iter()
            .map(|column| column.area.width)
            .collect();
        let row_data = layout
            .rows
            .iter()
            .filter_map(|row| explorer_table_row(row, layout, model, assets))
            .collect::<Vec<_>>();
        let rows = row_data
            .iter()
            .map(|(row, _)| row.clone())
            .collect::<Vec<_>>();
        let tones = row_data.iter().map(|(_, tone)| *tone).collect();
        let mut table = DataTable::new(
            "explorer.table",
            explorer_table_header(layout, model, assets),
            rows,
        )
        .bordered(false)
        .with_column_widths(widths)
        .with_row_tones(tones);
        table.selected = model
            .selected_index
            .and_then(|selected| selected.checked_sub(layout.visible_start));
        table.render_frame(
            frame,
            Rect::new(
                first.area.x,
                layout.table_header.y,
                last.area.right().saturating_sub(first.area.x),
                layout
                    .table_header
                    .height
                    .saturating_add(layout.table_body.height),
            ),
            context,
        );
    }

    if model.entries.is_empty() && layout.table_body.height > 0 {
        frame.render_widget(
            Paragraph::new(if model.is_trash {
                "(Trash is empty)"
            } else {
                "(empty directory)"
            })
            .style(context.compatibility_theme().muted_style())
            .alignment(HorizontalAlignment::Center),
            layout.table_body,
        );
    }

    if let Some(scrollbar) = layout.scrollbar {
        Scrollbar::new(
            model.entries.len(),
            layout.visible_capacity,
            layout.visible_start,
        )
        .render_frame(frame, scrollbar.track, context);
    }
}

fn render_explorer_footer(
    frame: &mut Frame<'_>,
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    assets: &RuntimeAsciiAssets,
    theme: &TundraTheme,
) {
    if layout.footer.height == 0 {
        return;
    }
    let selected_names = selected_entry_names(model);
    let selected_summary = if selected_names.is_empty() {
        format!("{} selected", model.effective_selected_count())
    } else {
        format!("Selected: {}", selected_names.join(", "))
    };
    let mut lines = vec![Line::from(selected_summary)];
    if let Some(entry) = model.selected_entry() {
        lines.push(Line::from(format!(
            "Name: {} | Type: {} | Size: {}",
            entry.name,
            entry.kind,
            entry.size.as_deref().unwrap_or("-")
        )));
        lines.push(Line::from(format!(
            "Modified: {} | Attributes: {}",
            entry.modified.as_deref().unwrap_or("-"),
            format_attributes(&entry.attributes)
        )));
    } else {
        lines.push(Line::from("No entry selected"));
        lines.push(Line::from(""));
    }

    let feedback = if let Some(error) = &model.error {
        Line::styled(format!("Error: {error}"), theme.error_style())
    } else if let Some(operation) = &model.operation {
        let progress = operation.percent().map_or_else(
            || format!("{}: {} items", operation.label, operation.completed_items),
            |percent| format!("{}: {percent}%", operation.label),
        );
        Line::styled(progress, theme.title_style())
    } else if let Some(message) = &model.message {
        Line::styled(message.clone(), theme.muted_style())
    } else if model.listing_warning_count > 0 {
        Line::styled(
            format!("{} metadata warning(s)", model.listing_warning_count),
            theme.muted_style(),
        )
    } else {
        Line::from("")
    };
    lines.push(feedback);
    lines.push(Line::styled(
        format!(
            "Enter: open | Backspace: parent | /: search | Hidden files: {}{}",
            if model.show_hidden { "shown" } else { "hidden" },
            if layout.mode.shows_sidebar() {
                " | Tab/Shift+Tab: quick access"
            } else {
                ""
            }
        ),
        theme.muted_style(),
    ));
    lines.truncate(usize::from(layout.footer.height));
    frame.render_widget(
        Paragraph::new(lines).alignment(HorizontalAlignment::Left),
        layout.footer,
    );

    if let (Some(cancel), Some(operation)) = (layout.cancel_operation, model.operation.as_ref()) {
        let icon = explorer_icon_line(assets, "cancel");
        render_explorer_button(
            frame,
            cancel,
            "explorer.operation.cancel",
            fit_cell(
                &format!("{icon} {}", operation.cancel_label),
                usize::from(cancel.width),
            ),
            true,
            true,
            theme,
        );
    }
}

fn render_explorer_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExplorerViewModel,
    context: &crate::RenderContext,
    theme: &TundraTheme,
) {
    let layout = explorer_layout(area, model);
    let Some(overlay_layout) = layout.overlay.as_ref() else {
        return;
    };
    let title = match model.overlay.as_ref() {
        Some(ExplorerOverlayViewModel::ContextMenu(menu)) => menu.title.as_str(),
        Some(ExplorerOverlayViewModel::Name(dialog)) => dialog.title.as_str(),
        Some(ExplorerOverlayViewModel::Options(options)) => options.title.as_str(),
        Some(ExplorerOverlayViewModel::Conflict(conflict)) => conflict.title.as_str(),
        Some(ExplorerOverlayViewModel::Properties(properties)) => properties.title.as_str(),
        None => model
            .pending_dialog
            .as_ref()
            .map(|dialog| dialog.title.as_str())
            .unwrap_or("Explorer"),
    };
    frame.render_widget(Clear, overlay_layout.area);
    Panel::new(title).render_frame(frame, overlay_layout.area, context);

    match model.overlay.as_ref() {
        Some(ExplorerOverlayViewModel::ContextMenu(menu)) => {
            let rows = overlay_layout
                .controls
                .iter()
                .filter_map(|control| {
                    let ExplorerOverlayControl::ContextItem(index) = control.control else {
                        return None;
                    };
                    menu.items.get(index).map(|item| (control, index, item))
                })
                .collect::<Vec<_>>();
            if let (Some((first, _, _)), Some((last, _, _))) = (rows.first(), rows.last()) {
                let items = rows
                    .iter()
                    .map(|(control, index, item)| {
                        let shortcut = item
                            .shortcut
                            .as_ref()
                            .map(|shortcut| format!("  {shortcut}"))
                            .unwrap_or_default();
                        let text = fit_cell(
                            &format!("{}{shortcut}", item.label),
                            usize::from(control.area.width.saturating_sub(2)),
                        );
                        let tone = if !item.enabled {
                            ComponentTone::Muted
                        } else if item.dangerous {
                            ComponentTone::Danger
                        } else {
                            ComponentTone::Default
                        };
                        ListItem::new(format!("explorer.context.{index}"), text)
                            .disabled(!item.enabled)
                            .tone(tone)
                    })
                    .collect();
                let mut list = List::new("explorer.context", items);
                let selected = rows
                    .iter()
                    .position(|(_, index, _)| menu.selected_index == Some(*index));
                list.set_selected(selected);
                list.set_focused(true);
                list.render_borderless_with_context(
                    Rect::new(
                        first.area.x,
                        first.area.y,
                        first.area.width,
                        last.area.bottom().saturating_sub(first.area.y),
                    ),
                    frame.buffer_mut(),
                    context,
                );
            }
        }
        Some(ExplorerOverlayViewModel::Name(dialog)) => {
            render_explorer_name_dialog(frame, overlay_layout, dialog, context, theme);
        }
        Some(ExplorerOverlayViewModel::Options(options)) => {
            for control in &overlay_layout.controls {
                match control.control {
                    ExplorerOverlayControl::Option(index) => {
                        let Some(option) = options.options.get(index) else {
                            continue;
                        };
                        let text = fit_cell(
                            &format!("{}: {}", option.label, option.value),
                            usize::from(control.area.width),
                        );
                        let mut button =
                            Button::new(format!("explorer.options.{}", option.id), text);
                        button.set_focused(option.focused);
                        button.state.hovered = option.focused;
                        button.state.selected = option.selected;
                        button.set_disabled(!control.enabled);
                        button.render_borderless_frame(frame, control.area, theme);
                    }
                    ExplorerOverlayControl::OptionsClose => render_explorer_button(
                        frame,
                        control.area,
                        "explorer.options.close",
                        format!("[{}]", options.close_label),
                        true,
                        control.enabled,
                        theme,
                    ),
                    _ => {}
                }
            }
        }
        Some(ExplorerOverlayViewModel::Conflict(conflict)) => {
            render_explorer_conflict_dialog(frame, overlay_layout, conflict, theme);
        }
        Some(ExplorerOverlayViewModel::Properties(properties)) => {
            for (index, property) in properties
                .properties
                .iter()
                .take(usize::from(overlay_layout.content.height.saturating_sub(1)))
                .enumerate()
            {
                let area = Rect::new(
                    overlay_layout.content.x,
                    overlay_layout
                        .content
                        .y
                        .saturating_add(u16::try_from(index).unwrap_or(u16::MAX)),
                    overlay_layout.content.width,
                    1,
                );
                frame.render_widget(
                    Paragraph::new(format!("{}: {}", property.label, property.value))
                        .alignment(HorizontalAlignment::Left),
                    area,
                );
            }
            if let Some(control) = overlay_layout.controls.first() {
                render_explorer_button(
                    frame,
                    control.area,
                    "explorer.properties.close",
                    format!("[{}]", properties.close_label),
                    true,
                    control.enabled,
                    theme,
                );
            }
        }
        None => {
            if let Some(dialog) = &model.pending_dialog {
                render_legacy_explorer_dialog(frame, overlay_layout, dialog, theme);
            }
        }
    }
}

fn render_explorer_name_dialog(
    frame: &mut Frame<'_>,
    layout: &ExplorerOverlayLayout,
    dialog: &crate::ExplorerNameDialogViewModel,
    context: &crate::RenderContext,
    theme: &TundraTheme,
) {
    frame.render_widget(
        Paragraph::new(dialog.prompt.clone()).alignment(HorizontalAlignment::Left),
        Rect::new(layout.content.x, layout.content.y, layout.content.width, 1),
    );
    for control in &layout.controls {
        match control.control {
            ExplorerOverlayControl::NameInput => {
                let input_area = Rect::new(
                    control.area.x,
                    control.area.y.saturating_sub(1),
                    control.area.width,
                    3.min(layout.content.height),
                );
                Surface::new()
                    .bordered(true)
                    .render_frame(frame, input_area, context);
                let input_content = Rect::new(
                    input_area.x.saturating_add(1),
                    input_area.y.saturating_add(1),
                    input_area.width.saturating_sub(2),
                    input_area.height.saturating_sub(2),
                );

                let mut input = TextInput::new("explorer.name.input").with_cursor_symbol("_");
                input.set_value(&dialog.value);
                input.set_focused(true);
                input.state.hovered = true;
                input.render_borderless_frame_with_prefix(frame, input_content, theme, "> ");
            }
            ExplorerOverlayControl::Confirm => render_explorer_button(
                frame,
                control.area,
                "explorer.name.confirm",
                format!("[{}]", dialog.confirm_label),
                true,
                control.enabled,
                theme,
            ),
            ExplorerOverlayControl::Cancel => render_explorer_button(
                frame,
                control.area,
                "explorer.name.cancel",
                format!("[{}]", dialog.cancel_label),
                false,
                control.enabled,
                theme,
            ),
            _ => {}
        }
    }
    if let Some(error) = &dialog.error {
        let error_area = Rect::new(
            layout.content.x,
            layout.content.y.saturating_add(4),
            layout.content.width,
            u16::from(layout.content.height > 4),
        );
        frame.render_widget(
            Paragraph::new(error.clone())
                .alignment(HorizontalAlignment::Left)
                .style(theme.error_style()),
            error_area,
        );
    }
}

fn render_explorer_conflict_dialog(
    frame: &mut Frame<'_>,
    layout: &ExplorerOverlayLayout,
    conflict: &crate::ExplorerConflictViewModel,
    theme: &TundraTheme,
) {
    let lines = vec![
        Line::from(format!("Source: {}", conflict.source)),
        Line::from(format!("Destination: {}", conflict.destination)),
        Line::styled(
            "An item with this name already exists.",
            theme.muted_style(),
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines).alignment(HorizontalAlignment::Left),
        layout.content,
    );
    for control in &layout.controls {
        match control.control {
            ExplorerOverlayControl::ConflictChoice(choice) => {
                let selected = conflict.selected_choice == choice;
                render_explorer_button(
                    frame,
                    control.area,
                    format!("explorer.conflict.{}", choice.label()),
                    choice.label(),
                    selected,
                    control.enabled,
                    theme,
                );
            }
            ExplorerOverlayControl::ApplyToRemaining => {
                let label = fit_cell(
                    &format!(
                        "Apply to remaining items: {}",
                        if conflict.apply_to_remaining {
                            "On"
                        } else {
                            "Off"
                        }
                    ),
                    usize::from(control.area.width),
                );
                let mut button = Button::new("explorer.conflict.apply-to-remaining", label);
                button.state.selected = conflict.apply_to_remaining;
                button.set_disabled(!control.enabled);
                button.render_borderless_frame(frame, control.area, theme);
            }
            _ => {}
        }
    }
}

fn render_legacy_explorer_dialog(
    frame: &mut Frame<'_>,
    layout: &ExplorerOverlayLayout,
    dialog: &ExplorerDialogViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(
        Paragraph::new(dialog.message.clone())
            .alignment(HorizontalAlignment::Center)
            .wrap(Wrap { trim: true }),
        layout.content,
    );
    for control in &layout.controls {
        let label = match control.control {
            ExplorerOverlayControl::Confirm => Some(dialog.confirm_label.as_str()),
            ExplorerOverlayControl::Cancel => Some(dialog.cancel_label.as_str()),
            _ => None,
        };
        if let Some(label) = label {
            render_explorer_button(
                frame,
                control.area,
                match control.control {
                    ExplorerOverlayControl::Confirm => "explorer.dialog.confirm",
                    ExplorerOverlayControl::Cancel => "explorer.dialog.cancel",
                    _ => unreachable!("legacy Explorer dialog has only action controls"),
                },
                label,
                true,
                control.enabled,
                theme,
            );
        }
    }
}

fn render_explorer_button(
    frame: &mut Frame<'_>,
    area: Rect,
    id: impl Into<crate::components::ComponentId>,
    label: impl Into<String>,
    emphasized: bool,
    enabled: bool,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut button = Button::new(id, label);
    button.set_disabled(!enabled);
    button.set_focused(emphasized);
    button.state.hovered = emphasized;
    button.render_borderless_frame(frame, area, theme);
}

fn render_explorer_search(
    frame: &mut Frame<'_>,
    area: Rect,
    search: Option<&ExplorerSearchViewModel>,
    theme: &TundraTheme,
) {
    let Some(search) = search else {
        frame.render_widget(
            Paragraph::new(fit_cell("Search: /", usize::from(area.width)))
                .alignment(HorizontalAlignment::Left)
                .style(theme.muted_style()),
            area,
        );
        return;
    };

    let mut input = TextInput::new("explorer.search.input")
        .with_placeholder("<empty>")
        .with_placeholder_when_focused(true)
        .with_cursor_symbol("_");
    input.set_value(&search.query);
    input.set_focused(search.active);
    input.state.hovered = search.active;

    let mut input_theme = *theme;
    if !search.active {
        input_theme.foreground = theme.muted;
    }
    const PREFIX: &str = "Search: ";
    input.render_borderless_frame_with_prefix(frame, area, &input_theme, PREFIX);

    let visible_input_width = terminal_width(PREFIX).saturating_add(if search.query.is_empty() {
        terminal_width("<empty>")
    } else {
        terminal_width(&search.query).saturating_add(usize::from(search.active))
    });
    let visible_input_width = visible_input_width.min(usize::from(area.width));
    let suffix_x = area
        .x
        .saturating_add(u16::try_from(visible_input_width).unwrap_or(u16::MAX));
    let suffix_area = Rect::new(
        suffix_x,
        area.y,
        area.right().saturating_sub(suffix_x),
        area.height,
    );
    frame.render_widget(
        Paragraph::new(explorer_search_suffix(search))
            .alignment(HorizontalAlignment::Left)
            .style(if search.active {
                theme.title_style()
            } else {
                theme.muted_style()
            }),
        suffix_area,
    );
}

fn explorer_icon_line(assets: &RuntimeAsciiAssets, key: &str) -> String {
    assets
        .explorer_icon(key)
        .unwrap_or_else(|error| panic!("required Explorer icon {key} is unavailable: {error}"))
        .lines()
        .first()
        .cloned()
        .expect("validated Explorer icon must contain one line")
}

fn explorer_table_cell(text: &str, width: u16, separator: bool) -> String {
    let separator_width = if separator { 3 } else { 0 };
    let content_width = usize::from(width.saturating_sub(separator_width));
    let mut cell = fit_cell(text, content_width);
    if separator && width >= 3 {
        cell.push_str(" | ");
    } else {
        cell = fit_cell(&cell, usize::from(width));
    }
    cell
}

fn legacy_explorer_icon_key(entry: &ExplorerEntryViewModel) -> &'static str {
    let kind = entry.kind.to_ascii_lowercase();
    if kind.contains("directory") || kind.contains("folder") {
        return "folder";
    }
    if entry
        .attributes
        .iter()
        .any(|attribute| attribute.eq_ignore_ascii_case("link"))
    {
        return "link";
    }
    if kind.contains("executable") {
        return "executable";
    }
    let extension = entry
        .name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    match extension.as_deref() {
        Some("txt" | "md" | "rst" | "log") => "text",
        Some(
            "rs" | "c" | "h" | "cpp" | "hpp" | "go" | "py" | "rb" | "js" | "ts" | "tsx" | "jsx"
            | "java" | "kt" | "swift" | "toml" | "yaml" | "yml" | "json" | "xml" | "html" | "css"
            | "sh" | "ps1",
        ) => "code",
        Some("pdf" | "doc" | "docx" | "odt" | "rtf") => "document",
        Some("png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico") => "image",
        Some("mp3" | "wav" | "flac" | "m4a" | "ogg" | "aac") => "audio",
        Some("mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v") => "video",
        Some("zip" | "7z" | "rar" | "tar" | "gz" | "bz2" | "xz") => "archive",
        Some(
            "exe" | "com" | "scr" | "cpl" | "msi" | "msp" | "appx" | "bat" | "cmd" | "vbs" | "jar"
            | "app" | "pkg" | "run" | "appimage",
        ) => "executable",
        Some(_) => "file",
        None => "other",
    }
}

pub fn explorer_first_entry_content_line(model: &ExplorerViewModel, content_width: u16) -> usize {
    let width = usize::from(content_width.max(1));
    let mut line = 0usize;
    line += wrapped_line_count(&format!("Path: {}", model.current_path), width);
    line += wrapped_line_count(
        &format!(
            "Hidden files: {}",
            if model.show_hidden { "shown" } else { "hidden" }
        ),
        width,
    );
    if let Some(search) = &model.search {
        line += wrapped_line_count(&explorer_search_line(search), width);
    }
    line += wrapped_line_count(EXPLORER_HELP_LINE, width);
    line += 1;
    line += wrapped_line_count("Entries", width);
    line
}

fn wrapped_line_count(text: &str, width: usize) -> usize {
    terminal_width(text).max(1).div_ceil(width.max(1))
}

fn explorer_search_line(search: &ExplorerSearchViewModel) -> String {
    let query = if search.query.is_empty() {
        "<empty>"
    } else {
        search.query.as_str()
    };
    format!("Search: {query}{}", explorer_search_suffix(search))
}

fn explorer_search_suffix(search: &ExplorerSearchViewModel) -> String {
    let mode = if search.active { "active" } else { "inactive" };
    match search.match_count {
        Some(1) => format!(" (1 match, {mode})"),
        Some(count) => format!(" ({count} matches, {mode})"),
        None => format!(" ({mode})"),
    }
}

fn selected_entry_names(model: &ExplorerViewModel) -> Vec<String> {
    model
        .entries
        .iter()
        .enumerate()
        .filter(|(index, entry)| {
            model
                .entry_presentation(*index)
                .map(|presentation| presentation.selected)
                .unwrap_or(entry.selected)
        })
        .map(|(_, entry)| entry.name.clone())
        .collect()
}

fn format_attributes(attributes: &[String]) -> String {
    if attributes.is_empty() {
        "none".to_string()
    } else {
        attributes.join(", ")
    }
}
