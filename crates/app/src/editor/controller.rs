use super::*;

impl EditorController {
    pub fn apply(self, state: &mut EditorState, command: EditorCommand) -> Vec<EditorEffect> {
        if state.is_read_only() && !command.is_allowed_in_read_only() {
            return Vec::new();
        }
        match command {
            EditorCommand::SetEncoding { encoding, utf8_bom } => {
                let mut metadata = state.document.metadata;
                metadata.encoding = encoding;
                metadata.utf8_bom = encoding == TextEncoding::Utf8 && utf8_bom;
                commit_metadata(state, metadata);
                Vec::new()
            }
            EditorCommand::Indent {
                width,
                tabs,
                outdent,
            } => {
                indent_source(state, width.clamp(1, 8), tabs, outdent);
                Vec::new()
            }
            EditorCommand::DeleteWord { backwards } => {
                if !state.has_selection() {
                    move_cursor(
                        state,
                        if backwards {
                            CursorMove::WordLeft
                        } else {
                            CursorMove::WordRight
                        },
                        true,
                    );
                }
                apply_delete_selection(state);
                Vec::new()
            }
            EditorCommand::SetUtf8Bom(bom) => {
                let mut metadata = state.document.metadata;
                metadata.utf8_bom = bom;
                commit_metadata(state, metadata);
                Vec::new()
            }
            EditorCommand::NormalizeLineEndings(ending) => {
                if state.mode != EditorMode::Source {
                    return Vec::new();
                }
                let position = state.source_display_position(state.cursor.byte_offset);
                let selection = state.selection.and_then(|selection| {
                    Some((
                        state.source_display_position(selection.anchor)?,
                        state.source_display_position(selection.focus)?,
                    ))
                });
                let original = state.source();
                let text = original
                    .replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .replace('\n', ending.as_str());
                let changed = text != original;
                let len = original.len();
                drop(original);
                if changed {
                    state.commit_source_edit(
                        0..len,
                        &text,
                        Cursor::default(),
                        None,
                        EditKind::Format,
                    );
                }
                let mut metadata = state.document.metadata;
                metadata.preferred_line_ending = ending;
                if changed {
                    state.document.metadata = metadata;
                    if let Some((line, column)) = position {
                        state.cursor.byte_offset = state.source_offset(line, column).unwrap_or(0);
                    }
                    state.selection = selection.and_then(|((al, ac), (fl, fc))| {
                        Some(Selection::new(
                            state.source_offset(al, ac)?,
                            state.source_offset(fl, fc)?,
                        ))
                    });
                    if let Some(EditTransaction::Source {
                        after_metadata,
                        after_cursor,
                        after_selection,
                        ..
                    }) = state.undo_stack.last_mut()
                    {
                        *after_metadata = metadata;
                        *after_cursor = state.cursor;
                        *after_selection = state.selection;
                    }
                } else {
                    commit_metadata(state, metadata);
                }
                Vec::new()
            }
            EditorCommand::Find {
                query,
                options,
                backwards,
            } => {
                return find(state, query, options, backwards, false);
            }
            EditorCommand::FindNext { backwards } => {
                if let Some(search) = &state.search {
                    return find(state, search.query.clone(), search.options, backwards, true);
                }
                Vec::new()
            }
            EditorCommand::ReplaceMatches {
                revision,
                replacements,
            } => {
                return replace_matches(state, revision, replacements);
            }
            EditorCommand::FormatJson => {
                if state.mode != EditorMode::Source {
                    return Vec::new();
                }
                match json_format::format_json(
                    &state.source(),
                    state.document.metadata.preferred_line_ending.as_str(),
                ) {
                    Ok(formatted) => {
                        let end = state.source().len();
                        state.commit_source_edit(
                            0..end,
                            &formatted,
                            Cursor::default(),
                            None,
                            EditKind::Format,
                        );
                    }
                    Err(error) => return vec![EditorEffect::Message(error.to_string())],
                }
                Vec::new()
            }
            EditorCommand::MoveVisual {
                width,
                down,
                extend_selection,
            } => {
                if let Some(layout) = state.source_wrap_layout(width) {
                    let (line, column) = layout.position(state.cursor.byte_offset);
                    let desired = state.cursor.preferred_column.unwrap_or(column);
                    let target = if down {
                        (line + 1).min(layout.row_count() - 1)
                    } else {
                        line.saturating_sub(1)
                    };
                    if target != line {
                        move_to(state, layout.offset(target, desired), extend_selection);
                        state.cursor.preferred_column = Some(desired);
                    }
                }
                Vec::new()
            }
            EditorCommand::InsertText(text) | EditorCommand::Paste(text) => {
                apply_insert_text(state, &text);
                Vec::new()
            }
            EditorCommand::InsertNewline => {
                match &state.buffer {
                    EditorBuffer::Rich(_) => {
                        apply_insert_newline(state);
                    }
                    EditorBuffer::Source(_) => {
                        let newline = state.document.metadata.preferred_line_ending.as_str();
                        apply_insert_text(state, newline);
                    }
                };
                Vec::new()
            }
            EditorCommand::Backspace => {
                apply_backspace(state);
                Vec::new()
            }
            EditorCommand::DeleteForward => {
                apply_delete_forward(state);
                Vec::new()
            }
            EditorCommand::DeleteSelection => {
                apply_delete_selection(state);
                Vec::new()
            }
            EditorCommand::MoveCursor {
                movement,
                extend_selection,
            } => {
                match &mut state.buffer {
                    EditorBuffer::Rich(buffer) => {
                        buffer.editor.move_cursor(movement, extend_selection);
                    }
                    EditorBuffer::Source(_) => move_cursor(state, movement, extend_selection),
                }
                Vec::new()
            }
            EditorCommand::MoveTo {
                position,
                extend_selection,
            } => {
                match (&mut state.buffer, position) {
                    (EditorBuffer::Rich(buffer), EditorPosition::Rich(position)) => {
                        buffer.editor.move_to(position, extend_selection);
                    }
                    (EditorBuffer::Source(_), EditorPosition::Source(byte_offset)) => {
                        move_to(state, byte_offset, extend_selection);
                    }
                    _ => {}
                }
                Vec::new()
            }
            EditorCommand::SelectAll => {
                match &mut state.buffer {
                    EditorBuffer::Rich(buffer) => buffer.editor.select_all(),
                    EditorBuffer::Source(buffer) => {
                        let end = buffer.len_bytes();
                        state.selection = (end > 0).then_some(Selection::new(0, end));
                        state.cursor.byte_offset = end;
                        state.cursor.preferred_column = None;
                    }
                }
                Vec::new()
            }
            EditorCommand::ClearSelection => {
                match &mut state.buffer {
                    EditorBuffer::Rich(buffer) => buffer.editor.clear_selection(),
                    EditorBuffer::Source(_) => state.selection = None,
                }
                Vec::new()
            }
            EditorCommand::Undo => {
                undo(state);
                Vec::new()
            }
            EditorCommand::Redo => {
                redo(state);
                Vec::new()
            }
            EditorCommand::ApplyFormat(format) => {
                if state.mode == EditorMode::Rich && state.document.kind == DocumentKind::Markdown {
                    let before = state.snapshot();
                    if let EditorBuffer::Rich(buffer) = &mut state.buffer {
                        buffer.editor.apply_format(&format);
                    }
                    state.commit_edit(before, EditKind::Format);
                }
                Vec::new()
            }
            EditorCommand::EditTableColumn {
                table_id,
                edge,
                edit,
            } => {
                if state.mode == EditorMode::Rich && state.document.kind == DocumentKind::Markdown {
                    let before = state.snapshot();
                    if let EditorBuffer::Rich(buffer) = &mut state.buffer {
                        buffer.editor.edit_table_column(table_id, edge, edit);
                    }
                    state.commit_edit(before, EditKind::Format);
                }
                Vec::new()
            }
            EditorCommand::SetMode(mode) => {
                if state.document.kind == DocumentKind::Markdown || mode == EditorMode::Source {
                    state.set_mode(mode);
                }
                Vec::new()
            }
            EditorCommand::ToggleMode => {
                if state.document.kind == DocumentKind::Markdown {
                    let mode = match state.mode {
                        EditorMode::Rich => EditorMode::Source,
                        EditorMode::Source => EditorMode::Rich,
                    };
                    state.set_mode(mode);
                }
                Vec::new()
            }
            EditorCommand::Copy => state
                .selected_text()
                .map(|text| vec![EditorEffect::WriteClipboard(text)])
                .unwrap_or_default(),
            EditorCommand::Cut => {
                let Some(text) = state.selected_text() else {
                    return Vec::new();
                };
                apply_delete_selection(state);
                vec![EditorEffect::WriteClipboard(text)]
            }
            EditorCommand::RequestPaste => vec![EditorEffect::ReadClipboard],
            EditorCommand::RequestOpen => vec![EditorEffect::OpenFilePicker],
            EditorCommand::RequestSave => {
                let snapshot = state.prepare_save_snapshot();
                if let Some(path) = &state.document.path {
                    vec![EditorEffect::SaveFile {
                        path: path.clone(),
                        snapshot,
                    }]
                } else {
                    vec![EditorEffect::SaveFilePicker {
                        suggested_name: state.document.display_name(),
                        snapshot,
                    }]
                }
            }
            EditorCommand::RequestSaveAs => {
                let snapshot = state.prepare_save_snapshot();
                vec![EditorEffect::SaveFilePicker {
                    suggested_name: state.document.display_name(),
                    snapshot,
                }]
            }
            EditorCommand::RequestClose => {
                if state.is_dirty() {
                    vec![EditorEffect::ConfirmClose]
                } else {
                    vec![EditorEffect::Close]
                }
            }
            EditorCommand::ReplaceDocument(document) => {
                *state = EditorState::from_document(document);
                Vec::new()
            }
            EditorCommand::MarkSaved { path, revision } => {
                state.mark_saved(path, revision);
                Vec::new()
            }
        }
    }
}

fn find(
    state: &mut EditorState,
    query: String,
    options: search::SearchOptions,
    backwards: bool,
    advance: bool,
) -> Vec<EditorEffect> {
    if state.mode != EditorMode::Source {
        return Vec::new();
    }
    let engine = match search::SearchEngine::new(&query, options) {
        Ok(engine) => engine,
        Err(error) => return vec![EditorEffect::Message(error.to_string())],
    };
    let cached = state.search.as_ref().filter(|s| {
        s.query == query && s.options == options && s.revision == state.revision() && !s.pending
    });
    let previous = cached
        .and_then(|s| s.current.map(|i| (s, i)))
        .filter(|(s, i)| {
            s.matches
                .get(*i)
                .is_some_and(|r| r.end == state.cursor.byte_offset)
        });
    let target = if advance && let Some((session, previous)) = previous {
        let index = if backwards {
            (session.first_match + previous + session.total_matches - 1) % session.total_matches
        } else {
            (session.first_match + previous + 1) % session.total_matches
        };
        if let Some(local) = index
            .checked_sub(session.first_match)
            .filter(|&i| i < session.matches.len())
        {
            let range = session.matches[local].clone();
            move_to(state, range.start, false);
            move_to(state, range.end, true);
            state.search.as_mut().unwrap().current = Some(local);
            return Vec::new();
        }
        search::Target::Index(index)
    } else if backwards {
        search::Target::Backward(state.cursor.byte_offset)
    } else {
        search::Target::Forward(state.cursor.byte_offset)
    };
    let source = state.source_rope_snapshot().unwrap();
    let revision = state.revision();
    let cursor = state.cursor.byte_offset;
    state.search_job.cancel();
    state.search = Some(SearchSession {
        query: query.clone(),
        options,
        revision,
        matches: Arc::from([]),
        current: None,
        first_match: 0,
        total_matches: 0,
        pending: true,
    });
    if source.len_bytes() <= background::SYNC_BYTES {
        let page = engine.page(&source, target, &std::sync::atomic::AtomicBool::new(false));
        finish_search(
            state,
            search::Completion {
                revision,
                cursor,
                query,
                options,
                page,
            },
        );
    } else {
        state.search_job.submit(move |cancel| {
            let page = engine.page(&source, target, cancel);
            search::Completion {
                revision,
                cursor,
                query,
                options,
                page,
            }
        });
    }
    Vec::new()
}

pub(super) fn finish_search(state: &mut EditorState, result: search::Completion) -> bool {
    if result.revision != state.revision()
        || state.mode != EditorMode::Source
        || !state
            .search
            .as_ref()
            .is_some_and(|s| s.query == result.query && s.options == result.options)
    {
        return false;
    }
    let page = result.page;
    let current = page
        .current
        .filter(|_| state.cursor.byte_offset == result.cursor);
    if let Some(index) = current {
        move_to(state, page.matches[index].start, false);
        move_to(state, page.matches[index].end, true);
    }
    state.search = Some(SearchSession {
        query: result.query,
        options: result.options,
        revision: result.revision,
        matches: page.matches,
        current,
        first_match: page.first,
        total_matches: page.total,
        pending: false,
    });
    true
}

fn replace_matches(
    state: &mut EditorState,
    revision: u64,
    replacements: Vec<search::Replacement>,
) -> Vec<EditorEffect> {
    if state.mode != EditorMode::Source || replacements.is_empty() {
        return Vec::new();
    }
    if revision != state.revision() {
        return vec![EditorEffect::Message(i18n::tr!(
            "editor-search-stale-preview"
        ))];
    }
    let source = state.source_rope_snapshot().unwrap();
    let mut boundaries = search::Boundaries::new(source.len_bytes());
    let mut end = 0;
    for (index, edit) in replacements.iter().enumerate() {
        if !boundaries.rope_range(&source, &edit.range)
            || edit.range.start < end
            || (index > 0 && edit.range.start == replacements[index - 1].range.start)
            || !source
                .byte_slice(edit.range.clone())
                .chars()
                .eq(edit.original.chars())
        {
            return vec![EditorEffect::Message(i18n::tr!(
                "editor-search-invalid-range"
            ))];
        }
        end = edit.range.end;
    }
    // One existing Source transaction covers the affected span, so a batch is
    // atomic and a single Undo restores the original text and selection.
    let start = replacements[0].range.start;
    let mut output = String::new();
    let mut offset = start;
    for edit in replacements {
        for chunk in source.byte_slice(offset..edit.range.start).chunks() {
            output.push_str(chunk);
        }
        output.push_str(&edit.replacement);
        offset = edit.range.end;
    }
    drop(source);
    let changed = state.commit_source_edit(
        start..end,
        &output,
        Cursor {
            byte_offset: start + output.len(),
            preferred_column: None,
        },
        None,
        EditKind::Format,
    );
    // A replacement may join with a following combining character.
    state.clamp_positions();
    if changed
        && let Some(EditTransaction::Source {
            after_cursor,
            after_selection,
            ..
        }) = state.undo_stack.last_mut()
    {
        *after_cursor = state.cursor;
        *after_selection = state.selection;
    }
    Vec::new()
}

fn indent_source(state: &mut EditorState, width: usize, tabs: bool, outdent: bool) {
    let Some(source) = state.source_rope_snapshot() else {
        return;
    };
    let selection = state.selected_range().map(std::ops::Range::from);
    let range = selection
        .clone()
        .unwrap_or(state.cursor.byte_offset..state.cursor.byte_offset);
    if selection.is_none() && !outdent {
        let column = state
            .source_display_position(range.start)
            .map_or(0, |(_, column)| column);
        apply_insert_text(
            state,
            &if tabs {
                "\t".into()
            } else {
                " ".repeat(width - column % width)
            },
        );
        return;
    }
    let first = source.byte_to_line(range.start);
    let mut last = source.byte_to_line(range.end);
    if last > first && source.line_to_byte(last) == range.end {
        last -= 1;
    }
    let start = source.line_to_byte(first);
    let end = if last + 1 < source.len_lines() {
        source.line_to_byte(last + 1)
    } else {
        source.len_bytes()
    };
    let mut output = String::new();
    let mut cursor_delta = 0usize;
    for line in first..=last {
        let text = source.line(line).to_string();
        if outdent {
            let remove = if text.starts_with('\t') {
                1
            } else {
                text.bytes().take(width).take_while(|b| *b == b' ').count()
            };
            if line == first {
                cursor_delta = remove;
            }
            output.push_str(&text[remove..]);
        } else {
            output.push_str(&if tabs { "\t".into() } else { " ".repeat(width) });
            output.push_str(&text);
        }
    }
    let after_cursor = if selection.is_some() {
        start + output.len()
    } else {
        state
            .cursor
            .byte_offset
            .saturating_sub(cursor_delta)
            .max(start)
    };
    let after_selection = selection.map(|_| Selection::new(start, start + output.len()));
    state.commit_source_edit(
        start..end,
        &output,
        Cursor {
            byte_offset: after_cursor,
            preferred_column: None,
        },
        after_selection,
        EditKind::Format,
    );
}

fn commit_metadata(state: &mut EditorState, metadata: TextMetadata) {
    let before = state.document.metadata;
    if before == metadata {
        return;
    }
    let before_revision = state.revision;
    state.document.metadata = metadata;
    state.revision = state.next_revision;
    state.next_revision = state.next_revision.saturating_add(1).max(1);
    state.undo_stack.push(EditTransaction::Metadata {
        before,
        after: metadata,
        before_revision,
        after_revision: state.revision,
    });
    if state.undo_stack.len() > state.history_limit {
        state.undo_stack.remove(0);
    }
    state.redo_stack.clear();
}
