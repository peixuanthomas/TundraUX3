//! Domain model for Tundra's single-document terminal editor.
//!
//! Source mode edits the canonical document. Rich mode edits an isolated
//! working buffer and only synchronizes it at explicit mode/save boundaries.
//! This keeps live UI input away from the persisted Markdown snapshot.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use ropey::{Rope, RopeSlice};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete, UnicodeSegmentation};
use unicode_width::UnicodeWidthStr;

use crate::markdown_codec::{MarkdownCodec, MarkdownExport};
use crate::rich_document::{
    InlineContent, InlineNode, NodeId, ProjectedBlock, ProjectedBlockKind, ProjectedInline,
    RewriteState, RichBlock, RichBlockKind, RichDocument, RichLineEnding, RichListKind,
    RichPosition, RichProjection, RichTableAlignment,
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

/// Controls whether an editor document may be changed through domain commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorAccess {
    #[default]
    Editable,
    ReadOnly,
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

/// Returns whether a path names a plain or numerically rotated log file.
/// Matching is case-insensitive for the `.log` marker; compressed and named
/// rotations such as `.log.gz` or `.log.old` are intentionally excluded.
pub fn is_log_document_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("log"))
    {
        return true;
    }
    let Some((prefix, rotation)) = name.rsplit_once('.') else {
        return false;
    };
    !rotation.is_empty()
        && rotation.bytes().all(|byte| byte.is_ascii_digit())
        && prefix
            .rsplit_once('.')
            .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("log"))
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
    source: DocumentText,
    saved_source: DocumentText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DocumentText {
    Contiguous(Arc<String>),
    Rope {
        text: Rope,
        word_count: usize,
        line_endings: LineEndingStats,
    },
}

impl DocumentText {
    fn new(kind: DocumentKind, source: String) -> Self {
        match kind {
            DocumentKind::Markdown => Self::Contiguous(Arc::new(source)),
            DocumentKind::PlainText => Self::Rope {
                text: Rope::from_str(&source),
                word_count: source.unicode_words().count(),
                line_endings: LineEndingStats::from_text(&source),
            },
        }
    }

    fn from_arc(kind: DocumentKind, source: Arc<String>) -> Self {
        match kind {
            DocumentKind::Markdown => Self::Contiguous(source),
            DocumentKind::PlainText => Self::Rope {
                text: Rope::from_str(source.as_str()),
                word_count: source.unicode_words().count(),
                line_endings: LineEndingStats::from_text(source.as_str()),
            },
        }
    }

    fn text(&self) -> Cow<'_, str> {
        match self {
            Self::Contiguous(source) => Cow::Borrowed(source.as_str()),
            Self::Rope { text, .. } => Cow::from(text),
        }
    }

    fn rope(&self) -> Option<(&Rope, usize, LineEndingStats)> {
        match self {
            Self::Rope {
                text,
                word_count,
                line_endings,
            } => Some((text, *word_count, *line_endings)),
            Self::Contiguous(_) => None,
        }
    }

    fn as_contiguous(&self) -> Arc<String> {
        match self {
            Self::Contiguous(source) => Arc::clone(source),
            Self::Rope { text, .. } => Arc::new(String::from(text)),
        }
    }
}

impl EditorDocument {
    pub fn untitled(kind: DocumentKind) -> Self {
        let source = DocumentText::new(kind, String::new());
        Self {
            path: None,
            kind,
            metadata: TextMetadata::default(),
            saved_source: source.clone(),
            source,
        }
    }

    pub fn from_text(path: Option<PathBuf>, kind: DocumentKind, source: impl Into<String>) -> Self {
        let source = source.into();
        let metadata = TextMetadata::from_source(&source, false);
        let source = DocumentText::new(kind, source);
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
        Ok(Self::from_decoded_text(path, kind, source, utf8_bom))
    }

    /// Decodes an owned UTF-8 buffer without copying its body for the common
    /// no-BOM case. This is intended for asynchronous file readers that can
    /// transfer ownership of their completed byte buffer to the editor.
    pub fn from_owned_bytes(
        path: Option<PathBuf>,
        kind: DocumentKind,
        mut bytes: Vec<u8>,
    ) -> Result<Self, DocumentDecodeError> {
        let utf8_bom = bytes.starts_with(UTF8_BOM);
        if utf8_bom {
            bytes.drain(..UTF8_BOM.len());
        }
        let source = String::from_utf8(bytes).map_err(|error| DocumentDecodeError {
            valid_up_to: error.utf8_error().valid_up_to(),
        })?;
        Ok(Self::from_decoded_text(path, kind, source, utf8_bom))
    }

    fn from_decoded_text(
        path: Option<PathBuf>,
        kind: DocumentKind,
        source: String,
        utf8_bom: bool,
    ) -> Self {
        let metadata = TextMetadata::from_source(&source, utf8_bom);
        let source = DocumentText::new(kind, source);
        Self {
            path,
            kind,
            metadata,
            saved_source: source.clone(),
            source,
        }
    }

    pub fn open(path: impl Into<PathBuf>, bytes: &[u8]) -> Result<Self, DocumentDecodeError> {
        let path = path.into();
        let kind = DocumentKind::from_path(&path);
        Self::from_bytes(Some(path), kind, bytes)
    }

    pub fn open_owned(
        path: impl Into<PathBuf>,
        bytes: Vec<u8>,
    ) -> Result<Self, DocumentDecodeError> {
        let path = path.into();
        let kind = DocumentKind::from_path(&path);
        Self::from_owned_bytes(Some(path), kind, bytes)
    }

    pub fn source(&self) -> Cow<'_, str> {
        self.source.text()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let source_len = match &self.source {
            DocumentText::Contiguous(source) => source.len(),
            DocumentText::Rope { text, .. } => text.len_bytes(),
        };
        let mut bytes = Vec::with_capacity(source_len + usize::from(self.metadata.utf8_bom) * 3);
        if self.metadata.utf8_bom {
            bytes.extend_from_slice(UTF8_BOM);
        }
        match &self.source {
            DocumentText::Contiguous(source) => bytes.extend_from_slice(source.as_bytes()),
            DocumentText::Rope { text, .. } => {
                for chunk in text.chunks() {
                    bytes.extend_from_slice(chunk.as_bytes());
                }
            }
        }
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
        let storage_matches_kind = matches!(
            (&self.source, self.kind),
            (DocumentText::Contiguous(_), DocumentKind::Markdown)
                | (DocumentText::Rope { .. }, DocumentKind::PlainText)
        );
        if !storage_matches_kind {
            self.source = DocumentText::new(self.kind, self.source.text().into_owned());
        }
        self.saved_source = self.source.clone();
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
        match &self.source {
            DocumentText::Contiguous(source) => line_ranges(source).len(),
            DocumentText::Rope { text, .. } => text.len_lines(),
        }
    }

    pub fn word_count(&self) -> usize {
        match &self.source {
            DocumentText::Contiguous(source) => source.unicode_words().count(),
            DocumentText::Rope { word_count, .. } => *word_count,
        }
    }

    fn restore_source(&mut self, source: String) {
        self.source = DocumentText::new(self.kind, source);
        let fallback = self.metadata.preferred_line_ending;
        let source = self.source();
        self.metadata =
            TextMetadata::from_source_with_fallback(&source, self.metadata.utf8_bom, fallback);
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

impl EditorCommand {
    /// Whether this command may run against a read-only editor. Read-only
    /// documents retain view operations (including mode changes), selection,
    /// copying, and closing while rejecting edit and file-operation commands.
    pub const fn is_allowed_in_read_only(&self) -> bool {
        matches!(
            self,
            Self::MoveCursor { .. }
                | Self::MoveTo { .. }
                | Self::SelectAll
                | Self::ClearSelection
                | Self::SetMode(_)
                | Self::ToggleMode
                | Self::Copy
                | Self::RequestClose
        )
    }
}

/// Immutable, owned content exported from one exact editor revision.
#[derive(Debug, Clone)]
pub struct SaveSnapshot {
    pub revision: u64,
    payload: Arc<SavePayload>,
    prepared: Arc<OnceLock<PreparedSave>>,
}

impl PartialEq for SaveSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision && self.payload == other.payload
    }
}

impl Eq for SaveSnapshot {}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SavePayload {
    Source {
        text: Rope,
        word_count: usize,
        line_endings: LineEndingStats,
        utf8_bom: bool,
    },
    Rich {
        document: RichDocument,
        fallback: Arc<String>,
        utf8_bom: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreparedSave {
    source: Option<Arc<String>>,
    export: Option<MarkdownExport>,
}

impl SaveSnapshot {
    fn source(
        revision: u64,
        text: Rope,
        word_count: usize,
        line_endings: LineEndingStats,
        utf8_bom: bool,
    ) -> Self {
        Self {
            revision,
            payload: Arc::new(SavePayload::Source {
                text,
                word_count,
                line_endings,
                utf8_bom,
            }),
            prepared: Arc::new(OnceLock::new()),
        }
    }

    fn rich(revision: u64, document: RichDocument, fallback: Arc<String>, utf8_bom: bool) -> Self {
        Self {
            revision,
            payload: Arc::new(SavePayload::Rich {
                document,
                fallback,
                utf8_bom,
            }),
            prepared: Arc::new(OnceLock::new()),
        }
    }

    /// Serializes this exact revision to a writer. Source buffers are streamed
    /// directly from Ropey chunks. Rich Markdown export occurs here, allowing
    /// callers to invoke this method on a background worker.
    pub fn write_to(&self, writer: &mut dyn Write) -> io::Result<()> {
        match self.payload.as_ref() {
            SavePayload::Source { text, utf8_bom, .. } => {
                if *utf8_bom {
                    writer.write_all(UTF8_BOM)?;
                }
                for chunk in text.chunks() {
                    writer.write_all(chunk.as_bytes())?;
                }
                let _ = self.prepared.set(PreparedSave {
                    source: None,
                    export: None,
                });
            }
            SavePayload::Rich {
                document,
                fallback,
                utf8_bom,
            } => {
                let prepared =
                    self.prepared
                        .get_or_init(|| match MarkdownCodec::export(document) {
                            Ok(export) => PreparedSave {
                                source: Some(Arc::new(export.markdown.clone())),
                                export: Some(export),
                            },
                            Err(_) => PreparedSave {
                                source: Some(Arc::clone(fallback)),
                                export: None,
                            },
                        });
                if *utf8_bom {
                    writer.write_all(UTF8_BOM)?;
                }
                writer.write_all(
                    prepared
                        .source
                        .as_deref()
                        .expect("Rich preparation always has source")
                        .as_bytes(),
                )?;
            }
        }
        Ok(())
    }

    /// Convenience materialization for tests and non-streaming integrations.
    pub fn to_bytes(&self) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.write_to(&mut bytes)?;
        Ok(bytes)
    }

    fn prepared(&self) -> &PreparedSave {
        self.prepared.get_or_init(|| match self.payload.as_ref() {
            SavePayload::Source { .. } => PreparedSave {
                source: None,
                export: None,
            },
            SavePayload::Rich {
                document, fallback, ..
            } => match MarkdownCodec::export(document) {
                Ok(export) => PreparedSave {
                    source: Some(Arc::new(export.markdown.clone())),
                    export: Some(export),
                },
                Err(_) => PreparedSave {
                    source: Some(Arc::clone(fallback)),
                    export: None,
                },
            },
        })
    }
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
    text: Rope,
    word_count: usize,
    line_endings: LineEndingStats,
    non_ascii_bytes: usize,
    display_widths: SourceDisplayWidthCache,
}

/// Derived Source-line metrics. The histogram makes the global maximum an
/// O(1) query while allowing an edit to replace only the widths of the lines
/// it touches. As derived state, it does not participate in document value
/// equality.
#[derive(Debug, Clone, Default)]
struct SourceDisplayWidthCache {
    line_width_counts: BTreeMap<usize, usize>,
    max_width: usize,
}

impl SourceDisplayWidthCache {
    fn insert(&mut self, width: usize) {
        *self.line_width_counts.entry(width).or_default() += 1;
        self.max_width = self.max_width.max(width);
    }

    fn remove(&mut self, width: usize) {
        let count = self
            .line_width_counts
            .get_mut(&width)
            .expect("cached Source line width must exist");
        let remove_entry = {
            *count = count.saturating_sub(1);
            *count == 0
        };
        if remove_entry {
            self.line_width_counts.remove(&width);
        }
        if width == self.max_width && !self.line_width_counts.contains_key(&width) {
            self.max_width = self
                .line_width_counts
                .last_key_value()
                .map_or(0, |(width, _)| *width);
        }
    }

    const fn max_width(&self) -> usize {
        self.max_width
    }
}

impl PartialEq for SourceDisplayWidthCache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for SourceDisplayWidthCache {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct LineEndingStats {
    lf: usize,
    crlf: usize,
    cr: usize,
}

/// A derived-value cache must not participate in document identity. Keeping
/// equality independent from whether a caller happened to request the word
/// count also preserves `EditorState`'s existing value semantics.
#[derive(Debug, Default)]
struct RichWordCountCache {
    value: Mutex<Option<(u64, usize)>>,
}

impl RichWordCountCache {
    fn get(&self) -> Option<(u64, usize)> {
        *self
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn set(&self, value: Option<(u64, usize)>) {
        *self
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = value;
    }
}

impl Clone for RichWordCountCache {
    fn clone(&self) -> Self {
        Self {
            value: Mutex::new(self.get()),
        }
    }
}

impl PartialEq for RichWordCountCache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for RichWordCountCache {}

impl LineEndingStats {
    fn from_text(text: &str) -> Self {
        let mut stats = Self::default();
        let bytes = text.as_bytes();
        let mut index = 0usize;
        while index < bytes.len() {
            match bytes[index] {
                b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                    stats.crlf += 1;
                    index += 2;
                }
                b'\r' => {
                    stats.cr += 1;
                    index += 1;
                }
                b'\n' => {
                    stats.lf += 1;
                    index += 1;
                }
                _ => index += 1,
            }
        }
        stats
    }

    fn replace_window(&mut self, old: Self, new: Self) {
        self.lf = self.lf.saturating_sub(old.lf).saturating_add(new.lf);
        self.crlf = self.crlf.saturating_sub(old.crlf).saturating_add(new.crlf);
        self.cr = self.cr.saturating_sub(old.cr).saturating_add(new.cr);
    }
}

/// One materialized Source-mode line. Text is copied only for the requested
/// line window; the canonical document remains in the rope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLine {
    pub line_index: usize,
    pub byte_range: SourceRange,
    pub text: String,
}

/// A horizontally clipped Source-mode line suitable for direct viewport
/// rendering. Both ranges use canonical UTF-8 byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceViewportLine {
    pub line_index: usize,
    pub line_byte_range: SourceRange,
    pub visible_byte_range: SourceRange,
    pub start_column: usize,
    pub end_column: usize,
    pub truncated_left: bool,
    pub truncated_right: bool,
    pub text: String,
}

impl SourceBuffer {
    pub fn from_text(text: impl AsRef<str>) -> Self {
        let text = text.as_ref();
        let mut buffer = Self {
            text: Rope::from_str(text),
            word_count: text.unicode_words().count(),
            line_endings: LineEndingStats::from_text(text),
            non_ascii_bytes: non_ascii_byte_count(text),
            display_widths: SourceDisplayWidthCache::default(),
        };
        buffer.rebuild_display_width_cache();
        buffer
    }

    fn from_rope(text: &Rope, word_count: usize, line_endings: LineEndingStats) -> Self {
        let mut buffer = Self {
            text: text.clone(),
            word_count,
            line_endings,
            non_ascii_bytes: text.chunks().map(non_ascii_byte_count).sum(),
            display_widths: SourceDisplayWidthCache::default(),
        };
        buffer.rebuild_display_width_cache();
        buffer
    }

    pub fn len_bytes(&self) -> usize {
        self.text.len_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.text.len_bytes() == 0
    }

    pub fn line_count(&self) -> usize {
        self.text.len_lines()
    }

    pub fn word_count(&self) -> usize {
        self.word_count
    }

    pub fn max_display_width(&self) -> usize {
        self.display_widths.max_width()
    }

    fn line_display_widths(&self, line_range: Range<usize>) -> Vec<usize> {
        let start = line_range.start.min(self.line_count());
        let end = line_range.end.min(self.line_count()).max(start);
        self.text
            .lines_at(start)
            .take(end.saturating_sub(start))
            .map(|line| rope_slice_display_width(source_line_content(line)))
            .collect()
    }

    fn rebuild_display_width_cache(&mut self) {
        let mut display_widths = SourceDisplayWidthCache::default();
        // `Rope::lines` advances sequentially through the tree. In
        // particular, an all-ASCII log uses only cached RopeSlice lengths and
        // never performs one `line_to_byte` lookup per short line.
        for line in self.text.lines() {
            display_widths.insert(rope_slice_display_width(source_line_content(line)));
        }
        self.display_widths = display_widths;
    }

    fn metadata(&self, utf8_bom: bool, fallback: LineEnding) -> TextMetadata {
        let distinct = usize::from(self.line_endings.lf > 0)
            + usize::from(self.line_endings.crlf > 0)
            + usize::from(self.line_endings.cr > 0);
        let preferred_line_ending = [
            (self.line_endings.lf, LineEnding::Lf),
            (self.line_endings.crlf, LineEnding::CrLf),
            (self.line_endings.cr, LineEnding::Cr),
        ]
        .into_iter()
        .max_by_key(|(count, _)| *count)
        .filter(|(count, _)| *count > 0)
        .map(|(_, ending)| ending)
        .unwrap_or(fallback);
        TextMetadata {
            utf8_bom,
            preferred_line_ending,
            mixed_line_endings: distinct > 1,
            has_final_newline: self
                .text
                .get_byte(self.len_bytes().saturating_sub(1))
                .is_some_and(|byte| matches!(byte, b'\r' | b'\n')),
        }
    }

    /// Returns the complete source, borrowing only when Ropey stores it in a
    /// single contiguous leaf. Callers rendering a viewport should use
    /// [`Self::lines`] instead to avoid flattening large documents.
    pub fn text(&self) -> Cow<'_, str> {
        Cow::from(&self.text)
    }

    pub fn to_string(&self) -> String {
        String::from(&self.text)
    }

    pub fn byte_slice(&self, range: SourceRange) -> Option<String> {
        self.validated_range(range.start..range.end)
            .map(|range| self.text.byte_slice(range).to_string())
    }

    /// UTF-8 byte range of a line's visible content, excluding LF, CRLF, or
    /// CR. The final empty line after a trailing newline is retained.
    pub fn line_range(&self, line_index: usize) -> Option<SourceRange> {
        if line_index >= self.line_count() {
            return None;
        }
        let start = self.text.line_to_byte(line_index);
        let mut end = if line_index + 1 < self.line_count() {
            self.text.line_to_byte(line_index + 1)
        } else {
            self.len_bytes()
        };
        if end > start && self.text.byte(end - 1) == b'\n' {
            end -= 1;
            if end > start && self.text.byte(end - 1) == b'\r' {
                end -= 1;
            }
        } else if end > start && self.text.byte(end - 1) == b'\r' {
            end -= 1;
        }
        Some(SourceRange::new(start, end))
    }

    /// Materializes only the requested half-open line range. Out-of-bounds
    /// ends are clamped, making it convenient for viewport + overscan reads.
    pub fn lines(&self, line_range: Range<usize>) -> Vec<SourceLine> {
        let start = line_range.start.min(self.line_count());
        let end = line_range.end.min(self.line_count()).max(start);
        (start..end)
            .filter_map(|line_index| {
                let byte_range = self.line_range(line_index)?;
                let text = self
                    .text
                    .byte_slice(byte_range.start..byte_range.end)
                    .to_string();
                Some(SourceLine {
                    line_index,
                    byte_range,
                    text,
                })
            })
            .collect()
    }

    /// Materializes a bounded vertical and horizontal window. Grapheme
    /// boundaries are resolved directly against Ropey chunks, so a multi-MiB
    /// single line is not flattened merely to display its first screenful.
    pub fn viewport_lines(
        &self,
        line_range: Range<usize>,
        left_column: usize,
        width: usize,
    ) -> Vec<SourceViewportLine> {
        let start = line_range.start.min(self.line_count());
        let end = line_range.end.min(self.line_count()).max(start);
        (start..end)
            .filter_map(|line_index| self.viewport_line(line_index, left_column, width))
            .collect()
    }

    fn viewport_line(
        &self,
        line_index: usize,
        left_column: usize,
        width: usize,
    ) -> Option<SourceViewportLine> {
        let line = self.line_range(line_index)?;
        if self.non_ascii_bytes == 0 && width > 0 {
            return Some(self.ascii_viewport_line(line_index, line, left_column, width));
        }
        let right_column = left_column.saturating_add(width);
        let mut cursor = GraphemeCursor::new(line.start, self.len_bytes(), true);
        let mut byte = line.start;
        let mut column = 0usize;
        let mut visible_start = None;
        let mut visible_end = line.start;
        let mut visible_start_column = left_column;
        let mut visible_end_column = left_column;

        while byte < line.end && width > 0 {
            let Some(next) = next_rope_grapheme_boundary(&self.text, &mut cursor) else {
                break;
            };
            let next = next.min(line.end);
            if next <= byte {
                break;
            }
            let grapheme = self.text.byte_slice(byte..next).to_string();
            let grapheme_width = source_grapheme_display_width(&grapheme);
            let next_column = column.saturating_add(grapheme_width);
            let intersects = (next_column > left_column
                || grapheme_width == 0 && column >= left_column)
                && column < right_column;
            if intersects {
                if visible_start.is_none() {
                    visible_start = Some(byte);
                    visible_start_column = column;
                }
                visible_end = next;
                visible_end_column = next_column;
            } else if visible_start.is_some() && column >= right_column {
                break;
            }
            byte = next;
            column = next_column;
        }

        let visible_start = visible_start.unwrap_or_else(|| line.end.min(byte));
        if visible_start == visible_end {
            visible_end = visible_start;
            visible_start_column = column;
            visible_end_column = column;
        }
        let visible_byte_range = SourceRange::new(visible_start, visible_end);
        let text = self
            .text
            .byte_slice(visible_byte_range.start..visible_byte_range.end)
            .to_string();
        Some(SourceViewportLine {
            line_index,
            line_byte_range: line,
            visible_byte_range,
            start_column: visible_start_column,
            end_column: visible_end_column,
            truncated_left: visible_start > line.start,
            truncated_right: visible_end < line.end,
            text,
        })
    }

    fn ascii_viewport_line(
        &self,
        line_index: usize,
        line: SourceRange,
        left_column: usize,
        width: usize,
    ) -> SourceViewportLine {
        // Every ASCII byte is one extended grapheme and is rendered as one
        // terminal cell, including the control-picture substitution used for
        // otherwise unsafe bytes. Line terminators are excluded by `line`.
        let line_len = line.end.saturating_sub(line.start);
        let start_column = left_column.min(line_len);
        let end_column = left_column.saturating_add(width).min(line_len);
        let visible_byte_range = SourceRange::new(
            line.start.saturating_add(start_column),
            line.start.saturating_add(end_column),
        );
        let text = self
            .text
            .byte_slice(visible_byte_range.start..visible_byte_range.end)
            .to_string();
        SourceViewportLine {
            line_index,
            line_byte_range: line,
            visible_byte_range,
            start_column,
            end_column,
            truncated_left: start_column > 0,
            truncated_right: end_column < line_len,
            text,
        }
    }

    fn validated_range(&self, range: Range<usize>) -> Option<Range<usize>> {
        (range.start <= range.end
            && range.end <= self.len_bytes()
            && self.is_cursor_boundary(range.start)
            && self.is_cursor_boundary(range.end))
        .then_some(range)
    }

    fn is_cursor_boundary(&self, position: usize) -> bool {
        position <= self.len_bytes()
            && self.text.try_byte_to_char(position).is_ok()
            && !(position > 0
                && position < self.len_bytes()
                && self.text.byte(position - 1) == b'\r'
                && self.text.byte(position) == b'\n')
    }

    fn normalize_position(&self, position: usize) -> usize {
        let mut position = position.min(self.len_bytes());
        while self.text.try_byte_to_char(position).is_err() {
            position = position.saturating_sub(1);
        }
        if position > 0
            && position < self.len_bytes()
            && self.text.byte(position - 1) == b'\r'
            && self.text.byte(position) == b'\n'
        {
            position -= 1;
        }
        position
    }

    fn replace_range(&mut self, range: Range<usize>, replacement: &str) -> Option<String> {
        let range = self.validated_range(range)?;
        self.replace_validated_range(range, replacement)
    }

    /// History may need to undo an edit that created a CRLF pair. One edge of
    /// the inverse range can therefore sit between CR and LF even though a UI
    /// cursor is never allowed there.
    fn replace_history_range(&mut self, range: Range<usize>, replacement: &str) -> Option<String> {
        if range.start > range.end
            || range.end > self.len_bytes()
            || self.text.try_byte_to_char(range.start).is_err()
            || self.text.try_byte_to_char(range.end).is_err()
        {
            return None;
        }
        self.replace_validated_range(range, replacement)
    }

    fn replace_validated_range(
        &mut self,
        range: Range<usize>,
        replacement: &str,
    ) -> Option<String> {
        let start_char = self.text.byte_to_char(range.start);
        let end_char = self.text.byte_to_char(range.end);
        let context_start = self.text.char_to_byte(start_char.saturating_sub(1));
        let context_end = self
            .text
            .char_to_byte((end_char + 1).min(self.text.len_chars()));
        let suffix_context_len = context_end.saturating_sub(range.end);
        let old_line_endings = LineEndingStats::from_text(
            &self.text.byte_slice(context_start..context_end).to_string(),
        );
        let old_start_line = self.text.byte_to_line(range.start);
        let old_end_line = self.text.byte_to_line(range.end);
        // Keep stable byte anchors around the edit. History ranges are
        // allowed to start between CR and LF, where `byte_to_line` can change
        // after the edit even though the neighboring content lines do not.
        let old_display_start_line = old_start_line.saturating_sub(1);
        let old_display_end_line = old_end_line.saturating_add(2).min(self.line_count());
        let display_window_start_byte = self.text.line_to_byte(old_display_start_line);
        let display_window_end_byte = if old_display_end_line < self.line_count() {
            self.text.line_to_byte(old_display_end_line)
        } else {
            self.len_bytes()
        };
        let old_line_widths =
            self.line_display_widths(old_display_start_line..old_display_end_line);
        let old_window_start = self.line_range(old_start_line.saturating_sub(1))?.start;
        let old_window_end = self
            .line_range((old_end_line + 1).min(self.line_count().saturating_sub(1)))?
            .end;
        let word_suffix_len = old_window_end.saturating_sub(range.end);
        let old_word_count = self
            .text
            .byte_slice(old_window_start..old_window_end)
            .to_string()
            .unicode_words()
            .count();
        let removed = self.text.byte_slice(range.clone()).to_string();
        self.text.remove(start_char..end_char);
        self.text.insert(start_char, replacement);
        let new_end = range.start + replacement.len();
        let new_context_end = new_end
            .saturating_add(suffix_context_len)
            .min(self.len_bytes());
        let new_line_endings = LineEndingStats::from_text(
            &self
                .text
                .byte_slice(context_start..new_context_end)
                .to_string(),
        );
        self.line_endings
            .replace_window(old_line_endings, new_line_endings);
        let new_window_start = old_window_start;
        let new_window_end = new_end
            .saturating_add(word_suffix_len)
            .min(self.len_bytes());
        let new_word_count = self
            .text
            .byte_slice(new_window_start..new_window_end)
            .to_string()
            .unicode_words()
            .count();
        self.word_count = self
            .word_count
            .saturating_sub(old_word_count)
            .saturating_add(new_word_count);
        self.non_ascii_bytes = self
            .non_ascii_bytes
            .saturating_sub(non_ascii_byte_count(&removed))
            .saturating_add(non_ascii_byte_count(replacement));
        let new_display_start_line = self
            .text
            .byte_to_line(display_window_start_byte.min(self.len_bytes()));
        let new_display_end_byte = display_window_end_byte
            .saturating_sub(range.end.saturating_sub(range.start))
            .saturating_add(replacement.len())
            .min(self.len_bytes());
        let new_display_end_line = if new_display_end_byte == self.len_bytes() {
            self.line_count()
        } else {
            self.text.byte_to_line(new_display_end_byte)
        };
        let new_line_widths =
            self.line_display_widths(new_display_start_line..new_display_end_line);
        for width in old_line_widths {
            self.display_widths.remove(width);
        }
        for width in new_line_widths {
            self.display_widths.insert(width);
        }
        Some(removed)
    }

    fn previous_grapheme_boundary(&self, byte_offset: usize) -> usize {
        let offset = self.normalize_position(byte_offset);
        if offset == 0 {
            return 0;
        }
        let mut cursor = GraphemeCursor::new(offset, self.len_bytes(), true);
        previous_rope_grapheme_boundary(&self.text, &mut cursor).unwrap_or(offset)
    }

    fn next_grapheme_boundary(&self, byte_offset: usize) -> usize {
        let offset = self.normalize_position(byte_offset);
        if offset >= self.len_bytes() {
            return self.len_bytes();
        }
        let mut cursor = GraphemeCursor::new(offset, self.len_bytes(), true);
        next_rope_grapheme_boundary(&self.text, &mut cursor).unwrap_or(offset)
    }

    fn previous_word_boundary(&self, byte_offset: usize) -> usize {
        let offset = self.normalize_position(byte_offset);
        let mut line_index = self.text.byte_to_line(offset);
        loop {
            let Some(line) = self.line_range(line_index) else {
                return 0;
            };
            let text = self.text.byte_slice(line.start..line.end).to_string();
            let local_offset = if line_index == self.text.byte_to_line(offset) {
                offset.saturating_sub(line.start).min(text.len())
            } else {
                text.len()
            };
            let mut previous = None;
            for (start, word) in text.unicode_word_indices() {
                let end = start + word.len();
                if start >= local_offset {
                    break;
                }
                if local_offset <= end {
                    return line.start + start;
                }
                previous = Some(start);
            }
            if let Some(previous) = previous {
                return line.start + previous;
            }
            let Some(previous_line) = line_index.checked_sub(1) else {
                return 0;
            };
            line_index = previous_line;
        }
    }

    fn next_word_boundary(&self, byte_offset: usize) -> usize {
        let offset = self.normalize_position(byte_offset);
        let initial_line = self.text.byte_to_line(offset);
        for line_index in initial_line..self.line_count() {
            let Some(line) = self.line_range(line_index) else {
                break;
            };
            let text = self.text.byte_slice(line.start..line.end).to_string();
            let local_offset = if line_index == initial_line {
                offset.saturating_sub(line.start).min(text.len())
            } else {
                0
            };
            for (start, word) in text.unicode_word_indices() {
                let end = start + word.len();
                if local_offset < start {
                    return line.start + start;
                }
                if local_offset < end {
                    return line.start + end;
                }
            }
        }
        self.len_bytes()
    }

    fn byte_at_grapheme_column(&self, line_index: usize, column: usize) -> Option<usize> {
        let line = self.line_range(line_index)?;
        let mut byte = line.start;
        let mut cursor = GraphemeCursor::new(byte, self.len_bytes(), true);
        for _ in 0..column {
            let Some(next) = next_rope_grapheme_boundary(&self.text, &mut cursor) else {
                return Some(byte.min(line.end));
            };
            if next > line.end || next <= byte {
                return Some(line.end);
            }
            byte = next;
        }
        Some(byte.min(line.end))
    }

    fn line_grapheme_column(&self, line: SourceRange, byte_offset: usize) -> usize {
        let end = byte_offset.min(line.end);
        if end <= line.start {
            return 0;
        }
        let mut byte = line.start;
        let mut column = 0usize;
        let mut cursor = GraphemeCursor::new(byte, self.len_bytes(), true);
        while byte < end {
            let Some(next) = next_rope_grapheme_boundary(&self.text, &mut cursor) else {
                break;
            };
            if next <= byte {
                break;
            }
            column = column.saturating_add(1);
            if next >= end || next > line.end {
                break;
            }
            byte = next;
        }
        column
    }

    fn line_display_column(&self, line: SourceRange, byte_offset: usize) -> usize {
        let end = byte_offset.min(line.end);
        if end <= line.start {
            return 0;
        }
        rope_slice_display_width(self.text.byte_slice(line.start..end))
    }
}

fn source_line_content(line: RopeSlice<'_>) -> RopeSlice<'_> {
    let mut end = line.len_bytes();
    if end > 0 && line.byte(end - 1) == b'\n' {
        end -= 1;
        if end > 0 && line.byte(end - 1) == b'\r' {
            end -= 1;
        }
    } else if end > 0 && line.byte(end - 1) == b'\r' {
        end -= 1;
    }
    line.byte_slice(..end)
}

/// Computes terminal-cell width directly over Rope chunks. Contiguous
/// graphemes stay borrowed; only a grapheme that itself crosses a chunk
/// boundary is materialized.
fn rope_slice_display_width(text: RopeSlice<'_>) -> usize {
    if text.len_bytes() == text.len_chars() {
        return text.len_bytes();
    }
    if let Some(text) = text.as_str() {
        return text
            .graphemes(true)
            .map(source_grapheme_display_width)
            .fold(0usize, usize::saturating_add);
    }

    let text_len = text.len_bytes();
    let mut chunks = text.chunks();
    let Some(mut chunk) = chunks.next() else {
        return 0;
    };
    let mut chunk_start = 0usize;
    let mut grapheme_start = 0usize;
    let mut width = 0usize;
    let mut cursor = GraphemeCursor::new(0, text_len, true);
    loop {
        match cursor.next_boundary(chunk, chunk_start) {
            Ok(Some(grapheme_end)) => {
                let grapheme = text.byte_slice(grapheme_start..grapheme_end);
                if let Some(grapheme) = grapheme.as_str() {
                    width = width.saturating_add(source_grapheme_display_width(grapheme));
                } else {
                    let grapheme = grapheme.to_string();
                    width = width.saturating_add(source_grapheme_display_width(&grapheme));
                }
                grapheme_start = grapheme_end;
            }
            Ok(None) => break,
            Err(GraphemeIncomplete::NextChunk) => {
                chunk_start = chunk_start.saturating_add(chunk.len());
                let Some(next_chunk) = chunks.next() else {
                    break;
                };
                chunk = next_chunk;
            }
            Err(GraphemeIncomplete::PreContext(context_end)) => {
                if context_end == 0 {
                    break;
                }
                let (context, context_start, _, _) = text.chunk_at_byte(context_end - 1);
                let context_len = context_end.saturating_sub(context_start);
                cursor.provide_context(&context[..context_len], context_start);
            }
            Err(GraphemeIncomplete::PrevChunk | GraphemeIncomplete::InvalidOffset) => break,
        }
    }
    width
}

fn non_ascii_byte_count(text: &str) -> usize {
    text.as_bytes()
        .iter()
        .filter(|byte| !byte.is_ascii())
        .count()
}

fn previous_rope_grapheme_boundary(rope: &Rope, cursor: &mut GraphemeCursor) -> Option<usize> {
    if cursor.cur_cursor() == 0 {
        return None;
    }
    let (mut chunk, mut chunk_start, _, _) = rope.chunk_at_byte(cursor.cur_cursor());
    loop {
        match cursor.prev_boundary(chunk, chunk_start) {
            Ok(boundary) => return boundary,
            Err(GraphemeIncomplete::PrevChunk) => {
                if chunk_start == 0 {
                    return None;
                }
                (chunk, chunk_start, _, _) = rope.chunk_at_byte(chunk_start - 1);
            }
            Err(GraphemeIncomplete::PreContext(context_end)) => {
                if context_end == 0 {
                    return None;
                }
                let (context, context_start, _, _) = rope.chunk_at_byte(context_end - 1);
                let context_len = context_end.saturating_sub(context_start);
                cursor.provide_context(&context[..context_len], context_start);
            }
            Err(GraphemeIncomplete::NextChunk | GraphemeIncomplete::InvalidOffset) => return None,
        }
    }
}

fn next_rope_grapheme_boundary(rope: &Rope, cursor: &mut GraphemeCursor) -> Option<usize> {
    loop {
        let offset = cursor.cur_cursor();
        if offset >= rope.len_bytes() {
            return None;
        }
        let (chunk, chunk_start, _, _) = rope.chunk_at_byte(offset);
        match cursor.next_boundary(chunk, chunk_start) {
            Ok(boundary) => return boundary,
            Err(GraphemeIncomplete::NextChunk) => continue,
            Err(GraphemeIncomplete::PreContext(context_end)) => {
                if context_end == 0 {
                    return None;
                }
                let (context, context_start, _, _) = rope.chunk_at_byte(context_end - 1);
                let context_len = context_end.saturating_sub(context_start);
                cursor.provide_context(&context[..context_len], context_start);
            }
            Err(GraphemeIncomplete::PrevChunk | GraphemeIncomplete::InvalidOffset) => return None,
        }
    }
}

fn source_grapheme_display_width(grapheme: &str) -> usize {
    let safe;
    let text = if grapheme.chars().any(is_terminal_unsafe_character) {
        safe = grapheme
            .chars()
            .map(|character| match character {
                '\u{0000}'..='\u{001f}' => {
                    char::from_u32(0x2400 + u32::from(character)).unwrap_or('\u{fffd}')
                }
                '\u{007f}' => '\u{2421}',
                '\u{0080}'..='\u{009f}' => '\u{fffd}',
                character if is_unsafe_format_character(character) => '\u{fffd}',
                character => character,
            })
            .collect::<String>();
        safe.as_str()
    } else {
        grapheme
    };
    UnicodeWidthStr::width(text).max(1)
}

fn is_terminal_unsafe_character(character: char) -> bool {
    character.is_control() || is_unsafe_format_character(character)
}

fn is_unsafe_format_character(character: char) -> bool {
    matches!(
        character,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{206f}'
            | '\u{feff}'
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorBuffer {
    Rich(RichBuffer),
    Source(SourceBuffer),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EditorSnapshot {
    Rich { editor: RichEditor, revision: u64 },
}

impl EditorSnapshot {
    fn same_content(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Rich { editor: left, .. }, Self::Rich { editor: right, .. }) => {
                left.document == right.document
            }
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
enum EditTransaction {
    Rich {
        before: EditorSnapshot,
        after: EditorSnapshot,
        kind: EditKind,
    },
    Source {
        start: usize,
        removed: String,
        inserted: String,
        before_cursor: Cursor,
        before_selection: Option<Selection>,
        before_revision: u64,
        after_cursor: Cursor,
        after_selection: Option<Selection>,
        after_revision: u64,
        kind: EditKind,
    },
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
pub struct EditorState {
    pub document: EditorDocument,
    pub mode: EditorMode,
    access: EditorAccess,
    /// Source-mode caret retained for compatibility. Rich mode uses
    /// [`RichEditor::cursor`] and never interprets this as a Markdown offset.
    pub cursor: Cursor,
    pub selection: Option<Selection>,
    pub viewport: EditorViewport,
    buffer: EditorBuffer,
    undo_stack: Vec<EditTransaction>,
    redo_stack: Vec<EditTransaction>,
    history_limit: usize,
    revision: u64,
    saved_revision: u64,
    next_revision: u64,
    source_session: Option<SourceSession>,
    pending_saves: Vec<SaveSnapshot>,
    rich_word_count_cache: RichWordCountCache,
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

    fn from_document_with_access(document: EditorDocument, access: EditorAccess) -> Self {
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

    fn snapshot(&self) -> EditorSnapshot {
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

    fn restore_snapshot(&mut self, snapshot: &EditorSnapshot) {
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

    fn commit_edit(&mut self, before: EditorSnapshot, kind: EditKind) -> bool {
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

    fn commit_source_edit(
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

    fn clamp_positions(&mut self) {
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
                if let Some(cursor) = buffer.editor.cursor {
                    if !buffer.editor.move_to(cursor, false) {
                        buffer.editor.cursor = buffer.editor.document.first_editable_position();
                        buffer.editor.selection = None;
                    }
                }
            }
        }
    }

    fn refresh_metadata_from_active_source(&mut self) {
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

    fn prepare_save_snapshot(&mut self) -> SaveSnapshot {
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

    fn mark_saved(&mut self, path: Option<PathBuf>, revision: u64) {
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

#[derive(Debug, Clone, Copy, Default)]
pub struct EditorController;

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

#[derive(Debug, Clone, Copy)]
struct LegacyHeadingBreak {
    heading_id: NodeId,
    grapheme_offset: usize,
}

#[derive(Debug, Clone, Copy)]
enum LegacyHeadingBreakMigration {
    ParagraphBefore {
        heading_id: NodeId,
        paragraph_id: NodeId,
    },
    ParagraphAfter {
        heading_id: NodeId,
        paragraph_id: NodeId,
        break_offset: usize,
    },
    HeadingToParagraph {
        container_id: NodeId,
    },
}

impl LegacyHeadingBreakMigration {
    fn remap(self, position: RichPosition) -> RichPosition {
        match self {
            Self::ParagraphBefore {
                heading_id,
                paragraph_id,
            } if position.container_id == heading_id => {
                if position.grapheme_offset == 0 {
                    RichPosition::new(paragraph_id, 0)
                } else {
                    RichPosition::new(heading_id, position.grapheme_offset - 1)
                }
            }
            Self::ParagraphAfter {
                heading_id,
                paragraph_id,
                break_offset,
            } if position.container_id == heading_id && position.grapheme_offset > break_offset => {
                RichPosition::new(paragraph_id, position.grapheme_offset - break_offset - 1)
            }
            Self::HeadingToParagraph { container_id } if position.container_id == container_id => {
                RichPosition::new(container_id, 0)
            }
            _ => position,
        }
    }
}

/// Schema-two drafts written before Heading Enter became block-aware can
/// contain inline breaks that Markdown headings cannot represent. Repair only
/// those containers in place so unrelated node identities and opaque source
/// stay untouched.
fn normalize_legacy_recovered_heading_breaks(
    document: &mut RichDocument,
    cursor: &mut Option<RichPosition>,
    selection: &mut Option<RichSelection>,
) {
    let block_separator = document.preferred_line_ending.as_str().repeat(2);
    while let Some(legacy_break) = first_legacy_heading_break(&document.blocks) {
        let paragraph_id = document.allocate_node_id();
        let Some(migration) = split_legacy_heading_break(
            &mut document.blocks,
            legacy_break,
            paragraph_id,
            &block_separator,
        ) else {
            break;
        };
        document.mark_containing_block_dirty(legacy_break.heading_id);
        *cursor = cursor.map(|position| migration.remap(position));
        *selection = selection.map(|selection| {
            RichSelection::new(
                migration.remap(selection.anchor),
                migration.remap(selection.focus),
            )
        });
    }
}

fn first_legacy_heading_break(blocks: &[RichBlock]) -> Option<LegacyHeadingBreak> {
    for block in blocks {
        match &block.kind {
            RichBlockKind::Heading { content, .. } => {
                let mut grapheme_offset = 0usize;
                for node in &content.0 {
                    if matches!(node, InlineNode::SoftBreak | InlineNode::HardBreak) {
                        return Some(LegacyHeadingBreak {
                            heading_id: block.id,
                            grapheme_offset,
                        });
                    }
                    grapheme_offset = grapheme_offset.saturating_add(node.grapheme_len());
                }
            }
            RichBlockKind::Quote { blocks } => {
                if let Some(legacy_break) = first_legacy_heading_break(blocks) {
                    return Some(legacy_break);
                }
            }
            RichBlockKind::List { items, .. } => {
                for item in items {
                    if let Some(legacy_break) = first_legacy_heading_break(&item.blocks) {
                        return Some(legacy_break);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn split_legacy_heading_break(
    blocks: &mut Vec<RichBlock>,
    legacy_break: LegacyHeadingBreak,
    paragraph_id: NodeId,
    block_separator: &str,
) -> Option<LegacyHeadingBreakMigration> {
    for index in 0..blocks.len() {
        if blocks[index].id == legacy_break.heading_id {
            let kind = std::mem::replace(&mut blocks[index].kind, RichBlockKind::Rule);
            let RichBlockKind::Heading { level, content } = kind else {
                blocks[index].kind = kind;
                return None;
            };
            let content_len = content.grapheme_len();
            let Some(break_index) = content
                .0
                .iter()
                .position(|node| matches!(node, InlineNode::SoftBreak | InlineNode::HardBreak))
            else {
                blocks[index].kind = RichBlockKind::Heading { level, content };
                return None;
            };
            let mut left_nodes = content.0;
            let right_nodes = left_nodes.split_off(break_index + 1);
            left_nodes.pop();
            blocks[index].original_raw = None;
            blocks[index].rewrite = RewriteState::Dirty;

            if legacy_break.grapheme_offset == 0 && content_len == 1 {
                blocks[index].kind = RichBlockKind::Paragraph {
                    content: InlineContent::default(),
                };
                return Some(LegacyHeadingBreakMigration::HeadingToParagraph {
                    container_id: legacy_break.heading_id,
                });
            }

            if legacy_break.grapheme_offset == 0 {
                blocks[index].kind = RichBlockKind::Heading {
                    level,
                    content: InlineContent(right_nodes),
                };
                let leading_trivia = std::mem::take(&mut blocks[index].leading_trivia);
                blocks[index].leading_trivia = block_separator.to_owned();
                let mut paragraph = RichBlock::new(
                    paragraph_id,
                    RichBlockKind::Paragraph {
                        content: InlineContent::default(),
                    },
                );
                paragraph.leading_trivia = leading_trivia;
                blocks.insert(index, paragraph);
                return Some(LegacyHeadingBreakMigration::ParagraphBefore {
                    heading_id: legacy_break.heading_id,
                    paragraph_id,
                });
            }

            blocks[index].kind = RichBlockKind::Heading {
                level,
                content: InlineContent(left_nodes),
            };
            blocks.insert(
                index + 1,
                RichBlock::new(
                    paragraph_id,
                    RichBlockKind::Paragraph {
                        content: InlineContent(right_nodes),
                    },
                ),
            );
            return Some(LegacyHeadingBreakMigration::ParagraphAfter {
                heading_id: legacy_break.heading_id,
                paragraph_id,
                break_offset: legacy_break.grapheme_offset,
            });
        }

        match &mut blocks[index].kind {
            RichBlockKind::Quote { blocks } => {
                if let Some(migration) =
                    split_legacy_heading_break(blocks, legacy_break, paragraph_id, block_separator)
                {
                    return Some(migration);
                }
            }
            RichBlockKind::List { items, .. } => {
                for item in items {
                    if let Some(migration) = split_legacy_heading_break(
                        &mut item.blocks,
                        legacy_break,
                        paragraph_id,
                        block_separator,
                    ) {
                        return Some(migration);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn import_rich_document(document: &EditorDocument) -> RichDocument {
    let source = document.source();
    MarkdownCodec::import_with_metadata(
        &source,
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
            source.to_string(),
            crate::rich_document::RichBlockKind::OpaqueMarkdown {
                raw: source.into_owned(),
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

fn apply_insert_newline(state: &mut EditorState) -> bool {
    let before = state.snapshot();
    if let EditorBuffer::Rich(buffer) = &mut state.buffer {
        buffer.editor.insert_newline();
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
    let cursor = Cursor {
        byte_offset: range.start + text.len(),
        preferred_column: None,
    };
    state.commit_source_edit(range, text, cursor, None, EditKind::Insert)
}

fn backspace(state: &mut EditorState) -> bool {
    if state.selected_range().is_some() {
        return delete_selection(state, EditKind::Delete);
    }
    let cursor = state.cursor.byte_offset;
    if cursor == 0 {
        return false;
    }
    let EditorBuffer::Source(buffer) = &state.buffer else {
        return false;
    };
    let start = buffer.previous_grapheme_boundary(cursor);
    replace_for_delete(state, start..cursor)
}

fn delete_forward(state: &mut EditorState) -> bool {
    if state.selected_range().is_some() {
        return delete_selection(state, EditKind::Delete);
    }
    let cursor = state.cursor.byte_offset;
    let EditorBuffer::Source(buffer) = &state.buffer else {
        return false;
    };
    if cursor >= buffer.len_bytes() {
        return false;
    }
    let end = buffer.next_grapheme_boundary(cursor);
    replace_for_delete(state, cursor..end)
}

fn delete_selection(state: &mut EditorState, kind: EditKind) -> bool {
    let Some(range) = state.selected_range() else {
        return false;
    };
    let range = semantic_delete_range(state, range.start..range.end);
    let cursor = Cursor {
        byte_offset: range.start,
        preferred_column: None,
    };
    state.commit_source_edit(range, "", cursor, None, kind)
}

fn replace_for_delete(state: &mut EditorState, range: Range<usize>) -> bool {
    let range = semantic_delete_range(state, range);
    let start = range.start;
    let cursor = Cursor {
        byte_offset: start,
        preferred_column: None,
    };
    state.commit_source_edit(range, "", cursor, None, EditKind::Delete)
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

    let EditorBuffer::Source(buffer) = &state.buffer else {
        return;
    };
    let current = state.cursor.byte_offset;
    let mut preferred_column = None;
    let target = match movement {
        CursorMove::Left => buffer.previous_grapheme_boundary(current),
        CursorMove::Right => buffer.next_grapheme_boundary(current),
        CursorMove::WordLeft => buffer.previous_word_boundary(current),
        CursorMove::WordRight => buffer.next_word_boundary(current),
        CursorMove::DocumentStart => 0,
        CursorMove::DocumentEnd => buffer.len_bytes(),
        CursorMove::LineStart => {
            let line = buffer.text.byte_to_line(buffer.normalize_position(current));
            buffer.line_range(line).map_or(0, |range| range.start)
        }
        CursorMove::LineEnd => {
            let line = buffer.text.byte_to_line(buffer.normalize_position(current));
            buffer
                .line_range(line)
                .map_or(buffer.len_bytes(), |range| range.end)
        }
        CursorMove::Up | CursorMove::Down => {
            let current = buffer.normalize_position(current);
            let line = buffer.text.byte_to_line(current);
            let target_line = if movement == CursorMove::Up {
                line.saturating_sub(1)
            } else {
                (line + 1).min(buffer.line_count().saturating_sub(1))
            };
            if target_line == line {
                preferred_column = state.cursor.preferred_column;
                current
            } else {
                let desired_column = state
                    .cursor
                    .preferred_column
                    .unwrap_or_else(|| line_column_for_offset(buffer, current).1);
                preferred_column = Some(desired_column);
                buffer
                    .byte_at_grapheme_column(target_line, desired_column)
                    .unwrap_or(current)
            }
        }
    };
    move_to(state, target, extend_selection);
    state.cursor.preferred_column = preferred_column;
}

fn move_to(state: &mut EditorState, byte_offset: usize, extend_selection: bool) {
    let EditorBuffer::Source(buffer) = &state.buffer else {
        return;
    };
    let target = buffer.normalize_position(byte_offset);
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
        (&state.buffer, transaction),
        (EditorBuffer::Rich(_), EditTransaction::Rich { .. })
            | (EditorBuffer::Source(_), EditTransaction::Source { .. })
    );
    if !mode_matches {
        return false;
    }
    let transaction = state.undo_stack.pop().expect("last transaction exists");
    match &transaction {
        EditTransaction::Rich { before, .. } => state.restore_snapshot(before),
        EditTransaction::Source {
            start,
            removed,
            inserted,
            before_cursor,
            before_selection,
            before_revision,
            ..
        } => {
            let EditorBuffer::Source(buffer) = &mut state.buffer else {
                unreachable!("mode checked before applying Source undo")
            };
            let _ = buffer.replace_history_range(*start..*start + inserted.len(), removed);
            state.cursor = *before_cursor;
            state.selection = *before_selection;
            state.revision = *before_revision;
            state.refresh_metadata_from_active_source();
        }
    }
    state.redo_stack.push(transaction);
    true
}

fn redo(state: &mut EditorState) -> bool {
    let Some(transaction) = state.redo_stack.last() else {
        return false;
    };
    let mode_matches = matches!(
        (&state.buffer, transaction),
        (EditorBuffer::Rich(_), EditTransaction::Rich { .. })
            | (EditorBuffer::Source(_), EditTransaction::Source { .. })
    );
    if !mode_matches {
        return false;
    }
    let transaction = state.redo_stack.pop().expect("last transaction exists");
    match &transaction {
        EditTransaction::Rich { after, .. } => state.restore_snapshot(after),
        EditTransaction::Source {
            start,
            removed,
            inserted,
            after_cursor,
            after_selection,
            after_revision,
            ..
        } => {
            let EditorBuffer::Source(buffer) = &mut state.buffer else {
                unreachable!("mode checked before applying Source redo")
            };
            let _ = buffer.replace_history_range(*start..*start + removed.len(), inserted);
            state.cursor = *after_cursor;
            state.selection = *after_selection;
            state.revision = *after_revision;
            state.refresh_metadata_from_active_source();
        }
    }
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

fn line_column_for_offset(buffer: &SourceBuffer, byte_offset: usize) -> (usize, usize) {
    let offset = buffer.normalize_position(byte_offset);
    let line_index = buffer.text.byte_to_line(offset);
    let Some(line) = buffer.line_range(line_index) else {
        return (0, 0);
    };
    let column = buffer.line_grapheme_column(line, offset);
    (line_index, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rich_word_count_cache_is_scoped_to_the_document_revision() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<EditorState>();

        let mut editor = EditorState::new();
        assert_eq!(editor.rich_word_count_cache.get(), None);

        assert_eq!(editor.word_count(), 0);
        assert_eq!(
            editor.rich_word_count_cache.get(),
            Some((editor.revision(), 0))
        );
        assert_eq!(editor.word_count(), 0);

        editor.apply(EditorCommand::InsertText("one two".to_owned()));
        assert_ne!(
            editor
                .rich_word_count_cache
                .get()
                .map(|(revision, _)| revision),
            Some(editor.revision())
        );
        assert_eq!(editor.word_count(), 2);
        assert_eq!(
            editor.rich_word_count_cache.get(),
            Some((editor.revision(), 2))
        );

        let clone = editor.clone();
        clone.rich_word_count_cache.set(None);
        assert_eq!(
            editor, clone,
            "derived caches must not affect value equality"
        );

        editor.apply(EditorCommand::Undo);
        assert_eq!(editor.word_count(), 0);
        assert_eq!(
            editor.rich_word_count_cache.get(),
            Some((editor.revision(), 0))
        );
    }
}
