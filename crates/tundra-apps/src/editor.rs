//! Domain model for Tundra's single-document terminal editor.
//!
//! The Markdown source is always the canonical document. Rich rendering is a
//! disposable projection whose ranges point back into that source.

use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

use comrak::nodes::{AstNode, ListType, NodeValue, TableAlignment as ComrakTableAlignment};
use comrak::{Arena, Options, parse_document};
use unicode_segmentation::UnicodeSegmentation;

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

    fn replace_range(&mut self, range: Range<usize>, replacement: &str) {
        self.source.replace_range(range, replacement);
        let fallback = self.metadata.preferred_line_ending;
        self.metadata =
            TextMetadata::from_source_with_fallback(&self.source, self.metadata.utf8_bom, fallback);
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
        byte_offset: usize,
        extend_selection: bool,
    },
    SelectAll,
    ClearSelection,
    Undo,
    Redo,
    ApplyFormat(FormatCommand),
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
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorEffect {
    OpenFilePicker,
    SaveFile {
        path: PathBuf,
        contents: Vec<u8>,
    },
    SaveFilePicker {
        suggested_name: String,
        contents: Vec<u8>,
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
struct EditorSnapshot {
    source: String,
    cursor: Cursor,
    selection: Option<Selection>,
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
pub struct EditorState {
    pub document: EditorDocument,
    pub mode: EditorMode,
    pub cursor: Cursor,
    pub selection: Option<Selection>,
    pub viewport: EditorViewport,
    undo_stack: Vec<EditTransaction>,
    redo_stack: Vec<EditTransaction>,
    history_limit: usize,
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
        Self {
            document,
            mode,
            cursor: Cursor::default(),
            selection: None,
            viewport: EditorViewport::default(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            history_limit: DEFAULT_HISTORY_LIMIT,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.document.is_dirty()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn history_depth(&self) -> (usize, usize) {
        (self.undo_stack.len(), self.redo_stack.len())
    }

    pub fn selected_range(&self) -> Option<SourceRange> {
        self.selection
            .filter(|selection| !selection.is_collapsed())
            .map(|selection| SourceRange::from(selection.range()))
    }

    pub fn selected_text(&self) -> Option<&str> {
        let range = self.selected_range()?;
        self.document.source().get(range.start..range.end)
    }

    pub fn cursor_line_column(&self) -> (usize, usize) {
        line_column_for_offset(self.document.source(), self.cursor.byte_offset)
    }

    pub fn render_blocks(&self) -> Vec<RenderBlock> {
        match self.mode {
            EditorMode::Rich => parse_markdown(self.document.source()),
            EditorMode::Source => render_plain_text(self.document.source()),
        }
    }

    pub fn current_block(&self) -> Option<RenderBlock> {
        let cursor = self.cursor.byte_offset;
        self.render_blocks().into_iter().find(|block| {
            let range = block.source_range();
            range.contains(cursor)
                || (cursor == range.end && range.end == self.document.source.len())
        })
    }

    pub fn apply(&mut self, command: EditorCommand) -> Vec<EditorEffect> {
        EditorController.apply(self, command)
    }

    pub fn replace_source_range(&mut self, range: SourceRange, replacement: &str) -> bool {
        let Some(range) = validated_range(self.document.source(), range.start..range.end) else {
            return false;
        };
        let before = self.snapshot();
        self.document.replace_range(range.clone(), replacement);
        self.cursor.byte_offset = range.start + replacement.len();
        self.cursor.preferred_column = None;
        self.selection = None;
        self.commit_edit(before, EditKind::Insert)
    }

    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            source: self.document.source.clone(),
            cursor: self.cursor,
            selection: self.selection,
        }
    }

    fn restore_snapshot(&mut self, snapshot: &EditorSnapshot) {
        self.document.restore_source(snapshot.source.clone());
        self.cursor = snapshot.cursor;
        self.selection = snapshot.selection;
        self.clamp_positions();
    }

    fn commit_edit(&mut self, before: EditorSnapshot, kind: EditKind) -> bool {
        let after = self.snapshot();
        if before.source == after.source {
            return false;
        }
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
        let source = self.document.source();
        self.cursor.byte_offset = normalize_position(source, self.cursor.byte_offset);
        if let Some(selection) = &mut self.selection {
            selection.anchor = normalize_position(source, selection.anchor);
            selection.focus = normalize_position(source, selection.focus);
            if selection.is_collapsed() {
                self.selection = None;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EditorController;

impl EditorController {
    pub fn apply(self, state: &mut EditorState, command: EditorCommand) -> Vec<EditorEffect> {
        match command {
            EditorCommand::InsertText(text) | EditorCommand::Paste(text) => {
                insert_text(state, &text);
                Vec::new()
            }
            EditorCommand::InsertNewline => {
                let newline = state.document.metadata.preferred_line_ending.as_str();
                insert_text(state, newline);
                Vec::new()
            }
            EditorCommand::Backspace => {
                backspace(state);
                Vec::new()
            }
            EditorCommand::DeleteForward => {
                delete_forward(state);
                Vec::new()
            }
            EditorCommand::DeleteSelection => {
                delete_selection(state, EditKind::Delete);
                Vec::new()
            }
            EditorCommand::MoveCursor {
                movement,
                extend_selection,
            } => {
                move_cursor(state, movement, extend_selection);
                Vec::new()
            }
            EditorCommand::MoveTo {
                byte_offset,
                extend_selection,
            } => {
                move_to(state, byte_offset, extend_selection);
                Vec::new()
            }
            EditorCommand::SelectAll => {
                let end = state.document.source().len();
                state.selection = (end > 0).then_some(Selection::new(0, end));
                state.cursor.byte_offset = end;
                state.cursor.preferred_column = None;
                Vec::new()
            }
            EditorCommand::ClearSelection => {
                state.selection = None;
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
                apply_format(state, format);
                Vec::new()
            }
            EditorCommand::SetMode(mode) => {
                state.mode = mode;
                Vec::new()
            }
            EditorCommand::ToggleMode => {
                state.mode = match state.mode {
                    EditorMode::Rich => EditorMode::Source,
                    EditorMode::Source => EditorMode::Rich,
                };
                Vec::new()
            }
            EditorCommand::Copy => state
                .selected_text()
                .map(|text| vec![EditorEffect::WriteClipboard(text.to_owned())])
                .unwrap_or_default(),
            EditorCommand::Cut => {
                let Some(text) = state.selected_text().map(str::to_owned) else {
                    return Vec::new();
                };
                delete_selection(state, EditKind::Delete);
                vec![EditorEffect::WriteClipboard(text)]
            }
            EditorCommand::RequestPaste => vec![EditorEffect::ReadClipboard],
            EditorCommand::RequestOpen => vec![EditorEffect::OpenFilePicker],
            EditorCommand::RequestSave => {
                let contents = state.document.to_bytes();
                if let Some(path) = &state.document.path {
                    vec![EditorEffect::SaveFile {
                        path: path.clone(),
                        contents,
                    }]
                } else {
                    vec![EditorEffect::SaveFilePicker {
                        suggested_name: state.document.display_name(),
                        contents,
                    }]
                }
            }
            EditorCommand::RequestSaveAs => vec![EditorEffect::SaveFilePicker {
                suggested_name: state.document.display_name(),
                contents: state.document.to_bytes(),
            }],
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
            EditorCommand::MarkSaved { path } => {
                state.document.mark_saved(path);
                Vec::new()
            }
        }
    }
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

/// Parses CommonMark/GFM into terminal-oriented blocks without modifying the source.
///
/// Keeping this conversion at one boundary lets a future renderer replace the
/// projection without coupling it to the editing transaction model.
pub fn parse_markdown(source: &str) -> Vec<RenderBlock> {
    let arena = Arena::new();
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.footnotes = true;
    let root = parse_document(&arena, source, &options);
    root.children()
        .map(|node| render_block_from_node(node, source))
        .collect()
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

fn render_block_from_node<'a>(node: &'a AstNode<'a>, source: &str) -> RenderBlock {
    let data = node.data();
    let value = data.value.clone();
    let source_range = source_range_from_position(source, data.sourcepos);
    drop(data);
    let kind = value.xml_node_name().to_owned();
    match value {
        NodeValue::Heading(heading) => {
            let content = inline_content(node, source);
            let content_range = span_content_range(&content).unwrap_or(source_range);
            RenderBlock::Heading {
                level: heading.level,
                content,
                source_range,
                content_range,
            }
        }
        NodeValue::Paragraph => {
            if let Some(image) = standalone_image(node, source, source_range) {
                return image;
            }
            let content = inline_content(node, source);
            let content_range = span_content_range(&content).unwrap_or(source_range);
            RenderBlock::Paragraph {
                content,
                source_range,
                content_range,
            }
        }
        NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => RenderBlock::Quote {
            blocks: node
                .children()
                .map(|child| render_block_from_node(child, source))
                .collect(),
            source_range,
        },
        NodeValue::CodeBlock(code) => {
            let language = code
                .info
                .split_whitespace()
                .next()
                .filter(|language| !language.is_empty())
                .map(str::to_owned);
            let content_range = source
                .get(source_range.start..source_range.end)
                .and_then(|raw| {
                    raw.find(&code.literal)
                        .map(|offset| offset + source_range.start)
                })
                .map(|start| SourceRange::new(start, start + code.literal.len()))
                .unwrap_or(source_range);
            RenderBlock::CodeBlock {
                language,
                code: code.literal,
                source_range,
                content_range,
            }
        }
        NodeValue::List(list) => {
            let items = node
                .children()
                .map(|item| render_list_item(item, source))
                .collect::<Vec<_>>();
            if list.is_task_list || items.iter().any(|item| item.checked.is_some()) {
                RenderBlock::TaskList {
                    items,
                    source_range,
                }
            } else if list.list_type == ListType::Ordered {
                RenderBlock::OrderedList {
                    start: list.start,
                    items,
                    tight: list.tight,
                    source_range,
                }
            } else {
                RenderBlock::BulletList {
                    items,
                    tight: list.tight,
                    source_range,
                }
            }
        }
        NodeValue::Table(table) => {
            let alignments = table
                .alignments
                .iter()
                .map(|alignment| match alignment {
                    ComrakTableAlignment::None => TableAlignment::None,
                    ComrakTableAlignment::Left => TableAlignment::Left,
                    ComrakTableAlignment::Center => TableAlignment::Center,
                    ComrakTableAlignment::Right => TableAlignment::Right,
                })
                .collect();
            let mut header = Vec::new();
            let mut rows = Vec::new();
            for row in node.children() {
                let is_header = matches!(row.data().value, NodeValue::TableRow(true));
                let cells = row
                    .children()
                    .map(|cell| RenderTableCell {
                        content: inline_content(cell, source),
                        source_range: node_source_range(cell, source),
                    })
                    .collect::<Vec<_>>();
                if is_header {
                    header = cells;
                } else {
                    rows.push(cells);
                }
            }
            RenderBlock::Table {
                header,
                rows,
                alignments,
                source_range,
            }
        }
        NodeValue::ThematicBreak => RenderBlock::Rule { source_range },
        NodeValue::HtmlBlock(html) => RenderBlock::RawHtml {
            html: html.literal,
            source_range,
        },
        NodeValue::FootnoteDefinition(definition) => RenderBlock::FootnoteDefinition {
            name: definition.name,
            blocks: node
                .children()
                .map(|child| render_block_from_node(child, source))
                .collect(),
            source_range,
        },
        _ => RenderBlock::Raw {
            kind,
            source: source
                .get(source_range.start..source_range.end)
                .unwrap_or_default()
                .to_owned(),
            source_range,
        },
    }
}

fn standalone_image<'a>(
    node: &'a AstNode<'a>,
    source: &str,
    paragraph_range: SourceRange,
) -> Option<RenderBlock> {
    let mut children = node.children();
    let image = children.next()?;
    if children.next().is_some() {
        return None;
    }
    let data = image.data();
    let NodeValue::Image(link) = &data.value else {
        return None;
    };
    let url = link.url.clone();
    let title = (!link.title.is_empty()).then(|| link.title.clone());
    drop(data);
    let alt = inline_content(image, source)
        .into_iter()
        .map(|span| span.text)
        .collect();
    Some(RenderBlock::Image {
        alt,
        url,
        title,
        source_range: paragraph_range,
    })
}

fn render_list_item<'a>(node: &'a AstNode<'a>, source: &str) -> RenderListItem {
    let source_range = node_source_range(node, source);
    let checked = node.descendants().find_map(|descendant| {
        let data = descendant.data();
        match data.value {
            NodeValue::TaskItem(task) => Some(task.symbol.is_some()),
            _ => None,
        }
    });
    let mut content = Vec::new();
    let mut children = Vec::new();
    for child in node.children() {
        let child_value = child.data().value.clone();
        match child_value {
            NodeValue::Paragraph | NodeValue::TaskItem(_) => {
                if !content.is_empty() {
                    content.push(RenderSpan::plain("\n", node_source_range(child, source)));
                }
                content.extend(inline_content(child, source));
            }
            NodeValue::List(_) | NodeValue::BlockQuote => {
                children.push(render_block_from_node(child, source));
            }
            _ if child_value.block() => children.push(render_block_from_node(child, source)),
            _ => collect_inline_spans(child, source, &[], None, None, &mut content),
        }
    }
    RenderListItem {
        content,
        children,
        checked,
        source_range,
    }
}

fn inline_content<'a>(node: &'a AstNode<'a>, source: &str) -> Vec<RenderSpan> {
    let mut spans = Vec::new();
    for child in node.children() {
        collect_inline_spans(child, source, &[], None, None, &mut spans);
    }
    spans
}

fn collect_inline_spans<'a>(
    node: &'a AstNode<'a>,
    source: &str,
    inherited_styles: &[InlineStyle],
    inherited_target: Option<&str>,
    inherited_title: Option<&str>,
    spans: &mut Vec<RenderSpan>,
) {
    let data = node.data();
    let value = data.value.clone();
    let source_range = source_range_from_position(source, data.sourcepos);
    drop(data);
    match value {
        NodeValue::Text(text) => spans.push(RenderSpan {
            text: text.into_owned(),
            styles: inherited_styles.to_vec(),
            target: inherited_target.map(str::to_owned),
            title: inherited_title.map(str::to_owned),
            source_range,
        }),
        NodeValue::Code(code) => spans.push(RenderSpan {
            text: code.literal,
            styles: styles_with(inherited_styles, InlineStyle::Code),
            target: inherited_target.map(str::to_owned),
            title: inherited_title.map(str::to_owned),
            source_range,
        }),
        NodeValue::SoftBreak => spans.push(RenderSpan {
            text: "\n".to_owned(),
            styles: inherited_styles.to_vec(),
            target: inherited_target.map(str::to_owned),
            title: inherited_title.map(str::to_owned),
            source_range,
        }),
        NodeValue::LineBreak => spans.push(RenderSpan {
            text: "\n".to_owned(),
            styles: styles_with(inherited_styles, InlineStyle::HardBreak),
            target: inherited_target.map(str::to_owned),
            title: inherited_title.map(str::to_owned),
            source_range,
        }),
        NodeValue::HtmlInline(html) | NodeValue::Raw(html) => spans.push(RenderSpan {
            text: html,
            styles: styles_with(inherited_styles, InlineStyle::RawHtml),
            target: inherited_target.map(str::to_owned),
            title: inherited_title.map(str::to_owned),
            source_range,
        }),
        NodeValue::Strong => collect_inline_children(
            node,
            source,
            &styles_with(inherited_styles, InlineStyle::Bold),
            inherited_target,
            inherited_title,
            spans,
        ),
        NodeValue::Emph => collect_inline_children(
            node,
            source,
            &styles_with(inherited_styles, InlineStyle::Italic),
            inherited_target,
            inherited_title,
            spans,
        ),
        NodeValue::Strikethrough => collect_inline_children(
            node,
            source,
            &styles_with(inherited_styles, InlineStyle::Strikethrough),
            inherited_target,
            inherited_title,
            spans,
        ),
        NodeValue::Link(link) => collect_inline_children(
            node,
            source,
            &styles_with(inherited_styles, InlineStyle::Link),
            Some(&link.url),
            (!link.title.is_empty()).then_some(link.title.as_str()),
            spans,
        ),
        NodeValue::Image(link) => {
            let alt = inline_content(node, source)
                .into_iter()
                .map(|span| span.text)
                .collect();
            spans.push(RenderSpan {
                text: alt,
                styles: styles_with(inherited_styles, InlineStyle::Image),
                target: Some(link.url),
                title: (!link.title.is_empty()).then_some(link.title),
                source_range,
            });
        }
        NodeValue::FootnoteReference(reference) => spans.push(RenderSpan {
            text: format!("[^{}]", reference.name),
            styles: styles_with(inherited_styles, InlineStyle::Footnote),
            target: Some(reference.name),
            title: None,
            source_range,
        }),
        _ => collect_inline_children(
            node,
            source,
            inherited_styles,
            inherited_target,
            inherited_title,
            spans,
        ),
    }
}

fn collect_inline_children<'a>(
    node: &'a AstNode<'a>,
    source: &str,
    styles: &[InlineStyle],
    target: Option<&str>,
    title: Option<&str>,
    spans: &mut Vec<RenderSpan>,
) {
    for child in node.children() {
        collect_inline_spans(child, source, styles, target, title, spans);
    }
}

fn styles_with(styles: &[InlineStyle], style: InlineStyle) -> Vec<InlineStyle> {
    let mut result = styles.to_vec();
    if !result.contains(&style) {
        result.push(style);
    }
    result
}

fn span_content_range(spans: &[RenderSpan]) -> Option<SourceRange> {
    Some(SourceRange::new(
        spans.iter().map(|span| span.source_range.start).min()?,
        spans.iter().map(|span| span.source_range.end).max()?,
    ))
}

fn node_source_range(node: &AstNode<'_>, source: &str) -> SourceRange {
    source_range_from_position(source, node.data().sourcepos)
}

fn source_range_from_position(source: &str, position: comrak::nodes::Sourcepos) -> SourceRange {
    if position.start.line == 0 || position.end.line == 0 {
        return SourceRange::default();
    }
    let lines = line_ranges(source);
    let Some(start_line) = lines.get(position.start.line.saturating_sub(1)) else {
        return SourceRange::new(source.len(), source.len());
    };
    let Some(end_line) = lines.get(position.end.line.saturating_sub(1)) else {
        return SourceRange::new(start_line.start, source.len());
    };
    let start = (start_line.start + position.start.column.saturating_sub(1)).min(start_line.end);
    let end = (end_line.start + position.end.column).min(end_line.end);
    SourceRange::new(
        normalize_position(source, start),
        normalize_end_position(source, end),
    )
}

fn insert_text(state: &mut EditorState, text: &str) -> bool {
    if text.is_empty() && state.selected_range().is_none() {
        return false;
    }
    let range = active_edit_range(state);
    let before = state.snapshot();
    state.document.replace_range(range.clone(), text);
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
    let start = previous_grapheme_boundary(state.document.source(), cursor);
    replace_for_delete(state, start..cursor)
}

fn delete_forward(state: &mut EditorState) -> bool {
    if state.selected_range().is_some() {
        return delete_selection(state, EditKind::Delete);
    }
    let cursor = state.cursor.byte_offset;
    if cursor >= state.document.source().len() {
        return false;
    }
    let end = next_grapheme_boundary(state.document.source(), cursor);
    replace_for_delete(state, cursor..end)
}

fn delete_selection(state: &mut EditorState, kind: EditKind) -> bool {
    let Some(range) = state.selected_range() else {
        return false;
    };
    let before = state.snapshot();
    state.document.replace_range(range.start..range.end, "");
    state.cursor.byte_offset = range.start;
    state.cursor.preferred_column = None;
    state.selection = None;
    state.commit_edit(before, kind)
}

fn replace_for_delete(state: &mut EditorState, range: Range<usize>) -> bool {
    let before = state.snapshot();
    let start = range.start;
    state.document.replace_range(range, "");
    state.cursor.byte_offset = start;
    state.cursor.preferred_column = None;
    state.selection = None;
    state.commit_edit(before, EditKind::Delete)
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

    let source = state.document.source();
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
    let target = normalize_position(state.document.source(), byte_offset);
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
    let Some(transaction) = state.undo_stack.pop() else {
        return false;
    };
    state.restore_snapshot(&transaction.before);
    state.redo_stack.push(transaction);
    true
}

fn redo(state: &mut EditorState) -> bool {
    let Some(transaction) = state.redo_stack.pop() else {
        return false;
    };
    state.restore_snapshot(&transaction.after);
    state.undo_stack.push(transaction);
    true
}

fn apply_format(state: &mut EditorState, format: FormatCommand) -> bool {
    match format {
        FormatCommand::Bold => toggle_surround(state, "**", "**"),
        FormatCommand::Italic => toggle_surround(state, "*", "*"),
        FormatCommand::Strikethrough => toggle_surround(state, "~~", "~~"),
        FormatCommand::InlineCode => toggle_surround(state, "`", "`"),
        FormatCommand::Heading(level) => set_heading(state, level),
        FormatCommand::Paragraph => set_heading(state, 0),
        FormatCommand::Quote => toggle_line_format(state, LineFormat::Quote),
        FormatCommand::BulletList => toggle_line_format(state, LineFormat::Bullet),
        FormatCommand::OrderedList => toggle_line_format(state, LineFormat::Ordered),
        FormatCommand::TaskList => toggle_line_format(state, LineFormat::Task),
        FormatCommand::Link { url, title } => insert_link(state, &url, title.as_deref()),
        FormatCommand::Image { url, alt, title } => {
            insert_image(state, &url, &alt, title.as_deref())
        }
        FormatCommand::Table { columns, rows } => insert_table(state, columns, rows),
    }
}

fn toggle_surround(state: &mut EditorState, prefix: &str, suffix: &str) -> bool {
    let range = active_edit_range(state);
    let source = state.document.source();
    let marker_present = range.start >= prefix.len()
        && range.end + suffix.len() <= source.len()
        && source.get(range.start - prefix.len()..range.start) == Some(prefix)
        && source.get(range.end..range.end + suffix.len()) == Some(suffix);
    // A single `*` beside a selection inside `**bold**` is not italic markup.
    // Odd runs represent emphasis; even runs represent strong emphasis.
    let already_wrapped = marker_present
        && (prefix != "*"
            || (adjacent_byte_count(source, range.start, b'*', false) % 2 == 1
                && adjacent_byte_count(source, range.end, b'*', true) % 2 == 1));
    let before = state.snapshot();
    if already_wrapped {
        let original_selection = state.selection;
        state
            .document
            .replace_range(range.end..range.end + suffix.len(), "");
        state
            .document
            .replace_range(range.start - prefix.len()..range.start, "");
        let new_start = range.start - prefix.len();
        let new_end = range.end - prefix.len();
        state.cursor.byte_offset = new_end;
        state.selection = original_selection.map(|selection| {
            if selection.anchor <= selection.focus {
                Selection::new(new_start, new_end)
            } else {
                Selection::new(new_end, new_start)
            }
        });
    } else if range.is_empty() {
        let insertion = format!("{prefix}{suffix}");
        state.document.replace_range(range.clone(), &insertion);
        state.cursor.byte_offset = range.start + prefix.len();
        state.selection = None;
    } else {
        let reversed = state
            .selection
            .is_some_and(|selection| selection.anchor > selection.focus);
        state.document.replace_range(range.end..range.end, suffix);
        state
            .document
            .replace_range(range.start..range.start, prefix);
        let new_start = range.start + prefix.len();
        let new_end = range.end + prefix.len();
        state.cursor.byte_offset = if reversed { new_start } else { new_end };
        state.selection = Some(if reversed {
            Selection::new(new_end, new_start)
        } else {
            Selection::new(new_start, new_end)
        });
    }
    state.cursor.preferred_column = None;
    state.commit_edit(before, EditKind::Format)
}

fn adjacent_byte_count(source: &str, position: usize, needle: u8, forward: bool) -> usize {
    if forward {
        source.as_bytes()[position..]
            .iter()
            .take_while(|byte| **byte == needle)
            .count()
    } else {
        source.as_bytes()[..position]
            .iter()
            .rev()
            .take_while(|byte| **byte == needle)
            .count()
    }
}

fn set_heading(state: &mut EditorState, level: u8) -> bool {
    if level > 6 {
        return false;
    }
    let source = state.document.source();
    let (line_index, _) = line_column_for_offset(source, state.cursor.byte_offset);
    let Some(line_range) = line_ranges(source).get(line_index).cloned() else {
        return false;
    };
    let line = &source[line_range.clone()];
    let indent = leading_markdown_indent(line);
    let prefix_end = atx_heading_prefix_end(&line[indent..])
        .map(|end| indent + end)
        .unwrap_or(indent);
    let replacement = if level == 0 {
        String::new()
    } else {
        format!("{} ", "#".repeat(usize::from(level)))
    };
    if line[indent..prefix_end] == replacement {
        return false;
    }
    apply_source_edits(
        state,
        vec![(
            line_range.start + indent..line_range.start + prefix_end,
            replacement,
        )],
        EditKind::Format,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineFormat {
    Quote,
    Bullet,
    Ordered,
    Task,
}

impl LineFormat {
    const fn prefix(self) -> &'static str {
        match self {
            Self::Quote => "> ",
            Self::Bullet => "- ",
            Self::Ordered => "1. ",
            Self::Task => "- [ ] ",
        }
    }
}

fn toggle_line_format(state: &mut EditorState, format: LineFormat) -> bool {
    let source = state.document.source();
    let lines = line_ranges(source);
    let (first, last) = selected_line_span(state, &lines);
    let selected = lines.get(first..=last).unwrap_or_default();
    if selected.is_empty() {
        return false;
    }
    let all_formatted = selected.iter().all(|range| {
        let line = &source[range.clone()];
        target_line_prefix(line, format).is_some()
    });
    let mut edits = Vec::new();
    for range in selected {
        let line = &source[range.clone()];
        let indent = leading_markdown_indent(line);
        let prefix = if all_formatted || format == LineFormat::Quote {
            target_line_prefix(line, format)
        } else {
            existing_list_prefix(line)
        };
        let replace_end = prefix.map_or(indent, |prefix| prefix.end);
        let replacement = if all_formatted {
            String::new()
        } else {
            format.prefix().to_owned()
        };
        edits.push((range.start + indent..range.start + replace_end, replacement));
    }
    apply_source_edits(state, edits, EditKind::Format)
}

fn insert_link(state: &mut EditorState, url: &str, title: Option<&str>) -> bool {
    let range = active_edit_range(state);
    let label = state
        .document
        .source()
        .get(range.clone())
        .filter(|label| !label.is_empty())
        .unwrap_or("link");
    let title = markdown_title(title);
    let replacement = format!("[{}]({url}{title})", escape_markdown_label(label));
    replace_active_with(state, range, &replacement, EditKind::Format)
}

fn insert_image(state: &mut EditorState, url: &str, alt: &str, title: Option<&str>) -> bool {
    let range = active_edit_range(state);
    let selection = state
        .document
        .source()
        .get(range.clone())
        .unwrap_or_default();
    let alt = if alt.is_empty() { selection } else { alt };
    let title = markdown_title(title);
    let replacement = format!("![{}]({url}{title})", escape_markdown_label(alt));
    replace_active_with(state, range, &replacement, EditKind::Format)
}

fn insert_table(state: &mut EditorState, columns: usize, rows: usize) -> bool {
    if columns == 0 || columns > 32 || rows > 256 {
        return false;
    }
    let newline = state.document.metadata.preferred_line_ending.as_str();
    let header = (1..=columns)
        .map(|column| format!("Column {column}"))
        .collect::<Vec<_>>()
        .join(" | ");
    let separator = std::iter::repeat_n("---", columns)
        .collect::<Vec<_>>()
        .join(" | ");
    let empty_row = std::iter::repeat_n(" ", columns)
        .collect::<Vec<_>>()
        .join(" | ");
    let mut table = format!("| {header} |{newline}| {separator} |");
    for _ in 0..rows {
        table.push_str(newline);
        table.push_str("| ");
        table.push_str(&empty_row);
        table.push_str(" |");
    }
    let range = active_edit_range(state);
    replace_active_with(state, range, &table, EditKind::Format)
}

fn replace_active_with(
    state: &mut EditorState,
    range: Range<usize>,
    replacement: &str,
    kind: EditKind,
) -> bool {
    let before = state.snapshot();
    state.document.replace_range(range.clone(), replacement);
    state.cursor.byte_offset = range.start + replacement.len();
    state.cursor.preferred_column = None;
    state.selection = None;
    state.commit_edit(before, kind)
}

fn apply_source_edits(
    state: &mut EditorState,
    mut edits: Vec<(Range<usize>, String)>,
    kind: EditKind,
) -> bool {
    if edits.is_empty()
        || edits
            .iter()
            .any(|(range, _)| validated_range(state.document.source(), range.clone()).is_none())
    {
        return false;
    }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let before = state.snapshot();
    for (range, replacement) in edits {
        replace_range_adjust_positions(state, range, &replacement);
    }
    state.cursor.preferred_column = None;
    state.commit_edit(before, kind)
}

fn replace_range_adjust_positions(state: &mut EditorState, range: Range<usize>, replacement: &str) {
    let replacement_len = replacement.len();
    state.document.replace_range(range.clone(), replacement);
    state.cursor.byte_offset =
        transform_position(state.cursor.byte_offset, &range, replacement_len);
    if let Some(selection) = &mut state.selection {
        selection.anchor = transform_position(selection.anchor, &range, replacement_len);
        selection.focus = transform_position(selection.focus, &range, replacement_len);
    }
}

fn transform_position(position: usize, range: &Range<usize>, replacement_len: usize) -> usize {
    if range.is_empty() {
        return if position >= range.start {
            position + replacement_len
        } else {
            position
        };
    }
    if position <= range.start {
        position
    } else if position < range.end {
        range.start + replacement_len
    } else if replacement_len >= range.len() {
        position + (replacement_len - range.len())
    } else {
        position - (range.len() - replacement_len)
    }
}

fn selected_line_span(state: &EditorState, lines: &[Range<usize>]) -> (usize, usize) {
    let selection = state.selected_range();
    let start = selection.map_or(state.cursor.byte_offset, |range| range.start);
    let mut end = selection.map_or(start, |range| range.end);
    if selection.is_some_and(|range| !range.is_empty()) && end > start {
        end = previous_grapheme_boundary(state.document.source(), end);
    }
    (
        line_index_for_offset(lines, start),
        line_index_for_offset(lines, end),
    )
}

fn target_line_prefix(line: &str, format: LineFormat) -> Option<Range<usize>> {
    let indent = leading_markdown_indent(line);
    let rest = &line[indent..];
    let length = match format {
        LineFormat::Quote => rest
            .strip_prefix('>')
            .map(|tail| 1 + usize::from(tail.starts_with(' '))),
        LineFormat::Bullet => bullet_prefix_len(rest),
        LineFormat::Ordered => ordered_prefix_len(rest),
        LineFormat::Task => task_prefix_len(rest),
    }?;
    Some(indent..indent + length)
}

fn existing_list_prefix(line: &str) -> Option<Range<usize>> {
    let indent = leading_markdown_indent(line);
    let rest = &line[indent..];
    let length = task_prefix_len(rest)
        .or_else(|| ordered_prefix_len(rest))
        .or_else(|| bullet_prefix_len(rest))?;
    Some(indent..indent + length)
}

fn bullet_prefix_len(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    (bytes.len() >= 2 && matches!(bytes[0], b'-' | b'*' | b'+') && bytes[1] == b' ').then_some(2)
}

fn ordered_prefix_len(line: &str) -> Option<usize> {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let bytes = line.as_bytes();
    (matches!(bytes.get(digits), Some(b'.' | b')')) && bytes.get(digits + 1) == Some(&b' '))
        .then_some(digits + 2)
}

fn task_prefix_len(line: &str) -> Option<usize> {
    let bullet = bullet_prefix_len(line)?;
    let tail = line.as_bytes().get(bullet..)?;
    (tail.len() >= 4
        && tail[0] == b'['
        && matches!(tail[1], b' ' | b'x' | b'X')
        && tail[2] == b']'
        && tail[3] == b' ')
        .then_some(bullet + 4)
}

fn atx_heading_prefix_end(line: &str) -> Option<usize> {
    let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&hashes) || line.as_bytes().get(hashes) != Some(&b' ') {
        return None;
    }
    Some(
        hashes
            + line[hashes..]
                .bytes()
                .take_while(|byte| *byte == b' ')
                .count(),
    )
}

fn leading_markdown_indent(line: &str) -> usize {
    line.bytes()
        .take(3)
        .take_while(|byte| *byte == b' ')
        .count()
}

fn markdown_title(title: Option<&str>) -> String {
    title
        .filter(|title| !title.is_empty())
        .map(|title| format!(" \"{}\"", title.replace('"', "\\\"")))
        .unwrap_or_default()
}

fn escape_markdown_label(label: &str) -> String {
    label.replace('\\', "\\\\").replace(']', "\\]")
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

fn normalize_end_position(source: &str, position: usize) -> usize {
    let mut position = position.min(source.len());
    while !source.is_char_boundary(position) {
        position = position.saturating_sub(1);
    }
    if position > 0
        && position < source.len()
        && source.as_bytes()[position - 1] == b'\r'
        && source.as_bytes()[position] == b'\n'
    {
        position += 1;
    }
    position
}
