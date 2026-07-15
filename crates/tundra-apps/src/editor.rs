//! Domain model for Tundra's single-document terminal editor.
//!
//! Source mode edits the canonical document. Rich mode edits an isolated
//! working buffer and only synchronizes it at explicit mode/save boundaries.
//! This keeps live UI input away from the persisted Markdown snapshot.

use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

use unicode_segmentation::UnicodeSegmentation;

use crate::markdown_codec::{MarkdownCodec, MarkdownExport};
use crate::rich_document::{
    NodeId, ProjectedBlock, ProjectedBlockKind, ProjectedInline, RichDocument, RichLineEnding,
    RichListKind, RichPosition, RichProjection, RichTableAlignment,
};
use crate::rich_edit::{RichEditor, RichSelection};

const UTF8_BOM: &[u8; 3] = b"\xEF\xBB\xBF";
const DEFAULT_HISTORY_LIMIT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorMode {
    #[default]
    Rich,
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocumentKind {
    #[default]
    Markdown,
    PlainText,
}

impl DocumentKind {
    pub fn from_path(path: &Path) -> Self {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default();
        if ["md", "markdown", "mdown", "mkd"]
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        {
            Self::Markdown
        } else {
            Self::PlainText
        }
    }

    pub const fn initial_mode(self) -> EditorMode {
        match self {
            Self::Markdown => EditorMode::Rich,
            Self::PlainText => EditorMode::Source,
        }
    }

    pub const fn default_file_name(self) -> &'static str {
        match self {
            Self::Markdown => "Untitled.md",
            Self::PlainText => "Untitled.txt",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
    Cr,
}

impl LineEnding {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextMetadata {
    pub utf8_bom: bool,
    pub preferred_line_ending: LineEnding,
    pub mixed_line_endings: bool,
    pub has_final_newline: bool,
}

impl Default for TextMetadata {
    fn default() -> Self {
        Self {
            utf8_bom: false,
            preferred_line_ending: LineEnding::Lf,
            mixed_line_endings: false,
            has_final_newline: false,
        }
    }
}

impl TextMetadata {
    pub fn from_source(source: &str, utf8_bom: bool) -> Self {
        Self::from_source_with_fallback(source, utf8_bom, LineEnding::Lf)
    }

    fn from_source_with_fallback(source: &str, utf8_bom: bool, fallback: LineEnding) -> Self {
        let mut lf = 0usize;
        let mut crlf = 0usize;
        let mut cr = 0usize;
        let bytes = source.as_bytes();
        let mut index = 0usize;
        while index < bytes.len() {
            match bytes[index] {
                b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                    crlf += 1;
                    index += 2;
                }
                b'\r' => {
                    cr += 1;
                    index += 1;
                }
                b'\n' => {
                    lf += 1;
                    index += 1;
                }
                _ => index += 1,
            }
        }
        let distinct = usize::from(lf > 0) + usize::from(crlf > 0) + usize::from(cr > 0);
        let preferred_line_ending = [
            (lf, LineEnding::Lf),
            (crlf, LineEnding::CrLf),
            (cr, LineEnding::Cr),
        ]
        .into_iter()
        .max_by_key(|(count, _)| *count)
        .filter(|(count, _)| *count > 0)
        .map(|(_, ending)| ending)
        .unwrap_or(fallback);
        Self {
            utf8_bom,
            preferred_line_ending,
            mixed_line_endings: distinct > 1,
            has_final_newline: source.ends_with('\n') || source.ends_with('\r'),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorDocument {
    pub path: Option<PathBuf>,
    pub kind: DocumentKind,
    pub metadata: TextMetadata,
    source: String,
    saved_source: String,
}

impl EditorDocument {
    pub fn untitled(kind: DocumentKind) -> Self {
        Self {
            path: None,
            kind,
            metadata: TextMetadata::default(),
            source: String::new(),
            saved_source: String::new(),
        }
    }

    pub fn from_text(path: Option<PathBuf>, kind: DocumentKind, source: impl Into<String>) -> Self {
        let source = source.into();
        let metadata = TextMetadata::from_source(&source, false);
        Self {
            path,
            kind,
            metadata,
            saved_source: source.clone(),
            source,
        }
    }

    pub fn from_bytes(
        path: Option<PathBuf>,
        kind: DocumentKind,
        bytes: &[u8],
    ) -> Result<Self, DocumentDecodeError> {
        let (utf8_bom, body) = if bytes.starts_with(UTF8_BOM) {
            (true, &bytes[UTF8_BOM.len()..])
        } else {
            (false, bytes)
        };
        let source = std::str::from_utf8(body)
            .map_err(|error| DocumentDecodeError {
                valid_up_to: error.valid_up_to(),
            })?
            .to_owned();
        let metadata = TextMetadata::from_source(&source, utf8_bom);
        Ok(Self {
            path,
            kind,
            metadata,
            saved_source: source.clone(),
            source,
        })
    }

    pub fn open(path: impl Into<PathBuf>, bytes: &[u8]) -> Result<Self, DocumentDecodeError> {
        let path = path.into();
        let kind = DocumentKind::from_path(&path);
        Self::from_bytes(Some(path), kind, bytes)
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes =
            Vec::with_capacity(self.source.len() + usize::from(self.metadata.utf8_bom) * 3);
        if self.metadata.utf8_bom {
            bytes.extend_from_slice(UTF8_BOM);
        }
        bytes.extend_from_slice(self.source.as_bytes());
        bytes
    }

    pub fn is_dirty(&self) -> bool {
        self.source != self.saved_source
    }

    pub fn mark_saved(&mut self, path: Option<PathBuf>) {
        if let Some(path) = path {
            self.kind = DocumentKind::from_path(&path);
            self.path = Some(path);
        }
        self.saved_source.clone_from(&self.source);
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| self.kind.default_file_name().to_owned())
    }

    pub fn line_count(&self) -> usize {
        line_ranges(&self.source).len()
    }

    pub fn word_count(&self) -> usize {
        self.source.unicode_words().count()
    }

    fn restore_source(&mut self, source: String) {
        self.source = source;
        let fallback = self.metadata.preferred_line_ending;
        self.metadata =
            TextMetadata::from_source_with_fallback(&self.source, self.metadata.utf8_bom, fallback);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentDecodeError {
    pub valid_up_to: usize,
}

impl fmt::Display for DocumentDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "document is not valid UTF-8 (valid through byte {})",
            self.valid_up_to
        )
    }
}

impl std::error::Error for DocumentDecodeError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cursor {
    pub byte_offset: usize,
    pub preferred_column: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub focus: usize,
}

impl Selection {
    pub const fn new(anchor: usize, focus: usize) -> Self {
        Self { anchor, focus }
    }

    pub fn range(self) -> Range<usize> {
        self.anchor.min(self.focus)..self.anchor.max(self.focus)
    }

    pub const fn is_collapsed(self) -> bool {
        self.anchor == self.focus
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorMove {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    DocumentStart,
    DocumentEnd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatCommand {
    Bold,
    Italic,
    Strikethrough,
    InlineCode,
    Heading(u8),
    Paragraph,
    Quote,
    BulletList,
    OrderedList,
    TaskList,
    Link {
        url: String,
        title: Option<String>,
    },
    Image {
        url: String,
        alt: String,
        title: Option<String>,
    },
    Table {
        columns: usize,
        rows: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableColumnEdge {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableColumnEdit {
    Insert,
    Remove,
}

/// A mode-aware editor position. Markdown byte offsets never enter the Rich
/// branch; Rich coordinates are stable node identities plus grapheme offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorPosition {
    Rich(RichPosition),
    Source(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorCommand {
    InsertText(String),
    InsertNewline,
    Paste(String),
    Backspace,
    DeleteForward,
    DeleteSelection,
    MoveCursor {
        movement: CursorMove,
        extend_selection: bool,
    },
    MoveTo {
        position: EditorPosition,
        extend_selection: bool,
    },
    SelectAll,
    ClearSelection,
    Undo,
    Redo,
    ApplyFormat(FormatCommand),
    EditTableColumn {
        table_id: NodeId,
        edge: TableColumnEdge,
        edit: TableColumnEdit,
    },
    SetMode(EditorMode),
    ToggleMode,
    Copy,
    Cut,
    RequestPaste,
    RequestOpen,
    RequestSave,
    RequestSaveAs,
    RequestClose,
    ReplaceDocument(EditorDocument),
    MarkSaved {
        path: Option<PathBuf>,
        revision: u64,
    },
}

/// Immutable bytes exported from one exact editor revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveSnapshot {
    pub revision: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorEffect {
    OpenFilePicker,
    SaveFile {
        path: PathBuf,
        snapshot: SaveSnapshot,
    },
    SaveFilePicker {
        suggested_name: String,
        snapshot: SaveSnapshot,
    },
    WriteClipboard(String),
    ReadClipboard,
    ConfirmClose,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EditorViewport {
    pub top_line: usize,
    pub left_column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichBuffer {
    pub editor: RichEditor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBuffer {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorBuffer {
    Rich(RichBuffer),
    Source(SourceBuffer),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EditorSnapshot {
    Rich {
        editor: RichEditor,
        revision: u64,
    },
    Source {
        source: String,
        cursor: Cursor,
        selection: Option<Selection>,
        revision: u64,
    },
}

impl EditorSnapshot {
    fn same_content(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Rich { editor: left, .. }, Self::Rich { editor: right, .. }) => {
                left.document == right.document
            }
            (Self::Source { source: left, .. }, Self::Source { source: right, .. }) => {
                left == right
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Insert,
    Delete,
    Format,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditTransaction {
    before: EditorSnapshot,
    after: EditorSnapshot,
    kind: EditKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceSession {
    rich_before: EditorSnapshot,
    history_base: usize,
    start_revision: u64,
    rich_redo: Vec<EditTransaction>,
    exported_source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingSave {
    revision: u64,
    source: String,
    export: Option<MarkdownExport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorState {
    pub document: EditorDocument,
    pub mode: EditorMode,
    /// Source-mode caret retained for compatibility. Rich mode uses
    /// [`RichEditor::cursor`] and never interprets this as a Markdown offset.
    pub cursor: Cursor,
    pub selection: Option<Selection>,
    pub viewport: EditorViewport,
    pub buffer: EditorBuffer,
    undo_stack: Vec<EditTransaction>,
    redo_stack: Vec<EditTransaction>,
    history_limit: usize,
    revision: u64,
    saved_revision: u64,
    next_revision: u64,
    source_session: Option<SourceSession>,
    pending_saves: Vec<PendingSave>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorState {
    pub fn new() -> Self {
        Self::untitled(DocumentKind::Markdown)
    }

    pub fn untitled(kind: DocumentKind) -> Self {
        Self::from_document(EditorDocument::untitled(kind))
    }

    pub fn open(path: impl Into<PathBuf>, bytes: &[u8]) -> Result<Self, DocumentDecodeError> {
        Ok(Self::from_document(EditorDocument::open(path, bytes)?))
    }

    pub fn from_document(document: EditorDocument) -> Self {
        let mode = document.kind.initial_mode();
        let buffer = match mode {
            EditorMode::Rich => EditorBuffer::Rich(RichBuffer {
                editor: RichEditor::new(import_rich_document(&document)),
            }),
            EditorMode::Source => EditorBuffer::Source(SourceBuffer {
                text: document.source().to_owned(),
            }),
        };
        Self {
            document,
            mode,
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
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// Source-mode text, or the last boundary Markdown snapshot in Rich mode.
    /// Rich input never refreshes this string.
    pub fn source(&self) -> &str {
        match &self.buffer {
            EditorBuffer::Rich(_) => self.document.source(),
            EditorBuffer::Source(buffer) => &buffer.text,
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

    pub fn source_buffer(&self) -> Option<&str> {
        match &self.buffer {
            EditorBuffer::Source(buffer) => Some(&buffer.text),
            EditorBuffer::Rich(_) => None,
        }
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
                .unwrap_or_else(|_| self.document.source().to_owned()),
            EditorBuffer::Source(buffer) => buffer.text.clone(),
        }
    }

    /// Installs an application-private Rich recovery payload. Recovery is not
    /// an edit transaction, but the recovered draft is intentionally dirty.
    pub fn install_rich_draft(
        &mut self,
        mut document: RichDocument,
        cursor: Option<RichPosition>,
        selection: Option<RichSelection>,
    ) {
        document.repair_node_id_allocator();
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
        self.buffer = EditorBuffer::Source(SourceBuffer { text });
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

    fn install_dirty_recovery_checkpoint(&mut self) {
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
                bytes_with_bom(&buffer.text, self.document.metadata.utf8_bom)
            }
        }
    }

    pub fn word_count(&self) -> usize {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => buffer.editor.word_count(),
            EditorBuffer::Source(buffer) => buffer.text.unicode_words().count(),
        }
    }

    pub fn can_undo(&self) -> bool {
        self.undo_stack.last().is_some_and(|transaction| {
            matches!(
                (&self.buffer, &transaction.before),
                (EditorBuffer::Rich(_), EditorSnapshot::Rich { .. })
                    | (EditorBuffer::Source(_), EditorSnapshot::Source { .. })
            )
        })
    }

    pub fn can_redo(&self) -> bool {
        self.redo_stack.last().is_some_and(|transaction| {
            matches!(
                (&self.buffer, &transaction.after),
                (EditorBuffer::Rich(_), EditorSnapshot::Rich { .. })
                    | (EditorBuffer::Source(_), EditorSnapshot::Source { .. })
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
                buffer.text.get(range.start..range.end).map(str::to_owned)
            }
        }
    }

    pub fn cursor_line_column(&self) -> (usize, usize) {
        match &self.buffer {
            EditorBuffer::Source(buffer) => {
                line_column_for_offset(&buffer.text, self.cursor.byte_offset)
            }
            EditorBuffer::Rich(buffer) => rich_line_column(&buffer.editor),
        }
    }

    pub fn render_blocks(&self) -> Vec<RenderBlock> {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => {
                legacy_blocks_from_projection(&buffer.editor.projection())
            }
            EditorBuffer::Source(buffer) => render_plain_text(&buffer.text),
        }
    }

    pub fn current_block(&self) -> Option<RenderBlock> {
        match &self.buffer {
            EditorBuffer::Source(buffer) => {
                let cursor = self.cursor.byte_offset;
                self.render_blocks().into_iter().find(|block| {
                    let range = block.source_range();
                    range.contains(cursor)
                        || (cursor == range.end && range.end == buffer.text.len())
                })
            }
            EditorBuffer::Rich(_) => self.render_blocks().into_iter().next(),
        }
    }

    pub fn apply(&mut self, command: EditorCommand) -> Vec<EditorEffect> {
        EditorController.apply(self, command)
    }

    pub fn replace_source_range(&mut self, range: SourceRange, replacement: &str) -> bool {
        let EditorBuffer::Source(buffer) = &self.buffer else {
            return false;
        };
        let Some(range) = validated_range(&buffer.text, range.start..range.end) else {
            return false;
        };
        let before = self.snapshot();
        self.replace_edit_range(range.clone(), replacement);
        self.cursor.byte_offset = range.start + replacement.len();
        self.cursor.preferred_column = None;
        self.selection = None;
        self.commit_edit(before, EditKind::Insert)
    }

    fn snapshot(&self) -> EditorSnapshot {
        match &self.buffer {
            EditorBuffer::Rich(buffer) => EditorSnapshot::Rich {
                editor: buffer.editor.clone(),
                revision: self.revision,
            },
            EditorBuffer::Source(buffer) => EditorSnapshot::Source {
                source: buffer.text.clone(),
                cursor: self.cursor,
                selection: self.selection,
                revision: self.revision,
            },
        }
    }

    fn restore_snapshot(&mut self, snapshot: &EditorSnapshot) {
        match snapshot {
            EditorSnapshot::Rich { editor, revision } => {
                self.buffer = EditorBuffer::Rich(RichBuffer {
                    editor: editor.clone(),
                });
                self.mode = EditorMode::Rich;
                self.revision = *revision;
            }
            EditorSnapshot::Source {
                source,
                cursor,
                selection,
                revision,
            } => {
                self.buffer = EditorBuffer::Source(SourceBuffer {
                    text: source.clone(),
                });
                self.mode = EditorMode::Source;
                self.cursor = *cursor;
                self.selection = *selection;
                self.revision = *revision;
            }
        }
        self.clamp_positions();
    }

    fn commit_edit(&mut self, before: EditorSnapshot, kind: EditKind) -> bool {
        let changed = !before.same_content(&self.snapshot());
        if !changed {
            return false;
        }
        self.revision = self.next_revision;
        self.next_revision = self.next_revision.saturating_add(1).max(1);
        let after = self.snapshot();
        self.undo_stack.push(EditTransaction {
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

    fn clamp_positions(&mut self) {
        match &mut self.buffer {
            EditorBuffer::Source(buffer) => {
                let cursor = normalize_position(&buffer.text, self.cursor.byte_offset);
                let selection = self.selection.and_then(|selection| {
                    let selection = Selection::new(
                        normalize_position(&buffer.text, selection.anchor),
                        normalize_position(&buffer.text, selection.focus),
                    );
                    (!selection.is_collapsed()).then_some(selection)
                });
                self.cursor.byte_offset = cursor;
                self.selection = selection;
            }
            EditorBuffer::Rich(buffer) => {
                if let Some(cursor) = buffer.editor.cursor {
                    if !buffer.editor.move_to(cursor, false) {
                        buffer.editor.cursor = buffer.editor.document.first_editable_position();
                        buffer.editor.selection = None;
                    }
                }
            }
        }
    }

    fn replace_edit_range(&mut self, range: Range<usize>, replacement: &str) {
        if let EditorBuffer::Source(buffer) = &mut self.buffer {
            buffer.text.replace_range(range, replacement);
            self.refresh_metadata_from_active_source();
        }
    }

    fn refresh_metadata_from_active_source(&mut self) {
        let fallback = self.document.metadata.preferred_line_ending;
        self.document.metadata = TextMetadata::from_source_with_fallback(
            self.source_buffer().unwrap_or(self.document.source()),
            self.document.metadata.utf8_bom,
            fallback,
        );
    }

    fn set_mode(&mut self, mode: EditorMode) {
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
                self.buffer = EditorBuffer::Source(SourceBuffer {
                    text: export.markdown,
                });
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
                let source = buffer.text.clone();
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
                        self.undo_stack.push(EditTransaction {
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

    fn prepare_save_snapshot(&mut self) -> SaveSnapshot {
        let (source, export) = match &self.buffer {
            EditorBuffer::Rich(buffer) => match MarkdownCodec::export(&buffer.editor.document) {
                Ok(export) => (export.markdown.clone(), Some(export)),
                Err(_) => (self.document.source().to_owned(), None),
            },
            EditorBuffer::Source(buffer) => (buffer.text.clone(), None),
        };
        let snapshot = SaveSnapshot {
            revision: self.revision,
            bytes: bytes_with_bom(&source, self.document.metadata.utf8_bom),
        };
        self.pending_saves
            .retain(|pending| pending.revision != self.revision);
        self.pending_saves.push(PendingSave {
            revision: self.revision,
            source,
            export,
        });
        if self.pending_saves.len() > 8 {
            self.pending_saves.remove(0);
        }
        snapshot
    }

    fn mark_saved(&mut self, path: Option<PathBuf>, revision: u64) {
        let Some(index) = self
            .pending_saves
            .iter()
            .position(|pending| pending.revision == revision)
        else {
            return;
        };
        let pending = self.pending_saves.remove(index);
        if let Some(path) = path {
            self.document.kind = DocumentKind::from_path(&path);
            self.document.path = Some(path);
        }
        self.document.saved_source.clone_from(&pending.source);
        self.document.source.clone_from(&pending.source);
        self.saved_revision = revision;

        if self.revision == revision
            && let (EditorBuffer::Rich(buffer), Some(export)) = (&mut self.buffer, &pending.export)
        {
            let _ = MarkdownCodec::accept_export(&mut buffer.editor.document, export);
        }

        if self.document.kind != DocumentKind::Markdown {
            if let EditorBuffer::Rich(buffer) = &self.buffer {
                if let Ok(export) = MarkdownCodec::export(&buffer.editor.document) {
                    let cursor = buffer
                        .editor
                        .cursor
                        .and_then(|position| export.positions.source_offset_for(position))
                        .unwrap_or(0);
                    self.buffer = EditorBuffer::Source(SourceBuffer {
                        text: export.markdown,
                    });
                    self.cursor = Cursor {
                        byte_offset: cursor,
                        preferred_column: None,
                    };
                    self.selection = None;
                }
            }
            self.source_session = None;
            self.mode = EditorMode::Source;
            self.clamp_positions();
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EditorController;

impl EditorController {
    pub fn apply(self, state: &mut EditorState, command: EditorCommand) -> Vec<EditorEffect> {
        match command {
            EditorCommand::InsertText(text) | EditorCommand::Paste(text) => {
                apply_insert_text(state, &text);
                Vec::new()
            }
            EditorCommand::InsertNewline => {
                match &state.buffer {
                    EditorBuffer::Rich(_) => {
                        apply_insert_text(state, "\n");
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
                        let end = buffer.text.len();
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

fn import_rich_document(document: &EditorDocument) -> RichDocument {
    MarkdownCodec::import_with_metadata(
        document.source(),
        document.metadata.utf8_bom,
        rich_line_ending(document.metadata.preferred_line_ending),
    )
    .map(|import| import.document)
    .unwrap_or_else(|_| {
        let mut rich = RichDocument::new();
        rich.utf8_bom = document.metadata.utf8_bom;
        rich.preferred_line_ending = rich_line_ending(document.metadata.preferred_line_ending);
        let id = rich.allocate_node_id();
        rich.blocks.push(crate::rich_document::RichBlock::imported(
            id,
            String::new(),
            document.source().to_owned(),
            crate::rich_document::RichBlockKind::OpaqueMarkdown {
                raw: document.source().to_owned(),
                reason: "Markdown could not be imported; edit this block in Source mode".to_owned(),
            },
        ));
        rich
    })
}

const fn rich_line_ending(ending: LineEnding) -> RichLineEnding {
    match ending {
        LineEnding::Lf => RichLineEnding::Lf,
        LineEnding::CrLf => RichLineEnding::CrLf,
        LineEnding::Cr => RichLineEnding::Cr,
    }
}

fn bytes_with_bom(source: &str, utf8_bom: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(source.len() + usize::from(utf8_bom) * UTF8_BOM.len());
    if utf8_bom {
        bytes.extend_from_slice(UTF8_BOM);
    }
    bytes.extend_from_slice(source.as_bytes());
    bytes
}

fn rich_line_column(editor: &RichEditor) -> (usize, usize) {
    let Some(cursor) = editor.cursor else {
        return (0, 0);
    };
    let mut containers = Vec::new();
    collect_projected_containers(&editor.projection().blocks, &mut containers);
    let mut line = 0usize;
    for (id, text) in containers {
        if id == cursor.container_id {
            let prefix = text
                .graphemes(true)
                .take(cursor.grapheme_offset)
                .collect::<String>();
            let local_lines = prefix.split('\n').collect::<Vec<_>>();
            line += local_lines.len().saturating_sub(1);
            let column = local_lines
                .last()
                .map_or(0, |segment| segment.graphemes(true).count());
            return (line, column);
        }
        line += text.matches('\n').count() + 1;
    }
    (line, 0)
}

fn collect_projected_containers(blocks: &[ProjectedBlock], output: &mut Vec<(NodeId, String)>) {
    for block in blocks {
        match &block.kind {
            ProjectedBlockKind::Paragraph { content }
            | ProjectedBlockKind::Heading { content, .. } => {
                output.push((
                    block.id,
                    content.iter().map(|span| span.text.as_str()).collect(),
                ));
            }
            ProjectedBlockKind::CodeBlock { code, .. } => output.push((block.id, code.clone())),
            ProjectedBlockKind::Quote { blocks } => collect_projected_containers(blocks, output),
            ProjectedBlockKind::List { items, .. } => {
                for item in items {
                    collect_projected_containers(&item.blocks, output);
                }
            }
            ProjectedBlockKind::Table { header, rows, .. } => {
                output.extend(header.iter().chain(rows.iter().flatten()).map(|cell| {
                    (
                        cell.id,
                        cell.content.iter().map(|span| span.text.as_str()).collect(),
                    )
                }));
            }
            ProjectedBlockKind::Rule | ProjectedBlockKind::OpaqueMarkdown { .. } => {}
        }
    }
}

fn legacy_blocks_from_projection(projection: &RichProjection) -> Vec<RenderBlock> {
    projection
        .blocks
        .iter()
        .map(legacy_block_from_projection)
        .collect()
}

fn legacy_block_from_projection(block: &ProjectedBlock) -> RenderBlock {
    let source_range = SourceRange::default();
    match &block.kind {
        ProjectedBlockKind::Paragraph { content } => RenderBlock::Paragraph {
            content: legacy_spans(content),
            source_range,
            content_range: source_range,
        },
        ProjectedBlockKind::Heading { level, content } => RenderBlock::Heading {
            level: *level,
            content: legacy_spans(content),
            source_range,
            content_range: source_range,
        },
        ProjectedBlockKind::Quote { blocks } => RenderBlock::Quote {
            blocks: blocks.iter().map(legacy_block_from_projection).collect(),
            source_range,
        },
        ProjectedBlockKind::CodeBlock { language, code, .. } => RenderBlock::CodeBlock {
            language: language.clone(),
            code: code.clone(),
            source_range,
            content_range: source_range,
        },
        ProjectedBlockKind::List {
            kind,
            start,
            tight,
            items,
        } => {
            let items = items
                .iter()
                .map(|item| {
                    let mut blocks = item
                        .blocks
                        .iter()
                        .map(legacy_block_from_projection)
                        .collect::<Vec<_>>();
                    let content = match blocks.first() {
                        Some(RenderBlock::Paragraph { content, .. })
                        | Some(RenderBlock::Heading { content, .. }) => content.clone(),
                        _ => Vec::new(),
                    };
                    if !blocks.is_empty() {
                        blocks.remove(0);
                    }
                    RenderListItem {
                        content,
                        children: blocks,
                        checked: item.checked,
                        source_range,
                    }
                })
                .collect();
            match kind {
                RichListKind::Bullet => RenderBlock::BulletList {
                    items,
                    tight: *tight,
                    source_range,
                },
                RichListKind::Ordered => RenderBlock::OrderedList {
                    start: *start,
                    items,
                    tight: *tight,
                    source_range,
                },
                RichListKind::Task => RenderBlock::TaskList {
                    items,
                    source_range,
                },
            }
        }
        ProjectedBlockKind::Table {
            alignments,
            header,
            rows,
        } => RenderBlock::Table {
            header: header
                .iter()
                .map(|cell| RenderTableCell {
                    content: legacy_spans(&cell.content),
                    source_range,
                })
                .collect(),
            rows: rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|cell| RenderTableCell {
                            content: legacy_spans(&cell.content),
                            source_range,
                        })
                        .collect()
                })
                .collect(),
            alignments: alignments
                .iter()
                .map(|alignment| match alignment {
                    RichTableAlignment::None => TableAlignment::None,
                    RichTableAlignment::Left => TableAlignment::Left,
                    RichTableAlignment::Center => TableAlignment::Center,
                    RichTableAlignment::Right => TableAlignment::Right,
                })
                .collect(),
            source_range,
        },
        ProjectedBlockKind::Rule => RenderBlock::Rule { source_range },
        ProjectedBlockKind::OpaqueMarkdown { raw, reason } => RenderBlock::Raw {
            kind: format!("opaque: {reason}"),
            source: raw.clone(),
            source_range,
        },
    }
}

fn legacy_spans(content: &[ProjectedInline]) -> Vec<RenderSpan> {
    content
        .iter()
        .map(|span| {
            let mut styles = Vec::new();
            if span.marks.bold {
                styles.push(InlineStyle::Bold);
            }
            if span.marks.italic {
                styles.push(InlineStyle::Italic);
            }
            if span.marks.strikethrough {
                styles.push(InlineStyle::Strikethrough);
            }
            if span.marks.code {
                styles.push(InlineStyle::Code);
            }
            if span.link.is_some() {
                styles.push(InlineStyle::Link);
            }
            if span.image.is_some() {
                styles.push(InlineStyle::Image);
            }
            if span.hard_break {
                styles.push(InlineStyle::HardBreak);
            }
            RenderSpan {
                text: span.text.clone(),
                styles,
                target: span
                    .link
                    .as_ref()
                    .map(|link| link.url.clone())
                    .or_else(|| span.image.as_ref().map(|image| image.url.clone())),
                title: span
                    .link
                    .as_ref()
                    .and_then(|link| link.title.clone())
                    .or_else(|| span.image.as_ref().and_then(|image| image.title.clone())),
                source_range: SourceRange::default(),
            }
        })
        .collect()
}

/// A half-open UTF-8 byte range into [`EditorDocument::source`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourceRange {
    pub start: usize,
    pub end: usize,
}

impl SourceRange {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(self) -> bool {
        self.start >= self.end
    }

    pub const fn contains(self, byte_offset: usize) -> bool {
        self.start <= byte_offset && byte_offset < self.end
    }
}

impl From<Range<usize>> for SourceRange {
    fn from(range: Range<usize>) -> Self {
        Self::new(range.start, range.end)
    }
}

impl From<SourceRange> for Range<usize> {
    fn from(range: SourceRange) -> Self {
        range.start..range.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineStyle {
    Bold,
    Italic,
    Strikethrough,
    Code,
    Link,
    Image,
    RawHtml,
    Footnote,
    HardBreak,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderSpan {
    pub text: String,
    pub styles: Vec<InlineStyle>,
    pub target: Option<String>,
    pub title: Option<String>,
    pub source_range: SourceRange,
}

impl RenderSpan {
    pub fn plain(text: impl Into<String>, source_range: SourceRange) -> Self {
        Self {
            text: text.into(),
            styles: Vec::new(),
            target: None,
            title: None,
            source_range,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderListItem {
    pub content: Vec<RenderSpan>,
    pub children: Vec<RenderBlock>,
    pub checked: Option<bool>,
    pub source_range: SourceRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderTableCell {
    pub content: Vec<RenderSpan>,
    pub source_range: SourceRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAlignment {
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderBlock {
    Heading {
        level: u8,
        content: Vec<RenderSpan>,
        source_range: SourceRange,
        content_range: SourceRange,
    },
    Paragraph {
        content: Vec<RenderSpan>,
        source_range: SourceRange,
        content_range: SourceRange,
    },
    Quote {
        blocks: Vec<RenderBlock>,
        source_range: SourceRange,
    },
    CodeBlock {
        language: Option<String>,
        code: String,
        source_range: SourceRange,
        content_range: SourceRange,
    },
    BulletList {
        items: Vec<RenderListItem>,
        tight: bool,
        source_range: SourceRange,
    },
    OrderedList {
        start: usize,
        items: Vec<RenderListItem>,
        tight: bool,
        source_range: SourceRange,
    },
    TaskList {
        items: Vec<RenderListItem>,
        source_range: SourceRange,
    },
    Table {
        header: Vec<RenderTableCell>,
        rows: Vec<Vec<RenderTableCell>>,
        alignments: Vec<TableAlignment>,
        source_range: SourceRange,
    },
    Rule {
        source_range: SourceRange,
    },
    RawHtml {
        html: String,
        source_range: SourceRange,
    },
    Image {
        alt: String,
        url: String,
        title: Option<String>,
        source_range: SourceRange,
    },
    FootnoteDefinition {
        name: String,
        blocks: Vec<RenderBlock>,
        source_range: SourceRange,
    },
    PlainText {
        text: String,
        source_range: SourceRange,
    },
    Raw {
        kind: String,
        source: String,
        source_range: SourceRange,
    },
}

impl RenderBlock {
    pub const fn source_range(&self) -> SourceRange {
        match self {
            Self::Heading { source_range, .. }
            | Self::Paragraph { source_range, .. }
            | Self::Quote { source_range, .. }
            | Self::CodeBlock { source_range, .. }
            | Self::BulletList { source_range, .. }
            | Self::OrderedList { source_range, .. }
            | Self::TaskList { source_range, .. }
            | Self::Table { source_range, .. }
            | Self::Rule { source_range }
            | Self::RawHtml { source_range, .. }
            | Self::Image { source_range, .. }
            | Self::FootnoteDefinition { source_range, .. }
            | Self::PlainText { source_range, .. }
            | Self::Raw { source_range, .. } => *source_range,
        }
    }
}

fn render_plain_text(source: &str) -> Vec<RenderBlock> {
    if source.is_empty() {
        Vec::new()
    } else {
        vec![RenderBlock::PlainText {
            text: source.to_owned(),
            source_range: SourceRange::new(0, source.len()),
        }]
    }
}

fn apply_insert_text(state: &mut EditorState, text: &str) -> bool {
    if matches!(state.buffer, EditorBuffer::Source(_)) {
        return insert_text(state, text);
    }
    let before = state.snapshot();
    if let EditorBuffer::Rich(buffer) = &mut state.buffer {
        buffer.editor.insert_text(text);
    }
    state.commit_edit(before, EditKind::Insert)
}

fn apply_backspace(state: &mut EditorState) -> bool {
    if matches!(state.buffer, EditorBuffer::Source(_)) {
        return backspace(state);
    }
    let before = state.snapshot();
    if let EditorBuffer::Rich(buffer) = &mut state.buffer {
        buffer.editor.backspace();
    }
    state.commit_edit(before, EditKind::Delete)
}

fn apply_delete_forward(state: &mut EditorState) -> bool {
    if matches!(state.buffer, EditorBuffer::Source(_)) {
        return delete_forward(state);
    }
    let before = state.snapshot();
    if let EditorBuffer::Rich(buffer) = &mut state.buffer {
        buffer.editor.delete_forward();
    }
    state.commit_edit(before, EditKind::Delete)
}

fn apply_delete_selection(state: &mut EditorState) -> bool {
    if matches!(state.buffer, EditorBuffer::Source(_)) {
        return delete_selection(state, EditKind::Delete);
    }
    let before = state.snapshot();
    if let EditorBuffer::Rich(buffer) = &mut state.buffer {
        buffer.editor.delete_selection();
    }
    state.commit_edit(before, EditKind::Delete)
}

fn insert_text(state: &mut EditorState, text: &str) -> bool {
    if text.is_empty() && state.selected_range().is_none() {
        return false;
    }
    let range = active_edit_range(state);
    let before = state.snapshot();
    state.replace_edit_range(range.clone(), text);
    state.cursor.byte_offset = range.start + text.len();
    state.cursor.preferred_column = None;
    state.selection = None;
    state.commit_edit(before, EditKind::Insert)
}

fn backspace(state: &mut EditorState) -> bool {
    if state.selected_range().is_some() {
        return delete_selection(state, EditKind::Delete);
    }
    let cursor = state.cursor.byte_offset;
    if cursor == 0 {
        return false;
    }
    let start = previous_grapheme_boundary(state.source(), cursor);
    replace_for_delete(state, start..cursor)
}

fn delete_forward(state: &mut EditorState) -> bool {
    if state.selected_range().is_some() {
        return delete_selection(state, EditKind::Delete);
    }
    let cursor = state.cursor.byte_offset;
    if cursor >= state.source().len() {
        return false;
    }
    let end = next_grapheme_boundary(state.source(), cursor);
    replace_for_delete(state, cursor..end)
}

fn delete_selection(state: &mut EditorState, kind: EditKind) -> bool {
    let Some(range) = state.selected_range() else {
        return false;
    };
    let range = semantic_delete_range(state, range.start..range.end);
    let before = state.snapshot();
    state.replace_edit_range(range.clone(), "");
    state.cursor.byte_offset = range.start;
    state.cursor.preferred_column = None;
    state.selection = None;
    state.commit_edit(before, kind)
}

fn replace_for_delete(state: &mut EditorState, range: Range<usize>) -> bool {
    let range = semantic_delete_range(state, range);
    let before = state.snapshot();
    let start = range.start;
    state.replace_edit_range(range, "");
    state.cursor.byte_offset = start;
    state.cursor.preferred_column = None;
    state.selection = None;
    state.commit_edit(before, EditKind::Delete)
}

fn semantic_delete_range(_state: &EditorState, range: Range<usize>) -> Range<usize> {
    range
}

fn active_edit_range(state: &EditorState) -> Range<usize> {
    state
        .selected_range()
        .map(Range::<usize>::from)
        .unwrap_or(state.cursor.byte_offset..state.cursor.byte_offset)
}

fn move_cursor(state: &mut EditorState, movement: CursorMove, extend_selection: bool) {
    if !extend_selection
        && let Some(selection) = state
            .selection
            .filter(|selection| !selection.is_collapsed())
    {
        let range = selection.range();
        match movement {
            CursorMove::Left | CursorMove::WordLeft | CursorMove::LineStart => {
                state.cursor.byte_offset = range.start;
                state.cursor.preferred_column = None;
                state.selection = None;
                return;
            }
            CursorMove::Right | CursorMove::WordRight | CursorMove::LineEnd => {
                state.cursor.byte_offset = range.end;
                state.cursor.preferred_column = None;
                state.selection = None;
                return;
            }
            _ => {}
        }
    }

    let source = state.source();
    let current = state.cursor.byte_offset;
    let mut preferred_column = None;
    let target = match movement {
        CursorMove::Left => previous_grapheme_boundary(source, current),
        CursorMove::Right => next_grapheme_boundary(source, current),
        CursorMove::WordLeft => previous_word_boundary(source, current),
        CursorMove::WordRight => next_word_boundary(source, current),
        CursorMove::DocumentStart => 0,
        CursorMove::DocumentEnd => source.len(),
        CursorMove::LineStart => {
            let (line, _) = line_column_for_offset(source, current);
            line_ranges(source).get(line).map_or(0, |range| range.start)
        }
        CursorMove::LineEnd => {
            let (line, _) = line_column_for_offset(source, current);
            line_ranges(source)
                .get(line)
                .map_or(source.len(), |range| range.end)
        }
        CursorMove::Up | CursorMove::Down => {
            let lines = line_ranges(source);
            let (line, column) = line_column_for_offset(source, current);
            let desired_column = state.cursor.preferred_column.unwrap_or(column);
            preferred_column = Some(desired_column);
            let target_line = if movement == CursorMove::Up {
                line.saturating_sub(1)
            } else {
                (line + 1).min(lines.len().saturating_sub(1))
            };
            lines
                .get(target_line)
                .map(|range| byte_at_grapheme_column(source, range, desired_column))
                .unwrap_or(current)
        }
    };
    move_to(state, target, extend_selection);
    state.cursor.preferred_column = preferred_column;
}

fn move_to(state: &mut EditorState, byte_offset: usize, extend_selection: bool) {
    let target = normalize_position(state.source(), byte_offset);
    let old_cursor = state.cursor.byte_offset;
    if extend_selection {
        let anchor = state
            .selection
            .map_or(old_cursor, |selection| selection.anchor);
        state.selection = (anchor != target).then_some(Selection::new(anchor, target));
    } else {
        state.selection = None;
    }
    state.cursor.byte_offset = target;
    state.cursor.preferred_column = None;
}

fn undo(state: &mut EditorState) -> bool {
    let Some(transaction) = state.undo_stack.last() else {
        return false;
    };
    let mode_matches = matches!(
        (&state.buffer, &transaction.before),
        (EditorBuffer::Rich(_), EditorSnapshot::Rich { .. })
            | (EditorBuffer::Source(_), EditorSnapshot::Source { .. })
    );
    if !mode_matches {
        return false;
    }
    let transaction = state.undo_stack.pop().expect("last transaction exists");
    state.restore_snapshot(&transaction.before);
    state.redo_stack.push(transaction);
    true
}

fn redo(state: &mut EditorState) -> bool {
    let Some(transaction) = state.redo_stack.last() else {
        return false;
    };
    let mode_matches = matches!(
        (&state.buffer, &transaction.after),
        (EditorBuffer::Rich(_), EditorSnapshot::Rich { .. })
            | (EditorBuffer::Source(_), EditorSnapshot::Source { .. })
    );
    if !mode_matches {
        return false;
    }
    let transaction = state.redo_stack.pop().expect("last transaction exists");
    state.restore_snapshot(&transaction.after);
    state.undo_stack.push(transaction);
    true
}

fn line_ranges(source: &str) -> Vec<Range<usize>> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                ranges.push(start..index);
                index += 2;
                start = index;
            }
            b'\r' | b'\n' => {
                ranges.push(start..index);
                index += 1;
                start = index;
            }
            _ => index += 1,
        }
    }
    ranges.push(start..source.len());
    ranges
}

fn line_column_for_offset(source: &str, byte_offset: usize) -> (usize, usize) {
    let offset = normalize_position(source, byte_offset);
    let lines = line_ranges(source);
    let line_index = line_index_for_offset(&lines, offset);
    let Some(line) = lines.get(line_index) else {
        return (0, 0);
    };
    let end = offset.min(line.end);
    let column = source[line.start..end].graphemes(true).count();
    (line_index, column)
}

fn line_index_for_offset(lines: &[Range<usize>], offset: usize) -> usize {
    lines
        .iter()
        .position(|range| offset <= range.end)
        .unwrap_or_else(|| lines.len().saturating_sub(1))
}

fn byte_at_grapheme_column(source: &str, line: &Range<usize>, column: usize) -> usize {
    source[line.clone()]
        .grapheme_indices(true)
        .nth(column)
        .map_or(line.end, |(offset, _)| line.start + offset)
}

fn previous_grapheme_boundary(source: &str, byte_offset: usize) -> usize {
    let offset = normalize_position(source, byte_offset);
    source[..offset]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_grapheme_boundary(source: &str, byte_offset: usize) -> usize {
    let offset = normalize_position(source, byte_offset);
    source[offset..]
        .graphemes(true)
        .next()
        .map_or(source.len(), |grapheme| offset + grapheme.len())
}

fn previous_word_boundary(source: &str, byte_offset: usize) -> usize {
    let offset = normalize_position(source, byte_offset);
    let mut previous = 0usize;
    for (start, word) in source.unicode_word_indices() {
        let end = start + word.len();
        if start >= offset {
            break;
        }
        if offset <= end {
            return start;
        }
        previous = start;
    }
    previous
}

fn next_word_boundary(source: &str, byte_offset: usize) -> usize {
    let offset = normalize_position(source, byte_offset);
    for (start, word) in source.unicode_word_indices() {
        let end = start + word.len();
        if offset < start {
            return start;
        }
        if offset < end {
            return end;
        }
    }
    source.len()
}

fn validated_range(source: &str, range: Range<usize>) -> Option<Range<usize>> {
    (range.start <= range.end
        && range.end <= source.len()
        && is_cursor_boundary(source, range.start)
        && is_cursor_boundary(source, range.end))
    .then_some(range)
}

fn is_cursor_boundary(source: &str, position: usize) -> bool {
    position <= source.len()
        && source.is_char_boundary(position)
        && !(position > 0
            && position < source.len()
            && source.as_bytes()[position - 1] == b'\r'
            && source.as_bytes()[position] == b'\n')
}

fn normalize_position(source: &str, position: usize) -> usize {
    let mut position = position.min(source.len());
    while !source.is_char_boundary(position) {
        position = position.saturating_sub(1);
    }
    if position > 0
        && position < source.len()
        && source.as_bytes()[position - 1] == b'\r'
        && source.as_bytes()[position] == b'\n'
    {
        position -= 1;
    }
    position
}
