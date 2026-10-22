use super::*;

#[test]
fn external_takeover_always_installs_source_mode_for_unchanged_rich_text() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        Some("note.md".into()),
        DocumentKind::Markdown,
        "# Heading\n",
    ));
    assert_eq!(editor.mode, EditorMode::Rich);
    let text = editor.export_text();
    editor.synchronize_external_source(
        &text,
        Some("note.md".into()),
        editor.document.metadata,
        2,
        false,
    );
    assert_eq!(editor.mode, EditorMode::Source);
    assert_eq!(editor.source(), text);
    assert!(editor.source_line_count().is_some());
    assert!(!editor.is_dirty());
}

#[test]
fn external_save_snapshots_preserve_arbitrary_encoded_bytes() {
    for bytes in [
        Vec::new(),
        (0..=255).collect::<Vec<u8>>(),
        b"\xef\xbb\xbftext\0\r\nlast\r".to_vec(),
        vec![0xff, 0xfe, 0x2d, 0x4e, 0, 0, 0x3d, 0xd8, 0x42, 0xde],
    ] {
        let snapshot = SaveSnapshot::external_bytes(37, bytes.clone());
        assert_eq!(snapshot.revision, 37);
        assert!(snapshot.prepared().source.is_none());
        assert!(snapshot.prepared().export.is_none());
        assert_eq!(snapshot.to_bytes().unwrap(), bytes);
        assert_eq!(snapshot.clone().to_bytes().unwrap(), bytes);
    }
}

struct LimitedWriter {
    bytes: Vec<u8>,
    maximum_chunk: usize,
    failure_after: usize,
}

impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len() == self.failure_after {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "test writer stopped",
            ));
        }
        let count = bytes
            .len()
            .min(self.maximum_chunk)
            .min(self.failure_after - self.bytes.len());
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn external_save_streams_short_writes_and_propagates_partial_failure() {
    let bytes = vec![0xff, 0, 0x80, b'\r', b'\n', 0xef, 0xbb, 0xbf, b'Z'];
    let snapshot = SaveSnapshot::external_bytes(5, bytes.clone());
    let mut complete = LimitedWriter {
        bytes: Vec::new(),
        maximum_chunk: 2,
        failure_after: usize::MAX,
    };
    snapshot.write_to(&mut complete).unwrap();
    assert_eq!(complete.bytes, bytes);

    let mut failed = LimitedWriter {
        bytes: Vec::new(),
        maximum_chunk: 3,
        failure_after: 4,
    };
    assert_eq!(
        snapshot.write_to(&mut failed).unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(failed.bytes, bytes[..4]);
    assert_eq!(snapshot.to_bytes().unwrap(), bytes);
}

#[test]
fn external_save_acknowledgement_cannot_mark_a_host_buffer_saved() {
    let mut editor = EditorState::open("host.txt", b"saved").unwrap();
    editor.apply(EditorCommand::InsertText("changed ".into()));
    let snapshot = SaveSnapshot::external_bytes(editor.revision, vec![0xff, 0, 0x80]);
    snapshot.write_to(&mut Vec::new()).unwrap();
    editor.apply(EditorCommand::MarkSaved {
        path: Some("external.bin".into()),
        revision: snapshot.revision,
    });
    assert!(editor.pending_saves.is_empty());
    assert!(editor.is_dirty());
    assert_eq!(editor.document.path.as_deref(), Some(Path::new("host.txt")));
    assert_eq!(editor.source(), "changed saved");
}

#[test]
fn external_source_mirror_tracks_metadata_cursor_and_modified_state() {
    let mut editor = EditorState::open("old.txt", b"old\n").unwrap();
    editor.apply(EditorCommand::InsertText("host edit".into()));
    editor.selection = Some(Selection::new(0, 3));
    let text = "A中🙂\r\nZ";
    let metadata = TextMetadata {
        encoding: TextEncoding::Utf16Be,
        preferred_line_ending: LineEnding::CrLf,
        ..TextMetadata::from_source(text, false)
    };
    editor.synchronize_external_source(text, Some("new.txt".into()), metadata, 5, true);
    assert_eq!(editor.source(), text);
    assert_eq!(editor.document.path.as_deref(), Some(Path::new("new.txt")));
    assert_eq!(editor.document.metadata, metadata);
    assert_eq!(editor.cursor.byte_offset, 4);
    assert!(editor.selection.is_none());
    assert!(editor.undo_stack.is_empty() && editor.redo_stack.is_empty());
    assert!(editor.pending_saves.is_empty());
    assert!(editor.is_dirty());
    let revision = editor.revision;
    editor.synchronize_external_source(text, Some("new.txt".into()), metadata, usize::MAX, true);
    assert_eq!(editor.revision, revision);
    assert_eq!(editor.cursor.byte_offset, text.len());
    editor.synchronize_external_source(text, Some("new.txt".into()), metadata, 0, false);
    assert!(!editor.is_dirty());
    assert_eq!(editor.document.metadata, metadata);
    assert_eq!(editor.to_bytes(), encoded_bytes(text, metadata));
    editor.synchronize_external_source(text, Some("new.txt".into()), metadata, 0, true);
    assert!(editor.is_dirty());
}

#[test]
fn external_source_mirror_preserves_read_only_save_boundaries() {
    let mut editor = EditorState::open_read_only("log.txt", b"old").unwrap();
    let text = "externally updated\n";
    editor.synchronize_external_source(
        text,
        Some("log.txt".into()),
        TextMetadata::from_source(text, false),
        usize::MAX,
        true,
    );
    assert!(editor.is_read_only());
    assert_eq!(editor.source(), text);
    assert_eq!(editor.cursor.byte_offset, text.len());
    assert!(editor.apply(EditorCommand::RequestSave).is_empty());
    assert!(editor.apply(EditorCommand::RequestSaveAs).is_empty());
    assert!(editor.pending_saves.is_empty());
    assert!(editor.is_dirty());
}

#[test]
fn external_source_takeover_clears_host_save_and_history_without_text_changes() {
    let mut editor = EditorState::open("host.txt", b"saved").unwrap();
    editor.apply(EditorCommand::InsertText("changed ".into()));
    let pending = editor.prepare_save_snapshot();
    assert!(!editor.pending_saves.is_empty() && !editor.undo_stack.is_empty());
    let text = editor.source().into_owned();
    editor.synchronize_external_source(
        &text,
        Some("host.txt".into()),
        editor.document.metadata,
        0,
        true,
    );
    assert!(editor.pending_saves.is_empty());
    assert!(editor.undo_stack.is_empty() && editor.redo_stack.is_empty());
    editor.apply(EditorCommand::MarkSaved {
        path: None,
        revision: pending.revision,
    });
    assert!(editor.is_dirty());
}

#[test]
fn source_positions_normalize_utf8_continuations_and_crlf_before_editing() {
    let text = "é中🙂\r\nz";
    let buffer = SourceBuffer::from_text(text);
    for (position, expected) in [
        (0, 0),
        (1, 0),
        (2, 2),
        (3, 2),
        (4, 2),
        (5, 5),
        (6, 5),
        (7, 5),
        (8, 5),
        (9, 9),
        (10, 9),
        (11, 11),
        (12, 12),
        (usize::MAX, 12),
    ] {
        assert_eq!(buffer.normalize_position(position), expected);
    }
    let mut editor = EditorState::open("unicode.txt", text.as_bytes()).unwrap();
    editor.apply(EditorCommand::MoveTo {
        position: EditorPosition::Source(7),
        extend_selection: false,
    });
    editor.apply(EditorCommand::InsertText("X".into()));
    assert_eq!(editor.source(), "é中X🙂\r\nz");
    editor.apply(EditorCommand::Undo);
    assert_eq!(editor.to_bytes(), text.as_bytes());
}
