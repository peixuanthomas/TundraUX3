use super::document::*;
use super::*;
use crate::RenderContext;
use crate::components::{
    BigText, Button, List, ListItem, Scrollbar, ScrollbarOrientation, Surface, terminal_width,
};
use ratatui::layout::HorizontalAlignment;

/// Render only the editor's main area. Shell chrome remains the caller's responsibility.
pub fn render_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &EditorViewModel,
    theme: &TundraTheme,
) -> EditorLayout {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_editor_contextual(frame, area, model, &context)
}

pub fn render_editor_contextual(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &EditorViewModel,
    context: &RenderContext,
) -> EditorLayout {
    let theme = &context.compatibility_theme();
    let layout = editor_layout(area, model);
    frame.render_widget(Clear, area);
    Surface::new().render_frame(frame, area, context);
    render_menu_bar(frame, &layout, model, theme, context);
    render_toolbar(frame, &layout, model, theme, context);
    render_canvas(frame, &layout, model, theme, context);
    render_status_bar(frame, &layout, model, theme);
    // Popups overlap the editor chrome and canvas. Settings is modal, so it
    // is painted last and receives the highest hit-test priority.
    render_menu_popup(frame, &layout, model, theme, context);
    render_quick_menu(frame, &layout, theme, context);
    render_settings(frame, &layout, model, theme, context);
    layout
}

fn render_menu_bar(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
    context: &RenderContext,
) {
    if layout.menu_bar.is_empty() {
        return;
    }
    Surface::new()
        .raised(true)
        .render_frame(frame, layout.menu_bar, context);
    for item in &layout.menus {
        let active = model.open_menu == Some(item.menu)
            || (item.menu == EditorMenu::Settings && model.settings.is_some());
        let mut item_theme = *theme;
        if !active {
            item_theme.background = theme.muted;
            if model.focus == EditorFocus::MenuBar {
                item_theme.foreground = theme.accent_color;
            }
        }
        render_editor_button(
            frame,
            item.area,
            format!("editor.menu.{:?}", item.menu),
            format!(" {} ", menu_label(item.menu)),
            active,
            false,
            &item_theme,
        );
    }
    for item in &layout.modes {
        let active = item.mode == model.mode;
        let mut item_theme = *theme;
        if !active {
            item_theme.background = theme.muted;
            item_theme.foreground = theme.muted;
        }
        render_editor_button(
            frame,
            item.area,
            format!("editor.mode.{:?}", item.mode),
            format!(" {} ", mode_label(item.mode)),
            active,
            false,
            &item_theme,
        );
    }
}

fn render_menu_popup(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
    context: &RenderContext,
) {
    let Some(area) = layout.menu_popup else {
        return;
    };
    frame.render_widget(Clear, area);
    Surface::new()
        .bordered(true)
        .raised(true)
        .render_frame(frame, area, context);
    let Some(first) = layout.menu_items.first() else {
        return;
    };
    let list_area = Rect::new(
        first.area.x,
        first.area.y,
        first.area.width,
        u16::try_from(layout.menu_items.len()).unwrap_or(u16::MAX),
    );
    let items = layout
        .menu_items
        .iter()
        .map(|item| {
            ListItem::new(
                format!("editor.menu.action.{:?}", item.action),
                menu_action_label(item.action),
            )
            .disabled(!item.enabled)
        })
        .collect();
    let mut list = List::new("editor.menu.actions", items);
    list.set_selected(None);
    list.set_focused(model.focus == EditorFocus::MenuBar);
    list.render_borderless_frame(frame, list_area, theme);
}

fn render_quick_menu(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    _theme: &TundraTheme,
    context: &RenderContext,
) {
    let Some(area) = layout.quick_menu_popup else {
        return;
    };
    frame.render_widget(Clear, area);
    Surface::new()
        .bordered(true)
        .raised(true)
        .render_frame(frame, area, context);

    for item in &layout.quick_menu_items {
        let mut item_theme = context.compatibility_theme();
        if matches!(item.action, EditorQuickAction::Heading(_)) {
            item_theme.foreground = item_theme.accent_color;
        }
        let mut button = Button::new(
            format!("editor.quick.{:?}", item.action),
            quick_action_label(item.action),
        );
        button.state.selected = matches!(item.action, EditorQuickAction::Heading(_));
        button.set_disabled(!item.enabled);
        button.render_borderless_frame(frame, item.area, &item_theme);
    }
}

fn render_settings(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
    context: &RenderContext,
) {
    let (Some(settings_layout), Some(settings)) = (&layout.settings, model.settings.as_ref())
    else {
        return;
    };
    frame.render_widget(Clear, settings_layout.dialog);
    Surface::new()
        .titled(i18n::tr!("ui-editor-editor-settings-padded"))
        .bordered(true)
        .raised(true)
        .render_frame(frame, settings_layout.dialog, context);
    let description = Rect::new(
        settings_layout.dialog.x.saturating_add(2),
        settings_layout.dialog.y.saturating_add(1),
        settings_layout.dialog.width.saturating_sub(4),
        1,
    );
    let description_text = if settings.editable {
        i18n::tr!("ui-editor-hold-one-direction-to-accelerate-with-a-quadratic-curve")
    } else {
        i18n::tr!(
            "ui-editor-read-only-administrator-permission-is-required-to-change-these-settings"
        )
    };
    frame.render_widget(
        Paragraph::new(description_text)
            .alignment(HorizontalAlignment::Left)
            .style(theme.muted_style()),
        description,
    );

    for field in &settings_layout.fields {
        let selected = field.field == settings.selected;
        let locked = !settings.editable && field.field != EditorSettingsField::Cancel;
        let label = match field.field {
            EditorSettingsField::Enabled => i18n::tr!("ui-editor-cursor-acceleration-padded"),
            EditorSettingsField::ActivationDelay => i18n::tr!("ui-editor-start-delay-padded"),
            EditorSettingsField::RampDuration => i18n::tr!("ui-editor-ramp-to-maximum-padded"),
            EditorSettingsField::HorizontalMaxStep => {
                i18n::tr!("ui-editor-horizontal-maximum-padded")
            }
            EditorSettingsField::VerticalMaxStep => i18n::tr!("ui-editor-vertical-maximum-padded"),
            EditorSettingsField::RestoreDefaults
            | EditorSettingsField::Save
            | EditorSettingsField::Cancel => String::new(),
        };
        let padding = usize::from(field.area.width).saturating_sub(terminal_width(&label));
        let label = format!("{label}{}", " ".repeat(padding));
        render_editor_button(
            frame,
            field.area,
            format!("editor.settings.field.{:?}", field.field),
            label,
            selected,
            locked,
            theme,
        );
    }

    for control in &settings_layout.controls {
        let field = settings_control_field(control.control);
        let selected = field.is_some_and(|field| field == settings.selected);
        let locked =
            !settings.editable && !matches!(control.control, EditorSettingsControl::Cancel);
        let label = match control.control {
            EditorSettingsControl::ToggleEnabled => {
                if settings.enabled {
                    i18n::tr!("ui-editor-on-button")
                } else {
                    i18n::tr!("ui-editor-off-button")
                }
            }
            EditorSettingsControl::Decrease(_) => "[-]".to_owned(),
            EditorSettingsControl::Increase(_) => "[+]".to_owned(),
            EditorSettingsControl::RestoreDefaults => {
                i18n::tr!("ui-editor-restore-defaults-button")
            }
            EditorSettingsControl::Save => i18n::tr!("ui-editor-save-button"),
            EditorSettingsControl::Cancel => i18n::tr!("ui-editor-cancel-button"),
        };
        render_editor_button(
            frame,
            control.area,
            format!("editor.settings.control.{:?}", control.control),
            label,
            selected,
            locked,
            theme,
        );
    }

    for (field, value) in [
        (
            EditorSettingsField::ActivationDelay,
            i18n::tr!(
                "ui-editor-milliseconds",
                count = settings.activation_delay_ms
            ),
        ),
        (
            EditorSettingsField::RampDuration,
            i18n::tr!("ui-editor-milliseconds", count = settings.ramp_duration_ms),
        ),
        (
            EditorSettingsField::HorizontalMaxStep,
            i18n::tr!("ui-editor-cells", count = settings.horizontal_max_step),
        ),
        (
            EditorSettingsField::VerticalMaxStep,
            i18n::tr!("ui-editor-lines", count = settings.vertical_max_step),
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
        let style = if !settings.editable {
            theme.muted_style()
        } else if settings.selected == field {
            theme
                .tokens()
                .filled_style(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.body_style()
        };
        frame.render_widget(
            Paragraph::new(value)
                .alignment(HorizontalAlignment::Center)
                .style(style),
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
        Paragraph::new(i18n::tr!(
            "ui-editor-tab-select-left-right-adjust-enter-activate-esc-cancel"
        ))
        .alignment(HorizontalAlignment::Left)
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
    context: &RenderContext,
) {
    if layout.toolbar.is_empty() {
        return;
    }
    Surface::new().render_frame(frame, layout.toolbar, context);
    for item in &layout.toolbar_items {
        let active = model.toolbar.is_active(item.action);
        let selected = model.selected_toolbar_action == Some(item.action)
            && model.focus == EditorFocus::Toolbar;
        render_editor_button(
            frame,
            item.area,
            format!("editor.toolbar.{:?}", item.action),
            toolbar_label(item.action),
            active || selected,
            !item.enabled,
            theme,
        );
    }
}

fn render_editor_button(
    frame: &mut Frame<'_>,
    area: Rect,
    id: impl Into<crate::components::ComponentId>,
    label: impl Into<String>,
    selected: bool,
    disabled: bool,
    theme: &TundraTheme,
) {
    let mut button = Button::new(id, label);
    button.state.selected = selected;
    button.set_disabled(disabled);
    button.render_borderless_frame(frame, area, theme);
}

fn render_canvas(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
    context: &RenderContext,
) {
    if layout.canvas_panel.is_empty() {
        return;
    }
    let canvas_theme = TundraTheme {
        background: context.theme.editor_canvas,
        ..*theme
    };
    let canvas_context = RenderContext {
        theme: crate::ThemeTokens {
            surface: context.theme.editor_canvas,
            ..context.theme
        },
        ..*context
    };
    if layout.canvas_framed {
        let mut title = model.file_name.clone();
        if model.dirty {
            title.push_str(" *");
        }
        if model.read_only {
            title = i18n::tr!("ui-editor-read-only-title", title = title);
        }
        if model
            .read_window
            .is_some_and(|window| window.start_byte > 0)
        {
            title = i18n::tr!("ui-editor-tail-title", title = title);
        }
        let title = terminal_safe_text(&title).into_owned();
        Surface::new().titled(title).bordered(true).render_frame(
            frame,
            layout.canvas_panel,
            &canvas_context,
        );
    } else {
        frame.render_widget(
            Paragraph::new("").style(canvas_theme.body_style()),
            layout.canvas_panel,
        );
    }
    if layout.canvas.is_empty() {
        return;
    }

    for line_layout in &layout.line_areas {
        let relative_line = line_layout
            .document_line
            .saturating_sub(layout.prepared_start);
        let Some(display_line) = layout.prepared_lines.get(relative_line) else {
            continue;
        };
        match display_line.role {
            DisplayLineRole::HeadingTop(level) => {
                let has_visible_bottom = layout
                    .prepared_lines
                    .get(relative_line.saturating_add(1))
                    .is_some_and(|line| {
                        matches!(line.role, DisplayLineRole::HeadingBottom(bottom_level) if bottom_level == level)
                    });
                if !has_visible_bottom || line_layout.area.bottom() >= layout.canvas.bottom() {
                    continue;
                }
                let text = display_line
                    .runs
                    .iter()
                    .map(|run| terminal_safe_text(run.text.resolve(model.source.as_deref())))
                    .collect::<String>();
                frame.render_widget(
                    BigText::new(&text, level, theme.foreground),
                    Rect::new(
                        line_layout.area.x,
                        line_layout.area.y,
                        line_layout.area.width,
                        2,
                    ),
                );
                continue;
            }
            DisplayLineRole::HeadingBottom(_) => continue,
            DisplayLineRole::Normal => {}
        }
        let line = styled_line(
            display_line,
            line_layout.document_line,
            layout,
            model,
            &canvas_theme,
            usize::from(layout.canvas.width),
        );
        frame.render_widget(
            Paragraph::new(line)
                .alignment(HorizontalAlignment::Left)
                .style(canvas_theme.body_style()),
            line_layout.area,
        );
    }

    if let Some(scrollbar) = layout.vertical_scrollbar {
        Scrollbar::new(
            layout.document_line_count,
            layout.visible_capacity,
            layout.visible_start,
        )
        .render_frame(frame, scrollbar.track, &canvas_context);
    }

    if let Some(scrollbar) = layout.horizontal_scrollbar {
        Scrollbar::new(
            layout.horizontal_content_width,
            usize::from(layout.canvas.width),
            layout.horizontal_scroll,
        )
        .orientation(ScrollbarOrientation::HorizontalBottom)
        .render_frame(frame, scrollbar.track, &canvas_context);
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
        EditorImageProtocolStatus::Detecting => i18n::tr!("ui-editor-image-detecting"),
        EditorImageProtocolStatus::Unsupported => i18n::tr!("ui-editor-image-fallback"),
        EditorImageProtocolStatus::Available => i18n::tr!("ui-editor-image-terminal"),
    };
    let mode = mode_label(model.mode);
    let read_window = model.read_window.map(|window| {
        if window.total_bytes == 0 {
            i18n::tr!("ui-editor-bytes-0-of-0")
        } else {
            let start = window.start_byte.min(window.total_bytes.saturating_sub(1));
            i18n::tr!(
                "ui-editor-byte-window",
                start = start.saturating_add(1),
                end = window.total_bytes,
                total = window.total_bytes
            )
        }
    });
    let left = model.status_message.clone().unwrap_or_else(|| {
        if model.read_only {
            i18n::tr!("ui-editor-read-only")
        } else {
            i18n::tr!("ui-editor-ready")
        }
    });
    let left = if model.reload_available {
        i18n::tr!("ui-editor-reload-help", left = left)
    } else {
        left.to_string()
    };
    let right = i18n::tr!(
        "ui-editor-status",
        mode = mode,
        line = cursor.line.saturating_add(1),
        column = cursor.column.saturating_add(1),
        count = model.word_count.clone(),
        encoding = model.encoding.clone(),
        line_ending = model.line_ending.clone(),
        protocol = image,
        window = read_window.map_or_else(String::new, |window| format!("  {window}"))
    );
    let available = usize::from(layout.status_bar.width);
    let left_width = terminal_width(&left);
    let right_width = terminal_width(&right);
    let text = if available == 0 {
        String::new()
    } else if left_width.saturating_add(right_width).saturating_add(2) <= available {
        format!(
            "{}{}{}",
            left,
            " ".repeat(available - left_width - right_width),
            right
        )
    } else {
        fit_text(&format!("{} | {}", left, right), available)
    };
    let text = terminal_safe_text(&text).into_owned();
    let style = if model.focus == EditorFocus::StatusBar {
        theme.tokens().filled_style(theme.accent_color)
    } else {
        theme.tokens().filled_style(theme.muted)
    };
    frame.render_widget(
        Paragraph::new(text)
            .alignment(HorizontalAlignment::Left)
            .style(style),
        layout.status_bar,
    );
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
            let cell_width = terminal_width(&safe).max(1);
            let start = column;
            column = column.saturating_add(cell_width);
            if column <= scroll {
                continue;
            }
            if visible_width.saturating_add(cell_width) > width {
                break;
            }
            let position = EditorTextPosition::new(document_line, start);
            let base_style = c_syntax_style(base_style, grapheme_source, model, theme);
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
                    .patch(theme.tokens().filled_style(theme.accent_color))
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

fn c_syntax_style(
    style: Style,
    source: DisplaySource,
    model: &EditorViewModel,
    theme: &TundraTheme,
) -> Style {
    use app::editor::c_syntax::{CTokenKind, is_c_file};

    if model.mode != EditorMode::Source || !is_c_file(&model.file_name) {
        return style;
    }
    let DisplaySource::Range(range) = source else {
        return style;
    };
    let index = model
        .c_highlights
        .partition_point(|token| token.range.end <= range.start);
    let Some(token) = model
        .c_highlights
        .get(index)
        .filter(|token| token.range.start <= range.start)
    else {
        return style;
    };
    let color = match token.kind {
        CTokenKind::Keyword => theme.accent_color,
        CTokenKind::String => ratatui::style::Color::Green,
        CTokenKind::Number => ratatui::style::Color::Yellow,
        CTokenKind::Comment => theme.muted,
        CTokenKind::Preprocessor => ratatui::style::Color::LightMagenta,
    };
    style.fg(color)
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
                    .then_some(model.cursor_offset)
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
        EditorSpanColor::Warning => theme.accent_color,
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
        style = style.fg(theme.border_color);
    }
    style
}
