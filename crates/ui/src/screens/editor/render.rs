use super::document::*;
use super::*;

/// Render only the editor's main area. Shell chrome remains the caller's responsibility.
pub fn render_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &EditorViewModel,
    theme: &TundraTheme,
) -> EditorLayout {
    let layout = editor_layout(area, model);
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(theme.body_style()), area);
    render_menu_bar(frame, &layout, model, theme);
    render_toolbar(frame, &layout, model, theme);
    render_canvas(frame, &layout, model, theme);
    render_status_bar(frame, &layout, model, theme);
    // Popups overlap the editor chrome and canvas. Settings is modal, so it
    // is painted last and receives the highest hit-test priority.
    render_menu_popup(frame, &layout, model, theme);
    render_quick_menu(frame, &layout, theme);
    render_settings(frame, &layout, model, theme);
    layout
}

fn render_menu_bar(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
) {
    if layout.menu_bar.is_empty() {
        return;
    }
    frame.render_widget(
        Block::default().style(Style::default().fg(theme.foreground).bg(Color::DarkGray)),
        layout.menu_bar,
    );
    for item in &layout.menus {
        let active = model.open_menu == Some(item.menu)
            || (item.menu == EditorMenu::Settings && model.settings.is_some());
        let style = if active {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else if model.focus == EditorFocus::MenuBar {
            Style::default().fg(theme.accent_color).bg(Color::DarkGray)
        } else {
            Style::default().fg(theme.foreground).bg(Color::DarkGray)
        };
        frame.render_widget(
            Paragraph::new(format!(" {} ", menu_label(item.menu))).style(style),
            item.area,
        );
    }
    for item in &layout.modes {
        let active = item.mode == model.mode;
        let style = if active {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted).bg(Color::DarkGray)
        };
        frame.render_widget(
            Paragraph::new(format!(" {} ", mode_label(item.mode))).style(style),
            item.area,
        );
    }
}

fn render_menu_popup(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
) {
    let Some(area) = layout.menu_popup else {
        return;
    };
    frame.render_widget(Clear, area);
    frame.render_widget(
        theme
            .block()
            .borders(Borders::ALL)
            .style(Style::default().fg(theme.foreground).bg(theme.background)),
        area,
    );
    for item in &layout.menu_items {
        let active_mode = matches!(item.action, EditorMenuAction::Mode(mode) if mode == model.mode);
        let style = if !item.enabled {
            theme.muted_style()
        } else if active_mode {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.body_style()
        };
        frame.render_widget(
            Paragraph::new(format!(" {}", menu_action_label(item.action))).style(style),
            item.area,
        );
    }
}

fn render_quick_menu(frame: &mut Frame<'_>, layout: &EditorLayout, theme: &TundraTheme) {
    let Some(area) = layout.quick_menu_popup else {
        return;
    };
    frame.render_widget(Clear, area);
    frame.render_widget(
        theme
            .block()
            .borders(Borders::ALL)
            .style(Style::default().fg(theme.foreground).bg(theme.background)),
        area,
    );
    for item in &layout.quick_menu_items {
        let style = if !item.enabled {
            theme.muted_style()
        } else {
            match item.action {
                EditorQuickAction::Bold => theme.body_style().add_modifier(Modifier::BOLD),
                EditorQuickAction::Italic => theme.body_style().add_modifier(Modifier::ITALIC),
                EditorQuickAction::Paragraph => theme.body_style(),
                EditorQuickAction::Heading(level) => {
                    let mut style = Style::default()
                        .fg(theme.accent_color)
                        .bg(theme.background)
                        .add_modifier(Modifier::BOLD);
                    if level == 1 {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    } else if level >= 3 {
                        style = style.add_modifier(Modifier::ITALIC);
                    }
                    style
                }
            }
        };
        frame.render_widget(
            Paragraph::new(format!(" {} ", quick_action_label(item.action))).style(style),
            item.area,
        );
    }
}

fn render_settings(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
) {
    let (Some(settings_layout), Some(settings)) = (&layout.settings, model.settings.as_ref())
    else {
        return;
    };
    frame.render_widget(Clear, settings_layout.dialog);
    frame.render_widget(
        theme
            .block()
            .borders(Borders::ALL)
            .title(" Editor Settings ")
            .style(Style::default().fg(theme.foreground).bg(theme.background)),
        settings_layout.dialog,
    );
    let description = Rect::new(
        settings_layout.dialog.x.saturating_add(2),
        settings_layout.dialog.y.saturating_add(1),
        settings_layout.dialog.width.saturating_sub(4),
        1,
    );
    let description_text = if settings.editable {
        "Hold one direction to accelerate with a quadratic curve."
    } else {
        "Read-only: administrator permission is required to change these settings."
    };
    frame.render_widget(
        Paragraph::new(description_text).style(theme.muted_style()),
        description,
    );

    for field in &settings_layout.fields {
        let selected = field.field == settings.selected;
        let locked = !settings.editable && field.field != EditorSettingsField::Cancel;
        let style = if locked {
            theme.muted_style()
        } else if selected {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.body_style()
        };
        frame.render_widget(Block::default().style(style), field.area);
        let label = match field.field {
            EditorSettingsField::Enabled => " Cursor acceleration",
            EditorSettingsField::ActivationDelay => " Start delay",
            EditorSettingsField::RampDuration => " Ramp to maximum",
            EditorSettingsField::HorizontalMaxStep => " Horizontal maximum",
            EditorSettingsField::VerticalMaxStep => " Vertical maximum",
            EditorSettingsField::RestoreDefaults
            | EditorSettingsField::Save
            | EditorSettingsField::Cancel => "",
        };
        if !label.is_empty() {
            frame.render_widget(Paragraph::new(label).style(style), field.area);
        }
    }

    for control in &settings_layout.controls {
        let field = settings_control_field(control.control);
        let selected = field.is_some_and(|field| field == settings.selected);
        let locked =
            !settings.editable && !matches!(control.control, EditorSettingsControl::Cancel);
        let style = if locked {
            theme.muted_style()
        } else if selected {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.body_style()
        };
        let label = match control.control {
            EditorSettingsControl::ToggleEnabled => {
                if settings.enabled {
                    "[ ON ]"
                } else {
                    "[OFF ]"
                }
            }
            EditorSettingsControl::Decrease(_) => "[-]",
            EditorSettingsControl::Increase(_) => "[+]",
            EditorSettingsControl::RestoreDefaults => "[ Restore defaults ]",
            EditorSettingsControl::Save => "[ Save ]",
            EditorSettingsControl::Cancel => "[ Cancel ]",
        };
        frame.render_widget(Paragraph::new(label).style(style), control.area);
    }

    for (field, value) in [
        (
            EditorSettingsField::ActivationDelay,
            format!("{} ms", settings.activation_delay_ms),
        ),
        (
            EditorSettingsField::RampDuration,
            format!("{} ms", settings.ramp_duration_ms),
        ),
        (
            EditorSettingsField::HorizontalMaxStep,
            format!("{} cells", settings.horizontal_max_step),
        ),
        (
            EditorSettingsField::VerticalMaxStep,
            format!("{} lines", settings.vertical_max_step),
        ),
    ] {
        let Some(decrease) = settings_layout
            .controls
            .iter()
            .find(|control| control.control == EditorSettingsControl::Decrease(field))
        else {
            continue;
        };
        let Some(increase) = settings_layout
            .controls
            .iter()
            .find(|control| control.control == EditorSettingsControl::Increase(field))
        else {
            continue;
        };
        let value_area = Rect::new(
            decrease.area.right(),
            decrease.area.y,
            increase.area.x.saturating_sub(decrease.area.right()),
            1,
        );
        let width = usize::from(value_area.width);
        let style = if !settings.editable {
            theme.muted_style()
        } else if settings.selected == field {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.body_style()
        };
        frame.render_widget(
            Paragraph::new(format!("{value:^width$}")).style(style),
            value_area,
        );
    }

    let help = Rect::new(
        settings_layout.dialog.x.saturating_add(2),
        settings_layout.dialog.bottom().saturating_sub(4),
        settings_layout.dialog.width.saturating_sub(4),
        1,
    );
    frame.render_widget(
        Paragraph::new("Tab select · Left/Right adjust · Enter activate · Esc cancel")
            .style(theme.muted_style()),
        help,
    );
}

fn settings_control_field(control: EditorSettingsControl) -> Option<EditorSettingsField> {
    match control {
        EditorSettingsControl::ToggleEnabled => Some(EditorSettingsField::Enabled),
        EditorSettingsControl::Decrease(field) | EditorSettingsControl::Increase(field) => {
            Some(field)
        }
        EditorSettingsControl::RestoreDefaults => Some(EditorSettingsField::RestoreDefaults),
        EditorSettingsControl::Save => Some(EditorSettingsField::Save),
        EditorSettingsControl::Cancel => Some(EditorSettingsField::Cancel),
    }
}

fn render_toolbar(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
) {
    if layout.toolbar.is_empty() {
        return;
    }
    frame.render_widget(
        Block::default().style(Style::default().fg(theme.foreground).bg(theme.background)),
        layout.toolbar,
    );
    for item in &layout.toolbar_items {
        let active = model.toolbar.is_active(item.action);
        let selected = model.selected_toolbar_action == Some(item.action)
            && model.focus == EditorFocus::Toolbar;
        let style = if !item.enabled {
            theme.muted_style()
        } else if active || selected {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.body_style()
        };
        frame.render_widget(
            Paragraph::new(toolbar_label(item.action)).style(style),
            item.area,
        );
    }
}

fn render_canvas(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
) {
    if layout.canvas_panel.is_empty() {
        return;
    }
    if layout.canvas_framed {
        let mut title = model.file_name.clone();
        if model.dirty {
            title.push_str(" *");
        }
        if model.read_only {
            title.push_str(" [read-only]");
        }
        if model
            .read_window
            .is_some_and(|window| window.start_byte > 0)
        {
            title.push_str(" [tail]");
        }
        let title = terminal_safe_text(&title).into_owned();
        frame.render_widget(
            theme
                .block()
                .borders(Borders::ALL)
                .title(title)
                .style(theme.body_style()),
            layout.canvas_panel,
        );
    } else {
        frame.render_widget(
            Block::default().style(theme.body_style()),
            layout.canvas_panel,
        );
    }
    if layout.canvas.is_empty() {
        return;
    }

    for line_layout in &layout.line_areas {
        let Some(display_line) = layout.prepared_lines.get(
            line_layout
                .document_line
                .saturating_sub(layout.prepared_start),
        ) else {
            continue;
        };
        let line = styled_line(
            display_line,
            line_layout.document_line,
            layout,
            model,
            theme,
            usize::from(layout.canvas.width),
        );
        frame.render_widget(
            Paragraph::new(line).style(theme.body_style()),
            line_layout.area,
        );
    }

    if let Some(scrollbar) = layout.vertical_scrollbar {
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

    if let Some(scrollbar) = layout.horizontal_scrollbar {
        for x in scrollbar.track.x..scrollbar.track.right() {
            frame.render_widget(
                Paragraph::new("-").style(theme.muted_style()),
                Rect::new(x, scrollbar.track.y, 1, 1),
            );
        }
        for x in scrollbar.thumb.x..scrollbar.thumb.right() {
            frame.render_widget(
                Paragraph::new("#").style(theme.title_style()),
                Rect::new(x, scrollbar.thumb.y, 1, 1),
            );
        }
    }

    if model.focus == EditorFocus::Canvas
        && let Some(cursor) = effective_cursor(layout, model)
        && cursor.line >= layout.visible_start
        && cursor.line < layout.visible_start.saturating_add(layout.visible_capacity)
    {
        let horizontal_scroll = layout
            .prepared_lines
            .get(cursor.line.saturating_sub(layout.prepared_start))
            .filter(|line| model.mode == EditorMode::Source || line.no_wrap)
            .map_or(0, |_| layout.horizontal_scroll);
        if cursor.column >= horizontal_scroll
            && cursor.column.saturating_sub(horizontal_scroll) < usize::from(layout.canvas.width)
        {
            let cursor_column = cursor.column - horizontal_scroll;
            frame.set_cursor_position((
                layout.canvas.x.saturating_add(to_u16(cursor_column)),
                layout
                    .canvas
                    .y
                    .saturating_add(to_u16(cursor.line.saturating_sub(layout.visible_start))),
            ));
        }
    }
}

fn render_status_bar(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
) {
    if layout.status_bar.is_empty() {
        return;
    }
    let cursor = effective_cursor(layout, model).unwrap_or_default();
    let image = match model.image_protocol {
        EditorImageProtocolStatus::Detecting => "image:detecting",
        EditorImageProtocolStatus::Unsupported => "image:fallback",
        EditorImageProtocolStatus::Available => "image:terminal",
    };
    let mode = mode_label(model.mode);
    let read_window = model.read_window.map(|window| {
        if window.total_bytes == 0 {
            "Bytes 0 of 0".to_string()
        } else {
            let start = window.start_byte.min(window.total_bytes.saturating_sub(1));
            format!(
                "Bytes {}-{} of {}",
                start.saturating_add(1),
                window.total_bytes,
                window.total_bytes
            )
        }
    });
    let left = model
        .status_message
        .as_deref()
        .unwrap_or(if model.read_only {
            "Read only"
        } else {
            "Ready"
        });
    let left = if model.reload_available {
        format!("{left} · R Reload")
    } else {
        left.to_string()
    };
    let right = format!(
        "{}  Ln {}, Col {}  {} words  {}/{}  {}{}",
        mode,
        cursor.line.saturating_add(1),
        cursor.column.saturating_add(1),
        model.word_count,
        model.encoding,
        model.line_ending,
        image,
        read_window.map_or_else(String::new, |window| format!("  {window}")),
    );
    let available = usize::from(layout.status_bar.width);
    let text = if available == 0 {
        String::new()
    } else if left.chars().count() + right.chars().count() + 2 <= available {
        format!(
            "{}{}{}",
            left,
            " ".repeat(available - left.chars().count() - right.chars().count()),
            right
        )
    } else {
        fit_text(&format!("{} | {}", left, right), available)
    };
    let text = terminal_safe_text(&text).into_owned();
    let style = if model.focus == EditorFocus::StatusBar {
        Style::default().fg(theme.background).bg(theme.accent_color)
    } else {
        Style::default().fg(theme.foreground).bg(Color::DarkGray)
    };
    frame.render_widget(Paragraph::new(text).style(style), layout.status_bar);
}

fn styled_line(
    line: &DisplayLine,
    document_line: usize,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
    width: usize,
) -> Line<'static> {
    let scroll = if model.mode == EditorMode::Source || line.no_wrap {
        layout.horizontal_scroll
    } else {
        0
    };
    let mut output = Vec::new();
    let mut column = line.column_start;
    let mut visible_width = 0usize;
    for run in &line.runs {
        let base_style = span_style(&run.style, theme);
        let run_text = run.text.resolve(model.source.as_deref());
        let run_span = Span::raw(run_text);
        let mut relative_byte = 0usize;
        let mut relative_grapheme = 0usize;
        for grapheme in run_span.styled_graphemes(Style::default()) {
            let grapheme_start = relative_byte;
            relative_byte = relative_byte.saturating_add(grapheme.symbol.len());
            let grapheme_source = display_source_for_segment(
                run.source,
                run_text.len(),
                grapheme_start,
                relative_byte,
            );
            let grapheme_rich =
                display_rich_for_grapheme(run.rich, relative_grapheme, relative_grapheme + 1);
            relative_grapheme = relative_grapheme.saturating_add(1);
            let safe = terminal_safe_text(grapheme.symbol).into_owned();
            let cell_width = Span::raw(safe.as_str()).width().max(1);
            let start = column;
            column = column.saturating_add(cell_width);
            if column <= scroll {
                continue;
            }
            if visible_width.saturating_add(cell_width) > width {
                break;
            }
            let position = EditorTextPosition::new(document_line, start);
            let selected = match model.mode {
                EditorMode::Rich => model.rich_selection.map_or_else(
                    || {
                        if layout.rich_line_maps.is_empty() {
                            model
                                .selection_offsets
                                .is_some_and(|selection| source_run_is_selected(run, selection))
                        } else {
                            model
                                .selection
                                .is_some_and(|selection| selection.contains(position))
                        }
                    },
                    |selection| {
                        rich_mapping_is_selected(grapheme_rich, layout, selection, position)
                    },
                ),
                EditorMode::Source => model.selection_offsets.map_or_else(
                    || {
                        model
                            .selection
                            .is_some_and(|selection| selection.contains(position))
                    },
                    |selection| source_mapping_is_selected(grapheme_source, selection),
                ),
            };
            let style = if selected {
                base_style
                    .fg(theme.background)
                    .bg(theme.accent_color)
                    .add_modifier(Modifier::BOLD)
            } else {
                base_style
            };
            output.push(Span::styled(safe, style));
            visible_width = visible_width.saturating_add(cell_width);
        }
        if visible_width >= width {
            break;
        }
    }
    Line::from(output)
}

fn effective_cursor(layout: &EditorLayout, model: &EditorViewModel) -> Option<EditorTextPosition> {
    match model.mode {
        EditorMode::Rich => model
            .rich_cursor
            .and_then(|position| layout.visual_position_for_rich(position))
            // Transitional compatibility for old Rich view models. A model
            // that supplies logical ranges never consults source offsets.
            .or_else(|| {
                layout
                    .rich_line_maps
                    .is_empty()
                    .then(|| model.cursor_offset)
                    .flatten()
                    .and_then(|offset| layout.visual_position_for_source(offset))
            })
            .or(model.cursor),
        EditorMode::Source => model
            .cursor_offset
            .and_then(|offset| layout.visual_position_for_source(offset))
            .or(model.cursor),
    }
}

fn rich_mapping_is_selected(
    mapping: DisplayRich,
    layout: &EditorLayout,
    selection: RichRange,
    visual: EditorTextPosition,
) -> bool {
    if selection.is_empty() || !matches!(mapping, DisplayRich::Range(_)) {
        return false;
    }
    let Some(anchor) = layout.visual_position_for_rich(selection.start) else {
        return false;
    };
    let Some(active) = layout.visual_position_for_rich(selection.end) else {
        return false;
    };
    let (start, end) = if anchor <= active {
        (anchor, active)
    } else {
        (active, anchor)
    };
    start <= visual && visual < end
}

fn source_run_is_selected(run: &DisplayRun, selection: EditorSourceSelection) -> bool {
    source_mapping_is_selected(run.source, selection)
}

fn source_mapping_is_selected(mapping: DisplaySource, selection: EditorSourceSelection) -> bool {
    let selected = selection.normalized();
    if selected.is_empty() {
        return false;
    }
    match mapping {
        DisplaySource::Range(range) => range.start < selected.end && selected.start < range.end,
        DisplaySource::Unmapped | DisplaySource::Virtual(_) => false,
    }
}

fn span_style(span: &EditorRenderSpan, theme: &TundraTheme) -> Style {
    let foreground = match span.color {
        EditorSpanColor::Normal => theme.foreground,
        EditorSpanColor::Accent => theme.accent_color,
        EditorSpanColor::Muted => theme.muted,
        EditorSpanColor::Warning => Color::Yellow,
        EditorSpanColor::Error => theme.error,
    };
    let mut style = Style::default().fg(foreground).bg(theme.background);
    if span.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if span.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if span.strikethrough {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    if span.underlined || span.link {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if span.inline_code {
        style = style.fg(Color::White).bg(Color::DarkGray);
    }
    style
}
