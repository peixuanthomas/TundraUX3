use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Borders, Clear, Paragraph, Wrap};

use super::{
    ExplorerDialogViewModel, ExplorerEntryViewModel, ExplorerLayout, ExplorerOverlayControl,
    ExplorerOverlayLayout, ExplorerOverlayViewModel, ExplorerSearchViewModel, ExplorerSortColumn,
    ExplorerToolbarAction, ExplorerViewModel, explorer_layout,
};
use crate::screens::shell::{
    ShellChromeViewModel, ShellLayout, compute_shell_layout, fit_cell, render_compact_home,
    render_status, render_top,
};
use crate::{RuntimeAsciiAssets, TundraTheme};

const EXPLORER_HELP_LINE: &str = "Enter: open    Backspace: parent    N: folder    T: text file    R: rename    X/Delete: delete    C: copy    V: paste    /: search    H: hidden    Esc: back";

pub fn render_explorer(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &ExplorerViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_explorer_main(frame, main, model, theme);
            render_status(frame, status, chrome, theme);
            render_explorer_overlay(frame, main, model, theme);
        }
    }
}

fn render_explorer_main(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExplorerViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(
        theme
            .block()
            .title("Explorer")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        area,
    );

    let layout = explorer_layout(area, model);
    let Some(assets) = model.ascii_assets.as_ref() else {
        frame.render_widget(
            Paragraph::new("Explorer ASCII assets are unavailable")
                .style(theme.error_style())
                .alignment(Alignment::Center),
            layout.table,
        );
        return;
    };

    render_explorer_toolbar(frame, &layout, model, assets, theme);
    render_explorer_path_bar(frame, &layout, model, theme);
    render_explorer_sidebar(frame, &layout, model, assets, theme);
    render_explorer_table(frame, &layout, model, assets, theme);
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
            format!("[{icon}] {}", button.label)
        } else {
            format!("[{icon}]")
        };
        let style = if !button.enabled {
            theme.muted_style()
        } else if button.active {
            theme.title_style()
        } else {
            theme.body_style()
        };
        frame.render_widget(
            Paragraph::new(fit_cell(&text, usize::from(button_layout.area.width))).style(style),
            button_layout.area,
        );
    }
}

fn render_explorer_path_bar(
    frame: &mut Frame<'_>,
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    theme: &TundraTheme,
) {
    let address_style = if model.address_editing {
        theme.title_style()
    } else {
        theme.body_style()
    };
    frame.render_widget(
        Paragraph::new(fit_cell("[Edit]", usize::from(layout.address_button.width)))
            .style(address_style),
        layout.address_button,
    );
    if model.address_editing || model.breadcrumbs.is_empty() {
        let text = if model.address_editing {
            format!("> {}_", model.address_value)
        } else {
            model.address_value.clone()
        };
        frame.render_widget(
            Paragraph::new(fit_cell(&text, usize::from(layout.address_input.width)))
                .style(address_style),
            layout.address_input,
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
        let style = if crumb.drop_target {
            theme.title_style()
        } else if crumb.enabled {
            theme.body_style()
        } else {
            theme.muted_style()
        };
        frame.render_widget(
            Paragraph::new(fit_cell(
                &format!("{}{suffix}", crumb.label),
                usize::from(crumb_layout.area.width),
            ))
            .style(style),
            crumb_layout.area,
        );
    }

    let search_text = model
        .search
        .as_ref()
        .map_or_else(|| "Search: /".to_string(), explorer_search_line);
    frame.render_widget(
        Paragraph::new(fit_cell(&search_text, usize::from(layout.search.width))).style(
            if model.search.as_ref().is_some_and(|search| search.active) {
                theme.title_style()
            } else {
                theme.muted_style()
            },
        ),
        layout.search,
    );
}

fn render_explorer_sidebar(
    frame: &mut Frame<'_>,
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    assets: &RuntimeAsciiAssets,
    theme: &TundraTheme,
) {
    if let Some(header) = layout.sidebar_header {
        frame.render_widget(
            Paragraph::new("Quick access").style(theme.title_style()),
            header,
        );
    }
    for location_layout in &layout.quick_locations {
        let Some(location) = model.quick_locations.get(location_layout.index) else {
            continue;
        };
        let icon = explorer_icon_line(assets, &location.icon_key);
        let text = format!("{icon} {}", location.label);
        let style = if location.current || location.drop_target {
            theme.title_style()
        } else if location.enabled {
            theme.body_style()
        } else {
            theme.muted_style()
        };
        frame.render_widget(
            Paragraph::new(fit_cell(&text, usize::from(location_layout.area.width))).style(style),
            location_layout.area,
        );
    }
}

fn render_explorer_table(
    frame: &mut Frame<'_>,
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    assets: &RuntimeAsciiAssets,
    theme: &TundraTheme,
) {
    for column in &layout.columns {
        let mut label = column.column.label().to_string();
        if model.sort_column == column.column {
            label.push(' ');
            label.push_str(&explorer_icon_line(
                assets,
                super::explorer_sort_direction_icon_key(model.sort_direction),
            ));
        }
        frame.render_widget(
            Paragraph::new(explorer_table_cell(
                &label,
                column.area.width,
                column.column
                    != *layout
                        .columns
                        .last()
                        .map(|column| &column.column)
                        .unwrap_or(&column.column),
            ))
            .style(theme.title_style()),
            column.area,
        );
    }

    if model.entries.is_empty() && layout.table_body.height > 0 {
        frame.render_widget(
            Paragraph::new(if model.is_trash {
                "(Trash is empty)"
            } else {
                "(empty directory)"
            })
            .style(theme.muted_style())
            .alignment(Alignment::Center),
            layout.table_body,
        );
    }

    for row in &layout.rows {
        let Some(entry) = model.entries.get(row.index) else {
            continue;
        };
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
        let style = if cut {
            theme.muted_style()
        } else if focused || drop_target {
            theme.title_style()
        } else {
            theme.body_style()
        };
        for (column_index, column) in layout.columns.iter().enumerate() {
            let value = values
                .iter()
                .find_map(|(candidate, value)| (*candidate == column.column).then_some(value))
                .map(String::as_str)
                .unwrap_or("");
            let area = Rect::new(column.area.x, row.area.y, column.area.width, 1);
            frame.render_widget(
                Paragraph::new(explorer_table_cell(
                    value,
                    area.width,
                    column_index + 1 < layout.columns.len(),
                ))
                .style(style),
                area,
            );
        }
    }

    if let Some(scrollbar) = layout.scrollbar {
        for y in scrollbar.track.y..scrollbar.track.bottom() {
            frame.render_widget(
                Paragraph::new("|").style(theme.muted_style()),
                Rect::new(scrollbar.track.x, y, 1, 1),
            );
        }
        for y in scrollbar.thumb.y..scrollbar.thumb.bottom() {
            frame.render_widget(
                Paragraph::new("#").style(theme.title_style()),
                Rect::new(scrollbar.thumb.x, y, 1, 1),
            );
        }
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
            "Enter: open | Backspace: parent | /: search | Hidden files: {}",
            if model.show_hidden { "shown" } else { "hidden" }
        ),
        theme.muted_style(),
    ));
    lines.truncate(usize::from(layout.footer.height));
    frame.render_widget(Paragraph::new(lines), layout.footer);

    if let (Some(cancel), Some(operation)) = (layout.cancel_operation, model.operation.as_ref()) {
        let icon = explorer_icon_line(assets, "cancel");
        frame.render_widget(
            Paragraph::new(fit_cell(
                &format!("[{icon}] {}", operation.cancel_label),
                usize::from(cancel.width),
            ))
            .style(theme.title_style()),
            cancel,
        );
    }
}

fn render_explorer_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExplorerViewModel,
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
    frame.render_widget(
        theme
            .block()
            .title(title)
            .borders(Borders::ALL)
            .style(theme.body_style()),
        overlay_layout.area,
    );

    match model.overlay.as_ref() {
        Some(ExplorerOverlayViewModel::ContextMenu(menu)) => {
            for control in &overlay_layout.controls {
                let ExplorerOverlayControl::ContextItem(index) = control.control else {
                    continue;
                };
                let Some(item) = menu.items.get(index) else {
                    continue;
                };
                let shortcut = item
                    .shortcut
                    .as_ref()
                    .map(|shortcut| format!("  {shortcut}"))
                    .unwrap_or_default();
                let marker = if menu.selected_index == Some(index) {
                    "> "
                } else {
                    "  "
                };
                let text = format!("{marker}{}{shortcut}", item.label);
                let style = if !item.enabled {
                    theme.muted_style()
                } else if item.dangerous {
                    theme.error_style()
                } else if menu.selected_index == Some(index) {
                    theme.title_style()
                } else {
                    theme.body_style()
                };
                frame.render_widget(
                    Paragraph::new(fit_cell(&text, usize::from(control.area.width))).style(style),
                    control.area,
                );
            }
        }
        Some(ExplorerOverlayViewModel::Name(dialog)) => {
            render_explorer_name_dialog(frame, overlay_layout, dialog, theme);
        }
        Some(ExplorerOverlayViewModel::Options(options)) => {
            for control in &overlay_layout.controls {
                match control.control {
                    ExplorerOverlayControl::Option(index) => {
                        let Some(option) = options.options.get(index) else {
                            continue;
                        };
                        let marker = if option.focused {
                            ">"
                        } else if option.selected {
                            "*"
                        } else {
                            " "
                        };
                        let text = format!("{marker} {}: {}", option.label, option.value);
                        let style = if !option.enabled {
                            theme.muted_style()
                        } else if option.focused {
                            theme.title_style()
                        } else {
                            theme.body_style()
                        };
                        frame.render_widget(Paragraph::new(text).style(style), control.area);
                    }
                    ExplorerOverlayControl::OptionsClose => frame.render_widget(
                        Paragraph::new(format!("[{}]", options.close_label))
                            .style(theme.title_style())
                            .alignment(Alignment::Center),
                        control.area,
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
                    Paragraph::new(format!("{}: {}", property.label, property.value)),
                    area,
                );
            }
            if let Some(control) = overlay_layout.controls.first() {
                frame.render_widget(
                    Paragraph::new(format!("[{}]", properties.close_label))
                        .style(theme.title_style())
                        .alignment(Alignment::Center),
                    control.area,
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
    theme: &TundraTheme,
) {
    frame.render_widget(
        Paragraph::new(dialog.prompt.clone()),
        Rect::new(layout.content.x, layout.content.y, layout.content.width, 1),
    );
    for control in &layout.controls {
        match control.control {
            ExplorerOverlayControl::NameInput => frame.render_widget(
                Paragraph::new(format!("> {}_", dialog.value))
                    .block(theme.block().borders(Borders::ALL))
                    .style(theme.title_style()),
                Rect::new(
                    control.area.x,
                    control.area.y.saturating_sub(1),
                    control.area.width,
                    3.min(layout.content.height),
                ),
            ),
            ExplorerOverlayControl::Confirm => frame.render_widget(
                Paragraph::new(format!("[{}]", dialog.confirm_label))
                    .alignment(Alignment::Center)
                    .style(theme.title_style()),
                control.area,
            ),
            ExplorerOverlayControl::Cancel => frame.render_widget(
                Paragraph::new(format!("[{}]", dialog.cancel_label)).alignment(Alignment::Center),
                control.area,
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
            Paragraph::new(error.clone()).style(theme.error_style()),
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
    frame.render_widget(Paragraph::new(lines), layout.content);
    for control in &layout.controls {
        match control.control {
            ExplorerOverlayControl::ConflictChoice(choice) => {
                let selected = conflict.selected_choice == choice;
                frame.render_widget(
                    Paragraph::new(if selected {
                        format!("[{}]", choice.label())
                    } else {
                        choice.label().to_string()
                    })
                    .alignment(Alignment::Center)
                    .style(if selected {
                        theme.title_style()
                    } else {
                        theme.body_style()
                    }),
                    control.area,
                );
            }
            ExplorerOverlayControl::ApplyToRemaining => frame.render_widget(
                Paragraph::new(format!(
                    "[{}] Apply to remaining items",
                    if conflict.apply_to_remaining {
                        "x"
                    } else {
                        " "
                    }
                )),
                control.area,
            ),
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
            .alignment(Alignment::Center)
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
            frame.render_widget(
                Paragraph::new(label)
                    .alignment(Alignment::Center)
                    .style(theme.title_style()),
                control.area,
            );
        }
    }
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
    text.chars().count().max(1).div_ceil(width.max(1))
}

fn explorer_search_line(search: &ExplorerSearchViewModel) -> String {
    let query = if search.query.is_empty() {
        "<empty>"
    } else {
        search.query.as_str()
    };
    let mode = if search.active { "active" } else { "inactive" };
    match search.match_count {
        Some(1) => format!("Search: {query} (1 match, {mode})"),
        Some(count) => format!("Search: {query} ({count} matches, {mode})"),
        None => format!("Search: {query} ({mode})"),
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
