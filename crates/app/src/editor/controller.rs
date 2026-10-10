use super::*;

impl EditorController {
    pub fn apply(self, state: &mut EditorState, command: EditorCommand) -> Vec<EditorEffect> {
        if state.is_read_only() && !command.is_allowed_in_read_only() {
            return Vec::new();
        }
        match command {
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
