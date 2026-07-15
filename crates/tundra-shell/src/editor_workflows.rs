const EDITOR_RECOVERY_IDLE: Duration = Duration::from_secs(2);
const EDITOR_RECOVERY_INTERVAL: Duration = Duration::from_secs(10);

impl ShellState {
    fn open_editor(&mut self) {
        self.editor_state = Some(EditorState::new());
        self.editor_focus = tundra_ui::EditorFocus::Canvas;
        self.editor_open_menu = None;
        self.editor_selected_toolbar_action = None;
        self.editor_drag_anchor = None;
        self.editor_fingerprint = None;
        self.editor_close_after_save = false;
        self.editor_open_after_save = false;
        self.editor_discard_for_open = false;
        self.editor_message = Some("New Markdown document".to_string());
        self.restore_editor_recovery_if_present();
        if self.active_screen() != ShellScreen::Editor {
            self.screen_stack.push(ShellScreen::Editor);
        }
        self.focused_component = ShellComponent::Editor;
        self.active_popup = None;
        self.notify_status("Editor");
        self.refresh_hit_map();
    }

    pub fn to_editor_view_model(&self) -> tundra_ui::EditorViewModel {
        let Some(state) = self.editor_state.as_ref() else {
            return tundra_ui::EditorViewModel::new("Untitled.md", Vec::new());
        };
        let source = state.document.source();
        let mut model = match state.mode {
            tundra_apps::editor::EditorMode::Rich => {
                let (blocks, block_sources) = editor_render_blocks(state.render_blocks(), source);
                let mut model =
                    tundra_ui::EditorViewModel::new(state.document.display_name(), blocks);
                model.source = Some(source.to_owned());
                model.block_sources = block_sources;
                model
            }
            tundra_apps::editor::EditorMode::Source => {
                tundra_ui::EditorViewModel::source(state.document.display_name(), source)
            }
        };
        model.path_hint = state
            .document
            .path
            .as_ref()
            .map(|path| path.display().to_string());
        model.dirty = state.is_dirty();
        model.mode = editor_mode_to_ui(state.mode);
        model.focus = self.editor_focus;
        model.open_menu = self.editor_open_menu;
        model.selected_toolbar_action = self.editor_selected_toolbar_action;
        model.scroll_line = state.viewport.top_line;
        model.horizontal_scroll = state.viewport.left_column;
        model.cursor = Some(editor_text_position(source, state.cursor.byte_offset));
        model.selection = state.selection.map(|selection| tundra_ui::EditorSelection {
            anchor: editor_text_position(source, selection.anchor),
            active: editor_text_position(source, selection.focus),
        });
        model.cursor_offset = Some(state.cursor.byte_offset);
        model.selection_offsets = state.selection.map(|selection| {
            tundra_ui::EditorSourceSelection::new(selection.anchor, selection.focus)
        });
        model.toolbar.can_save = state.document.path.is_some() || state.is_dirty();
        model.toolbar.can_undo = state.can_undo();
        model.toolbar.can_redo = state.can_redo();
        model.toolbar.can_cut = state.selected_text().is_some();
        model.toolbar.can_copy = state.selected_text().is_some();
        model.toolbar.can_paste = true;
        model.word_count = state.document.word_count();
        model.encoding = if state.document.metadata.utf8_bom {
            "UTF-8 BOM".to_string()
        } else {
            "UTF-8".to_string()
        };
        model.line_ending = editor_line_ending_label(state.document.metadata);
        model.image_protocol = tundra_ui::EditorImageProtocolStatus::Unsupported;
        model.status_message = self.editor_message.clone();
        model
    }
}

fn editor_mode_to_ui(mode: tundra_apps::editor::EditorMode) -> tundra_ui::EditorMode {
    match mode {
        tundra_apps::editor::EditorMode::Rich => tundra_ui::EditorMode::Rich,
        tundra_apps::editor::EditorMode::Source => tundra_ui::EditorMode::Source,
    }
}

fn editor_mode_from_ui(mode: tundra_ui::EditorMode) -> tundra_apps::editor::EditorMode {
    match mode {
        tundra_ui::EditorMode::Rich => tundra_apps::editor::EditorMode::Rich,
        tundra_ui::EditorMode::Source => tundra_apps::editor::EditorMode::Source,
    }
}

fn editor_text_position(source: &str, byte_offset: usize) -> tundra_ui::EditorTextPosition {
    let offset = byte_offset.min(source.len());
    let prefix = source.get(..offset).unwrap_or(source);
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let column = prefix[line_start..].chars().count();
    tundra_ui::EditorTextPosition { line, column }
}

fn editor_byte_offset(source: &str, position: tundra_ui::EditorTextPosition) -> usize {
    let mut line_start = 0usize;
    for _ in 0..position.line {
        let Some(relative) = source[line_start..].find('\n') else {
            return source.len();
        };
        line_start += relative + 1;
    }
    let line_end = source[line_start..]
        .find('\n')
        .map_or(source.len(), |relative| line_start + relative);
    source[line_start..line_end]
        .char_indices()
        .nth(position.column)
        .map_or(line_end, |(relative, _)| line_start + relative)
}

fn editor_line_ending_label(metadata: tundra_apps::editor::TextMetadata) -> String {
    if metadata.mixed_line_endings {
        return "Mixed".to_string();
    }
    match metadata.preferred_line_ending {
        tundra_apps::editor::LineEnding::Lf => "LF".to_string(),
        tundra_apps::editor::LineEnding::CrLf => "CRLF".to_string(),
        tundra_apps::editor::LineEnding::Cr => "CR".to_string(),
    }
}

fn editor_render_blocks(
    blocks: Vec<tundra_apps::editor::RenderBlock>,
    source: &str,
) -> (
    Vec<tundra_ui::EditorRenderBlock>,
    Vec<tundra_ui::EditorBlockSourceMap>,
) {
    let mut rendered = Vec::new();
    let mut block_sources = Vec::new();
    for block in blocks {
        append_editor_render_block(block, source, 0, &mut rendered, &mut block_sources);
    }
    (rendered, block_sources)
}

fn append_editor_render_block(
    block: tundra_apps::editor::RenderBlock,
    source: &str,
    depth: u8,
    output: &mut Vec<tundra_ui::EditorRenderBlock>,
    block_sources: &mut Vec<tundra_ui::EditorBlockSourceMap>,
) {
    use tundra_apps::editor::RenderBlock;
    match block {
        RenderBlock::Heading {
            level,
            content,
            source_range,
            content_range,
        } => {
            push_editor_render_block(
                output,
                block_sources,
                tundra_ui::EditorRenderBlock::Heading {
                    level,
                    spans: editor_render_spans(content),
                },
                editor_block_source_map(source_range, Some(content_range)),
            );
        }
        RenderBlock::Paragraph {
            content,
            source_range,
            content_range,
        } => push_editor_render_block(
            output,
            block_sources,
            tundra_ui::EditorRenderBlock::Paragraph(editor_render_spans(content)),
            editor_block_source_map(source_range, Some(content_range)),
        ),
        RenderBlock::Quote { blocks, .. } => {
            for block in blocks {
                let mapping = editor_block_map_for_domain_block(&block);
                push_editor_render_block(
                    output,
                    block_sources,
                    tundra_ui::EditorRenderBlock::Quote {
                        depth: depth.saturating_add(1),
                        spans: editor_block_spans(block),
                    },
                    mapping,
                );
            }
        }
        RenderBlock::CodeBlock {
            language,
            code,
            source_range,
            content_range: _,
        } => {
            push_editor_render_block(
                output,
                block_sources,
                tundra_ui::EditorRenderBlock::CodeBlock {
                    language,
                    lines: code.split('\n').map(str::to_string).collect(),
                },
                // Fenced-code normalization (info strings, indentation and
                // CRLF) is not guaranteed to be a byte-exact projection. Keep
                // the block anchor but require Source mode for edits.
                editor_block_source_map(source_range, None),
            );
        }
        RenderBlock::BulletList { items, .. } => {
            for item in items {
                let content_range = editor_span_content_range(&item.content);
                push_editor_render_block(
                    output,
                    block_sources,
                    tundra_ui::EditorRenderBlock::BulletListItem {
                        depth,
                        checked: item.checked,
                        spans: editor_render_spans(item.content),
                    },
                    editor_block_source_map(item.source_range, content_range),
                );
                for child in item.children {
                    append_editor_render_block(
                        child,
                        source,
                        depth.saturating_add(1),
                        output,
                        block_sources,
                    );
                }
            }
        }
        RenderBlock::OrderedList { start, items, .. } => {
            for (index, item) in items.into_iter().enumerate() {
                let content_range = editor_span_content_range(&item.content);
                push_editor_render_block(
                    output,
                    block_sources,
                    tundra_ui::EditorRenderBlock::OrderedListItem {
                        depth,
                        number: start.saturating_add(index) as u64,
                        spans: editor_render_spans(item.content),
                    },
                    editor_block_source_map(item.source_range, content_range),
                );
                for child in item.children {
                    append_editor_render_block(
                        child,
                        source,
                        depth.saturating_add(1),
                        output,
                        block_sources,
                    );
                }
            }
        }
        RenderBlock::TaskList { items, .. } => {
            for item in items {
                let content_range = editor_span_content_range(&item.content);
                push_editor_render_block(
                    output,
                    block_sources,
                    tundra_ui::EditorRenderBlock::BulletListItem {
                        depth,
                        checked: item.checked,
                        spans: editor_render_spans(item.content),
                    },
                    editor_block_source_map(item.source_range, content_range),
                );
                for child in item.children {
                    append_editor_render_block(
                        child,
                        source,
                        depth.saturating_add(1),
                        output,
                        block_sources,
                    );
                }
            }
        }
        RenderBlock::Table {
            header,
            rows,
            alignments,
            source_range,
        } => push_editor_render_block(
            output,
            block_sources,
            tundra_ui::EditorRenderBlock::Table {
                header: header
                    .into_iter()
                    .map(|cell| tundra_ui::EditorTableCell {
                        spans: editor_render_spans(cell.content),
                    })
                    .collect(),
                rows: rows
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .map(|cell| tundra_ui::EditorTableCell {
                                spans: editor_render_spans(cell.content),
                            })
                            .collect()
                    })
                    .collect(),
                alignments: alignments
                    .into_iter()
                    .map(|alignment| match alignment {
                        tundra_apps::editor::TableAlignment::None
                        | tundra_apps::editor::TableAlignment::Left => {
                            tundra_ui::EditorTableAlignment::Left
                        }
                        tundra_apps::editor::TableAlignment::Center => {
                            tundra_ui::EditorTableAlignment::Center
                        }
                        tundra_apps::editor::TableAlignment::Right => {
                            tundra_ui::EditorTableAlignment::Right
                        }
                    })
                    .collect(),
            },
            editor_block_source_map(source_range, None),
        ),
        RenderBlock::Rule { source_range } => push_editor_render_block(
            output,
            block_sources,
            tundra_ui::EditorRenderBlock::HorizontalRule,
            editor_block_source_map(source_range, None),
        ),
        RenderBlock::RawHtml { html, source_range } => {
            push_editor_render_block(
                output,
                block_sources,
                tundra_ui::EditorRenderBlock::RawHtml(html),
                editor_block_source_map(source_range, None),
            );
        }
        RenderBlock::Image {
            alt,
            url,
            title,
            source_range,
        } => {
            let markdown = source
                .get(source_range.start..source_range.end)
                .map(str::to_string)
                .unwrap_or_else(|| match title {
                    Some(title) => format!("![{alt}]({url} \"{title}\")"),
                    None => format!("![{alt}]({url})"),
                });
            push_editor_render_block(
                output,
                block_sources,
                tundra_ui::EditorRenderBlock::Image { markdown },
                editor_block_source_map(source_range, None),
            );
        }
        RenderBlock::FootnoteDefinition {
            name,
            blocks,
            source_range,
        } => {
            let spans = blocks
                .into_iter()
                .flat_map(editor_block_spans)
                .collect();
            push_editor_render_block(
                output,
                block_sources,
                tundra_ui::EditorRenderBlock::Footnote { label: name, spans },
                editor_block_source_map(source_range, None),
            );
        }
        RenderBlock::PlainText { text, source_range } => {
            for line in text.split('\n') {
                push_editor_render_block(
                    output,
                    block_sources,
                    tundra_ui::EditorRenderBlock::paragraph(line),
                    editor_block_source_map(source_range, None),
                );
            }
        }
        RenderBlock::Raw {
            source,
            source_range,
            ..
        } => {
            push_editor_render_block(
                output,
                block_sources,
                tundra_ui::EditorRenderBlock::RawHtml(source),
                editor_block_source_map(source_range, None),
            );
        }
    }
}

fn push_editor_render_block(
    output: &mut Vec<tundra_ui::EditorRenderBlock>,
    block_sources: &mut Vec<tundra_ui::EditorBlockSourceMap>,
    block: tundra_ui::EditorRenderBlock,
    source: tundra_ui::EditorBlockSourceMap,
) {
    output.push(block);
    block_sources.push(source);
}

fn editor_block_source_map(
    source_range: tundra_apps::editor::SourceRange,
    content_range: Option<tundra_apps::editor::SourceRange>,
) -> tundra_ui::EditorBlockSourceMap {
    let mapping = tundra_ui::EditorBlockSourceMap::new(editor_ui_source_range(source_range));
    content_range.map_or(mapping, |content| {
        mapping.with_content_range(editor_ui_source_range(content))
    })
}

fn editor_block_map_for_domain_block(
    block: &tundra_apps::editor::RenderBlock,
) -> tundra_ui::EditorBlockSourceMap {
    use tundra_apps::editor::RenderBlock;
    let content_range = match block {
        RenderBlock::Heading { content_range, .. }
        | RenderBlock::Paragraph { content_range, .. }
        | RenderBlock::CodeBlock { content_range, .. } => Some(*content_range),
        _ => None,
    };
    editor_block_source_map(block.source_range(), content_range)
}

fn editor_span_content_range(
    spans: &[tundra_apps::editor::RenderSpan],
) -> Option<tundra_apps::editor::SourceRange> {
    Some(tundra_apps::editor::SourceRange::new(
        spans.iter().map(|span| span.source_range.start).min()?,
        spans.iter().map(|span| span.source_range.end).max()?,
    ))
}

fn editor_ui_source_range(
    range: tundra_apps::editor::SourceRange,
) -> tundra_ui::EditorSourceRange {
    tundra_ui::EditorSourceRange::new(range.start, range.end)
}

fn editor_block_spans(block: tundra_apps::editor::RenderBlock) -> Vec<tundra_ui::EditorRenderSpan> {
    use tundra_apps::editor::RenderBlock;
    match block {
        RenderBlock::Heading { content, .. } | RenderBlock::Paragraph { content, .. } => {
            editor_render_spans(content)
        }
        RenderBlock::CodeBlock {
            code,
            content_range,
            ..
        } => vec![tundra_ui::EditorRenderSpan::code(code)
            .with_source_range(editor_ui_source_range(content_range))],
        RenderBlock::RawHtml { html, source_range }
        | RenderBlock::PlainText {
            text: html,
            source_range,
        }
        | RenderBlock::Raw {
            source: html,
            source_range,
            ..
        } => {
            vec![tundra_ui::EditorRenderSpan::plain(html)
                .with_source_range(editor_ui_source_range(source_range))]
        }
        RenderBlock::Image {
            alt,
            url,
            source_range,
            ..
        } => {
            vec![tundra_ui::EditorRenderSpan::plain(format!("![{alt}]({url})"))
                .with_source_range(editor_ui_source_range(source_range))]
        }
        RenderBlock::Quote { blocks, .. } | RenderBlock::FootnoteDefinition { blocks, .. } => {
            blocks.into_iter().flat_map(editor_block_spans).collect()
        }
        RenderBlock::BulletList { items, .. }
        | RenderBlock::OrderedList { items, .. }
        | RenderBlock::TaskList { items, .. } => items
            .into_iter()
            .flat_map(|item| editor_render_spans(item.content))
            .collect(),
        RenderBlock::Table { header, rows, .. } => header
            .into_iter()
            .chain(rows.into_iter().flatten())
            .flat_map(|cell| editor_render_spans(cell.content))
            .collect(),
        RenderBlock::Rule { source_range } => {
            vec![tundra_ui::EditorRenderSpan::plain("---")
                .with_source_range(editor_ui_source_range(source_range))]
        }
    }
}

fn editor_render_spans(
    spans: Vec<tundra_apps::editor::RenderSpan>,
) -> Vec<tundra_ui::EditorRenderSpan> {
    spans
        .into_iter()
        .map(|span| {
            let source_range = editor_ui_source_range(span.source_range);
            let text = if span
                .styles
                .contains(&tundra_apps::editor::InlineStyle::Image)
            {
                let url = span.target.as_deref().unwrap_or_default();
                match span.title.as_deref() {
                    Some(title) => format!("![{}]({url} \"{title}\")", span.text),
                    None => format!("![{}]({url})", span.text),
                }
            } else {
                span.text
            };
            let mut rendered =
                tundra_ui::EditorRenderSpan::plain(text).with_source_range(source_range);
            for style in span.styles {
                match style {
                    tundra_apps::editor::InlineStyle::Bold => rendered.bold = true,
                    tundra_apps::editor::InlineStyle::Italic => rendered.italic = true,
                    tundra_apps::editor::InlineStyle::Strikethrough => {
                        rendered.strikethrough = true;
                    }
                    tundra_apps::editor::InlineStyle::Code => rendered.inline_code = true,
                    tundra_apps::editor::InlineStyle::Link => {
                        rendered.link = true;
                        rendered.underlined = true;
                        rendered.color = tundra_ui::EditorSpanColor::Accent;
                    }
                    tundra_apps::editor::InlineStyle::RawHtml => {
                        rendered.color = tundra_ui::EditorSpanColor::Muted;
                    }
                    tundra_apps::editor::InlineStyle::Image
                    | tundra_apps::editor::InlineStyle::Footnote
                    | tundra_apps::editor::InlineStyle::HardBreak => {}
                }
            }
            rendered
        })
        .collect()
}

impl ShellState {
    fn request_editor_close(&mut self, platform: &dyn Platform) {
        self.apply_editor_command(tundra_apps::editor::EditorCommand::RequestClose, platform);
    }

    fn apply_editor_command(
        &mut self,
        command: tundra_apps::editor::EditorCommand,
        platform: &dyn Platform,
    ) {
        let Some(state) = self.editor_state.as_mut() else {
            return;
        };
        let source_before = state.document.source().to_string();
        let effects = state.apply(command);
        if state.document.source() != source_before {
            self.editor_recovery_dirty_since = Some(Instant::now());
        }
        if !state.is_dirty() {
            self.editor_recovery_dirty_since = None;
        }
        for effect in effects {
            self.handle_editor_effect(effect, platform);
        }
    }

    fn handle_editor_effect(
        &mut self,
        effect: tundra_apps::editor::EditorEffect,
        platform: &dyn Platform,
    ) {
        match effect {
            tundra_apps::editor::EditorEffect::WriteClipboard(text) => {
                match platform.write_clipboard_text(&text) {
                    Ok(()) => self.editor_message = Some("Copied".to_string()),
                    Err(error) => self.report_editor_error(format!("Could not copy: {error}")),
                }
            }
            tundra_apps::editor::EditorEffect::ReadClipboard => {
                match platform.read_clipboard_text() {
                    Ok(text) => self.apply_editor_command(
                        tundra_apps::editor::EditorCommand::Paste(text),
                        platform,
                    ),
                    Err(error) => self.report_editor_error(format!("Could not paste: {error}")),
                }
            }
            tundra_apps::editor::EditorEffect::OpenFilePicker => {
                if self
                    .editor_state
                    .as_ref()
                    .is_some_and(EditorState::is_dirty)
                {
                    self.confirm_editor_open();
                } else {
                    self.open_editor_picker(platform);
                }
            }
            tundra_apps::editor::EditorEffect::SaveFile { path, contents } => {
                self.save_editor_document(path, contents, platform);
            }
            tundra_apps::editor::EditorEffect::SaveFilePicker {
                suggested_name,
                contents,
            } => self.open_editor_save_picker(platform, suggested_name, contents),
            tundra_apps::editor::EditorEffect::ConfirmClose => {
                self.notify_modal_with_options(
                    ShellNotification::modal(
                        "Unsaved document",
                        "Save your changes before closing the Editor?",
                        tundra_ui::NotificationTone::Warning,
                        vec![
                            ShellNotificationAction::new("save", "Save")
                                .with_follow_up(ShellCommand::EditorSaveAndClose),
                            ShellNotificationAction::new("discard", "Discard")
                                .with_follow_up(ShellCommand::EditorDiscardAndClose),
                            ShellNotificationAction::new("cancel", "Cancel")
                                .cancel()
                                .with_follow_up(ShellCommand::EditorCancelClose),
                        ],
                    )
                    .with_key(EDITOR_CLOSE_NOTIFICATION_KEY)
                    .with_component(ShellComponent::NotificationDialog),
                );
            }
            tundra_apps::editor::EditorEffect::Close => self.finish_editor_close(false),
        }
    }

    fn handle_editor_key(&mut self, key: KeyInput, platform: &dyn Platform) {
        if !key.phase.is_press_like() {
            return;
        }
        let repeated = key.phase == InputPhase::Repeat;
        if key.key == InputKey::Escape {
            if repeated {
                return;
            }
            if self.editor_open_menu.take().is_some()
                || self.editor_selected_toolbar_action.take().is_some()
            {
                self.editor_focus = tundra_ui::EditorFocus::Canvas;
                return;
            }
        }
        self.editor_open_menu = None;
        self.editor_selected_toolbar_action = None;
        let command_key = key.modifiers.control
            || (platform.kind() == PlatformKind::Macos && key.modifiers.super_key);
        if command_key {
            let navigation = match key.key {
                InputKey::Left => Some(tundra_apps::editor::CursorMove::WordLeft),
                InputKey::Right => Some(tundra_apps::editor::CursorMove::WordRight),
                InputKey::Home => Some(tundra_apps::editor::CursorMove::DocumentStart),
                InputKey::End => Some(tundra_apps::editor::CursorMove::DocumentEnd),
                _ => None,
            };
            if let Some(movement) = navigation {
                self.apply_editor_command(
                    tundra_apps::editor::EditorCommand::MoveCursor {
                        movement,
                        extend_selection: key.modifiers.shift,
                    },
                    platform,
                );
                return;
            }
            // Navigation and text editing may repeat while a key is held, but
            // document/clipboard/format actions must run once per physical key
            // press. In particular, a repeated Ctrl+W must never close a newly
            // opened document after the first close has already completed.
            if repeated {
                return;
            }
            let character = match key.key {
                InputKey::Character(character) => character.to_ascii_lowercase(),
                _ => '\0',
            };
            if key.modifiers.alt {
                let format = match character {
                    '0' => Some(tundra_apps::editor::FormatCommand::Paragraph),
                    '1'..='6' => Some(tundra_apps::editor::FormatCommand::Heading(
                        character.to_digit(10).unwrap_or_default() as u8,
                    )),
                    _ => None,
                };
                if let Some(format) = format {
                    self.apply_editor_command(
                        tundra_apps::editor::EditorCommand::ApplyFormat(format),
                        platform,
                    );
                    return;
                }
            }
            let command = match (character, key.modifiers.shift) {
                ('n', _) => {
                    if self.editor_state.as_ref().is_some_and(EditorState::is_dirty) {
                        self.editor_message =
                            Some("Save or close the current document before creating a new one".to_string());
                    } else {
                        self.editor_state = Some(EditorState::new());
                        self.editor_fingerprint = None;
                        self.editor_message = Some("New Markdown document".to_string());
                    }
                    return;
                }
                ('o', _) => tundra_apps::editor::EditorCommand::RequestOpen,
                ('s', true) => tundra_apps::editor::EditorCommand::RequestSaveAs,
                ('s', false) => tundra_apps::editor::EditorCommand::RequestSave,
                ('w', _) => tundra_apps::editor::EditorCommand::RequestClose,
                ('z', false) => tundra_apps::editor::EditorCommand::Undo,
                ('y', _) | ('z', true) => tundra_apps::editor::EditorCommand::Redo,
                ('x', true) => tundra_apps::editor::EditorCommand::ApplyFormat(
                    tundra_apps::editor::FormatCommand::Strikethrough,
                ),
                ('x', false) => tundra_apps::editor::EditorCommand::Cut,
                ('c', _) => tundra_apps::editor::EditorCommand::Copy,
                ('v', _) => tundra_apps::editor::EditorCommand::RequestPaste,
                ('a', _) => tundra_apps::editor::EditorCommand::SelectAll,
                ('b', _) => tundra_apps::editor::EditorCommand::ApplyFormat(
                    tundra_apps::editor::FormatCommand::Bold,
                ),
                ('i', _) => tundra_apps::editor::EditorCommand::ApplyFormat(
                    tundra_apps::editor::FormatCommand::Italic,
                ),
                ('m', true) => tundra_apps::editor::EditorCommand::ToggleMode,
                ('f', _) => {
                    self.editor_message = Some("Find is not available in this build".to_string());
                    return;
                }
                ('h', _) => {
                    self.editor_message = Some("Replace is not available in this build".to_string());
                    return;
                }
                ('k', _) => tundra_apps::editor::EditorCommand::ApplyFormat(
                    tundra_apps::editor::FormatCommand::Link {
                        url: "https://".to_string(),
                        title: None,
                    },
                ),
                _ => return,
            };
            self.apply_editor_command(command, platform);
            return;
        }

        let command = match key.key {
            InputKey::Function(6) => {
                self.editor_focus = match self.editor_focus {
                    tundra_ui::EditorFocus::MenuBar => tundra_ui::EditorFocus::Toolbar,
                    tundra_ui::EditorFocus::Toolbar => tundra_ui::EditorFocus::Canvas,
                    tundra_ui::EditorFocus::Canvas => tundra_ui::EditorFocus::StatusBar,
                    tundra_ui::EditorFocus::StatusBar => tundra_ui::EditorFocus::MenuBar,
                };
                return;
            }
            InputKey::Escape => tundra_apps::editor::EditorCommand::RequestClose,
            InputKey::Enter => tundra_apps::editor::EditorCommand::InsertNewline,
            InputKey::Backspace => tundra_apps::editor::EditorCommand::Backspace,
            InputKey::Delete => tundra_apps::editor::EditorCommand::DeleteForward,
            InputKey::Tab => tundra_apps::editor::EditorCommand::InsertText("    ".to_string()),
            InputKey::BackTab => {
                self.editor_message = Some("Outdent is not available for this block".to_string());
                return;
            }
            InputKey::Left => tundra_apps::editor::EditorCommand::MoveCursor {
                movement: tundra_apps::editor::CursorMove::Left,
                extend_selection: key.modifiers.shift,
            },
            InputKey::Right => tundra_apps::editor::EditorCommand::MoveCursor {
                movement: tundra_apps::editor::CursorMove::Right,
                extend_selection: key.modifiers.shift,
            },
            InputKey::Up => tundra_apps::editor::EditorCommand::MoveCursor {
                movement: tundra_apps::editor::CursorMove::Up,
                extend_selection: key.modifiers.shift,
            },
            InputKey::Down => tundra_apps::editor::EditorCommand::MoveCursor {
                movement: tundra_apps::editor::CursorMove::Down,
                extend_selection: key.modifiers.shift,
            },
            InputKey::Home => tundra_apps::editor::EditorCommand::MoveCursor {
                movement: tundra_apps::editor::CursorMove::LineStart,
                extend_selection: key.modifiers.shift,
            },
            InputKey::End => tundra_apps::editor::EditorCommand::MoveCursor {
                movement: tundra_apps::editor::CursorMove::LineEnd,
                extend_selection: key.modifiers.shift,
            },
            InputKey::PageUp => {
                if let Some(state) = self.editor_state.as_mut() {
                    state.viewport.top_line = state.viewport.top_line.saturating_sub(10);
                }
                return;
            }
            InputKey::PageDown => {
                if let Some(state) = self.editor_state.as_mut() {
                    state.viewport.top_line = state.viewport.top_line.saturating_add(10);
                }
                return;
            }
            InputKey::Character(character) if !key.has_non_shift_modifier() => {
                tundra_apps::editor::EditorCommand::InsertText(character.to_string())
            }
            _ => return,
        };
        self.apply_editor_command(command, platform);
    }

    fn handle_editor_paste(&mut self, value: String) {
        let platform = tundra_platform::native_platform();
        self.apply_editor_command(
            tundra_apps::editor::EditorCommand::Paste(value),
            platform.as_ref(),
        );
    }

    fn handle_editor_pointer(&mut self, mouse: MouseInput, platform: &dyn Platform) {
        let coordinates = mouse.coordinates();
        let (hit, source_hit) = self.editor_hit_targets_at(coordinates);
        match mouse {
            MouseInput::Moved { .. } => {
                self.hovered_component = hit.map(|_| ShellComponent::Editor);
            }
            MouseInput::Scroll { direction, .. } => {
                if let Some(state) = self.editor_state.as_mut() {
                    match direction {
                        ScrollDirection::Up => {
                            state.viewport.top_line = state.viewport.top_line.saturating_sub(3);
                        }
                        ScrollDirection::Down => {
                            state.viewport.top_line = state.viewport.top_line.saturating_add(3);
                        }
                        ScrollDirection::Left => {
                            state.viewport.left_column = state.viewport.left_column.saturating_sub(4);
                        }
                        ScrollDirection::Right => {
                            state.viewport.left_column = state.viewport.left_column.saturating_add(4);
                        }
                    }
                }
            }
            MouseInput::Down {
                button: PointerButton::Left,
                modifiers,
                ..
            } => match hit {
                Some(tundra_ui::EditorHitTarget::Menu(menu)) => {
                    self.editor_focus = tundra_ui::EditorFocus::MenuBar;
                    self.editor_open_menu = (self.editor_open_menu != Some(menu)).then_some(menu);
                }
                Some(tundra_ui::EditorHitTarget::Toolbar(action)) => {
                    self.editor_focus = tundra_ui::EditorFocus::Toolbar;
                    self.editor_selected_toolbar_action = Some(action);
                    self.activate_editor_toolbar(action, platform);
                }
                Some(tundra_ui::EditorHitTarget::Mode(mode)) => self.apply_editor_command(
                    tundra_apps::editor::EditorCommand::SetMode(editor_mode_from_ui(mode)),
                    platform,
                ),
                Some(tundra_ui::EditorHitTarget::Canvas(position)) => {
                    self.editor_focus = tundra_ui::EditorFocus::Canvas;
                    let rich_mode = self.editor_state.as_ref().is_some_and(|state| {
                        state.mode == tundra_apps::editor::EditorMode::Rich
                    });
                    let offset = match source_hit {
                        Some(hit) if hit.editable || !rich_mode => hit.byte_offset,
                        Some(hit) => {
                            self.apply_editor_command(
                                tundra_apps::editor::EditorCommand::SetMode(
                                    tundra_apps::editor::EditorMode::Source,
                                ),
                                platform,
                            );
                            self.apply_editor_command(
                                tundra_apps::editor::EditorCommand::MoveTo {
                                    byte_offset: hit.byte_offset,
                                    extend_selection: modifiers.shift,
                                },
                                platform,
                            );
                            self.editor_drag_anchor = Some(hit.byte_offset);
                            self.editor_message = Some(
                                "Switched to Source mode for an exact Markdown edit".to_string(),
                            );
                            return;
                        }
                        None if rich_mode => {
                            self.apply_editor_command(
                                tundra_apps::editor::EditorCommand::SetMode(
                                    tundra_apps::editor::EditorMode::Source,
                                ),
                                platform,
                            );
                            self.editor_message = Some(
                                "Switched to Source mode because this rendered cell has no exact source position"
                                    .to_string(),
                            );
                            return;
                        }
                        None => self
                            .editor_state
                            .as_ref()
                            .map(|state| editor_byte_offset(state.document.source(), position))
                            .unwrap_or(0),
                    };
                    self.editor_drag_anchor = Some(offset);
                    self.apply_editor_command(
                        tundra_apps::editor::EditorCommand::MoveTo {
                            byte_offset: offset,
                            extend_selection: modifiers.shift,
                        },
                        platform,
                    );
                }
                Some(tundra_ui::EditorHitTarget::StatusBar) => {
                    self.editor_focus = tundra_ui::EditorFocus::StatusBar;
                }
                Some(tundra_ui::EditorHitTarget::VerticalScrollbar) => {
                    self.scroll_editor_to(coordinates);
                }
                None => {}
            },
            MouseInput::Drag {
                button: PointerButton::Left,
                ..
            } => {
                if let Some(tundra_ui::EditorHitTarget::Canvas(position)) = hit {
                    let rich_mode = self.editor_state.as_ref().is_some_and(|state| {
                        state.mode == tundra_apps::editor::EditorMode::Rich
                    });
                    let offset = match source_hit {
                        Some(hit) if hit.editable || !rich_mode => hit.byte_offset,
                        Some(hit) => {
                            self.apply_editor_command(
                                tundra_apps::editor::EditorCommand::SetMode(
                                    tundra_apps::editor::EditorMode::Source,
                                ),
                                platform,
                            );
                            self.apply_editor_command(
                                tundra_apps::editor::EditorCommand::MoveTo {
                                    byte_offset: hit.byte_offset,
                                    extend_selection: true,
                                },
                                platform,
                            );
                            self.editor_message = Some(
                                "Switched to Source mode for an exact Markdown selection".to_string(),
                            );
                            return;
                        }
                        None if rich_mode => {
                            self.apply_editor_command(
                                tundra_apps::editor::EditorCommand::SetMode(
                                    tundra_apps::editor::EditorMode::Source,
                                ),
                                platform,
                            );
                            return;
                        }
                        None => self
                            .editor_state
                            .as_ref()
                            .map(|state| editor_byte_offset(state.document.source(), position))
                            .unwrap_or(0),
                    };
                    self.apply_editor_command(
                        tundra_apps::editor::EditorCommand::MoveTo {
                            byte_offset: offset,
                            extend_selection: true,
                        },
                        platform,
                    );
                }
            }
            MouseInput::Up {
                button: PointerButton::Left,
                ..
            } => self.editor_drag_anchor = None,
            MouseInput::Down { .. } | MouseInput::Up { .. } | MouseInput::Drag { .. } => {}
        }
    }

    fn activate_editor_toolbar(
        &mut self,
        action: tundra_ui::EditorToolbarAction,
        platform: &dyn Platform,
    ) {
        use tundra_apps::editor::{EditorCommand, FormatCommand};
        let command = match action {
            tundra_ui::EditorToolbarAction::New => {
                if self.editor_state.as_ref().is_none_or(|state| !state.is_dirty()) {
                    self.editor_state = Some(EditorState::new());
                    self.editor_fingerprint = None;
                } else {
                    self.editor_message = Some("Save or close the current document first".to_string());
                }
                return;
            }
            tundra_ui::EditorToolbarAction::Open => EditorCommand::RequestOpen,
            tundra_ui::EditorToolbarAction::Save => EditorCommand::RequestSave,
            tundra_ui::EditorToolbarAction::Undo => EditorCommand::Undo,
            tundra_ui::EditorToolbarAction::Redo => EditorCommand::Redo,
            tundra_ui::EditorToolbarAction::ParagraphStyle => {
                EditorCommand::ApplyFormat(FormatCommand::Paragraph)
            }
            tundra_ui::EditorToolbarAction::Bold => {
                EditorCommand::ApplyFormat(FormatCommand::Bold)
            }
            tundra_ui::EditorToolbarAction::Italic => {
                EditorCommand::ApplyFormat(FormatCommand::Italic)
            }
            tundra_ui::EditorToolbarAction::Strikethrough => {
                EditorCommand::ApplyFormat(FormatCommand::Strikethrough)
            }
            tundra_ui::EditorToolbarAction::InlineCode => {
                EditorCommand::ApplyFormat(FormatCommand::InlineCode)
            }
            tundra_ui::EditorToolbarAction::BulletList => {
                EditorCommand::ApplyFormat(FormatCommand::BulletList)
            }
            tundra_ui::EditorToolbarAction::OrderedList => {
                EditorCommand::ApplyFormat(FormatCommand::OrderedList)
            }
            tundra_ui::EditorToolbarAction::Quote => {
                EditorCommand::ApplyFormat(FormatCommand::Quote)
            }
            tundra_ui::EditorToolbarAction::Table => {
                EditorCommand::ApplyFormat(FormatCommand::Table { columns: 3, rows: 2 })
            }
            tundra_ui::EditorToolbarAction::Link => {
                self.editor_message =
                    Some("Inserted a link placeholder; edit its URL in Source mode".to_string());
                EditorCommand::ApplyFormat(FormatCommand::Link {
                    url: "https://".to_string(),
                    title: None,
                })
            }
            tundra_ui::EditorToolbarAction::Image => {
                let alt = self
                    .editor_state
                    .as_ref()
                    .and_then(EditorState::selected_text)
                    .filter(|text| !text.is_empty())
                    .unwrap_or("image")
                    .to_string();
                self.editor_message = Some(
                    "Inserted an image placeholder; edit its path in Source mode".to_string(),
                );
                EditorCommand::ApplyFormat(FormatCommand::Image {
                    url: "path/to/image.png".to_string(),
                    alt,
                    title: None,
                })
            }
            tundra_ui::EditorToolbarAction::Find | tundra_ui::EditorToolbarAction::More => {
                self.editor_message = Some("Use Source mode for this operation".to_string());
                return;
            }
        };
        self.apply_editor_command(command, platform);
    }

    fn editor_hit_targets_at(
        &self,
        coordinates: CellPosition,
    ) -> (
        Option<tundra_ui::EditorHitTarget>,
        Option<tundra_ui::EditorCanvasHit>,
    ) {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let editor_area = match tundra_ui::compute_shell_layout(area) {
            tundra_ui::ShellLayout::Compact(compact) => compact,
            tundra_ui::ShellLayout::Full { main, .. } => main,
        };
        let layout = tundra_ui::editor_layout(editor_area, &self.to_editor_view_model());
        (
            layout.hit_test(coordinates.0, coordinates.1),
            layout.hit_test_source(coordinates.0, coordinates.1),
        )
    }

    fn scroll_editor_to(&mut self, coordinates: CellPosition) {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let editor_area = match tundra_ui::compute_shell_layout(area) {
            tundra_ui::ShellLayout::Compact(compact) => compact,
            tundra_ui::ShellLayout::Full { main, .. } => main,
        };
        let layout = tundra_ui::editor_layout(editor_area, &self.to_editor_view_model());
        let Some(scrollbar) = layout.vertical_scrollbar else {
            return;
        };
        let offset = coordinates.1.saturating_sub(scrollbar.track.y) as usize;
        let denominator = usize::from(scrollbar.track.height.saturating_sub(1)).max(1);
        let maximum = layout
            .document_line_count
            .saturating_sub(layout.visible_capacity);
        if let Some(state) = self.editor_state.as_mut() {
            state.viewport.top_line = maximum.saturating_mul(offset) / denominator;
        }
    }

    fn open_editor_path(&mut self, path: std::path::PathBuf) -> bool {
        let replacing_dirty = self
            .editor_state
            .as_ref()
            .is_some_and(EditorState::is_dirty);
        if replacing_dirty && !self.editor_discard_for_open {
            self.report_editor_error(
                "The current document has unsaved changes. Use Open in the Editor and choose Save or Discard first.",
            );
            return false;
        }
        if !self.authorize_editor_file(PermissionAction::ReadFile, &path) {
            return false;
        }
        let resource = path.display().to_string();
        let loaded = match tundra_platform::read_document_bytes(&path) {
            Ok(loaded) => loaded,
            Err(error) => {
                self.record_editor_file_audit(
                    PermissionAction::ReadFile,
                    &path,
                    AuditOutcome::Failure,
                    Some("read_failed"),
                );
                self.report_editor_error(format!("Could not open {resource}: {error}"));
                return false;
            }
        };
        let state = match EditorState::open(path.clone(), &loaded.bytes) {
            Ok(state) => state,
            Err(error) => {
                self.record_editor_file_audit(
                    PermissionAction::ReadFile,
                    &path,
                    AuditOutcome::Failure,
                    Some("invalid_utf8"),
                );
                self.report_editor_error(format!("Could not open {resource}: {error}"));
                return false;
            }
        };
        self.record_editor_file_audit(
            PermissionAction::ReadFile,
            &path,
            AuditOutcome::Success,
            Some("editor_open"),
        );
        if replacing_dirty {
            self.clear_editor_recovery();
        }
        self.editor_state = Some(state);
        self.editor_fingerprint = Some(loaded.fingerprint);
        self.editor_focus = tundra_ui::EditorFocus::Canvas;
        self.editor_open_menu = None;
        self.editor_selected_toolbar_action = None;
        self.editor_drag_anchor = None;
        self.editor_message = Some(format!("Opened {resource}"));
        self.editor_recovery_dirty_since = None;
        self.editor_last_recovery_write = None;
        self.editor_open_after_save = false;
        self.editor_discard_for_open = false;

        if self.active_screen() == ShellScreen::Explorer
            && matches!(self.explorer_purpose, ExplorerPurpose::EditorOpen)
        {
            self.screen_stack.pop();
            self.explorer_purpose = ExplorerPurpose::Browse;
            self.explorer_state = None;
        } else if self.active_screen() != ShellScreen::Editor {
            self.screen_stack.push(ShellScreen::Editor);
        }
        self.focused_component = ShellComponent::Editor;
        self.notify_status(format!("Editor: {resource}"));
        self.refresh_hit_map();
        true
    }

    fn confirm_editor_open(&mut self) {
        self.notify_modal_with_options(
            ShellNotification::modal(
                "Unsaved document",
                "Save your changes before opening another document?",
                tundra_ui::NotificationTone::Warning,
                vec![
                    ShellNotificationAction::new("save", "Save")
                        .with_follow_up(ShellCommand::EditorSaveAndOpen),
                    ShellNotificationAction::new("discard", "Discard")
                        .with_follow_up(ShellCommand::EditorDiscardAndOpen),
                    ShellNotificationAction::new("cancel", "Cancel")
                        .cancel()
                        .with_follow_up(ShellCommand::EditorCancelOpen),
                ],
            )
            .with_key(EDITOR_OPEN_NOTIFICATION_KEY)
            .with_component(ShellComponent::NotificationDialog),
        );
    }

    fn open_editor_picker(&mut self, platform: &dyn Platform) {
        self.open_explorer(platform);
        if self.active_screen() == ShellScreen::Explorer {
            self.explorer_purpose = ExplorerPurpose::EditorOpen;
            self.notify_status("Choose a Markdown or text document");
        } else {
            self.report_editor_error("Could not open the file picker");
        }
    }

    fn open_editor_save_picker(
        &mut self,
        platform: &dyn Platform,
        suggested_name: String,
        contents: Vec<u8>,
    ) {
        self.open_explorer(platform);
        if self.active_screen() != ShellScreen::Explorer {
            self.editor_close_after_save = false;
            self.report_editor_error("Could not open the Save As picker");
            return;
        }
        self.explorer_purpose = ExplorerPurpose::EditorSaveAs { contents };
        self.begin_explorer_input(ExplorerInputMode::NewTextFile);
        self.explorer_input = suggested_name;
        self.explorer_input_replace_all = true;
        self.notify_status("Save As: enter a file name in the current directory");
    }

    fn submit_editor_save_as_from_explorer(&mut self, platform: &dyn Platform) -> bool {
        let ExplorerPurpose::EditorSaveAs { contents, .. } = self.explorer_purpose.clone() else {
            return false;
        };
        if self.explorer_input_mode != ExplorerInputMode::NewTextFile {
            return false;
        }
        let name = self.explorer_input.trim();
        let name_path = std::path::Path::new(name);
        let valid_name = !name.is_empty()
            && !name_path.is_absolute()
            && name_path.components().count() == 1
            && matches!(
                name_path.components().next(),
                Some(std::path::Component::Normal(_))
            );
        if !valid_name {
            if let Some(state) = self.explorer_state.as_mut() {
                state.error = Some("Enter a single file name without path separators".to_string());
                state.message = None;
            }
            return true;
        }
        let Some(directory) = self
            .explorer_state
            .as_ref()
            .map(|state| state.current_path.clone())
        else {
            self.report_editor_error("Save As destination is unavailable");
            return true;
        };
        let path = directory.join(name);
        if self.save_editor_document(path, contents, platform)
            && self.active_screen() == ShellScreen::Explorer
            && !matches!(self.explorer_purpose, ExplorerPurpose::EditorOpen)
        {
            self.return_from_editor_picker();
        }
        true
    }

    fn return_from_editor_picker(&mut self) {
        if self.active_screen() == ShellScreen::Explorer {
            self.screen_stack.pop();
        }
        if self.editor_state.is_some() && self.active_screen() != ShellScreen::Editor {
            self.screen_stack.push(ShellScreen::Editor);
        }
        self.explorer_purpose = ExplorerPurpose::Browse;
        self.explorer_state = None;
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.explorer_input_replace_all = false;
        self.editor_discard_for_open = false;
        self.focused_component = ShellComponent::Editor;
        self.notify_status("Editor");
        self.refresh_hit_map();
    }

    fn save_editor_document(
        &mut self,
        path: std::path::PathBuf,
        contents: Vec<u8>,
        platform: &dyn Platform,
    ) -> bool {
        if !self.authorize_editor_file(PermissionAction::WriteFile, &path) {
            self.editor_close_after_save = false;
            self.editor_open_after_save = false;
            return false;
        }
        let is_current_path = self
            .editor_state
            .as_ref()
            .and_then(|state| state.document.path.as_ref())
            == Some(&path);
        let expected = is_current_path.then_some(self.editor_fingerprint).flatten();
        match tundra_platform::atomic_write_document_if_unchanged(&path, &contents, expected) {
            Ok(fingerprint) => {
                if let Some(state) = self.editor_state.as_mut() {
                    state.apply(tundra_apps::editor::EditorCommand::MarkSaved {
                        path: Some(path.clone()),
                    });
                }
                self.editor_fingerprint = Some(fingerprint);
                self.clear_editor_recovery();
                self.record_editor_file_audit(
                    PermissionAction::WriteFile,
                    &path,
                    AuditOutcome::Success,
                    Some("editor_save"),
                );
                self.error_message = None;
                self.resolve_notification_alert(EDITOR_ALERT_KEY);
                self.editor_message = Some(format!("Saved {}", path.display()));
                self.notify_toast(format!("Saved {}", path.display()));
                let close_after_save = std::mem::take(&mut self.editor_close_after_save);
                let open_after_save = std::mem::take(&mut self.editor_open_after_save);
                if close_after_save {
                    self.finish_editor_close(false);
                } else if open_after_save {
                    self.continue_editor_open_after_save(platform);
                }
                true
            }
            Err(tundra_platform::DocumentWriteError::ExternalModification { .. }) => {
                self.editor_close_after_save = false;
                self.editor_open_after_save = false;
                self.record_editor_file_audit(
                    PermissionAction::WriteFile,
                    &path,
                    AuditOutcome::Failure,
                    Some("external_modification"),
                );
                self.report_editor_error(
                    "The file changed outside the Editor. Use Save As or reload it before saving.",
                );
                false
            }
            Err(error) => {
                self.editor_close_after_save = false;
                self.editor_open_after_save = false;
                self.record_editor_file_audit(
                    PermissionAction::WriteFile,
                    &path,
                    AuditOutcome::Failure,
                    Some("write_failed"),
                );
                self.report_editor_error(format!("Could not save {}: {error}", path.display()));
                false
            }
        }
    }

    fn continue_editor_open_after_save(&mut self, platform: &dyn Platform) {
        if self.active_screen() == ShellScreen::Explorer {
            self.explorer_purpose = ExplorerPurpose::EditorOpen;
            self.explorer_input_mode = ExplorerInputMode::Browse;
            self.explorer_input.clear();
            self.explorer_input_replace_all = false;
            self.explorer_overlay_mode = None;
            self.focused_component = ShellComponent::Explorer;
            self.notify_status("Choose a Markdown or text document");
            self.apply_explorer_command(ExplorerCommand::Refresh, platform);
            self.refresh_hit_map();
        } else {
            self.open_editor_picker(platform);
        }
    }

    fn authorize_editor_file(
        &mut self,
        action: PermissionAction,
        path: &std::path::Path,
    ) -> bool {
        if self.storage_manager.is_none() {
            return true;
        }
        let authorization = PermissionService::new(self.debug_policy).authorize(
            self.auth_session.as_ref(),
            action,
            Some(path.display().to_string().as_str()),
        );
        if authorization.allowed {
            return true;
        }
        let reason = authorization
            .reason
            .unwrap_or_else(|| "permission_denied".to_string());
        self.record_editor_file_audit(
            action,
            path,
            AuditOutcome::Denied,
            Some(reason.as_str()),
        );
        self.report_editor_error(format!("Permission denied: {reason}"));
        false
    }

    fn record_editor_file_audit(
        &self,
        action: PermissionAction,
        path: &std::path::Path,
        outcome: AuditOutcome,
        reason: Option<&str>,
    ) {
        let Some(storage) = self.storage_manager.clone() else {
            return;
        };
        let resource = path.display().to_string();
        let _ = AuditService::with_permission_service(
            storage,
            PermissionService::new(self.debug_policy),
        )
        .record(
            self.auth_session.as_ref(),
            action,
            Some(resource.as_str()),
            outcome,
            reason,
        );
    }

    fn restore_editor_recovery_if_present(&mut self) {
        let Some((app_paths, user_key)) = self.editor_recovery_context() else {
            return;
        };
        let record = match tundra_apps::editor_recovery::read_editor_recovery(
            &app_paths,
            user_key.as_str(),
        ) {
            Ok(Some(record)) => record,
            Ok(None) => return,
            Err(error) => {
                self.report_editor_error(format!("Could not read the Editor recovery: {error}"));
                return;
            }
        };
        let kind = if record.markdown {
            tundra_apps::editor::DocumentKind::Markdown
        } else {
            tundra_apps::editor::DocumentKind::PlainText
        };
        let mut fingerprint = None;
        let mut state = record
            .path
            .as_ref()
            .and_then(|path| {
                let loaded = tundra_platform::read_document_bytes(path).ok()?;
                if record.saved_content_hash != Some(loaded.fingerprint.content_hash) {
                    return None;
                }
                fingerprint = Some(loaded.fingerprint);
                EditorState::open(path.clone(), &loaded.bytes).ok()
            })
            .unwrap_or_else(|| EditorState::untitled(kind));
        let source_len = state.document.source().len();
        state.replace_source_range(
            tundra_apps::editor::SourceRange::new(0, source_len),
            record.source.as_str(),
        );
        state.apply(tundra_apps::editor::EditorCommand::MoveTo {
            byte_offset: record.cursor,
            extend_selection: false,
        });
        state.mode = if record.source_mode {
            tundra_apps::editor::EditorMode::Source
        } else {
            kind.initial_mode()
        };
        self.editor_state = Some(state);
        self.editor_fingerprint = fingerprint;
        self.editor_recovery_dirty_since = Some(Instant::now());
        self.editor_message = Some("Recovered an unsaved document".to_string());
        self.notify_toast("Recovered an unsaved Editor document");
    }

    fn persist_editor_recovery_if_due(&mut self, now: Instant) {
        if self
            .editor_state
            .as_ref()
            .is_none_or(|state| !state.is_dirty())
        {
            return;
        }
        let Some(dirty_since) = self.editor_recovery_dirty_since else {
            self.editor_recovery_dirty_since = Some(now);
            return;
        };
        if now.saturating_duration_since(dirty_since) < EDITOR_RECOVERY_IDLE
            || self.editor_last_recovery_write.is_some_and(|last| {
                now.saturating_duration_since(last) < EDITOR_RECOVERY_INTERVAL
            })
        {
            return;
        }
        let _ = self.persist_editor_recovery_now(now);
    }

    /// Writes the current dirty buffer without debounce. Interactive exit and
    /// logout paths use the return value to avoid destroying the only copy of
    /// unsaved text when recovery storage is unavailable.
    fn persist_editor_recovery_now(&mut self, now: Instant) -> bool {
        let Some(state) = self.editor_state.as_ref() else {
            return true;
        };
        if !state.is_dirty() {
            return true;
        }
        let Some((app_paths, user_key)) = self.editor_recovery_context() else {
            // Storage-free/debug shells do not have a durable per-user context.
            return true;
        };
        let mut record = tundra_apps::editor_recovery::EditorRecoveryRecord::new(
            state.document.source(),
        );
        record.path = state.document.path.clone();
        record.markdown = state.document.kind == tundra_apps::editor::DocumentKind::Markdown;
        record.source_mode = state.mode == tundra_apps::editor::EditorMode::Source;
        record.cursor = state.cursor.byte_offset;
        record.saved_content_hash = self.editor_fingerprint.map(|value| value.content_hash);
        match tundra_apps::editor_recovery::write_editor_recovery(
            &app_paths,
            user_key.as_str(),
            &record,
        ) {
            Ok(()) => {
                self.editor_last_recovery_write = Some(now);
                true
            }
            Err(error) => {
                self.report_editor_error(format!("Could not save recovery: {error}"));
                false
            }
        }
    }

    fn editor_recovery_context(&self) -> Option<(tundra_platform::AppPaths, String)> {
        let storage = self.storage_manager.as_ref()?;
        let user_key = self.auth_session.as_ref()?.user_id.clone();
        let app_paths = app_paths_from_storage_layout(storage.layout()).ok()?;
        Some((app_paths, user_key))
    }

    fn finish_editor_close(&mut self, _discard: bool) {
        self.notifications
            .dismiss_modal_by_key(EDITOR_CLOSE_NOTIFICATION_KEY);
        self.clear_editor_recovery();
        self.editor_state = None;
        self.editor_fingerprint = None;
        self.editor_open_menu = None;
        self.editor_selected_toolbar_action = None;
        self.editor_drag_anchor = None;
        self.editor_close_after_save = false;
        self.editor_open_after_save = false;
        self.editor_discard_for_open = false;
        self.editor_message = None;
        self.pop_to_home();
        self.notify_status("Ready");
    }

    fn report_editor_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.editor_message = Some(message.clone());
        self.error_message = Some(message.clone());
        self.notify_alert_with_key(
            EDITOR_ALERT_KEY,
            message,
            tundra_ui::NotificationTone::Error,
        );
    }

    fn clear_editor_recovery(&mut self) {
        if let Some((app_paths, user_key)) = self.editor_recovery_context()
            && let Err(error) = tundra_apps::editor_recovery::clear_editor_recovery(
                &app_paths,
                user_key.as_str(),
            )
        {
            self.editor_message = Some(format!("Could not clear recovery: {error}"));
        }
        self.editor_recovery_dirty_since = None;
        self.editor_last_recovery_write = None;
    }
}
