use super::*;

impl EditorState {
    pub fn new() -> Self {
        Self::untitled(DocumentKind::PlainText)
    }

    pub fn untitled(kind: DocumentKind) -> Self {
        Self::from_document(EditorDocument::untitled(kind))
    }

    pub fn open(path: impl Into<PathBuf>, bytes: &[u8]) -> Result<Self, DocumentDecodeError> {
        Ok(Self::from_document(EditorDocument::open(path, bytes)?))
    }

    pub fn open_owned(
        path: impl Into<PathBuf>,
        bytes: Vec<u8>,
    ) -> Result<Self, DocumentDecodeError> {
        Ok(Self::from_document(EditorDocument::open_owned(
            path, bytes,
        )?))
    }

    pub fn open_read_only(
        path: impl Into<PathBuf>,
        bytes: &[u8],
    ) -> Result<Self, DocumentDecodeError> {
        Ok(Self::from_read_only_document(EditorDocument::open(
            path, bytes,
        )?))
    }

    pub fn open_read_only_owned(
        path: impl Into<PathBuf>,
        bytes: Vec<u8>,
    ) -> Result<Self, DocumentDecodeError> {
        Ok(Self::from_read_only_document(EditorDocument::open_owned(
            path, bytes,
        )?))
    }

    pub fn from_document(document: EditorDocument) -> Self {
        Self::from_document_with_access(document, EditorAccess::Editable)
    }

    pub fn from_read_only_document(document: EditorDocument) -> Self {
        Self::from_document_with_access(document, EditorAccess::ReadOnly)
    }

    pub(super) fn from_document_with_access(
        document: EditorDocument,
        access: EditorAccess,
    ) -> Self {
        let mode = document.kind.initial_mode();
        let buffer = match mode {
            EditorMode::Rich => EditorBuffer::Rich(RichBuffer {
                editor: RichEditor::new(import_rich_document(&document)),
            }),
            EditorMode::Source => {
                let buffer = document
                    .source
                    .rope()
                    .map(|(text, word_count, line_endings)| {
                        SourceBuffer::from_rope(text, word_count, line_endings)
                    })
                    .unwrap_or_else(|| SourceBuffer::from_text(document.source()));
                EditorBuffer::Source(buffer)
            }
        };
        Self {
            document,
            mode,
            access,
            cursor: Cursor::default(),
            selection: None,
            viewport: EditorViewport::default(),
            buffer,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            history_limit: DEFAULT_HISTORY_LIMIT,
            revision: 0,
            saved_revision: 0,
            next_revision: 1,
            source_session: None,
            pending_saves: Vec::new(),
            rich_word_count_cache: RichWordCountCache::default(),
            line_change_cache: line_changes::Cache::default(),
        }
    }

    pub const fn access(&self) -> EditorAccess {
        self.access
    }

    pub const fn is_read_only(&self) -> bool {
        matches!(self.access, EditorAccess::ReadOnly)
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// Compare the current source with the exact snapshot last saved successfully.
    /// Cursor motion and scrolling reuse the cached result; clean files need no scan.
    pub fn source_line_markers(&self) -> Arc<[line_changes::LineMarker]> {
        let EditorBuffer::Source(buffer) = &self.buffer else {
            return Arc::from([]);
        };
        if !self.is_dirty() {
            return Arc::from([]);
        }
        self.line_change_cache
            .get_or_compute((self.revision, self.saved_revision), || {
                fn lines(rope: &Rope) -> Vec<Cow<'_, str>> {
                    rope.lines().map(Cow::from).collect()
                }
                let old_rope;
                let old = if let Some((rope, _, _)) = self.document.saved_source.rope() {
                    rope
                } else {
                    old_rope = Rope::from_str(&self.document.saved_source.text());
                    &old_rope
                };
                let old_lines = if old.len_bytes() == 0 {
                    Vec::new()
                } else {
                    lines(old)
                };
                let new_lines = if buffer.text.len_bytes() == 0 {
                    Vec::new()
                } else {
                    lines(&buffer.text)
                };
                line_changes::compare(&old_lines, &new_lines)
            })
    }

    /// Source-mode text, or the last boundary Markdown snapshot in Rich mode.
    /// Rich input never refreshes this string.
    pub fn source(&self) -> Cow<'_, str> {
        match &self.buffer {
            EditorBuffer::Rich(_) => self.document.source(),
            EditorBuffer::Source(buffer) => buffer.text(),
        }
    }

    pub fn has_isolated_rich_buffer(&self) -> bool {
        matches!(self.buffer, EditorBuffer::Rich(_))
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn saved_revision(&self) -> u64 {
        self.saved_revision
    }

    pub fn position(&self) -> Option<EditorPosition> {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => buffer.editor.cursor.map(EditorPosition::Rich),
            EditorBuffer::Source(_) => Some(EditorPosition::Source(self.cursor.byte_offset)),
        }
    }

    pub fn rich_document(&self) -> Option<&RichDocument> {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => Some(&buffer.editor.document),
            EditorBuffer::Source(_) => None,
        }
    }

    pub fn rich_cursor(&self) -> Option<RichPosition> {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => buffer.editor.cursor,
            EditorBuffer::Source(_) => None,
        }
    }

    pub fn rich_selection(&self) -> Option<RichSelection> {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => buffer.editor.selection,
            EditorBuffer::Source(_) => None,
        }
    }

    pub fn can_apply_block_format_to_selection(&self) -> bool {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => buffer.editor.can_apply_block_format_to_selection(),
            EditorBuffer::Source(_) => false,
        }
    }

    pub fn source_buffer(&self) -> Option<Cow<'_, str>> {
        match &self.buffer {
            EditorBuffer::Source(buffer) => Some(buffer.text()),
            EditorBuffer::Rich(_) => None,
        }
    }

    pub fn source_len_bytes(&self) -> Option<usize> {
        match &self.buffer {
            EditorBuffer::Source(buffer) => Some(buffer.len_bytes()),
            EditorBuffer::Rich(_) => None,
        }
    }

    pub fn source_line_count(&self) -> Option<usize> {
        match &self.buffer {
            EditorBuffer::Source(buffer) => Some(buffer.line_count()),
            EditorBuffer::Rich(_) => None,
        }
    }

    /// Returns the widest Source line in terminal display cells. Line
    /// terminators and the virtual cell used to draw a caret at line end are
    /// not included.
    pub fn source_max_display_width(&self) -> Option<usize> {
        match &self.buffer {
            EditorBuffer::Source(buffer) => Some(buffer.max_display_width()),
            EditorBuffer::Rich(_) => None,
        }
    }

    pub fn source_line_range(&self, line_index: usize) -> Option<SourceRange> {
        match &self.buffer {
            EditorBuffer::Source(buffer) => buffer.line_range(line_index),
            EditorBuffer::Rich(_) => None,
        }
    }

    pub fn source_lines(&self, line_range: Range<usize>) -> Vec<SourceLine> {
        match &self.buffer {
            EditorBuffer::Source(buffer) => buffer.lines(line_range),
            EditorBuffer::Rich(_) => Vec::new(),
        }
    }

    pub fn source_viewport_lines(
        &self,
        line_range: Range<usize>,
        left_column: usize,
        width: usize,
    ) -> Vec<SourceViewportLine> {
        match &self.buffer {
            EditorBuffer::Source(buffer) => buffer.viewport_lines(line_range, left_column, width),
            EditorBuffer::Rich(_) => Vec::new(),
        }
    }

    /// C token ranges are shared across viewport snapshots. Non-C documents
    /// never build this cache, even if their content resembles C source.
    pub fn source_c_highlights(&self) -> Arc<[c_syntax::CToken]> {
        if !self.document.path.as_ref().is_some_and(c_syntax::is_c_file) {
            return Arc::from([]);
        }
        match &self.buffer {
            EditorBuffer::Source(buffer) => Arc::clone(
                buffer
                    .c_highlights
                    .0
                    .get_or_init(|| c_syntax::highlight_chars(buffer.text.chars())),
            ),
            EditorBuffer::Rich(_) => Arc::from([]),
        }
    }

    pub fn source_byte_slice(&self, range: SourceRange) -> Option<String> {
        match &self.buffer {
            EditorBuffer::Source(buffer) => buffer.byte_slice(range),
            EditorBuffer::Rich(_) => None,
        }
    }

    /// Resolves a canonical UTF-8 byte offset to `(line, Unicode-scalar
    /// column)` in O(log n). This matches the Source UI's
    /// text-position column semantics while avoiding a document-prefix scan.
    pub fn source_position(&self, byte_offset: usize) -> Option<(usize, usize)> {
        let EditorBuffer::Source(buffer) = &self.buffer else {
            return None;
        };
        let offset = buffer.normalize_position(byte_offset);
        let line = buffer.text.byte_to_line(offset);
        let range = buffer.line_range(line)?;
        let column = buffer
            .text
            .byte_to_char(offset.min(range.end))
            .saturating_sub(buffer.text.byte_to_char(range.start));
        Some((line, column))
    }

    /// Resolves a canonical UTF-8 byte offset to `(line, terminal-display
    /// column)`. Unlike [`Self::source_position`], the column accounts for
    /// extended grapheme clusters and wide glyphs. ASCII prefixes use Rope
    /// metadata directly; Unicode prefixes are traversed chunk-by-chunk
    /// without flattening the line.
    pub fn source_display_position(&self, byte_offset: usize) -> Option<(usize, usize)> {
        let EditorBuffer::Source(buffer) = &self.buffer else {
            return None;
        };
        let offset = buffer.normalize_position(byte_offset);
        let line = buffer.text.byte_to_line(offset);
        let range = buffer.line_range(line)?;
        Some((line, buffer.line_display_column(range, offset)))
    }

    /// Resolves a Source UI `(line, Unicode-scalar column)` to a canonical
    /// UTF-8 byte offset. Columns beyond the line clamp to its content end.
    pub fn source_offset(&self, line: usize, column: usize) -> Option<usize> {
        let EditorBuffer::Source(buffer) = &self.buffer else {
            return None;
        };
        let range = buffer.line_range(line)?;
        let start_char = buffer.text.byte_to_char(range.start);
        let end_char = buffer.text.byte_to_char(range.end);
        Some(
            buffer
                .text
                .char_to_byte(start_char.saturating_add(column).min(end_char)),
        )
    }

    pub fn rich_projection(&self) -> Option<RichProjection> {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => Some(buffer.editor.projection()),
            EditorBuffer::Source(_) => None,
        }
    }

    /// Exports the active buffer at an explicit boundary without mutating the
    /// model, history, revision, or save checkpoint.
    pub fn export_text(&self) -> String {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => MarkdownCodec::export(&buffer.editor.document)
                .map(|export| export.markdown)
                .unwrap_or_else(|_| self.document.source().into_owned()),
            EditorBuffer::Source(buffer) => buffer.to_string(),
        }
    }

    /// Installs an application-private Rich recovery payload. Recovery is not
    /// an edit transaction, but the recovered draft is intentionally dirty.
    pub fn install_rich_draft(
        &mut self,
        mut document: RichDocument,
        mut cursor: Option<RichPosition>,
        mut selection: Option<RichSelection>,
    ) {
        if self.is_read_only() {
            return;
        }
        document.repair_node_id_allocator();
        normalize_legacy_recovered_heading_breaks(&mut document, &mut cursor, &mut selection);
        let mut editor = RichEditor::new(document);
        if let Some(cursor) = cursor.filter(|position| editor.contains_position(*position)) {
            editor.cursor = Some(cursor);
        }
        editor.selection = selection.filter(|selection| {
            !selection.is_collapsed()
                && editor.contains_position(selection.anchor)
                && editor.contains_position(selection.focus)
        });
        self.buffer = EditorBuffer::Rich(RichBuffer { editor });
        self.mode = EditorMode::Rich;
        self.cursor = Cursor::default();
        self.selection = None;
        self.install_dirty_recovery_checkpoint();
    }

    /// Installs an application-private Source recovery payload. Byte offsets
    /// are clamped to valid UTF-8 boundaries and do not enter Rich history.
    pub fn install_source_draft(
        &mut self,
        text: String,
        cursor: usize,
        selection: Option<Selection>,
    ) {
        if self.is_read_only() {
            return;
        }
        self.buffer = EditorBuffer::Source(SourceBuffer::from_text(&text));
        self.mode = EditorMode::Source;
        self.cursor = Cursor {
            byte_offset: cursor,
            preferred_column: None,
        };
        self.selection = selection;
        self.refresh_metadata_from_active_source();
        self.clamp_positions();
        self.install_dirty_recovery_checkpoint();
    }

    pub(super) fn install_dirty_recovery_checkpoint(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.source_session = None;
        self.pending_saves.clear();
        self.saved_revision = 0;
        self.revision = self.next_revision.max(1);
        self.next_revision = self.revision.saturating_add(1);
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => MarkdownCodec::export(&buffer.editor.document)
                .map(|export| export.to_bytes(self.document.metadata.utf8_bom))
                .unwrap_or_else(|_| self.document.to_bytes()),
            EditorBuffer::Source(buffer) => {
                let text = buffer.text();
                bytes_with_bom(&text, self.document.metadata.utf8_bom)
            }
        }
    }

    pub fn word_count(&self) -> usize {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => {
                if let Some((revision, word_count)) = self.rich_word_count_cache.get()
                    && revision == self.revision
                {
                    return word_count;
                }
                let word_count = buffer.editor.word_count();
                self.rich_word_count_cache
                    .set(Some((self.revision, word_count)));
                word_count
            }
            EditorBuffer::Source(buffer) => buffer.word_count(),
        }
    }

    pub fn can_undo(&self) -> bool {
        self.undo_stack.last().is_some_and(|transaction| {
            matches!(
                (&self.buffer, transaction),
                (EditorBuffer::Rich(_), EditTransaction::Rich { .. })
                    | (EditorBuffer::Source(_), EditTransaction::Source { .. })
            )
        })
    }

    pub fn can_redo(&self) -> bool {
        self.redo_stack.last().is_some_and(|transaction| {
            matches!(
                (&self.buffer, transaction),
                (EditorBuffer::Rich(_), EditTransaction::Rich { .. })
                    | (EditorBuffer::Source(_), EditTransaction::Source { .. })
            )
        })
    }

    pub fn history_depth(&self) -> (usize, usize) {
        (self.undo_stack.len(), self.redo_stack.len())
    }

    pub fn selected_range(&self) -> Option<SourceRange> {
        match self.buffer {
            EditorBuffer::Source(_) => self
                .selection
                .filter(|selection| !selection.is_collapsed())
                .map(|selection| SourceRange::from(selection.range())),
            EditorBuffer::Rich(_) => None,
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => buffer.editor.selected_text(),
            EditorBuffer::Source(buffer) => {
                let range = self.selected_range()?;
                buffer.byte_slice(range)
            }
        }
    }

    pub fn has_selection(&self) -> bool {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => buffer.editor.has_selection(),
            EditorBuffer::Source(_) => self
                .selection
                .is_some_and(|selection| !selection.is_collapsed()),
        }
    }

    pub fn cursor_line_column(&self) -> (usize, usize) {
        match &self.buffer {
            EditorBuffer::Source(buffer) => line_column_for_offset(buffer, self.cursor.byte_offset),
            EditorBuffer::Rich(buffer) => rich_line_column(&buffer.editor),
        }
    }

    pub fn render_blocks(&self) -> Vec<RenderBlock> {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => {
                legacy_blocks_from_projection(&buffer.editor.projection())
            }
            EditorBuffer::Source(buffer) => render_plain_text(&buffer.text()),
        }
    }

    pub fn current_block(&self) -> Option<RenderBlock> {
        match &self.buffer {
            EditorBuffer::Source(buffer) => {
                let cursor = self.cursor.byte_offset;
                self.render_blocks().into_iter().find(|block| {
                    let range = block.source_range();
                    range.contains(cursor)
                        || (cursor == range.end && range.end == buffer.len_bytes())
                })
            }
            EditorBuffer::Rich(_) => self.render_blocks().into_iter().next(),
        }
    }

    pub fn apply(&mut self, command: EditorCommand) -> Vec<EditorEffect> {
        EditorController.apply(self, command)
    }

    pub fn replace_source_range(&mut self, range: SourceRange, replacement: &str) -> bool {
        if self.is_read_only() {
            return false;
        }
        let EditorBuffer::Source(buffer) = &self.buffer else {
            return false;
        };
        let Some(range) = buffer.validated_range(range.start..range.end) else {
            return false;
        };
        let cursor = Cursor {
            byte_offset: range.start + replacement.len(),
            preferred_column: None,
        };
        self.commit_source_edit(range, replacement, cursor, None, EditKind::Insert)
    }

    pub(super) fn snapshot(&self) -> EditorSnapshot {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => EditorSnapshot::Rich {
                editor: buffer.editor.clone(),
                revision: self.revision,
            },
            EditorBuffer::Source(_) => {
                unreachable!("Source edits use range transactions instead of snapshots")
            }
        }
    }

    pub(super) fn restore_snapshot(&mut self, snapshot: &EditorSnapshot) {
        match snapshot {
            EditorSnapshot::Rich { editor, revision } => {
                self.buffer = EditorBuffer::Rich(RichBuffer {
                    editor: editor.clone(),
                });
                self.mode = EditorMode::Rich;
                self.revision = *revision;
            }
        }
        self.clamp_positions();
    }

    pub(super) fn commit_edit(&mut self, before: EditorSnapshot, kind: EditKind) -> bool {
        let changed = !before.same_content(&self.snapshot());
        if !changed {
            return false;
        }
        self.revision = self.next_revision;
        self.next_revision = self.next_revision.saturating_add(1).max(1);
        let after = self.snapshot();
        self.undo_stack.push(EditTransaction::Rich {
            before,
            after,
            kind,
        });
        if self.undo_stack.len() > self.history_limit {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        true
    }

    pub(super) fn commit_source_edit(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        after_cursor: Cursor,
        after_selection: Option<Selection>,
        kind: EditKind,
    ) -> bool {
        let before_cursor = self.cursor;
        let before_selection = self.selection;
        let before_revision = self.revision;
        let Some(removed) = (match &mut self.buffer {
            EditorBuffer::Source(buffer) => buffer.replace_range(range.clone(), replacement),
            EditorBuffer::Rich(_) => None,
        }) else {
            return false;
        };
        self.cursor = after_cursor;
        self.selection = after_selection;
        if removed == replacement {
            return false;
        }

        self.refresh_metadata_from_active_source();
        self.revision = self.next_revision;
        self.next_revision = self.next_revision.saturating_add(1).max(1);
        self.undo_stack.push(EditTransaction::Source {
            start: range.start,
            removed,
            inserted: replacement.to_owned(),
            before_cursor,
            before_selection,
            before_revision,
            after_cursor,
            after_selection,
            after_revision: self.revision,
            kind,
        });
        if self.undo_stack.len() > self.history_limit {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        true
    }

    pub(super) fn clamp_positions(&mut self) {
        match &mut self.buffer {
            EditorBuffer::Source(buffer) => {
                let cursor = buffer.normalize_position(self.cursor.byte_offset);
                let selection = self.selection.and_then(|selection| {
                    let selection = Selection::new(
                        buffer.normalize_position(selection.anchor),
                        buffer.normalize_position(selection.focus),
                    );
                    (!selection.is_collapsed()).then_some(selection)
                });
                self.cursor.byte_offset = cursor;
                self.selection = selection;
            }
            EditorBuffer::Rich(buffer) => {
                if let Some(cursor) = buffer.editor.cursor
                    && !buffer.editor.move_to(cursor, false)
                {
                    buffer.editor.cursor = buffer.editor.document.first_editable_position();
                    buffer.editor.selection = None;
                }
            }
        }
    }

    pub(super) fn refresh_metadata_from_active_source(&mut self) {
        let fallback = self.document.metadata.preferred_line_ending;
        self.document.metadata = match &self.buffer {
            EditorBuffer::Source(buffer) => {
                buffer.metadata(self.document.metadata.utf8_bom, fallback)
            }
            EditorBuffer::Rich(_) => {
                let source = self.document.source();
                TextMetadata::from_source_with_fallback(
                    &source,
                    self.document.metadata.utf8_bom,
                    fallback,
                )
            }
        };
    }

    pub(super) fn set_mode(&mut self, mode: EditorMode) {
        if self.document.kind != DocumentKind::Markdown || self.mode == mode {
            return;
        }
        match mode {
            EditorMode::Source => {
                let EditorBuffer::Rich(buffer) = &self.buffer else {
                    return;
                };
                let Ok(export) = MarkdownCodec::export(&buffer.editor.document) else {
                    return;
                };
                let rich_before = self.snapshot();
                let cursor = buffer
                    .editor
                    .cursor
                    .and_then(|position| export.positions.source_offset_for(position))
                    .unwrap_or(0);
                let selection = buffer.editor.selection.and_then(|selection| {
                    let anchor = export.positions.source_offset_for(selection.anchor)?;
                    let focus = export.positions.source_offset_for(selection.focus)?;
                    (anchor != focus).then_some(Selection::new(anchor, focus))
                });
                self.document.restore_source(export.markdown.clone());
                let exported_source = export.markdown.clone();
                self.buffer = EditorBuffer::Source(SourceBuffer::from_text(&export.markdown));
                self.cursor = Cursor {
                    byte_offset: cursor,
                    preferred_column: None,
                };
                self.selection = selection;
                let rich_redo = std::mem::take(&mut self.redo_stack);
                self.source_session = Some(SourceSession {
                    rich_before,
                    history_base: self.undo_stack.len(),
                    start_revision: self.revision,
                    rich_redo,
                    exported_source,
                });
            }
            EditorMode::Rich => {
                let EditorBuffer::Source(buffer) = &self.buffer else {
                    return;
                };
                let source = buffer.to_string();
                let source_cursor = self.cursor.byte_offset;
                let source_selection = self.selection;
                let source_unchanged = self
                    .source_session
                    .as_ref()
                    .is_some_and(|session| source == session.exported_source);
                if source_unchanged {
                    let session = self.source_session.take().expect("Source session exists");
                    self.undo_stack.truncate(session.history_base);
                    self.redo_stack = session.rich_redo;
                    self.restore_snapshot(&session.rich_before);
                    self.mode = EditorMode::Rich;
                    self.clamp_positions();
                    return;
                }
                let Ok(import) = MarkdownCodec::import_with_metadata(
                    &source,
                    self.document.metadata.utf8_bom,
                    rich_line_ending(self.document.metadata.preferred_line_ending),
                ) else {
                    return;
                };
                let mut editor = RichEditor::new(import.document);
                editor.cursor = import
                    .positions
                    .rich_position_for(source_cursor)
                    .or(editor.cursor);
                editor.selection = source_selection.and_then(|selection| {
                    let anchor = import.positions.rich_position_for(selection.anchor)?;
                    let focus = import.positions.rich_position_for(selection.focus)?;
                    (anchor != focus).then_some(RichSelection::new(anchor, focus))
                });
                self.document.restore_source(source);
                self.buffer = EditorBuffer::Rich(RichBuffer { editor });
                if let Some(session) = self.source_session.take() {
                    self.undo_stack.truncate(session.history_base);
                    let after = self.snapshot();
                    if !session.rich_before.same_content(&after) {
                        self.undo_stack.push(EditTransaction::Rich {
                            before: session.rich_before,
                            after,
                            kind: EditKind::Insert,
                        });
                        if self.undo_stack.len() > self.history_limit {
                            self.undo_stack.remove(0);
                        }
                        self.redo_stack.clear();
                    } else {
                        self.revision = session.start_revision;
                        self.redo_stack = session.rich_redo;
                    }
                }
            }
        }
        self.mode = mode;
        self.clamp_positions();
    }

    pub(super) fn prepare_save_snapshot(&mut self) -> SaveSnapshot {
        let snapshot = match &self.buffer {
            EditorBuffer::Rich(buffer) => SaveSnapshot::rich(
                self.revision,
                buffer.editor.document.clone(),
                self.document.source.as_contiguous(),
                self.document.metadata.utf8_bom,
            ),
            EditorBuffer::Source(buffer) => SaveSnapshot::source(
                self.revision,
                buffer.text.clone(),
                buffer.word_count,
                buffer.line_endings,
                self.document.metadata.utf8_bom,
            ),
        };
        self.pending_saves
            .retain(|pending| pending.revision != self.revision);
        self.pending_saves.push(snapshot.clone());
        if self.pending_saves.len() > 8 {
            self.pending_saves.remove(0);
        }
        snapshot
    }

    pub(super) fn mark_saved(&mut self, path: Option<PathBuf>, revision: u64) {
        let Some(index) = self
            .pending_saves
            .iter()
            .position(|pending| pending.revision == revision)
        else {
            return;
        };
        let pending = self.pending_saves.remove(index);
        // In the normal asynchronous path `write_to` populated this cache on
        // the worker. The fallback keeps direct domain integrations correct.
        let prepared = pending.prepared();
        if let Some(path) = path {
            self.document.kind = DocumentKind::from_path(&path);
            self.document.path = Some(path);
        }
        let document_source = match (self.document.kind, pending.payload.as_ref()) {
            (
                DocumentKind::PlainText,
                SavePayload::Source {
                    text,
                    word_count,
                    line_endings,
                    ..
                },
            ) => DocumentText::Rope {
                text: text.clone(),
                word_count: *word_count,
                line_endings: *line_endings,
            },
            _ => {
                let source = prepared.source.as_ref().cloned().unwrap_or_else(|| {
                    let SavePayload::Source { text, .. } = pending.payload.as_ref() else {
                        unreachable!("Rich preparation always has source")
                    };
                    Arc::new(String::from(text))
                });
                DocumentText::from_arc(self.document.kind, source)
            }
        };
        self.document.saved_source = document_source.clone();
        self.document.source = document_source;
        self.saved_revision = revision;

        if self.revision == revision
            && let (EditorBuffer::Rich(buffer), Some(export)) = (&mut self.buffer, &prepared.export)
        {
            let _ = MarkdownCodec::accept_export(&mut buffer.editor.document, export);
        }

        if self.document.kind != DocumentKind::Markdown {
            if let EditorBuffer::Rich(buffer) = &self.buffer {
                let cursor = buffer
                    .editor
                    .cursor
                    .and_then(|position| {
                        prepared
                            .export
                            .as_ref()
                            .and_then(|export| export.positions.source_offset_for(position))
                    })
                    .unwrap_or(0);
                let (text, word_count, line_endings) = self
                    .document
                    .source
                    .rope()
                    .expect("PlainText saved source is Rope-backed");
                self.buffer =
                    EditorBuffer::Source(SourceBuffer::from_rope(text, word_count, line_endings));
                self.cursor = Cursor {
                    byte_offset: cursor,
                    preferred_column: None,
                };
                self.selection = None;
            }
            self.source_session = None;
            self.mode = EditorMode::Source;
            self.clamp_positions();
        }
    }
}
