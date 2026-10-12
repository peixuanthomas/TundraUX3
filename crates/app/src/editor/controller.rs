use super::*;

impl EditorController {
    pub fn apply(self, state: &mut EditorState, command: EditorCommand) -> Vec<EditorEffect> {
        if state.is_read_only() && !command.is_allowed_in_read_only() {
            return Vec::new();
        }
        match command {
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
                        (line + 1).min(layout.rows.len() - 1)
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
    let matches: Arc<[Range<usize>]> = state
        .search
        .as_ref()
        .filter(|s| s.query == query && s.options == options && s.revision == state.revision())
        .map_or_else(
            || engine.find_all(&state.source()).into(),
            |s| Arc::clone(&s.matches),
        );
    let previous = state
        .search
        .as_ref()
        .filter(|s| s.query == query && s.options == options && s.revision == state.revision())
        .and_then(|s| s.current)
        .filter(|&i| {
            matches
                .get(i)
                .is_some_and(|r| r.end == state.cursor.byte_offset)
        });
    let current = if matches.is_empty() {
        None
    } else if advance && let Some(previous) = previous {
        Some(if backwards {
            (previous + matches.len() - 1) % matches.len()
        } else {
            (previous + 1) % matches.len()
        })
    } else if backwards {
        Some(
            matches
                .iter()
                .rposition(|r| r.start < state.cursor.byte_offset)
                .unwrap_or(matches.len() - 1),
        )
    } else {
        Some(
            matches
                .iter()
                .position(|r| r.start >= state.cursor.byte_offset)
                .unwrap_or(0),
        )
    };
    if let Some(index) = current {
        move_to(state, matches[index].start, false);
        move_to(state, matches[index].end, true);
    }
    state.search = Some(SearchSession {
        query,
        options,
        revision: state.revision(),
        matches: matches.into(),
        current,
    });
    Vec::new()
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
    let source = state.source();
    let boundaries = search::grapheme_boundaries(&source);
    let mut end = 0;
    for (index, edit) in replacements.iter().enumerate() {
        if !search::valid_range(&boundaries, &edit.range)
            || edit.range.start < end
            || (index > 0 && edit.range.start == replacements[index - 1].range.start)
            || source.get(edit.range.clone()) != Some(edit.original.as_str())
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
        output.push_str(&source[offset..edit.range.start]);
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
