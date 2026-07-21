use std::borrow::Cow;
use std::cmp::{max, min};
use std::collections::BTreeMap;
use std::sync::Arc;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::TundraTheme;

/// The two representations exposed by the editor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EditorMode {
    #[default]
    Rich,
    Source,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EditorFocus {
    MenuBar,
    Toolbar,
    #[default]
    Canvas,
    StatusBar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMenu {
    File,
    Edit,
    Insert,
    Format,
    View,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorToolbarAction {
    New,
    Open,
    Save,
    Undo,
    Redo,
    ParagraphStyle,
    Bold,
    Italic,
    Strikethrough,
    InlineCode,
    BulletList,
    OrderedList,
    Quote,
    Link,
    Image,
    Table,
    Find,
    More,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMenuAction {
    Toolbar(EditorToolbarAction),
    Mode(EditorMode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorQuickAction {
    Bold,
    Italic,
    Paragraph,
    Heading(u8),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EditorSettingsField {
    #[default]
    Enabled,
    ActivationDelay,
    RampDuration,
    HorizontalMaxStep,
    VerticalMaxStep,
    RestoreDefaults,
    Save,
    Cancel,
}

impl EditorSettingsField {
    pub const ALL: [Self; 8] = [
        Self::Enabled,
        Self::ActivationDelay,
        Self::RampDuration,
        Self::HorizontalMaxStep,
        Self::VerticalMaxStep,
        Self::RestoreDefaults,
        Self::Save,
        Self::Cancel,
    ];

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|field| *field == self)
            .unwrap_or_default();
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn previous(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|field| *field == self)
            .unwrap_or_default();
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorSettingsControl {
    ToggleEnabled,
    Decrease(EditorSettingsField),
    Increase(EditorSettingsField),
    RestoreDefaults,
    Save,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorSettingsViewModel {
    pub enabled: bool,
    pub activation_delay_ms: u32,
    pub ramp_duration_ms: u32,
    pub horizontal_max_step: u8,
    pub vertical_max_step: u8,
    pub selected: EditorSettingsField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorQuickMenuViewModel {
    /// Absolute terminal-cell coordinate used to anchor the popup.
    pub anchor: (u16, u16),
    pub block_actions_enabled: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EditorSpanColor {
    #[default]
    Normal,
    Accent,
    Muted,
    Warning,
    Error,
}

/// A half-open UTF-8 byte range in the canonical editor source.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditorSourceRange {
    pub start: usize,
    pub end: usize,
}

/// Stable identity for an editable node/container in a rich document.
///
/// The UI deliberately treats this value as opaque. It is supplied by the
/// editor's document model and remains stable while blocks are inserted or
/// removed around the node.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u64);

impl NodeId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for NodeId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<NodeId> for u64 {
    fn from(value: NodeId) -> Self {
        value.0
    }
}

/// A cursor boundary in rich content. Offsets count Unicode grapheme
/// clusters, never Markdown bytes or Unicode scalar values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RichPosition {
    pub container_id: NodeId,
    pub grapheme_offset: usize,
}

impl RichPosition {
    pub const fn new(container_id: u64, grapheme_offset: usize) -> Self {
        Self {
            container_id: NodeId::new(container_id),
            grapheme_offset,
        }
    }

    pub const fn in_node(container_id: NodeId, grapheme_offset: usize) -> Self {
        Self {
            container_id,
            grapheme_offset,
        }
    }
}

/// A rich selection/range. Endpoints may be in different containers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct RichRange {
    pub start: RichPosition,
    pub end: RichPosition,
}

impl RichRange {
    pub const fn new(container_id: u64, start: usize, end: usize) -> Self {
        Self {
            start: RichPosition::new(container_id, start),
            end: RichPosition::new(container_id, end),
        }
    }

    pub const fn between(start: RichPosition, end: RichPosition) -> Self {
        Self { start, end }
    }

    pub fn is_empty(self) -> bool {
        self.start.container_id == self.end.container_id
            && self.start.grapheme_offset == self.end.grapheme_offset
    }
}

/// Mode-aware document coordinate used by input routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditorDocumentPosition {
    Rich(RichPosition),
    /// UTF-8 byte boundary in Source mode.
    Source(usize),
}

impl EditorSourceRange {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub const fn is_empty(self) -> bool {
        self.start >= self.end
    }
}

/// A semantic inline run. The application does not need to construct Ratatui styles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorRenderSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub underlined: bool,
    pub inline_code: bool,
    pub link: bool,
    pub color: EditorSpanColor,
    /// Optional mapping back to the exact Markdown/plain-text source.
    /// This is a legacy/Source-mode compatibility field; Rich input routing
    /// uses `rich_range` exclusively.
    pub source_range: Option<EditorSourceRange>,
    /// Logical range in the in-memory rich document. Its length is measured in
    /// grapheme clusters and normally matches the visible text in this span.
    pub rich_range: Option<RichRange>,
}

impl EditorRenderSpan {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    pub fn strong(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: true,
            ..Self::default()
        }
    }

    pub fn emphasis(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            italic: true,
            ..Self::default()
        }
    }

    pub fn code(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            inline_code: true,
            ..Self::default()
        }
    }

    pub fn with_link(mut self) -> Self {
        self.link = true;
        self.underlined = true;
        self.color = EditorSpanColor::Accent;
        self
    }

    pub fn with_source_range(mut self, source_range: EditorSourceRange) -> Self {
        self.source_range = Some(source_range);
        self
    }

    pub fn with_rich_range(mut self, rich_range: RichRange) -> Self {
        self.rich_range = Some(rich_range);
        self
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EditorTableAlignment {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorTableCell {
    pub spans: Vec<EditorRenderSpan>,
}

impl EditorTableCell {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            spans: vec![EditorRenderSpan::plain(text)],
        }
    }
}

/// Semantic Markdown blocks consumed by the terminal renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorRenderBlock {
    Paragraph(Vec<EditorRenderSpan>),
    Heading {
        level: u8,
        spans: Vec<EditorRenderSpan>,
    },
    BulletListItem {
        depth: u8,
        checked: Option<bool>,
        spans: Vec<EditorRenderSpan>,
    },
    OrderedListItem {
        depth: u8,
        number: u64,
        spans: Vec<EditorRenderSpan>,
    },
    Quote {
        depth: u8,
        spans: Vec<EditorRenderSpan>,
    },
    CodeBlock {
        language: Option<String>,
        lines: Vec<String>,
    },
    Table {
        header: Vec<EditorTableCell>,
        rows: Vec<Vec<EditorTableCell>>,
        alignments: Vec<EditorTableAlignment>,
    },
    /// A table projected from the native rich document model.
    ///
    /// Unlike the legacy [`EditorRenderBlock::Table`] payload, this variant
    /// has a stable identity and never requires a Markdown source range for
    /// layout, resizing, or structural edge commands.
    RichTable {
        table_id: NodeId,
        header: Vec<EditorTableCell>,
        rows: Vec<Vec<EditorTableCell>>,
        alignments: Vec<EditorTableAlignment>,
    },
    HorizontalRule,
    RawHtml(String),
    /// The original Markdown is rendered verbatim until an image protocol overlay is available.
    Image {
        markdown: String,
    },
    Footnote {
        label: String,
        spans: Vec<EditorRenderSpan>,
    },
    Blank,
}

impl EditorRenderBlock {
    pub fn paragraph(text: impl Into<String>) -> Self {
        Self::Paragraph(vec![EditorRenderSpan::plain(text)])
    }

    pub fn heading(level: u8, text: impl Into<String>) -> Self {
        Self::Heading {
            level: level.clamp(1, 6),
            spans: vec![EditorRenderSpan::plain(text)],
        }
    }

    /// Returns the stable native-document identity for Rich tables.
    pub const fn table_id(&self) -> Option<NodeId> {
        match self {
            Self::RichTable { table_id, .. } => Some(*table_id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct EditorTextPosition {
    /// A line in the flattened, rendered document (before vertical scrolling).
    pub line: usize,
    /// A terminal-cell column in that line (before horizontal scrolling).
    pub column: usize,
}

impl EditorTextPosition {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorSelection {
    pub anchor: EditorTextPosition,
    pub active: EditorTextPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorSourceSelection {
    pub anchor: usize,
    pub active: usize,
}

impl EditorSourceSelection {
    pub const fn new(anchor: usize, active: usize) -> Self {
        Self { anchor, active }
    }

    pub const fn normalized(self) -> EditorSourceRange {
        EditorSourceRange::new(
            if self.anchor < self.active {
                self.anchor
            } else {
                self.active
            },
            if self.anchor > self.active {
                self.anchor
            } else {
                self.active
            },
        )
    }
}

/// Source metadata for one entry in [`EditorViewModel::blocks`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditorBlockSourceMap {
    pub source_range: EditorSourceRange,
    pub content_range: Option<EditorSourceRange>,
}

impl EditorBlockSourceMap {
    pub const fn new(source_range: EditorSourceRange) -> Self {
        Self {
            source_range,
            content_range: None,
        }
    }

    pub const fn with_content_range(mut self, content_range: EditorSourceRange) -> Self {
        self.content_range = Some(content_range);
        self
    }

    const fn anchor(self) -> usize {
        match self.content_range {
            Some(range) => range.start,
            None => self.source_range.start,
        }
    }
}

impl EditorSelection {
    pub const fn new(anchor: EditorTextPosition, active: EditorTextPosition) -> Self {
        Self { anchor, active }
    }

    pub fn normalized(self) -> (EditorTextPosition, EditorTextPosition) {
        if self.anchor <= self.active {
            (self.anchor, self.active)
        } else {
            (self.active, self.anchor)
        }
    }

    pub fn contains(self, position: EditorTextPosition) -> bool {
        let (start, end) = self.normalized();
        start <= position && position < end
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EditorImageProtocolStatus {
    Detecting,
    #[default]
    Unsupported,
    Available,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorToolbarState {
    pub can_save: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub can_cut: bool,
    pub can_copy: bool,
    pub can_paste: bool,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub inline_code: bool,
    pub bullet_list: bool,
    pub ordered_list: bool,
    pub quote: bool,
}

impl Default for EditorToolbarState {
    fn default() -> Self {
        Self {
            can_save: true,
            can_undo: false,
            can_redo: false,
            can_cut: false,
            can_copy: false,
            can_paste: true,
            bold: false,
            italic: false,
            strikethrough: false,
            inline_code: false,
            bullet_list: false,
            ordered_list: false,
            quote: false,
        }
    }
}

impl EditorToolbarState {
    pub fn is_active(&self, action: EditorToolbarAction) -> bool {
        match action {
            EditorToolbarAction::Bold => self.bold,
            EditorToolbarAction::Italic => self.italic,
            EditorToolbarAction::Strikethrough => self.strikethrough,
            EditorToolbarAction::InlineCode => self.inline_code,
            EditorToolbarAction::BulletList => self.bullet_list,
            EditorToolbarAction::OrderedList => self.ordered_list,
            EditorToolbarAction::Quote => self.quote,
            _ => false,
        }
    }

    pub fn is_enabled(
        &self,
        action: EditorToolbarAction,
        read_only: bool,
        mode: EditorMode,
    ) -> bool {
        if read_only {
            return matches!(
                action,
                EditorToolbarAction::Find | EditorToolbarAction::More
            );
        }
        match action {
            EditorToolbarAction::Save => self.can_save,
            EditorToolbarAction::Undo => self.can_undo,
            EditorToolbarAction::Redo => self.can_redo,
            EditorToolbarAction::Bold
            | EditorToolbarAction::Italic
            | EditorToolbarAction::Strikethrough
            | EditorToolbarAction::InlineCode
            | EditorToolbarAction::BulletList
            | EditorToolbarAction::OrderedList
            | EditorToolbarAction::Quote
            | EditorToolbarAction::Link
            | EditorToolbarAction::Image
            | EditorToolbarAction::Table
            | EditorToolbarAction::ParagraphStyle => !read_only && mode == EditorMode::Rich,
            _ => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorViewModel {
    pub file_name: String,
    pub path_hint: Option<String>,
    pub dirty: bool,
    pub read_only: bool,
    /// Byte range currently loaded for a read-only, potentially tailed document.
    pub read_window: Option<EditorReadWindowViewModel>,
    /// Whether the backing diagnostic document can be reloaded from the editor.
    pub reload_available: bool,
    pub mode: EditorMode,
    pub focus: EditorFocus,
    pub open_menu: Option<EditorMenu>,
    pub settings: Option<EditorSettingsViewModel>,
    pub quick_menu: Option<EditorQuickMenuViewModel>,
    pub selected_toolbar_action: Option<EditorToolbarAction>,
    pub blocks: Vec<EditorRenderBlock>,
    /// Optional shared Rich projection. When present this is authoritative
    /// over the legacy `blocks` vector, allowing callers to reuse a cached
    /// projection without deep-cloning every block for each frame.
    pub shared_blocks: Option<Arc<[EditorRenderBlock]>>,
    pub source_lines: Vec<String>,
    /// Cached half-open byte ranges for Source-mode lines. This is populated
    /// alongside `source_lines` by [`EditorViewModel::source`] so layout can
    /// prepare a viewport without rescanning or allocating for the full file.
    pub source_line_ranges: Vec<EditorSourceRange>,
    /// Optional viewport-only Source data. When present it is authoritative
    /// for layout and avoids materializing the full document in the UI model.
    pub source_window: Option<EditorSourceWindow>,
    pub scroll_line: usize,
    pub horizontal_scroll: usize,
    /// Source-mode horizontal extent in terminal cells: the widest line plus
    /// its trailing caret cell, with an empty document therefore measuring
    /// one cell. Viewport-only callers supply this from the document model so
    /// layout never needs to rescan a large file merely to size the scrollbar.
    pub horizontal_content_width: usize,
    pub cursor: Option<EditorTextPosition>,
    pub selection: Option<EditorSelection>,
    /// Logical Rich-mode cursor. When present, this is authoritative and the
    /// renderer never consults Markdown byte offsets.
    pub rich_cursor: Option<RichPosition>,
    /// Logical Rich-mode selection. Endpoints remain directional so callers
    /// can preserve anchor/focus semantics across blocks.
    pub rich_selection: Option<RichRange>,
    /// Canonical source used only for visual-to-byte mapping. `None` preserves
    /// the legacy visual line/column contract.
    pub source: Option<String>,
    /// Parallel to `blocks`. Missing entries simply disable source mapping for
    /// the corresponding block.
    pub block_sources: Vec<EditorBlockSourceMap>,
    /// Optional user-sized table columns, parallel to `blocks`. Empty entries
    /// keep the natural Markdown table widths. This remains for legacy
    /// source-backed table projections.
    pub table_column_widths: Vec<Vec<usize>>,
    /// User-sized Rich table columns keyed by the table's stable document ID.
    /// Inserting or removing blocks before a table therefore cannot move its
    /// width state to a different table.
    pub rich_table_column_widths: BTreeMap<NodeId, Vec<usize>>,
    pub cursor_offset: Option<usize>,
    pub selection_offsets: Option<EditorSourceSelection>,
    pub toolbar: EditorToolbarState,
    pub word_count: usize,
    pub encoding: String,
    pub line_ending: String,
    pub image_protocol: EditorImageProtocolStatus,
    pub status_message: Option<String>,
}

impl EditorViewModel {
    pub fn new(file_name: impl Into<String>, blocks: Vec<EditorRenderBlock>) -> Self {
        Self {
            file_name: file_name.into(),
            path_hint: None,
            dirty: false,
            read_only: false,
            read_window: None,
            reload_available: false,
            mode: EditorMode::Rich,
            focus: EditorFocus::Canvas,
            open_menu: None,
            settings: None,
            quick_menu: None,
            selected_toolbar_action: None,
            blocks,
            shared_blocks: None,
            source_lines: vec![String::new()],
            source_line_ranges: Vec::new(),
            source_window: None,
            scroll_line: 0,
            horizontal_scroll: 0,
            horizontal_content_width: 1,
            cursor: Some(EditorTextPosition::default()),
            selection: None,
            rich_cursor: None,
            rich_selection: None,
            source: None,
            block_sources: Vec::new(),
            table_column_widths: Vec::new(),
            rich_table_column_widths: BTreeMap::new(),
            cursor_offset: None,
            selection_offsets: None,
            toolbar: EditorToolbarState::default(),
            word_count: 0,
            encoding: "UTF-8".to_string(),
            line_ending: "LF".to_string(),
            image_protocol: EditorImageProtocolStatus::Unsupported,
            status_message: None,
        }
    }

    pub fn source(file_name: impl Into<String>, source: impl AsRef<str>) -> Self {
        let source = source.as_ref();
        let mut model = Self::new(file_name, Vec::new());
        model.mode = EditorMode::Source;
        model.source = Some(source.to_owned());
        model.source_line_ranges = source_display_line_ranges(source);
        model.horizontal_content_width =
            source_horizontal_content_width(source, &model.source_line_ranges);
        model.source_lines = model
            .source_line_ranges
            .iter()
            .map(|range| {
                source
                    .get(range.start..range.end)
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect();
        if model.source_lines.is_empty() {
            model.source_lines.push(String::new());
        }
        model.word_count = source.split_whitespace().count();
        model
    }

    pub fn new_shared(file_name: impl Into<String>, blocks: Arc<[EditorRenderBlock]>) -> Self {
        let mut model = Self::new(file_name, Vec::new());
        model.shared_blocks = Some(blocks);
        model
    }

    /// Builds a Source-mode model from a bounded, globally-addressed window.
    /// The caller may update the remaining editor metadata and cursor fields
    /// exactly as it would for [`Self::source`].
    pub fn source_viewport(
        file_name: impl Into<String>,
        first_line: usize,
        total_line_count: usize,
        lines: Vec<EditorSourceWindowLine>,
    ) -> Self {
        let mut model = Self::new(file_name, Vec::new());
        model.mode = EditorMode::Source;
        // Keep the legacy projection bounded to the same window. Small
        // documents therefore preserve callers that inspect `source_lines`,
        // while large documents never regain a full-text duplicate.
        model.source_lines = lines
            .iter()
            .map(|line| line.text.as_ref().to_owned())
            .collect();
        model.scroll_line = first_line;
        model.source_window = Some(EditorSourceWindow {
            first_line,
            total_line_count,
            lines,
        });
        model
    }

    /// Returns user-specified widths for a table block. Rich tables use their
    /// stable ID; legacy tables fall back to the block-parallel source state.
    pub fn table_widths_for_block(
        &self,
        block_index: usize,
        table_id: Option<NodeId>,
    ) -> Option<&[usize]> {
        match table_id {
            Some(table_id) => self
                .rich_table_column_widths
                .get(&table_id)
                .map(Vec::as_slice),
            None => self.table_column_widths.get(block_index).map(Vec::as_slice),
        }
    }

    /// Rich blocks used by layout and rendering, regardless of whether the
    /// caller supplied the legacy owned vector or a cached shared projection.
    pub fn render_blocks(&self) -> &[EditorRenderBlock] {
        self.shared_blocks.as_deref().unwrap_or(&self.blocks)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorMenuLayout {
    pub menu: EditorMenu,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorMenuItemLayout {
    pub action: EditorMenuAction,
    pub area: Rect,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorSettingsFieldLayout {
    pub field: EditorSettingsField,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorSettingsControlLayout {
    pub control: EditorSettingsControl,
    pub area: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorSettingsLayout {
    pub dialog: Rect,
    pub fields: Vec<EditorSettingsFieldLayout>,
    pub controls: Vec<EditorSettingsControlLayout>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorReadWindowViewModel {
    pub start_byte: u64,
    pub total_bytes: u64,
}

/// One horizontally clipped Source-mode line. Byte offsets and display
/// columns remain global to the canonical document even though only the
/// visible text is materialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorSourceWindowLine {
    pub visible_byte_range: EditorSourceRange,
    pub start_column: usize,
    pub text: Arc<str>,
}

impl EditorSourceWindowLine {
    pub fn new(
        visible_byte_range: EditorSourceRange,
        start_column: usize,
        text: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            visible_byte_range,
            start_column,
            text: text.into(),
        }
    }
}

/// Materialized Source-mode viewport. `first_line` and `total_line_count`
/// keep scrolling and hit testing in global document coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorSourceWindow {
    pub first_line: usize,
    pub total_line_count: usize,
    pub lines: Vec<EditorSourceWindowLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorQuickMenuItemLayout {
    pub action: EditorQuickAction,
    pub area: Rect,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorToolbarItemLayout {
    pub action: EditorToolbarAction,
    pub area: Rect,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorModeLayout {
    pub mode: EditorMode,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorLineLayout {
    pub document_line: usize,
    pub block_index: Option<usize>,
    pub horizontally_scrollable: bool,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorBlockArea {
    pub block_index: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorScrollbarLayout {
    pub track: Rect,
    pub thumb: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorTableResizeHandle {
    /// Stable Rich table identity. `None` denotes a legacy source-backed
    /// projection whose structural commands still use `block_index`.
    pub table_id: Option<NodeId>,
    pub block_index: usize,
    pub column_index: usize,
    pub width: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorTableEdge {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorTableEdgeHandle {
    /// Stable Rich table identity. Present for native Rich tables even when
    /// no Markdown source mapping exists.
    pub table_id: Option<NodeId>,
    pub block_index: usize,
    pub edge: EditorTableEdge,
    /// Legacy source-backed identity. Native Rich tables leave this empty.
    pub source_range: Option<EditorSourceRange>,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorHitTarget {
    SettingsControl(EditorSettingsControl),
    SettingsField(EditorSettingsField),
    SettingsDialog,
    QuickMenuAction(EditorQuickAction),
    QuickMenuPopup,
    Menu(EditorMenu),
    MenuAction(EditorMenuAction),
    MenuPopup,
    Toolbar(EditorToolbarAction),
    Mode(EditorMode),
    TableResize {
        block_index: usize,
        column_index: usize,
        width: usize,
    },
    /// Column boundary in a native Rich table.
    RichTableResize {
        table_id: NodeId,
        column_index: usize,
        width: usize,
    },
    TableEdge {
        block_index: usize,
        edge: EditorTableEdge,
        source_range: EditorSourceRange,
    },
    /// Structural left/right edge in a native Rich table.
    RichTableEdge {
        table_id: NodeId,
        edge: EditorTableEdge,
    },
    Canvas(EditorTextPosition),
    VerticalScrollbar,
    HorizontalScrollbar,
    StatusBar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorCanvasHit {
    pub visual: EditorTextPosition,
    pub byte_offset: usize,
    /// False when the rendered cell is synthetic or cannot be mapped to one
    /// exact source grapheme. Callers must not edit through such a hit.
    pub editable: bool,
}

/// Result of a mode-aware canvas hit test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorDocumentHit {
    pub visual: EditorTextPosition,
    pub position: EditorDocumentPosition,
    /// False for read-only or synthetic display cells. The position remains a
    /// useful cursor anchor, but callers must not mutate through it.
    pub editable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorRichBoundary {
    pub column: usize,
    pub position: RichPosition,
    pub editable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorRichLineMap {
    pub document_line: usize,
    pub boundaries: Vec<EditorRichBoundary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorSourceBoundary {
    pub column: usize,
    pub byte_offset: usize,
    /// `true` for a glyph backed by document content, `false` for a virtual
    /// bullet/border/prefix. Cursor projection prefers editable boundaries.
    pub editable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorSourceLineMap {
    pub document_line: usize,
    pub boundaries: Vec<EditorSourceBoundary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorLayout {
    pub mode: EditorMode,
    pub area: Rect,
    pub menu_bar: Rect,
    pub toolbar: Rect,
    pub canvas_panel: Rect,
    /// Text cells only; borders and the optional scrollbar are excluded.
    pub canvas: Rect,
    pub status_bar: Rect,
    pub menus: Vec<EditorMenuLayout>,
    pub menu_popup: Option<Rect>,
    pub menu_items: Vec<EditorMenuItemLayout>,
    pub settings: Option<EditorSettingsLayout>,
    pub quick_menu_popup: Option<Rect>,
    pub quick_menu_items: Vec<EditorQuickMenuItemLayout>,
    pub toolbar_items: Vec<EditorToolbarItemLayout>,
    pub modes: Vec<EditorModeLayout>,
    pub line_areas: Vec<EditorLineLayout>,
    pub block_areas: Vec<EditorBlockArea>,
    /// Subset of `block_areas` that can receive a ratatui-image overlay.
    pub image_areas: Vec<EditorBlockArea>,
    pub table_edge_handles: Vec<EditorTableEdgeHandle>,
    pub table_resize_handles: Vec<EditorTableResizeHandle>,
    pub vertical_scrollbar: Option<EditorScrollbarLayout>,
    pub horizontal_scrollbar: Option<EditorScrollbarLayout>,
    pub visible_start: usize,
    pub visible_capacity: usize,
    pub document_line_count: usize,
    pub horizontal_scroll: usize,
    pub toolbar_overflow: bool,
    pub canvas_framed: bool,
    pub source_line_maps: Vec<EditorSourceLineMap>,
    /// Rich-mode visual mapping. Unlike `source_line_maps`, this contains only
    /// stable node identities and grapheme offsets.
    pub rich_line_maps: Vec<EditorRichLineMap>,
    /// Display content prepared for this exact viewport. Keeping it in the
    /// layout lets rendering reuse layout work instead of flattening the
    /// document a second time.
    prepared_lines: Vec<DisplayLine>,
    prepared_start: usize,
}

impl EditorLayout {
    pub fn hit_test(&self, x: u16, y: u16) -> Option<EditorHitTarget> {
        if let Some(settings) = self.settings.as_ref() {
            if let Some(control) = settings
                .controls
                .iter()
                .find(|control| contains(control.area, x, y))
            {
                return Some(EditorHitTarget::SettingsControl(control.control));
            }
            if let Some(field) = settings
                .fields
                .iter()
                .find(|field| contains(field.area, x, y))
            {
                return Some(EditorHitTarget::SettingsField(field.field));
            }
            return Some(EditorHitTarget::SettingsDialog);
        }
        if let Some(item) = self
            .quick_menu_items
            .iter()
            .find(|item| item.enabled && contains(item.area, x, y))
        {
            return Some(EditorHitTarget::QuickMenuAction(item.action));
        }
        if self
            .quick_menu_popup
            .is_some_and(|area| contains(area, x, y))
        {
            return Some(EditorHitTarget::QuickMenuPopup);
        }
        if let Some(item) = self.modes.iter().find(|item| contains(item.area, x, y)) {
            return Some(EditorHitTarget::Mode(item.mode));
        }
        if let Some(item) = self.menus.iter().find(|item| contains(item.area, x, y)) {
            return Some(EditorHitTarget::Menu(item.menu));
        }
        if let Some(item) = self
            .menu_items
            .iter()
            .find(|item| item.enabled && contains(item.area, x, y))
        {
            return Some(EditorHitTarget::MenuAction(item.action));
        }
        if self.menu_popup.is_some_and(|area| contains(area, x, y)) {
            return Some(EditorHitTarget::MenuPopup);
        }
        if let Some(item) = self
            .toolbar_items
            .iter()
            .find(|item| item.enabled && contains(item.area, x, y))
        {
            return Some(EditorHitTarget::Toolbar(item.action));
        }
        if self
            .vertical_scrollbar
            .is_some_and(|scrollbar| contains(scrollbar.track, x, y))
        {
            return Some(EditorHitTarget::VerticalScrollbar);
        }
        if self
            .horizontal_scrollbar
            .is_some_and(|scrollbar| contains(scrollbar.track, x, y))
        {
            return Some(EditorHitTarget::HorizontalScrollbar);
        }
        if let Some(handle) = self
            .table_edge_handles
            .iter()
            .find(|handle| contains(handle.area, x, y))
        {
            if let Some(table_id) = handle.table_id {
                return Some(EditorHitTarget::RichTableEdge {
                    table_id,
                    edge: handle.edge,
                });
            }
            if let Some(source_range) = handle.source_range {
                return Some(EditorHitTarget::TableEdge {
                    block_index: handle.block_index,
                    edge: handle.edge,
                    source_range,
                });
            }
        }
        if let Some(handle) = self
            .table_resize_handles
            .iter()
            .find(|handle| contains(handle.area, x, y))
        {
            if let Some(table_id) = handle.table_id {
                return Some(EditorHitTarget::RichTableResize {
                    table_id,
                    column_index: handle.column_index,
                    width: handle.width,
                });
            }
            return Some(EditorHitTarget::TableResize {
                block_index: handle.block_index,
                column_index: handle.column_index,
                width: handle.width,
            });
        }
        if contains(self.canvas, x, y) {
            let line = self
                .visible_start
                .saturating_add(usize::from(y.saturating_sub(self.canvas.y)))
                .min(self.document_line_count.saturating_sub(1));
            let horizontal_scroll = self
                .line_areas
                .iter()
                .find(|line_area| line_area.document_line == line)
                .filter(|line_area| line_area.horizontally_scrollable)
                .map_or(0, |_| self.horizontal_scroll);
            return Some(EditorHitTarget::Canvas(EditorTextPosition::new(
                line,
                usize::from(x.saturating_sub(self.canvas.x)).saturating_add(horizontal_scroll),
            )));
        }
        contains(self.status_bar, x, y).then_some(EditorHitTarget::StatusBar)
    }

    /// Resolves a canvas cell to the canonical UTF-8 byte offset when source
    /// mapping metadata was supplied. Other hit targets and legacy unmapped
    /// models return `None`.
    pub fn hit_test_source(&self, x: u16, y: u16) -> Option<EditorCanvasHit> {
        let EditorHitTarget::Canvas(visual) = self.hit_test(x, y)? else {
            return None;
        };
        let line = self
            .source_line_maps
            .iter()
            .find(|line| line.document_line == visual.line)?;
        let boundary = nearest_source_boundary(&line.boundaries, visual.column)?;
        Some(EditorCanvasHit {
            visual,
            byte_offset: boundary.byte_offset,
            editable: boundary.editable,
        })
    }

    pub fn visual_position_for_source(&self, byte_offset: usize) -> Option<EditorTextPosition> {
        nearest_visual_position(&self.source_line_maps, byte_offset)
    }

    /// Resolves a canvas cell to a Rich document position without consulting
    /// Markdown source text or byte offsets.
    pub fn hit_test_rich(&self, x: u16, y: u16) -> Option<EditorDocumentHit> {
        let EditorHitTarget::Canvas(visual) = self.hit_test(x, y)? else {
            return None;
        };
        let line = self
            .rich_line_maps
            .iter()
            .find(|line| line.document_line == visual.line)?;
        let boundary = nearest_rich_boundary(&line.boundaries, visual.column)?;
        let boundary = if boundary.editable {
            boundary
        } else {
            nearest_editable_rich_boundary(&self.rich_line_maps, visual, boundary.position)
                .unwrap_or(boundary)
        };
        Some(EditorDocumentHit {
            visual,
            position: EditorDocumentPosition::Rich(boundary.position),
            editable: boundary.editable,
        })
    }

    pub fn visual_position_for_rich(&self, position: RichPosition) -> Option<EditorTextPosition> {
        nearest_visual_position_for_rich(&self.rich_line_maps, position)
    }

    /// Mode-aware input mapping. Rich mode never falls back to source byte
    /// offsets; Source mode continues to return UTF-8 byte boundaries.
    pub fn hit_test_document(&self, x: u16, y: u16) -> Option<EditorDocumentHit> {
        match self.mode {
            EditorMode::Rich => self.hit_test_rich(x, y),
            EditorMode::Source => {
                let hit = self.hit_test_source(x, y)?;
                Some(EditorDocumentHit {
                    visual: hit.visual,
                    position: EditorDocumentPosition::Source(hit.byte_offset),
                    editable: hit.editable,
                })
            }
        }
    }

    pub fn visual_position_for_document(
        &self,
        position: EditorDocumentPosition,
    ) -> Option<EditorTextPosition> {
        match (self.mode, position) {
            (EditorMode::Rich, EditorDocumentPosition::Rich(position)) => {
                self.visual_position_for_rich(position)
            }
            (EditorMode::Source, EditorDocumentPosition::Source(byte_offset)) => {
                self.visual_position_for_source(byte_offset)
            }
            _ => None,
        }
    }
}

pub fn editor_layout(area: Rect, model: &EditorViewModel) -> EditorLayout {
    let menu_height = u16::from(area.height > 0);
    let toolbar_height = u16::from(area.height >= 3);
    let status_height = u16::from(area.height >= 4);
    let menu_bar = Rect::new(area.x, area.y, area.width, menu_height);
    let toolbar = Rect::new(
        area.x,
        area.y.saturating_add(menu_height),
        area.width,
        toolbar_height,
    );
    let canvas_panel_y = area
        .y
        .saturating_add(menu_height)
        .saturating_add(toolbar_height);
    let canvas_panel_height = area
        .height
        .saturating_sub(menu_height)
        .saturating_sub(toolbar_height)
        .saturating_sub(status_height);
    let canvas_panel = Rect::new(area.x, canvas_panel_y, area.width, canvas_panel_height);
    let status_bar = Rect::new(
        area.x,
        area.y
            .saturating_add(area.height.saturating_sub(status_height)),
        area.width,
        status_height,
    );

    let canvas_framed = canvas_panel.width >= 20 && canvas_panel.height >= 5;
    let base_canvas = if canvas_framed {
        inset(canvas_panel, 1)
    } else {
        canvas_panel
    };
    let mut canvas = base_canvas;
    let horizontal_scroll = model.horizontal_scroll;
    let (document_line_count, rich_lines) = match model.mode {
        EditorMode::Source => {
            let line_count = source_document_line_count(model);
            // Horizontal and vertical scrollbars affect one another: adding a
            // horizontal bar can make the document vertically overflow, while
            // adding a vertical bar can make the widest line overflow. Start
            // with the full canvas and grow reservations monotonically until
            // both decisions stabilize.
            let mut reserve_vertical = false;
            let mut reserve_horizontal = false;
            loop {
                let candidate_width = base_canvas
                    .width
                    .saturating_sub(u16::from(reserve_vertical));
                let candidate_height = base_canvas
                    .height
                    .saturating_sub(u16::from(reserve_horizontal));
                let next_vertical = base_canvas.width > 1
                    && candidate_height > 0
                    && line_count > usize::from(candidate_height);
                let next_horizontal = base_canvas.height > 1
                    && candidate_width > 0
                    && model.horizontal_content_width > usize::from(candidate_width);
                if next_vertical == reserve_vertical && next_horizontal == reserve_horizontal {
                    break;
                }
                reserve_vertical |= next_vertical;
                reserve_horizontal |= next_horizontal;
            }
            canvas.width = canvas.width.saturating_sub(u16::from(reserve_vertical));
            canvas.height = canvas.height.saturating_sub(u16::from(reserve_horizontal));
            (line_count, None)
        }
        EditorMode::Rich => {
            let base_width = usize::from(base_canvas.width.max(1));
            if base_canvas.width > 1
                && !rich_document_fits_height(model, base_width, usize::from(base_canvas.height))
            {
                canvas.width = canvas.width.saturating_sub(1);
            }
            // Measure first so the expensive, allocation-heavy projection is
            // built only once, at the final width. Previously a scrolling
            // document was flattened at the full width and then flattened in
            // its entirety again after reserving the scrollbar column.
            let lines = flatten_rich_document(model, usize::from(canvas.width.max(1)));
            (lines.len().max(1), Some(lines))
        }
    };
    let visible_capacity = usize::from(canvas.height);
    let max_start = document_line_count.saturating_sub(visible_capacity);
    let visible_start = model.scroll_line.min(max_start);
    let visible_end = visible_start
        .saturating_add(visible_capacity)
        .min(document_line_count);
    let prepared_lines = match rich_lines {
        Some(lines) => lines
            .into_iter()
            .skip(visible_start)
            .take(visible_end.saturating_sub(visible_start))
            .collect(),
        None => source_display_lines_for_viewport(model, visible_start, visible_end),
    };
    let line_areas = (visible_start..visible_end)
        .map(|document_line| EditorLineLayout {
            document_line,
            block_index: prepared_lines
                .get(document_line.saturating_sub(visible_start))
                .and_then(|line| line.block_index),
            horizontally_scrollable: prepared_lines
                .get(document_line.saturating_sub(visible_start))
                .is_some_and(|line| model.mode == EditorMode::Source || line.no_wrap),
            area: Rect::new(
                canvas.x,
                canvas
                    .y
                    .saturating_add(to_u16(document_line.saturating_sub(visible_start))),
                canvas.width,
                1,
            ),
        })
        .collect::<Vec<_>>();

    let mut block_areas: Vec<EditorBlockArea> = Vec::new();
    for line in &line_areas {
        let Some(block_index) = line.block_index else {
            continue;
        };
        if let Some(existing) = block_areas
            .iter_mut()
            .find(|entry| entry.block_index == block_index)
        {
            let bottom = max(existing.area.bottom(), line.area.bottom());
            existing.area.height = bottom.saturating_sub(existing.area.y);
        } else {
            block_areas.push(EditorBlockArea {
                block_index,
                area: line.area,
            });
        }
    }
    let image_areas = block_areas
        .iter()
        .copied()
        .filter(|entry| {
            matches!(
                model.render_blocks().get(entry.block_index),
                Some(EditorRenderBlock::Image { .. })
            )
        })
        .collect();
    let table_resize_handles = if model.read_only {
        Vec::new()
    } else {
        table_resize_handles(canvas, horizontal_scroll, &block_areas, model)
    };
    let table_edge_handles = if model.read_only {
        Vec::new()
    } else {
        table_edge_handles(canvas, horizontal_scroll, &block_areas, model)
    };

    let vertical_scrollbar =
        (document_line_count > visible_capacity && base_canvas.width > 1 && canvas.height > 0)
            .then(|| {
                scrollbar_layout(
                    Rect::new(
                        base_canvas.x,
                        base_canvas.y,
                        base_canvas.width,
                        canvas.height,
                    ),
                    document_line_count,
                    visible_start,
                    visible_capacity,
                )
            });
    let horizontal_scrollbar = (model.mode == EditorMode::Source
        && model.horizontal_content_width > usize::from(canvas.width)
        && base_canvas.height > 1
        && canvas.width > 0)
        .then(|| {
            horizontal_scrollbar_layout(
                Rect::new(canvas.x, canvas.bottom(), canvas.width, 1),
                model.horizontal_content_width,
                horizontal_scroll,
                usize::from(canvas.width),
            )
        });
    let (menus, modes) = menu_layout(menu_bar);
    let (menu_popup, menu_items) = menu_popup_layout(area, &menus, model);
    let settings = model
        .settings
        .as_ref()
        .and_then(|settings| settings_layout(area, settings));
    let (quick_menu_popup, quick_menu_items) = quick_menu_layout(
        area,
        (!model.read_only).then_some(model.quick_menu).flatten(),
    );
    let (toolbar_items, toolbar_overflow) = toolbar_layout(toolbar, model);
    let rich_line_maps = if model.mode == EditorMode::Rich {
        prepared_lines
            .iter()
            .enumerate()
            .filter_map(|(relative_line, line)| {
                let horizontal_start = usize::from(line.no_wrap).saturating_mul(horizontal_scroll);
                let boundaries = display_line_rich_boundaries(
                    line,
                    model.source.as_deref(),
                    horizontal_start,
                    usize::from(canvas.width),
                );
                (!boundaries.is_empty()).then_some(EditorRichLineMap {
                    document_line: visible_start.saturating_add(relative_line),
                    boundaries,
                })
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    // Source maps are authoritative in Source mode. Rich mode retains the
    // legacy source-backed fallback only when no logical Rich mapping exists.
    let source_line_maps = if model.mode == EditorMode::Source || rich_line_maps.is_empty() {
        prepared_lines
            .iter()
            .enumerate()
            .filter_map(|(relative_line, line)| {
                let horizontal_start =
                    usize::from(model.mode == EditorMode::Source || line.no_wrap)
                        .saturating_mul(horizontal_scroll);
                let boundaries = display_line_source_boundaries(
                    line,
                    model.source.as_deref(),
                    horizontal_start,
                    usize::from(canvas.width),
                );
                (!boundaries.is_empty()).then_some(EditorSourceLineMap {
                    document_line: visible_start.saturating_add(relative_line),
                    boundaries,
                })
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    EditorLayout {
        mode: model.mode,
        area,
        menu_bar,
        toolbar,
        canvas_panel,
        canvas,
        status_bar,
        menus,
        menu_popup,
        menu_items,
        settings,
        quick_menu_popup,
        quick_menu_items,
        toolbar_items,
        modes,
        line_areas,
        block_areas,
        image_areas,
        table_edge_handles,
        table_resize_handles,
        vertical_scrollbar,
        horizontal_scrollbar,
        visible_start,
        visible_capacity,
        document_line_count,
        horizontal_scroll,
        toolbar_overflow,
        canvas_framed,
        source_line_maps,
        rich_line_maps,
        prepared_lines,
        prepared_start: visible_start,
    }
}

/// Render only the editor's main area. Shell chrome remains the caller's responsibility.
pub fn render_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &EditorViewModel,
    theme: &TundraTheme,
) -> EditorLayout {
    let layout = editor_layout(area, model);
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(theme.body_style()), area);
    render_menu_bar(frame, &layout, model, theme);
    render_toolbar(frame, &layout, model, theme);
    render_canvas(frame, &layout, model, theme);
    render_status_bar(frame, &layout, model, theme);
    // Popups overlap the editor chrome and canvas. Settings is modal, so it
    // is painted last and receives the highest hit-test priority.
    render_menu_popup(frame, &layout, model, theme);
    render_quick_menu(frame, &layout, theme);
    render_settings(frame, &layout, model, theme);
    layout
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DisplayRun {
    text: DisplayText,
    style: EditorRenderSpan,
    source: DisplaySource,
    rich: DisplayRich,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DisplayText {
    Owned(String),
    Shared(Arc<str>),
    /// A zero-copy slice of `EditorViewModel::source`.
    SourceRange(EditorSourceRange),
}

impl DisplayText {
    fn resolve<'a>(&'a self, source: Option<&'a str>) -> &'a str {
        match self {
            Self::Owned(text) => text,
            Self::Shared(text) => text,
            Self::SourceRange(range) => source
                .and_then(|source| source.get(range.start..range.end))
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplaySource {
    Unmapped,
    Range(EditorSourceRange),
    Virtual(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayRich {
    Unmapped,
    Range(RichRange),
    Virtual(RichPosition),
}

impl DisplayRun {
    fn unmapped(text: impl Into<String>, style: EditorRenderSpan) -> Self {
        Self {
            text: DisplayText::Owned(text.into()),
            style,
            source: DisplaySource::Unmapped,
            rich: DisplayRich::Unmapped,
        }
    }

    fn virtual_text(
        text: impl Into<String>,
        style: EditorRenderSpan,
        byte_offset: Option<usize>,
    ) -> Self {
        Self {
            text: DisplayText::Owned(text.into()),
            style,
            source: byte_offset
                .map(DisplaySource::Virtual)
                .unwrap_or(DisplaySource::Unmapped),
            rich: DisplayRich::Unmapped,
        }
    }

    fn source_range(range: EditorSourceRange) -> Self {
        Self {
            text: DisplayText::SourceRange(range),
            style: EditorRenderSpan::plain(""),
            source: DisplaySource::Range(range),
            rich: DisplayRich::Unmapped,
        }
    }

    fn shared_source(text: Arc<str>, range: EditorSourceRange) -> Self {
        Self {
            text: DisplayText::Shared(text),
            style: EditorRenderSpan::plain(""),
            source: DisplaySource::Range(range),
            rich: DisplayRich::Unmapped,
        }
    }

    fn with_virtual_rich(mut self, position: Option<RichPosition>) -> Self {
        if let Some(position) = position {
            self.rich = DisplayRich::Virtual(position);
        }
        self
    }

    /// Marks an otherwise zero-width display run as an editable Rich cursor
    /// boundary. This differs from a virtual decoration: the position is a
    /// real boundary in the document model even though it has no terminal
    /// cell of its own (for example, immediately before or after a soft
    /// break).
    fn with_editable_rich_boundary(mut self, position: Option<RichPosition>) -> Self {
        if let Some(position) = position {
            self.rich = DisplayRich::Range(RichRange::between(position, position));
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DisplayLine {
    runs: Vec<DisplayRun>,
    block_index: Option<usize>,
    no_wrap: bool,
    /// Absolute display column of the first run. Non-zero only for a
    /// horizontally clipped Source viewport.
    column_start: usize,
}

fn render_menu_bar(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
) {
    if layout.menu_bar.is_empty() {
        return;
    }
    frame.render_widget(
        Block::default().style(Style::default().fg(theme.foreground).bg(Color::DarkGray)),
        layout.menu_bar,
    );
    for item in &layout.menus {
        let active = model.open_menu == Some(item.menu)
            || (item.menu == EditorMenu::Settings && model.settings.is_some());
        let style = if active {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else if model.focus == EditorFocus::MenuBar {
            Style::default().fg(theme.accent_color).bg(Color::DarkGray)
        } else {
            Style::default().fg(theme.foreground).bg(Color::DarkGray)
        };
        frame.render_widget(
            Paragraph::new(format!(" {} ", menu_label(item.menu))).style(style),
            item.area,
        );
    }
    for item in &layout.modes {
        let active = item.mode == model.mode;
        let style = if active {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted).bg(Color::DarkGray)
        };
        frame.render_widget(
            Paragraph::new(format!(" {} ", mode_label(item.mode))).style(style),
            item.area,
        );
    }
}

fn render_menu_popup(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
) {
    let Some(area) = layout.menu_popup else {
        return;
    };
    frame.render_widget(Clear, area);
    frame.render_widget(
        theme
            .block()
            .borders(Borders::ALL)
            .style(Style::default().fg(theme.foreground).bg(theme.background)),
        area,
    );
    for item in &layout.menu_items {
        let active_mode = matches!(item.action, EditorMenuAction::Mode(mode) if mode == model.mode);
        let style = if !item.enabled {
            theme.muted_style()
        } else if active_mode {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.body_style()
        };
        frame.render_widget(
            Paragraph::new(format!(" {}", menu_action_label(item.action))).style(style),
            item.area,
        );
    }
}

fn render_quick_menu(frame: &mut Frame<'_>, layout: &EditorLayout, theme: &TundraTheme) {
    let Some(area) = layout.quick_menu_popup else {
        return;
    };
    frame.render_widget(Clear, area);
    frame.render_widget(
        theme
            .block()
            .borders(Borders::ALL)
            .style(Style::default().fg(theme.foreground).bg(theme.background)),
        area,
    );
    for item in &layout.quick_menu_items {
        let style = if !item.enabled {
            theme.muted_style()
        } else {
            match item.action {
                EditorQuickAction::Bold => theme.body_style().add_modifier(Modifier::BOLD),
                EditorQuickAction::Italic => theme.body_style().add_modifier(Modifier::ITALIC),
                EditorQuickAction::Paragraph => theme.body_style(),
                EditorQuickAction::Heading(level) => {
                    let mut style = Style::default()
                        .fg(theme.accent_color)
                        .bg(theme.background)
                        .add_modifier(Modifier::BOLD);
                    if level == 1 {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    } else if level >= 3 {
                        style = style.add_modifier(Modifier::ITALIC);
                    }
                    style
                }
            }
        };
        frame.render_widget(
            Paragraph::new(format!(" {} ", quick_action_label(item.action))).style(style),
            item.area,
        );
    }
}

fn render_settings(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
) {
    let (Some(settings_layout), Some(settings)) = (&layout.settings, model.settings.as_ref())
    else {
        return;
    };
    frame.render_widget(Clear, settings_layout.dialog);
    frame.render_widget(
        theme
            .block()
            .borders(Borders::ALL)
            .title(" Editor Settings ")
            .style(Style::default().fg(theme.foreground).bg(theme.background)),
        settings_layout.dialog,
    );
    let description = Rect::new(
        settings_layout.dialog.x.saturating_add(2),
        settings_layout.dialog.y.saturating_add(1),
        settings_layout.dialog.width.saturating_sub(4),
        1,
    );
    frame.render_widget(
        Paragraph::new("Hold one direction to accelerate with a quadratic curve.")
            .style(theme.muted_style()),
        description,
    );

    for field in &settings_layout.fields {
        let selected = field.field == settings.selected;
        let style = if selected {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.body_style()
        };
        frame.render_widget(Block::default().style(style), field.area);
        let label = match field.field {
            EditorSettingsField::Enabled => " Cursor acceleration",
            EditorSettingsField::ActivationDelay => " Start delay",
            EditorSettingsField::RampDuration => " Ramp to maximum",
            EditorSettingsField::HorizontalMaxStep => " Horizontal maximum",
            EditorSettingsField::VerticalMaxStep => " Vertical maximum",
            EditorSettingsField::RestoreDefaults
            | EditorSettingsField::Save
            | EditorSettingsField::Cancel => "",
        };
        if !label.is_empty() {
            frame.render_widget(Paragraph::new(label).style(style), field.area);
        }
    }

    for control in &settings_layout.controls {
        let field = settings_control_field(control.control);
        let selected = field.is_some_and(|field| field == settings.selected);
        let style = if selected {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.body_style()
        };
        let label = match control.control {
            EditorSettingsControl::ToggleEnabled => {
                if settings.enabled {
                    "[ ON ]"
                } else {
                    "[OFF ]"
                }
            }
            EditorSettingsControl::Decrease(_) => "[-]",
            EditorSettingsControl::Increase(_) => "[+]",
            EditorSettingsControl::RestoreDefaults => "[ Restore defaults ]",
            EditorSettingsControl::Save => "[ Save ]",
            EditorSettingsControl::Cancel => "[ Cancel ]",
        };
        frame.render_widget(Paragraph::new(label).style(style), control.area);
    }

    for (field, value) in [
        (
            EditorSettingsField::ActivationDelay,
            format!("{} ms", settings.activation_delay_ms),
        ),
        (
            EditorSettingsField::RampDuration,
            format!("{} ms", settings.ramp_duration_ms),
        ),
        (
            EditorSettingsField::HorizontalMaxStep,
            format!("{} cells", settings.horizontal_max_step),
        ),
        (
            EditorSettingsField::VerticalMaxStep,
            format!("{} lines", settings.vertical_max_step),
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
        let width = usize::from(value_area.width);
        let style = if settings.selected == field {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.body_style()
        };
        frame.render_widget(
            Paragraph::new(format!("{value:^width$}")).style(style),
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
        Paragraph::new("Tab select · Left/Right adjust · Enter activate · Esc cancel")
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
) {
    if layout.toolbar.is_empty() {
        return;
    }
    frame.render_widget(
        Block::default().style(Style::default().fg(theme.foreground).bg(theme.background)),
        layout.toolbar,
    );
    for item in &layout.toolbar_items {
        let active = model.toolbar.is_active(item.action);
        let selected = model.selected_toolbar_action == Some(item.action)
            && model.focus == EditorFocus::Toolbar;
        let style = if !item.enabled {
            theme.muted_style()
        } else if active || selected {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_color)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.body_style()
        };
        frame.render_widget(
            Paragraph::new(toolbar_label(item.action)).style(style),
            item.area,
        );
    }
}

fn render_canvas(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    theme: &TundraTheme,
) {
    if layout.canvas_panel.is_empty() {
        return;
    }
    if layout.canvas_framed {
        let mut title = model.file_name.clone();
        if model.dirty {
            title.push_str(" *");
        }
        if model.read_only {
            title.push_str(" [read-only]");
        }
        if model
            .read_window
            .is_some_and(|window| window.start_byte > 0)
        {
            title.push_str(" [tail]");
        }
        let title = terminal_safe_text(&title).into_owned();
        frame.render_widget(
            theme
                .block()
                .borders(Borders::ALL)
                .title(title)
                .style(theme.body_style()),
            layout.canvas_panel,
        );
    } else {
        frame.render_widget(
            Block::default().style(theme.body_style()),
            layout.canvas_panel,
        );
    }
    if layout.canvas.is_empty() {
        return;
    }

    for line_layout in &layout.line_areas {
        let Some(display_line) = layout.prepared_lines.get(
            line_layout
                .document_line
                .saturating_sub(layout.prepared_start),
        ) else {
            continue;
        };
        let line = styled_line(
            display_line,
            line_layout.document_line,
            layout,
            model,
            theme,
            usize::from(layout.canvas.width),
        );
        frame.render_widget(
            Paragraph::new(line).style(theme.body_style()),
            line_layout.area,
        );
    }

    if let Some(scrollbar) = layout.vertical_scrollbar {
        for y in scrollbar.track.y..scrollbar.track.bottom() {
            frame.render_widget(
                Paragraph::new("|").style(theme.muted_style()),
                Rect::new(scrollbar.track.x, y, 1, 1),
            );
        }
        for y in scrollbar.thumb.y..scrollbar.thumb.bottom() {
            frame.render_widget(
                Paragraph::new("#").style(theme.title_style()),
                Rect::new(scrollbar.thumb.x, y, 1, 1),
            );
        }
    }

    if let Some(scrollbar) = layout.horizontal_scrollbar {
        for x in scrollbar.track.x..scrollbar.track.right() {
            frame.render_widget(
                Paragraph::new("-").style(theme.muted_style()),
                Rect::new(x, scrollbar.track.y, 1, 1),
            );
        }
        for x in scrollbar.thumb.x..scrollbar.thumb.right() {
            frame.render_widget(
                Paragraph::new("#").style(theme.title_style()),
                Rect::new(x, scrollbar.thumb.y, 1, 1),
            );
        }
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
        EditorImageProtocolStatus::Detecting => "image:detecting",
        EditorImageProtocolStatus::Unsupported => "image:fallback",
        EditorImageProtocolStatus::Available => "image:terminal",
    };
    let mode = mode_label(model.mode);
    let read_window = model.read_window.map(|window| {
        if window.total_bytes == 0 {
            "Bytes 0 of 0".to_string()
        } else {
            let start = window.start_byte.min(window.total_bytes.saturating_sub(1));
            format!(
                "Bytes {}-{} of {}",
                start.saturating_add(1),
                window.total_bytes,
                window.total_bytes
            )
        }
    });
    let left = model
        .status_message
        .as_deref()
        .unwrap_or(if model.read_only {
            "Read only"
        } else {
            "Ready"
        });
    let left = if model.reload_available {
        format!("{left} · R Reload")
    } else {
        left.to_string()
    };
    let right = format!(
        "{}  Ln {}, Col {}  {} words  {}/{}  {}{}",
        mode,
        cursor.line.saturating_add(1),
        cursor.column.saturating_add(1),
        model.word_count,
        model.encoding,
        model.line_ending,
        image,
        read_window.map_or_else(String::new, |window| format!("  {window}")),
    );
    let available = usize::from(layout.status_bar.width);
    let text = if available == 0 {
        String::new()
    } else if left.chars().count() + right.chars().count() + 2 <= available {
        format!(
            "{}{}{}",
            left,
            " ".repeat(available - left.chars().count() - right.chars().count()),
            right
        )
    } else {
        fit_text(&format!("{} | {}", left, right), available)
    };
    let text = terminal_safe_text(&text).into_owned();
    let style = if model.focus == EditorFocus::StatusBar {
        Style::default().fg(theme.background).bg(theme.accent_color)
    } else {
        Style::default().fg(theme.foreground).bg(Color::DarkGray)
    };
    frame.render_widget(Paragraph::new(text).style(style), layout.status_bar);
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
            let cell_width = Span::raw(safe.as_str()).width().max(1);
            let start = column;
            column = column.saturating_add(cell_width);
            if column <= scroll {
                continue;
            }
            if visible_width.saturating_add(cell_width) > width {
                break;
            }
            let position = EditorTextPosition::new(document_line, start);
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
                    .fg(theme.background)
                    .bg(theme.accent_color)
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
                    .then(|| model.cursor_offset)
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
        EditorSpanColor::Warning => Color::Yellow,
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
        style = style.fg(Color::White).bg(Color::DarkGray);
    }
    style
}

/// Returns whether the Rich document fits without a vertical scrollbar.
///
/// This mirrors the line-counting semantics of [`block_lines`] without
/// constructing `DisplayRun`s. Wrapped blocks stop as soon as the supplied
/// height is exceeded, so a large document normally measures only a bounded
/// prefix before being flattened once at the final canvas width.
fn rich_document_fits_height(model: &EditorViewModel, width: usize, height: usize) -> bool {
    let blocks = model.render_blocks();
    if blocks.is_empty() {
        return height >= 1;
    }

    let width = width.max(1);
    let mut line_count = 0usize;
    for block in blocks {
        let remaining = height.saturating_sub(line_count);
        let Some(block_line_count) = rich_block_line_count_up_to(block, width, remaining) else {
            return false;
        };
        line_count = line_count.saturating_add(block_line_count);
    }
    line_count <= height
}

fn rich_block_line_count_up_to(
    block: &EditorRenderBlock,
    width: usize,
    limit: usize,
) -> Option<usize> {
    match block {
        EditorRenderBlock::Paragraph(spans) | EditorRenderBlock::Heading { spans, .. } => {
            wrapped_line_count_up_to("", spans, width, limit)
        }
        EditorRenderBlock::BulletListItem {
            depth,
            checked,
            spans,
        } => {
            let marker = match checked {
                Some(true) => "☒ ",
                Some(false) => "☐ ",
                None => "• ",
            };
            let prefix = format!("{}{}", "  ".repeat(usize::from(*depth)), marker);
            wrapped_line_count_up_to(&prefix, spans, width, limit)
        }
        EditorRenderBlock::OrderedListItem {
            depth,
            number,
            spans,
        } => {
            let prefix = format!("{}{}. ", "  ".repeat(usize::from(*depth)), number);
            wrapped_line_count_up_to(&prefix, spans, width, limit)
        }
        EditorRenderBlock::Quote { depth, spans } => {
            let prefix = "│ ".repeat(usize::from((*depth).max(1)));
            wrapped_line_count_up_to(&prefix, spans, width, limit)
        }
        EditorRenderBlock::Footnote { label, spans } => {
            let prefix = format!("[^{label}] ");
            wrapped_line_count_up_to(&prefix, spans, width, limit)
        }
        EditorRenderBlock::CodeBlock { lines, .. } => {
            fixed_line_count_up_to(lines.len().saturating_add(2), limit)
        }
        EditorRenderBlock::Table { header, rows, .. }
        | EditorRenderBlock::RichTable { header, rows, .. } => {
            let has_columns = !header.is_empty() || rows.iter().any(|row| !row.is_empty());
            let line_count = if !has_columns {
                1
            } else {
                rows.len()
                    .saturating_add(2)
                    .saturating_add(usize::from(!header.is_empty()).saturating_mul(2))
            };
            fixed_line_count_up_to(line_count, limit)
        }
        EditorRenderBlock::HorizontalRule
        | EditorRenderBlock::RawHtml(_)
        | EditorRenderBlock::Image { .. }
        | EditorRenderBlock::Blank => fixed_line_count_up_to(1, limit),
    }
}

fn fixed_line_count_up_to(line_count: usize, limit: usize) -> Option<usize> {
    (line_count <= limit).then_some(line_count)
}

/// Counts the lines produced by [`wrap_runs`] without creating one owned
/// `DisplayRun` per grapheme. `None` means the count exceeded `limit`.
fn wrapped_line_count_up_to(
    prefix: &str,
    spans: &[EditorRenderSpan],
    width: usize,
    limit: usize,
) -> Option<usize> {
    let prefix_width = Span::raw(terminal_safe_text(prefix)).width();
    let mut measure = WrappedLineMeasure {
        width,
        limit,
        prefix_width,
        line_count: 0,
        current_width: prefix_width,
        current_has_runs: !prefix.is_empty(),
        continuation_has_runs: prefix_width > 0,
    };

    for span in spans {
        // `mapped_text_runs` emits one empty run for an empty span. It affects
        // whether a final empty visual line exists even though it has no width.
        if span.text.is_empty() {
            measure.current_has_runs = true;
            continue;
        }

        let bytes = span.text.as_bytes();
        let mut segment_start = 0usize;
        let mut index = 0usize;
        while index < bytes.len() {
            let newline_len = match bytes[index] {
                b'\r' if bytes.get(index + 1) == Some(&b'\n') => 2,
                b'\r' | b'\n' => 1,
                _ => {
                    index += 1;
                    continue;
                }
            };
            if !measure.push_segment(&span.text[segment_start..index]) {
                return None;
            }
            let mapped_boundary = span.source_range.is_some() || span.rich_range.is_some();
            if !measure.push_newline(mapped_boundary) {
                return None;
            }
            index += newline_len;
            segment_start = index;
        }
        if !measure.push_segment(&span.text[segment_start..]) {
            return None;
        }
    }

    measure.finish()
}

struct WrappedLineMeasure {
    width: usize,
    limit: usize,
    prefix_width: usize,
    line_count: usize,
    current_width: usize,
    current_has_runs: bool,
    continuation_has_runs: bool,
}

impl WrappedLineMeasure {
    fn push_segment(&mut self, segment: &str) -> bool {
        if segment.is_empty() {
            return true;
        }
        let span = Span::raw(segment);
        for grapheme in span.styled_graphemes(Style::default()) {
            let run_width = Span::raw(terminal_safe_text(grapheme.symbol)).width();
            if self.current_width > self.prefix_width
                && self.current_width.saturating_add(run_width) > self.width
                && !self.push_line()
            {
                return false;
            }
            self.current_width = self.current_width.saturating_add(run_width);
            self.current_has_runs = true;
        }
        true
    }

    fn push_newline(&mut self, mapped_boundary: bool) -> bool {
        if !self.push_line() {
            return false;
        }
        self.current_width = self.prefix_width;
        self.current_has_runs = self.continuation_has_runs || mapped_boundary;
        true
    }

    fn push_line(&mut self) -> bool {
        self.line_count = self.line_count.saturating_add(1);
        if self.line_count > self.limit {
            return false;
        }
        self.current_width = self.prefix_width;
        self.current_has_runs = self.continuation_has_runs;
        true
    }

    fn finish(mut self) -> Option<usize> {
        if (self.current_has_runs || self.line_count == 0) && !self.push_line() {
            return None;
        }
        Some(self.line_count)
    }
}

#[cfg(test)]
std::thread_local! {
    static RICH_FLATTEN_CALL_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn flatten_rich_document(model: &EditorViewModel, width: usize) -> Vec<DisplayLine> {
    #[cfg(test)]
    RICH_FLATTEN_CALL_COUNT.with(|count| count.set(count.get().saturating_add(1)));

    let width = width.max(1);
    let mut output = Vec::new();
    for (block_index, block) in model.render_blocks().iter().enumerate() {
        let table_widths = model.table_widths_for_block(block_index, block.table_id());
        let lines = block_lines(
            block,
            block_index,
            width,
            model.block_sources.get(block_index).copied(),
            table_widths,
            model.source.as_deref(),
        );
        output.extend(lines);
    }
    if output.is_empty() {
        output.push(empty_display_line(None));
    }
    output
}

fn block_lines(
    block: &EditorRenderBlock,
    block_index: usize,
    width: usize,
    block_source: Option<EditorBlockSourceMap>,
    table_widths: Option<&[usize]>,
    source: Option<&str>,
) -> Vec<DisplayLine> {
    let anchor = block_source.map(EditorBlockSourceMap::anchor);
    match block {
        EditorRenderBlock::Paragraph(spans) => {
            wrap_runs(Vec::new(), spans, width, block_index, false, source)
        }
        EditorRenderBlock::Heading { level, spans } => {
            let level = (*level).clamp(1, 6);
            let styled: Vec<_> = spans
                .iter()
                .cloned()
                .map(|mut span| {
                    span.bold = true;
                    span.color = EditorSpanColor::Accent;
                    span.underlined |= level == 1;
                    span.italic |= level >= 3;
                    span
                })
                .collect();
            wrap_runs(Vec::new(), &styled, width, block_index, false, source)
        }
        EditorRenderBlock::BulletListItem {
            depth,
            checked,
            spans,
        } => {
            let marker = match checked {
                Some(true) => "☒ ",
                Some(false) => "☐ ",
                None => "• ",
            };
            let prefix = format!("{}{}", "  ".repeat(usize::from(*depth)), marker);
            wrap_runs(
                vec![DisplayRun::virtual_text(prefix, accent_span(), anchor)],
                spans,
                width,
                block_index,
                false,
                source,
            )
        }
        EditorRenderBlock::OrderedListItem {
            depth,
            number,
            spans,
        } => {
            let prefix = format!("{}{}. ", "  ".repeat(usize::from(*depth)), number);
            wrap_runs(
                vec![DisplayRun::virtual_text(prefix, accent_span(), anchor)],
                spans,
                width,
                block_index,
                false,
                source,
            )
        }
        EditorRenderBlock::Quote { depth, spans } => {
            let prefix = "│ ".repeat(usize::from((*depth).max(1)));
            let quote_spans: Vec<_> = spans
                .iter()
                .cloned()
                .map(|mut span| {
                    span.italic = true;
                    span
                })
                .collect();
            wrap_runs(
                vec![DisplayRun::virtual_text(prefix, accent_span(), anchor)],
                &quote_spans,
                width,
                block_index,
                false,
                source,
            )
        }
        EditorRenderBlock::CodeBlock { language, lines } => {
            let language = language.as_deref().unwrap_or("code");
            let mut output = vec![DisplayLine {
                runs: vec![DisplayRun::virtual_text(
                    format!("┌─ {language} "),
                    accent_span(),
                    anchor,
                )],
                block_index: Some(block_index),
                no_wrap: true,
                column_start: 0,
            }];
            let line_ranges = code_line_source_ranges(lines, block_source, source);
            output.extend(lines.iter().enumerate().map(|(index, line)| {
                let line_range = line_ranges.get(index).copied().flatten();
                let line_anchor = line_range.map(|range| range.start).or(anchor);
                let mut runs = vec![DisplayRun::virtual_text("│ ", accent_span(), line_anchor)];
                runs.extend(mapped_text_runs(
                    line,
                    EditorRenderSpan::code(""),
                    line_range,
                    source,
                ));
                DisplayLine {
                    runs,
                    block_index: Some(block_index),
                    no_wrap: true,
                    column_start: 0,
                }
            }));
            let end_anchor = block_source
                .and_then(|mapping| mapping.content_range)
                .map(|range| range.end)
                .or(anchor);
            output.push(DisplayLine {
                runs: vec![DisplayRun::virtual_text("└─", accent_span(), end_anchor)],
                block_index: Some(block_index),
                no_wrap: true,
                column_start: 0,
            });
            output
        }
        EditorRenderBlock::Table {
            header,
            rows,
            alignments,
        }
        | EditorRenderBlock::RichTable {
            table_id: _,
            header,
            rows,
            alignments,
        } => table_lines(
            header,
            rows,
            alignments,
            table_widths,
            block_index,
            anchor,
            source,
        ),
        EditorRenderBlock::HorizontalRule => vec![DisplayLine {
            runs: vec![DisplayRun::virtual_text(
                "─".repeat(width),
                accent_span(),
                anchor,
            )],
            block_index: Some(block_index),
            no_wrap: true,
            column_start: 0,
        }],
        EditorRenderBlock::RawHtml(html) => {
            let mut runs = vec![DisplayRun::virtual_text("HTML ", warning_span(), anchor)];
            runs.extend(mapped_text_runs(
                html,
                warning_span(),
                block_source.map(|mapping| mapping.source_range),
                source,
            ));
            vec![DisplayLine {
                runs,
                block_index: Some(block_index),
                no_wrap: true,
                column_start: 0,
            }]
        }
        EditorRenderBlock::Image { markdown } => vec![DisplayLine {
            runs: mapped_text_runs(
                markdown,
                muted_span(),
                block_source.map(|mapping| mapping.source_range),
                source,
            ),
            block_index: Some(block_index),
            no_wrap: true,
            column_start: 0,
        }],
        EditorRenderBlock::Footnote { label, spans } => wrap_runs(
            vec![DisplayRun::virtual_text(
                format!("[^{label}] "),
                accent_span(),
                anchor,
            )],
            spans,
            width,
            block_index,
            false,
            source,
        ),
        EditorRenderBlock::Blank => vec![empty_display_line_at(Some(block_index), anchor)],
    }
}

fn table_lines(
    header: &[EditorTableCell],
    rows: &[Vec<EditorTableCell>],
    alignments: &[EditorTableAlignment],
    requested_widths: Option<&[usize]>,
    block_index: usize,
    anchor: Option<usize>,
    source: Option<&str>,
) -> Vec<DisplayLine> {
    let columns = max(
        header.len(),
        rows.iter().map(Vec::len).max().unwrap_or_default(),
    );
    if columns == 0 {
        return vec![empty_display_line_at(Some(block_index), anchor)];
    }
    let rich_anchor = std::iter::once(header)
        .chain(rows.iter().map(Vec::as_slice))
        .flat_map(|row| row.iter())
        .find_map(|cell| table_cell_rich_range(cell).map(|range| range.start));
    let widths = table_column_widths(header, rows, requested_widths);
    let first_row = if header.is_empty() {
        rows.first().map(Vec::as_slice).unwrap_or_default()
    } else {
        header
    };
    let mut output = vec![table_border_line(
        '┌',
        '┬',
        '┐',
        &widths,
        first_row,
        block_index,
        false,
        anchor,
        rich_anchor,
    )];
    if !header.is_empty() {
        output.push(table_row_line(
            header,
            &widths,
            alignments,
            block_index,
            true,
            anchor,
            rich_anchor,
            source,
        ));
        output.push(table_border_line(
            '├',
            '┼',
            '┤',
            &widths,
            header,
            block_index,
            false,
            anchor,
            rich_anchor,
        ));
    }
    output.extend(rows.iter().map(|row| {
        table_row_line(
            row,
            &widths,
            alignments,
            block_index,
            false,
            anchor,
            rich_anchor,
            source,
        )
    }));
    let last_row = rows.last().map(Vec::as_slice).unwrap_or(first_row);
    output.push(table_border_line(
        '└',
        '┴',
        '┘',
        &widths,
        last_row,
        block_index,
        false,
        anchor,
        rich_anchor,
    ));
    output
}

fn table_row_line(
    row: &[EditorTableCell],
    widths: &[usize],
    alignments: &[EditorTableAlignment],
    block_index: usize,
    header: bool,
    anchor: Option<usize>,
    rich_anchor: Option<RichPosition>,
    source: Option<&str>,
) -> DisplayLine {
    let row_anchor = row
        .iter()
        .find_map(|cell| table_cell_rich_range(cell).map(|range| range.start))
        .or(rich_anchor);
    let mut runs = vec![
        DisplayRun::virtual_text("│", table_span(header), anchor).with_virtual_rich(row_anchor),
    ];
    for (column, width) in widths.iter().enumerate() {
        let cell = row.get(column);
        let cell_range = cell.and_then(table_cell_source_range);
        let start = cell_range.map(|range| range.start);
        let end = cell_range.map(|range| range.end);
        let rich_range = cell.and_then(table_cell_rich_range);
        let rich_start = rich_range.map(|range| range.start).or(row_anchor);
        let rich_end = rich_range.map(|range| range.end).or(rich_start);
        let mut content = cell.map_or_else(Vec::new, |cell| {
            cell.spans
                .iter()
                .flat_map(|span| {
                    let mut style = span.clone();
                    style.color = EditorSpanColor::Accent;
                    style.bold |= header;
                    mapped_text_runs(&span.text, style, span.source_range, source)
                })
                .collect::<Vec<_>>()
        });
        let (fitted, used) = fit_table_content(
            std::mem::take(&mut content),
            *width,
            end.or(start).or(anchor),
            rich_end,
            header,
        );
        let remaining = width.saturating_sub(used);
        let (aligned_left, aligned_right) =
            match alignments.get(column).copied().unwrap_or_default() {
                EditorTableAlignment::Left => (0, remaining),
                EditorTableAlignment::Center => {
                    (remaining / 2, remaining.saturating_sub(remaining / 2))
                }
                EditorTableAlignment::Right => (remaining, 0),
            };
        runs.push(table_padding(
            " ".repeat(aligned_left.saturating_add(1)),
            start,
            anchor,
            rich_start,
            header,
        ));
        runs.extend(fitted);
        runs.push(table_padding(
            " ".repeat(aligned_right.saturating_add(1)),
            end.or(start),
            anchor,
            rich_end,
            header,
        ));
        runs.push(
            DisplayRun::virtual_text("│", table_span(header), end.or(start).or(anchor))
                .with_virtual_rich(rich_end),
        );
    }
    DisplayLine {
        runs,
        block_index: Some(block_index),
        no_wrap: true,
        column_start: 0,
    }
}

fn table_border_line(
    left: char,
    middle: char,
    right: char,
    widths: &[usize],
    cells: &[EditorTableCell],
    block_index: usize,
    header: bool,
    anchor: Option<usize>,
    rich_anchor: Option<RichPosition>,
) -> DisplayLine {
    let first_anchor = cells
        .first()
        .and_then(table_cell_rich_range)
        .map(|range| range.start)
        .or(rich_anchor);
    let mut runs = vec![
        DisplayRun::virtual_text(left.to_string(), table_span(header), anchor)
            .with_virtual_rich(first_anchor),
    ];
    for (column, width) in widths.iter().copied().enumerate() {
        let cell_range = cells.get(column).and_then(table_cell_rich_range);
        let start = cell_range.map(|range| range.start).or(rich_anchor);
        let end = cell_range.map(|range| range.end).or(start);
        runs.push(
            DisplayRun::virtual_text(
                "─".repeat(width.saturating_add(2)),
                table_span(header),
                anchor,
            )
            .with_virtual_rich(start),
        );
        let delimiter = if column + 1 == widths.len() {
            right
        } else {
            middle
        };
        runs.push(
            DisplayRun::virtual_text(delimiter.to_string(), table_span(header), anchor)
                .with_virtual_rich(end),
        );
    }
    DisplayLine {
        runs,
        block_index: Some(block_index),
        no_wrap: true,
        column_start: 0,
    }
}

fn table_span(header: bool) -> EditorRenderSpan {
    let mut style = EditorRenderSpan::plain("");
    style.color = EditorSpanColor::Accent;
    style.bold = header;
    style
}

fn table_padding(
    text: String,
    byte_offset: Option<usize>,
    fallback: Option<usize>,
    rich_position: Option<RichPosition>,
    header: bool,
) -> DisplayRun {
    match byte_offset {
        Some(byte_offset) => DisplayRun {
            text: DisplayText::Owned(text),
            style: table_span(header),
            source: DisplaySource::Range(EditorSourceRange::new(byte_offset, byte_offset)),
            rich: rich_position
                .map(DisplayRich::Virtual)
                .unwrap_or(DisplayRich::Unmapped),
        },
        None => DisplayRun::virtual_text(text, table_span(header), fallback)
            .with_virtual_rich(rich_position),
    }
}

fn table_cell_rich_range(cell: &EditorTableCell) -> Option<RichRange> {
    let mut ranges = cell.spans.iter().filter_map(|span| span.rich_range);
    let first = ranges.next()?;
    let mut start = first.start;
    let mut end = first.end;
    for range in ranges {
        if range.start.container_id != start.container_id
            || range.end.container_id != start.container_id
        {
            return Some(RichRange::between(first.start, first.end));
        }
        start.grapheme_offset = start.grapheme_offset.min(range.start.grapheme_offset);
        end.grapheme_offset = end.grapheme_offset.max(range.end.grapheme_offset);
    }
    Some(RichRange::between(start, end))
}

fn table_cell_source_range(cell: &EditorTableCell) -> Option<EditorSourceRange> {
    Some(EditorSourceRange::new(
        cell.spans
            .iter()
            .filter_map(|span| span.source_range)
            .map(|range| range.start)
            .min()?,
        cell.spans
            .iter()
            .filter_map(|span| span.source_range)
            .map(|range| range.end)
            .max()?,
    ))
}

fn fit_table_content(
    runs: Vec<DisplayRun>,
    width: usize,
    overflow_anchor: Option<usize>,
    rich_overflow_anchor: Option<RichPosition>,
    header: bool,
) -> (Vec<DisplayRun>, usize) {
    let used = runs_width(&runs);
    if used <= width {
        return (runs, used);
    }
    if width == 0 {
        return (Vec::new(), 0);
    }
    let content_width = width.saturating_sub(1);
    let mut fitted = Vec::new();
    let mut fitted_width = 0usize;
    for run in runs {
        let run_width = display_run_width(&run);
        if fitted_width.saturating_add(run_width) > content_width {
            break;
        }
        fitted_width = fitted_width.saturating_add(run_width);
        fitted.push(run);
    }
    fitted.push(
        DisplayRun::virtual_text("…", table_span(header), overflow_anchor)
            .with_virtual_rich(rich_overflow_anchor),
    );
    (fitted, width)
}

fn table_column_widths(
    header: &[EditorTableCell],
    rows: &[Vec<EditorTableCell>],
    requested: Option<&[usize]>,
) -> Vec<usize> {
    let columns = max(
        header.len(),
        rows.iter().map(Vec::len).max().unwrap_or_default(),
    );
    let mut widths = vec![1usize; columns];
    for row in std::iter::once(header).chain(rows.iter().map(Vec::as_slice)) {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(
                Span::raw(terminal_safe_text(&cell_text(cell)))
                    .width()
                    .min(24),
            );
        }
    }
    if let Some(requested) = requested {
        for (width, requested) in widths.iter_mut().zip(requested.iter().copied()) {
            if requested > 0 {
                *width = requested.clamp(1, 120);
            }
        }
    }
    widths
}

fn table_resize_handles(
    canvas: Rect,
    horizontal_scroll: usize,
    block_areas: &[EditorBlockArea],
    model: &EditorViewModel,
) -> Vec<EditorTableResizeHandle> {
    let mut handles = Vec::new();
    for block_area in block_areas {
        let Some(block) = model.render_blocks().get(block_area.block_index) else {
            continue;
        };
        let (table_id, header, rows) = match block {
            EditorRenderBlock::Table { header, rows, .. } => (None, header, rows),
            EditorRenderBlock::RichTable {
                table_id,
                header,
                rows,
                ..
            } => (Some(*table_id), header, rows),
            _ => continue,
        };
        let widths = table_column_widths(
            header,
            rows,
            model.table_widths_for_block(block_area.block_index, table_id),
        );
        let mut boundary_column = 0usize;
        for (column_index, width) in widths.into_iter().enumerate() {
            boundary_column = boundary_column.saturating_add(width.saturating_add(3));
            let Some(visible_column) = boundary_column.checked_sub(horizontal_scroll) else {
                continue;
            };
            if visible_column >= usize::from(canvas.width) {
                continue;
            }
            handles.push(EditorTableResizeHandle {
                table_id,
                block_index: block_area.block_index,
                column_index,
                width,
                area: Rect::new(
                    canvas.x.saturating_add(to_u16(visible_column)),
                    block_area.area.y,
                    1,
                    block_area.area.height,
                ),
            });
        }
    }
    handles
}

fn table_edge_handles(
    canvas: Rect,
    horizontal_scroll: usize,
    block_areas: &[EditorBlockArea],
    model: &EditorViewModel,
) -> Vec<EditorTableEdgeHandle> {
    let mut handles = Vec::new();
    for block_area in block_areas {
        let Some(block) = model.render_blocks().get(block_area.block_index) else {
            continue;
        };
        let (table_id, header, rows) = match block {
            EditorRenderBlock::Table { header, rows, .. } => (None, header, rows),
            EditorRenderBlock::RichTable {
                table_id,
                header,
                rows,
                ..
            } => (Some(*table_id), header, rows),
            _ => continue,
        };
        let source_range = model
            .block_sources
            .get(block_area.block_index)
            .map(|mapping| mapping.source_range);
        if table_id.is_none() && source_range.is_none() {
            continue;
        }
        let widths = table_column_widths(
            header,
            rows,
            model.table_widths_for_block(block_area.block_index, table_id),
        );
        if widths.is_empty() {
            continue;
        }

        if horizontal_scroll == 0 && canvas.width > 0 {
            handles.push(EditorTableEdgeHandle {
                table_id,
                block_index: block_area.block_index,
                edge: EditorTableEdge::Left,
                source_range,
                area: Rect::new(canvas.x, block_area.area.y, 1, block_area.area.height),
            });
        }

        let right_column = widths
            .iter()
            .fold(0usize, |total, width| total.saturating_add(width + 3));
        if let Some(visible_column) = right_column.checked_sub(horizontal_scroll)
            && visible_column < usize::from(canvas.width)
        {
            handles.push(EditorTableEdgeHandle {
                table_id,
                block_index: block_area.block_index,
                edge: EditorTableEdge::Right,
                source_range,
                area: Rect::new(
                    canvas.x.saturating_add(to_u16(visible_column)),
                    block_area.area.y,
                    1,
                    block_area.area.height,
                ),
            });
        }
    }
    handles
}

fn wrap_runs(
    prefix: Vec<DisplayRun>,
    spans: &[EditorRenderSpan],
    width: usize,
    block_index: usize,
    no_wrap: bool,
    source: Option<&str>,
) -> Vec<DisplayLine> {
    let rich_anchor = spans
        .iter()
        .find_map(|span| span.rich_range.map(|range| range.start));
    let mut prefix = prefix;
    for run in &mut prefix {
        if matches!(run.rich, DisplayRich::Unmapped)
            && let Some(position) = rich_anchor
        {
            run.rich = DisplayRich::Virtual(position);
        }
    }
    let prefix_width = runs_width(&prefix);
    let continuation_anchor = prefix.iter().find_map(display_run_anchor);
    let continuation_prefix = if prefix_width == 0 {
        Vec::new()
    } else {
        vec![
            DisplayRun::virtual_text(
                " ".repeat(prefix_width),
                EditorRenderSpan::plain(""),
                continuation_anchor,
            )
            .with_virtual_rich(rich_anchor),
        ]
    };
    let mut lines = Vec::new();
    let mut current = prefix.clone();
    let mut current_width = prefix_width;
    for span in spans {
        let span_range = span.source_range;
        for run in mapped_text_runs(&span.text, span.clone(), span_range, source) {
            if is_display_newline(run.text.resolve(source)) {
                let before = display_run_start(&run);
                let rich_before = display_run_rich_start(&run);
                if before.is_some() || rich_before.is_some() {
                    current.push(
                        DisplayRun::virtual_text("", EditorRenderSpan::plain(""), before)
                            .with_editable_rich_boundary(rich_before),
                    );
                }
                lines.push(DisplayLine {
                    runs: std::mem::take(&mut current),
                    block_index: Some(block_index),
                    no_wrap,
                    column_start: 0,
                });
                current = continuation_prefix.clone();
                let after = display_run_end(&run);
                let rich_after = display_run_rich_end(&run);
                if after.is_some() || rich_after.is_some() {
                    current.push(
                        DisplayRun::virtual_text("", EditorRenderSpan::plain(""), after)
                            .with_editable_rich_boundary(rich_after),
                    );
                }
                current_width = prefix_width;
                continue;
            }
            let run_width = display_run_width(&run);
            if current_width > prefix_width && current_width.saturating_add(run_width) > width {
                lines.push(DisplayLine {
                    runs: std::mem::take(&mut current),
                    block_index: Some(block_index),
                    no_wrap,
                    column_start: 0,
                });
                current = continuation_prefix.clone();
                current_width = prefix_width;
            }
            current_width = current_width.saturating_add(run_width);
            current.push(run);
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(DisplayLine {
            runs: current,
            block_index: Some(block_index),
            no_wrap,
            column_start: 0,
        });
    }
    lines
}

fn menu_layout(area: Rect) -> (Vec<EditorMenuLayout>, Vec<EditorModeLayout>) {
    if area.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mode_specs = [(EditorMode::Rich, 6u16), (EditorMode::Source, 8u16)];
    let mode_width = mode_specs.iter().map(|(_, width)| *width).sum::<u16>();
    let mode_start = area.right().saturating_sub(mode_width.min(area.width));
    let mut modes = Vec::new();
    let mut x = mode_start;
    for (mode, width) in mode_specs {
        let width = width.min(area.right().saturating_sub(x));
        if width == 0 {
            break;
        }
        modes.push(EditorModeLayout {
            mode,
            area: Rect::new(x, area.y, width, 1),
        });
        x = x.saturating_add(width);
    }
    let menu_specs = [
        (EditorMenu::File, 6u16),
        (EditorMenu::Edit, 6u16),
        (EditorMenu::Insert, 8u16),
        (EditorMenu::Format, 8u16),
        (EditorMenu::View, 6u16),
        (EditorMenu::Settings, 10u16),
    ];
    let mut menus = Vec::new();
    x = area.x;
    for (menu, width) in menu_specs {
        if x.saturating_add(width) > mode_start {
            break;
        }
        menus.push(EditorMenuLayout {
            menu,
            area: Rect::new(x, area.y, width, 1),
        });
        x = x.saturating_add(width);
    }
    (menus, modes)
}

fn menu_popup_layout(
    area: Rect,
    menus: &[EditorMenuLayout],
    model: &EditorViewModel,
) -> (Option<Rect>, Vec<EditorMenuItemLayout>) {
    let Some(menu) = model.open_menu else {
        return (None, Vec::new());
    };
    let Some(anchor) = menus.iter().find(|item| item.menu == menu) else {
        return (None, Vec::new());
    };
    let actions = menu_actions(menu);
    let desired_width = actions
        .iter()
        .map(|action| menu_action_label(*action).chars().count())
        .max()
        .unwrap_or_default()
        .saturating_add(4);
    let width = to_u16(desired_width).min(area.width);
    let y = anchor.area.bottom();
    let available_height = area.bottom().saturating_sub(y);
    let height = to_u16(actions.len().saturating_add(2)).min(available_height);
    if width < 3 || height < 3 {
        return (None, Vec::new());
    }
    let x = anchor.area.x.min(area.right().saturating_sub(width));
    let popup = Rect::new(x, y, width, height);
    let visible_items = usize::from(height.saturating_sub(2)).min(actions.len());
    let items = actions
        .into_iter()
        .take(visible_items)
        .enumerate()
        .map(|(index, action)| EditorMenuItemLayout {
            action,
            area: Rect::new(
                popup.x.saturating_add(1),
                popup.y.saturating_add(1).saturating_add(to_u16(index)),
                popup.width.saturating_sub(2),
                1,
            ),
            enabled: match action {
                EditorMenuAction::Toolbar(action) => {
                    model
                        .toolbar
                        .is_enabled(action, model.read_only, model.mode)
                }
                EditorMenuAction::Mode(_) => true,
            },
        })
        .collect();
    (Some(popup), items)
}

fn settings_layout(
    area: Rect,
    _settings: &EditorSettingsViewModel,
) -> Option<EditorSettingsLayout> {
    let width = area.width.min(64);
    let height = area.height.min(15);
    if width < 52 || height < 15 {
        return None;
    }
    let dialog = Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    );
    let row_x = dialog.x.saturating_add(2);
    let row_width = dialog.width.saturating_sub(4);
    let setting_fields = [
        EditorSettingsField::Enabled,
        EditorSettingsField::ActivationDelay,
        EditorSettingsField::RampDuration,
        EditorSettingsField::HorizontalMaxStep,
        EditorSettingsField::VerticalMaxStep,
    ];
    let mut fields = setting_fields
        .into_iter()
        .enumerate()
        .map(|(index, field)| EditorSettingsFieldLayout {
            field,
            area: Rect::new(
                row_x,
                dialog.y.saturating_add(3).saturating_add(to_u16(index)),
                row_width,
                1,
            ),
        })
        .collect::<Vec<_>>();
    let button_y = dialog.bottom().saturating_sub(2);
    let restore = Rect::new(row_x, button_y, 20, 1);
    let cancel = Rect::new(dialog.right().saturating_sub(11), button_y, 10, 1);
    let save = Rect::new(cancel.x.saturating_sub(10), button_y, 9, 1);
    fields.extend([
        EditorSettingsFieldLayout {
            field: EditorSettingsField::RestoreDefaults,
            area: restore,
        },
        EditorSettingsFieldLayout {
            field: EditorSettingsField::Save,
            area: save,
        },
        EditorSettingsFieldLayout {
            field: EditorSettingsField::Cancel,
            area: cancel,
        },
    ]);

    let mut controls = vec![EditorSettingsControlLayout {
        control: EditorSettingsControl::ToggleEnabled,
        area: Rect::new(
            dialog.right().saturating_sub(9),
            dialog.y.saturating_add(3),
            6,
            1,
        ),
    }];
    for (index, field) in [
        EditorSettingsField::ActivationDelay,
        EditorSettingsField::RampDuration,
        EditorSettingsField::HorizontalMaxStep,
        EditorSettingsField::VerticalMaxStep,
    ]
    .into_iter()
    .enumerate()
    {
        let y = dialog.y.saturating_add(4).saturating_add(to_u16(index));
        controls.extend([
            EditorSettingsControlLayout {
                control: EditorSettingsControl::Decrease(field),
                area: Rect::new(dialog.right().saturating_sub(22), y, 3, 1),
            },
            EditorSettingsControlLayout {
                control: EditorSettingsControl::Increase(field),
                area: Rect::new(dialog.right().saturating_sub(6), y, 3, 1),
            },
        ]);
    }
    controls.extend([
        EditorSettingsControlLayout {
            control: EditorSettingsControl::RestoreDefaults,
            area: restore,
        },
        EditorSettingsControlLayout {
            control: EditorSettingsControl::Save,
            area: save,
        },
        EditorSettingsControlLayout {
            control: EditorSettingsControl::Cancel,
            area: cancel,
        },
    ]);
    Some(EditorSettingsLayout {
        dialog,
        fields,
        controls,
    })
}

fn quick_menu_layout(
    area: Rect,
    menu: Option<EditorQuickMenuViewModel>,
) -> (Option<Rect>, Vec<EditorQuickMenuItemLayout>) {
    let Some(menu) = menu else {
        return (None, Vec::new());
    };
    if area.is_empty() || !contains(area, menu.anchor.0, menu.anchor.1) {
        return (None, Vec::new());
    }

    let actions = [
        EditorQuickAction::Bold,
        EditorQuickAction::Italic,
        EditorQuickAction::Paragraph,
        EditorQuickAction::Heading(1),
        EditorQuickAction::Heading(2),
        EditorQuickAction::Heading(3),
    ];
    let item_widths =
        actions.map(|action| to_u16(quick_action_label(action).chars().count().saturating_add(2)));
    let available_inner_width = area.width.saturating_sub(2);
    let minimum_inner_width = item_widths.iter().copied().max().unwrap_or_default();
    if available_inner_width < minimum_inner_width {
        return (None, Vec::new());
    }
    let desired_inner_width = item_widths.iter().copied().fold(0u16, u16::saturating_add);
    let inner_width = desired_inner_width.min(available_inner_width);

    let mut placements = Vec::with_capacity(actions.len());
    let mut row = 0u16;
    let mut column = 0u16;
    for (action, width) in actions.into_iter().zip(item_widths) {
        if column > 0 && column.saturating_add(width) > inner_width {
            row = row.saturating_add(1);
            column = 0;
        }
        placements.push((action, column, row, width));
        column = column.saturating_add(width);
    }

    let width = inner_width.saturating_add(2);
    let height = row.saturating_add(3);
    if width > area.width || height > area.height {
        return (None, Vec::new());
    }
    let below_y = menu.anchor.1.saturating_add(1);
    let y = if below_y <= area.bottom().saturating_sub(height) {
        below_y
    } else if menu.anchor.1 >= area.y.saturating_add(height) {
        menu.anchor.1.saturating_sub(height)
    } else {
        return (None, Vec::new());
    };
    let x = menu
        .anchor
        .0
        .max(area.x)
        .min(area.right().saturating_sub(width));
    let popup = Rect::new(x, y, width, height);
    let items = placements
        .into_iter()
        .map(|(action, column, row, width)| EditorQuickMenuItemLayout {
            action,
            area: Rect::new(
                popup.x.saturating_add(1).saturating_add(column),
                popup.y.saturating_add(1).saturating_add(row),
                width,
                1,
            ),
            enabled: match action {
                EditorQuickAction::Bold | EditorQuickAction::Italic => true,
                EditorQuickAction::Paragraph | EditorQuickAction::Heading(_) => {
                    menu.block_actions_enabled
                }
            },
        })
        .collect();
    (Some(popup), items)
}

fn menu_actions(menu: EditorMenu) -> Vec<EditorMenuAction> {
    use EditorMenuAction::{Mode, Toolbar};
    use EditorToolbarAction as ToolbarAction;
    match menu {
        EditorMenu::File => vec![
            Toolbar(ToolbarAction::New),
            Toolbar(ToolbarAction::Open),
            Toolbar(ToolbarAction::Save),
        ],
        EditorMenu::Edit => vec![
            Toolbar(ToolbarAction::Undo),
            Toolbar(ToolbarAction::Redo),
            Toolbar(ToolbarAction::Find),
        ],
        EditorMenu::Insert => vec![
            Toolbar(ToolbarAction::Link),
            Toolbar(ToolbarAction::Image),
            Toolbar(ToolbarAction::Table),
        ],
        EditorMenu::Format => vec![
            Toolbar(ToolbarAction::ParagraphStyle),
            Toolbar(ToolbarAction::Bold),
            Toolbar(ToolbarAction::Italic),
            Toolbar(ToolbarAction::Strikethrough),
            Toolbar(ToolbarAction::InlineCode),
            Toolbar(ToolbarAction::BulletList),
            Toolbar(ToolbarAction::OrderedList),
            Toolbar(ToolbarAction::Quote),
        ],
        EditorMenu::View => vec![Mode(EditorMode::Rich), Mode(EditorMode::Source)],
        EditorMenu::Settings => Vec::new(),
    }
}

fn quick_action_label(action: EditorQuickAction) -> &'static str {
    match action {
        EditorQuickAction::Bold => "B",
        EditorQuickAction::Italic => "I",
        EditorQuickAction::Paragraph => "Normal",
        EditorQuickAction::Heading(1) => "H1",
        EditorQuickAction::Heading(2) => "H2",
        EditorQuickAction::Heading(3) => "H3",
        EditorQuickAction::Heading(_) => "H",
    }
}

fn toolbar_layout(area: Rect, model: &EditorViewModel) -> (Vec<EditorToolbarItemLayout>, bool) {
    if area.is_empty() {
        return (Vec::new(), false);
    }
    let specs = toolbar_specs();
    let total_width = specs.iter().map(|(_, _, width)| *width).sum::<u16>();
    let overflow = total_width > area.width;
    let more_width = toolbar_spec(EditorToolbarAction::More).2;
    let available = if overflow {
        area.width.saturating_sub(more_width)
    } else {
        area.width
    };
    let mut items = Vec::new();
    let mut x = area.x;
    for (action, _, width) in specs {
        if action == EditorToolbarAction::More {
            continue;
        }
        if x.saturating_add(width) > area.x.saturating_add(available) {
            break;
        }
        items.push(EditorToolbarItemLayout {
            action,
            area: Rect::new(x, area.y, width, 1),
            enabled: model
                .toolbar
                .is_enabled(action, model.read_only, model.mode),
        });
        x = x.saturating_add(width);
    }
    if overflow && more_width <= area.width {
        items.push(EditorToolbarItemLayout {
            action: EditorToolbarAction::More,
            area: Rect::new(
                area.right().saturating_sub(more_width),
                area.y,
                more_width,
                1,
            ),
            enabled: true,
        });
    }
    (items, overflow)
}

fn toolbar_specs() -> Vec<(EditorToolbarAction, &'static str, u16)> {
    vec![
        toolbar_spec(EditorToolbarAction::New),
        toolbar_spec(EditorToolbarAction::Open),
        toolbar_spec(EditorToolbarAction::Save),
        toolbar_spec(EditorToolbarAction::Undo),
        toolbar_spec(EditorToolbarAction::Redo),
        toolbar_spec(EditorToolbarAction::ParagraphStyle),
        toolbar_spec(EditorToolbarAction::Bold),
        toolbar_spec(EditorToolbarAction::Italic),
        toolbar_spec(EditorToolbarAction::Strikethrough),
        toolbar_spec(EditorToolbarAction::InlineCode),
        toolbar_spec(EditorToolbarAction::BulletList),
        toolbar_spec(EditorToolbarAction::OrderedList),
        toolbar_spec(EditorToolbarAction::Quote),
        toolbar_spec(EditorToolbarAction::Link),
        toolbar_spec(EditorToolbarAction::Image),
        toolbar_spec(EditorToolbarAction::Table),
        toolbar_spec(EditorToolbarAction::Find),
        toolbar_spec(EditorToolbarAction::More),
    ]
}

fn toolbar_spec(action: EditorToolbarAction) -> (EditorToolbarAction, &'static str, u16) {
    let label = toolbar_label(action);
    (action, label, to_u16(label.chars().count()))
}

fn toolbar_label(action: EditorToolbarAction) -> &'static str {
    match action {
        EditorToolbarAction::New => " New ",
        EditorToolbarAction::Open => " Open ",
        EditorToolbarAction::Save => " Save ",
        EditorToolbarAction::Undo => " Undo ",
        EditorToolbarAction::Redo => " Redo ",
        EditorToolbarAction::ParagraphStyle => " Normal ",
        EditorToolbarAction::Bold => " B ",
        EditorToolbarAction::Italic => " I ",
        EditorToolbarAction::Strikethrough => " S ",
        EditorToolbarAction::InlineCode => " Code ",
        EditorToolbarAction::BulletList => " Bullets ",
        EditorToolbarAction::OrderedList => " Numbered ",
        EditorToolbarAction::Quote => " Quote ",
        EditorToolbarAction::Link => " Link ",
        EditorToolbarAction::Image => " Image ",
        EditorToolbarAction::Table => " Table ",
        EditorToolbarAction::Find => " Find ",
        EditorToolbarAction::More => " More ",
    }
}

fn menu_label(menu: EditorMenu) -> &'static str {
    match menu {
        EditorMenu::File => "File",
        EditorMenu::Edit => "Edit",
        EditorMenu::Insert => "Insert",
        EditorMenu::Format => "Format",
        EditorMenu::View => "View",
        EditorMenu::Settings => "Settings",
    }
}

fn menu_action_label(action: EditorMenuAction) -> &'static str {
    match action {
        EditorMenuAction::Toolbar(EditorToolbarAction::New) => "New",
        EditorMenuAction::Toolbar(EditorToolbarAction::Open) => "Open",
        EditorMenuAction::Toolbar(EditorToolbarAction::Save) => "Save",
        EditorMenuAction::Toolbar(EditorToolbarAction::Undo) => "Undo",
        EditorMenuAction::Toolbar(EditorToolbarAction::Redo) => "Redo",
        EditorMenuAction::Toolbar(EditorToolbarAction::ParagraphStyle) => "Normal text",
        EditorMenuAction::Toolbar(EditorToolbarAction::Bold) => "Bold",
        EditorMenuAction::Toolbar(EditorToolbarAction::Italic) => "Italic",
        EditorMenuAction::Toolbar(EditorToolbarAction::Strikethrough) => "Strikethrough",
        EditorMenuAction::Toolbar(EditorToolbarAction::InlineCode) => "Inline code",
        EditorMenuAction::Toolbar(EditorToolbarAction::BulletList) => "Bulleted list",
        EditorMenuAction::Toolbar(EditorToolbarAction::OrderedList) => "Numbered list",
        EditorMenuAction::Toolbar(EditorToolbarAction::Quote) => "Quote",
        EditorMenuAction::Toolbar(EditorToolbarAction::Link) => "Link",
        EditorMenuAction::Toolbar(EditorToolbarAction::Image) => "Image",
        EditorMenuAction::Toolbar(EditorToolbarAction::Table) => "Table",
        EditorMenuAction::Toolbar(EditorToolbarAction::Find) => "Find",
        EditorMenuAction::Toolbar(EditorToolbarAction::More) => "More",
        EditorMenuAction::Mode(EditorMode::Rich) => "Rich view",
        EditorMenuAction::Mode(EditorMode::Source) => "Source view",
    }
}

fn mode_label(mode: EditorMode) -> &'static str {
    match mode {
        EditorMode::Rich => "Rich",
        EditorMode::Source => "Source",
    }
}

fn scrollbar_layout(
    base_canvas: Rect,
    total: usize,
    start: usize,
    capacity: usize,
) -> EditorScrollbarLayout {
    let track = Rect::new(
        base_canvas.right().saturating_sub(1),
        base_canvas.y,
        1,
        base_canvas.height,
    );
    let track_height = usize::from(track.height);
    let thumb_height = max(1, track_height.saturating_mul(capacity) / total.max(1));
    let max_start = total.saturating_sub(capacity);
    let max_offset = track_height.saturating_sub(thumb_height);
    let offset = if max_start == 0 {
        0
    } else {
        max_offset.saturating_mul(start) / max_start
    };
    EditorScrollbarLayout {
        track,
        thumb: Rect::new(
            track.x,
            track.y.saturating_add(to_u16(offset)),
            1,
            to_u16(thumb_height),
        ),
    }
}

fn horizontal_scrollbar_layout(
    track: Rect,
    content_total: usize,
    start: usize,
    capacity: usize,
) -> EditorScrollbarLayout {
    // A viewport-only Source window has already been clipped at `start` by
    // the caller. Keep that raw offset authoritative even if content metadata
    // is temporarily stale, while still keeping the thumb within its track.
    let total = content_total.max(start.saturating_add(capacity));
    let track_width = usize::from(track.width);
    let thumb_width = max(1, track_width.saturating_mul(capacity) / total.max(1));
    let max_start = total.saturating_sub(capacity);
    let max_offset = track_width.saturating_sub(thumb_width);
    let offset = if max_start == 0 {
        0
    } else {
        max_offset.saturating_mul(start) / max_start
    };
    EditorScrollbarLayout {
        track,
        thumb: Rect::new(
            track.x.saturating_add(to_u16(offset)),
            track.y,
            to_u16(thumb_width),
            1,
        ),
    }
}

fn cell_text(cell: &EditorTableCell) -> String {
    cell.spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>()
}

fn fit_text(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut chars = text.chars();
    let mut fitted = chars.by_ref().take(width).collect::<String>();
    if chars.next().is_some() && width > 1 {
        fitted.pop();
        fitted.push('…');
    }
    fitted
}

fn runs_width(runs: &[DisplayRun]) -> usize {
    runs.iter().map(display_run_width).sum()
}

fn display_run_width(run: &DisplayRun) -> usize {
    Span::raw(terminal_safe_text(run.text.resolve(None))).width()
}

fn display_run_anchor(run: &DisplayRun) -> Option<usize> {
    match run.source {
        DisplaySource::Unmapped => None,
        DisplaySource::Range(range) => Some(range.start),
        DisplaySource::Virtual(offset) => Some(offset),
    }
}

fn display_run_start(run: &DisplayRun) -> Option<usize> {
    display_source_start(run.source)
}

fn display_run_end(run: &DisplayRun) -> Option<usize> {
    display_source_end(run.source)
}

fn display_source_start(mapping: DisplaySource) -> Option<usize> {
    match mapping {
        DisplaySource::Unmapped => None,
        DisplaySource::Range(range) => Some(range.start),
        DisplaySource::Virtual(offset) => Some(offset),
    }
}

fn display_source_end(mapping: DisplaySource) -> Option<usize> {
    match mapping {
        DisplaySource::Unmapped => None,
        DisplaySource::Range(range) => Some(range.end),
        DisplaySource::Virtual(offset) => Some(offset),
    }
}

fn is_display_newline(value: &str) -> bool {
    matches!(value, "\n" | "\r" | "\r\n")
}

fn mapped_text_runs(
    text: &str,
    style: EditorRenderSpan,
    source_range: Option<EditorSourceRange>,
    source: Option<&str>,
) -> Vec<DisplayRun> {
    let exact = source_range.filter(|range| {
        range.start <= range.end
            && source.and_then(|source| source.get(range.start..range.end)) == Some(text)
    });
    let mut runs = Vec::new();
    let mut segment_start = 0usize;
    let mut grapheme_offset = 0usize;
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let newline_len = match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => 2,
            b'\r' | b'\n' => 1,
            _ => {
                index += 1;
                continue;
            }
        };
        append_mapped_grapheme_runs(
            &mut runs,
            &text[segment_start..index],
            segment_start,
            &style,
            exact,
            source_range,
            &mut grapheme_offset,
        );
        push_mapped_text_run(
            &mut runs,
            &text[index..index + newline_len],
            index,
            index + newline_len,
            &style,
            exact,
            source_range,
            grapheme_offset,
            grapheme_offset.saturating_add(1),
        );
        grapheme_offset = grapheme_offset.saturating_add(1);
        index += newline_len;
        segment_start = index;
    }
    append_mapped_grapheme_runs(
        &mut runs,
        &text[segment_start..],
        segment_start,
        &style,
        exact,
        source_range,
        &mut grapheme_offset,
    );
    if runs.is_empty() {
        let rich = empty_rich_mapping(style.rich_range);
        runs.push(DisplayRun {
            text: DisplayText::Owned(String::new()),
            style,
            source: exact
                .map(DisplaySource::Range)
                .or_else(|| source_range.map(|range| DisplaySource::Virtual(range.start)))
                .unwrap_or(DisplaySource::Unmapped),
            rich,
        });
    }
    runs
}

fn append_mapped_grapheme_runs(
    runs: &mut Vec<DisplayRun>,
    text: &str,
    base: usize,
    style: &EditorRenderSpan,
    exact: Option<EditorSourceRange>,
    source_range: Option<EditorSourceRange>,
    grapheme_offset: &mut usize,
) {
    let span = Span::raw(text.to_owned());
    let mut relative = 0usize;
    for grapheme in span.styled_graphemes(Style::default()) {
        let start = base.saturating_add(relative);
        relative = relative.saturating_add(grapheme.symbol.len());
        push_mapped_text_run(
            runs,
            grapheme.symbol,
            start,
            base.saturating_add(relative),
            style,
            exact,
            source_range,
            *grapheme_offset,
            grapheme_offset.saturating_add(1),
        );
        *grapheme_offset = grapheme_offset.saturating_add(1);
    }
}

fn push_mapped_text_run(
    runs: &mut Vec<DisplayRun>,
    text: &str,
    start: usize,
    end: usize,
    style: &EditorRenderSpan,
    exact: Option<EditorSourceRange>,
    source_range: Option<EditorSourceRange>,
    grapheme_start: usize,
    grapheme_end: usize,
) {
    let source = match (exact, source_range) {
        (Some(range), _) => DisplaySource::Range(EditorSourceRange::new(
            range.start.saturating_add(start).min(range.end),
            range.start.saturating_add(end).min(range.end),
        )),
        // Decoded entities, generated labels and other non-exact text must
        // never pretend that every visible grapheme covers the whole source.
        (None, Some(range)) => DisplaySource::Virtual(range.start),
        (None, None) => DisplaySource::Unmapped,
    };
    runs.push(DisplayRun {
        text: DisplayText::Owned(text.to_owned()),
        style: style.clone(),
        source,
        rich: rich_mapping(style.rich_range, grapheme_start, grapheme_end),
    });
}

fn empty_rich_mapping(range: Option<RichRange>) -> DisplayRich {
    match range {
        Some(range) if range.start.container_id == range.end.container_id => {
            DisplayRich::Range(range)
        }
        Some(range) => DisplayRich::Virtual(range.start),
        None => DisplayRich::Unmapped,
    }
}

fn rich_mapping(
    range: Option<RichRange>,
    relative_start: usize,
    relative_end: usize,
) -> DisplayRich {
    let Some(range) = range else {
        return DisplayRich::Unmapped;
    };
    if range.start.container_id != range.end.container_id {
        return DisplayRich::Virtual(range.start);
    }
    let start = range
        .start
        .grapheme_offset
        .saturating_add(relative_start)
        .min(range.end.grapheme_offset);
    let end = range
        .start
        .grapheme_offset
        .saturating_add(relative_end)
        .min(range.end.grapheme_offset);
    DisplayRich::Range(RichRange::between(
        RichPosition::in_node(range.start.container_id, start),
        RichPosition::in_node(range.start.container_id, end),
    ))
}

fn display_source_for_segment(
    mapping: DisplaySource,
    text_len: usize,
    relative_start: usize,
    relative_end: usize,
) -> DisplaySource {
    match mapping {
        DisplaySource::Range(range)
            if range.end.saturating_sub(range.start) == text_len
                && relative_start <= relative_end =>
        {
            DisplaySource::Range(EditorSourceRange::new(
                range.start.saturating_add(relative_start).min(range.end),
                range.start.saturating_add(relative_end).min(range.end),
            ))
        }
        mapping => mapping,
    }
}

fn display_rich_for_grapheme(
    mapping: DisplayRich,
    relative_start: usize,
    relative_end: usize,
) -> DisplayRich {
    match mapping {
        DisplayRich::Range(range)
            if range.start.container_id == range.end.container_id
                && range
                    .end
                    .grapheme_offset
                    .saturating_sub(range.start.grapheme_offset)
                    >= relative_end =>
        {
            DisplayRich::Range(RichRange::between(
                RichPosition::in_node(
                    range.start.container_id,
                    range.start.grapheme_offset.saturating_add(relative_start),
                ),
                RichPosition::in_node(
                    range.start.container_id,
                    range.start.grapheme_offset.saturating_add(relative_end),
                ),
            ))
        }
        mapping => mapping,
    }
}

fn display_line_source_boundaries(
    line: &DisplayLine,
    source: Option<&str>,
    horizontal_start: usize,
    width: usize,
) -> Vec<EditorSourceBoundary> {
    let mut boundaries = Vec::new();
    let mut fallback_boundary = None;
    let mut column = line.column_start;
    let horizontal_end = horizontal_start.saturating_add(width);
    for run in &line.runs {
        let text = run.text.resolve(source);
        if text.is_empty() {
            if let Some(offset) = display_run_start(run) {
                push_source_boundary(
                    &mut boundaries,
                    column,
                    offset,
                    matches!(run.source, DisplaySource::Range(_)),
                );
            }
            if let Some(offset) = display_run_end(run) {
                push_source_boundary(
                    &mut boundaries,
                    column,
                    offset,
                    matches!(run.source, DisplaySource::Range(_)),
                );
            }
            continue;
        }
        let span = Span::raw(text);
        let mut relative_byte = 0usize;
        for grapheme in span.styled_graphemes(Style::default()) {
            let relative_start = relative_byte;
            relative_byte = relative_byte.saturating_add(grapheme.symbol.len());
            let mapping =
                display_source_for_segment(run.source, text.len(), relative_start, relative_byte);
            let safe = terminal_safe_text(grapheme.symbol);
            let next_column = column.saturating_add(Span::raw(safe).width().max(1));
            let intersects = next_column >= horizontal_start && column <= horizontal_end;
            if let Some(offset) = display_source_end(mapping) {
                fallback_boundary = Some(EditorSourceBoundary {
                    column: next_column,
                    byte_offset: offset,
                    editable: matches!(mapping, DisplaySource::Range(_)),
                });
            }
            if intersects && let Some(offset) = display_source_start(mapping) {
                push_source_boundary(
                    &mut boundaries,
                    column,
                    offset,
                    matches!(mapping, DisplaySource::Range(_)),
                );
            }
            column = next_column;
            if intersects && let Some(offset) = display_source_end(mapping) {
                push_source_boundary(
                    &mut boundaries,
                    column,
                    offset,
                    matches!(mapping, DisplaySource::Range(_)),
                );
            }
            if column > horizontal_end {
                break;
            }
        }
        if column > horizontal_end {
            break;
        }
    }
    if boundaries.is_empty()
        && let Some(boundary) = fallback_boundary
    {
        boundaries.push(boundary);
    }
    boundaries
}

fn display_line_rich_boundaries(
    line: &DisplayLine,
    source: Option<&str>,
    horizontal_start: usize,
    width: usize,
) -> Vec<EditorRichBoundary> {
    let mut boundaries = Vec::new();
    let mut fallback_boundary = None;
    let mut column = line.column_start;
    let horizontal_end = horizontal_start.saturating_add(width);
    for run in &line.runs {
        let text = run.text.resolve(source);
        if text.is_empty() {
            if let Some(position) = display_run_rich_start(run) {
                push_rich_boundary(
                    &mut boundaries,
                    column,
                    position,
                    matches!(run.rich, DisplayRich::Range(_)),
                );
            }
            if let Some(position) = display_run_rich_end(run) {
                push_rich_boundary(
                    &mut boundaries,
                    column,
                    position,
                    matches!(run.rich, DisplayRich::Range(_)),
                );
            }
            continue;
        }
        let span = Span::raw(text);
        let mut relative_grapheme = 0usize;
        for grapheme in span.styled_graphemes(Style::default()) {
            let mapping = display_rich_for_grapheme(
                run.rich,
                relative_grapheme,
                relative_grapheme.saturating_add(1),
            );
            relative_grapheme = relative_grapheme.saturating_add(1);
            let safe = terminal_safe_text(grapheme.symbol);
            let next_column = column.saturating_add(Span::raw(safe).width().max(1));
            let intersects = next_column >= horizontal_start && column <= horizontal_end;
            if let Some(position) = display_rich_end(mapping) {
                fallback_boundary = Some(EditorRichBoundary {
                    column: next_column,
                    position,
                    editable: matches!(mapping, DisplayRich::Range(_)),
                });
            }
            if intersects && let Some(position) = display_rich_start(mapping) {
                push_rich_boundary(
                    &mut boundaries,
                    column,
                    position,
                    matches!(mapping, DisplayRich::Range(_)),
                );
            }
            column = next_column;
            if intersects && let Some(position) = display_rich_end(mapping) {
                push_rich_boundary(
                    &mut boundaries,
                    column,
                    position,
                    matches!(mapping, DisplayRich::Range(_)),
                );
            }
            if column > horizontal_end {
                break;
            }
        }
        if column > horizontal_end {
            break;
        }
    }
    if boundaries.is_empty()
        && let Some(boundary) = fallback_boundary
    {
        boundaries.push(boundary);
    }
    boundaries
}

fn display_run_rich_start(run: &DisplayRun) -> Option<RichPosition> {
    display_rich_start(run.rich)
}

fn display_rich_start(mapping: DisplayRich) -> Option<RichPosition> {
    match mapping {
        DisplayRich::Unmapped => None,
        DisplayRich::Range(range) => Some(range.start),
        DisplayRich::Virtual(position) => Some(position),
    }
}

fn display_run_rich_end(run: &DisplayRun) -> Option<RichPosition> {
    display_rich_end(run.rich)
}

fn display_rich_end(mapping: DisplayRich) -> Option<RichPosition> {
    match mapping {
        DisplayRich::Unmapped => None,
        DisplayRich::Range(range) => Some(range.end),
        DisplayRich::Virtual(position) => Some(position),
    }
}

fn push_rich_boundary(
    boundaries: &mut Vec<EditorRichBoundary>,
    column: usize,
    position: RichPosition,
    editable: bool,
) {
    if let Some(last) = boundaries
        .last_mut()
        .filter(|last| last.column == column && last.position == position)
    {
        last.editable |= editable;
        return;
    }
    boundaries.push(EditorRichBoundary {
        column,
        position,
        editable,
    });
}

fn push_source_boundary(
    boundaries: &mut Vec<EditorSourceBoundary>,
    column: usize,
    byte_offset: usize,
    editable: bool,
) {
    if let Some(last) = boundaries
        .last_mut()
        .filter(|last| last.column == column && last.byte_offset == byte_offset)
    {
        last.editable |= editable;
        return;
    }
    boundaries.push(EditorSourceBoundary {
        column,
        byte_offset,
        editable,
    });
}

fn nearest_source_boundary(
    boundaries: &[EditorSourceBoundary],
    column: usize,
) -> Option<EditorSourceBoundary> {
    boundaries.iter().copied().min_by_key(|boundary| {
        (
            boundary.column.abs_diff(column),
            usize::from(!boundary.editable),
            usize::from(boundary.column < column),
        )
    })
}

fn nearest_rich_boundary(
    boundaries: &[EditorRichBoundary],
    column: usize,
) -> Option<EditorRichBoundary> {
    boundaries.iter().copied().min_by_key(|boundary| {
        (
            boundary.column.abs_diff(column),
            usize::from(!boundary.editable),
            usize::from(boundary.column < column),
        )
    })
}

fn nearest_editable_rich_boundary(
    lines: &[EditorRichLineMap],
    visual: EditorTextPosition,
    position: RichPosition,
) -> Option<EditorRichBoundary> {
    lines
        .iter()
        .flat_map(|line| {
            line.boundaries
                .iter()
                .filter(move |boundary| {
                    boundary.editable && boundary.position.container_id == position.container_id
                })
                .map(move |boundary| {
                    (
                        (
                            boundary
                                .position
                                .grapheme_offset
                                .abs_diff(position.grapheme_offset),
                            line.document_line.abs_diff(visual.line),
                            boundary.column.abs_diff(visual.column),
                        ),
                        *boundary,
                    )
                })
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, boundary)| boundary)
}

fn nearest_visual_position(
    lines: &[EditorSourceLineMap],
    byte_offset: usize,
) -> Option<EditorTextPosition> {
    lines
        .iter()
        .flat_map(|line| {
            line.boundaries.iter().map(move |boundary| {
                (
                    (
                        boundary.byte_offset.abs_diff(byte_offset),
                        usize::from(!boundary.editable),
                    ),
                    EditorTextPosition::new(line.document_line, boundary.column),
                )
            })
        })
        .min_by_key(|(distance, position)| (*distance, position.line, position.column))
        .map(|(_, position)| position)
}

fn nearest_visual_position_for_rich(
    lines: &[EditorRichLineMap],
    position: RichPosition,
) -> Option<EditorTextPosition> {
    lines
        .iter()
        .flat_map(|line| {
            line.boundaries
                .iter()
                .filter(move |boundary| boundary.position.container_id == position.container_id)
                .map(move |boundary| {
                    (
                        (
                            boundary
                                .position
                                .grapheme_offset
                                .abs_diff(position.grapheme_offset),
                            usize::from(!boundary.editable),
                        ),
                        EditorTextPosition::new(line.document_line, boundary.column),
                    )
                })
        })
        .min_by_key(|(distance, visual)| (*distance, visual.line, visual.column))
        .map(|(_, visual)| visual)
}

fn code_line_source_ranges(
    lines: &[String],
    block_source: Option<EditorBlockSourceMap>,
    source: Option<&str>,
) -> Vec<Option<EditorSourceRange>> {
    let Some(content) = block_source.and_then(|mapping| mapping.content_range) else {
        return vec![None; lines.len()];
    };
    let Some(source) = source else {
        return vec![None; lines.len()];
    };
    let Some(content_source) = source.get(content.start..content.end) else {
        return vec![None; lines.len()];
    };
    let mut cursor = 0usize;
    let mut ranges = Vec::with_capacity(lines.len());
    for line in lines {
        if !content_source[cursor.min(content_source.len())..].starts_with(line) {
            return vec![None; lines.len()];
        }
        let start = content.start.saturating_add(cursor);
        cursor = cursor.saturating_add(line.len()).min(content_source.len());
        ranges.push(Some(EditorSourceRange::new(
            start,
            content.start.saturating_add(cursor),
        )));
        let remaining = &content_source[cursor..];
        if remaining.starts_with("\r\n") {
            cursor += 2;
        } else if remaining.starts_with(['\r', '\n']) {
            cursor += 1;
        }
    }
    ranges
}

/// Replaces bytes that a terminal could interpret as control traffic with
/// visible, inert Unicode glyphs. This must be applied at the final rendering
/// boundary as documents, file names, and status messages are all untrusted.
///
/// C0 controls use the Unicode Control Pictures block, DEL uses its dedicated
/// symbol, and C1/bidirectional formatting controls use the replacement glyph.
/// Newlines are laid out structurally before this boundary; if one reaches this
/// function it is shown visibly instead of being emitted to the terminal.
pub fn terminal_safe_text(value: &str) -> Cow<'_, str> {
    if !value.chars().any(is_terminal_unsafe_character) {
        return Cow::Borrowed(value);
    }

    Cow::Owned(value.chars().map(terminal_safe_character).collect())
}

fn terminal_safe_character(character: char) -> char {
    match character {
        '\u{0000}'..='\u{001f}' => {
            char::from_u32(0x2400 + u32::from(character)).unwrap_or('\u{fffd}')
        }
        '\u{007f}' => '\u{2421}',
        '\u{0080}'..='\u{009f}' => '\u{fffd}',
        character if is_unsafe_format_character(character) => '\u{fffd}',
        character => character,
    }
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

fn source_document_line_count(model: &EditorViewModel) -> usize {
    if let Some(window) = &model.source_window {
        return window.total_line_count.max(1);
    }
    if let Some(ranges) = cached_source_line_ranges(model) {
        return ranges.len().max(1);
    }
    model
        .source
        .as_deref()
        .map(source_line_count)
        .unwrap_or_else(|| model.source_lines.len().max(1))
}

fn source_display_lines_for_viewport(
    model: &EditorViewModel,
    start: usize,
    end: usize,
) -> Vec<DisplayLine> {
    if let Some(window) = &model.source_window {
        return (start..end)
            .map(|document_line| {
                document_line
                    .checked_sub(window.first_line)
                    .and_then(|relative| window.lines.get(relative))
                    .map_or_else(
                        || empty_display_line(None),
                        |line| DisplayLine {
                            runs: vec![DisplayRun::shared_source(
                                Arc::clone(&line.text),
                                line.visible_byte_range,
                            )],
                            block_index: None,
                            no_wrap: true,
                            column_start: line.start_column,
                        },
                    )
            })
            .collect();
    }
    if let Some(source) = model.source.as_deref() {
        let fallback;
        let ranges = if let Some(ranges) = cached_source_line_ranges(model) {
            ranges
        } else {
            fallback = source_display_line_ranges(source);
            &fallback
        };
        return ranges
            .get(start.min(ranges.len())..end.min(ranges.len()))
            .unwrap_or_default()
            .iter()
            .copied()
            .map(|range| DisplayLine {
                runs: vec![DisplayRun::source_range(range)],
                block_index: None,
                no_wrap: true,
                column_start: 0,
            })
            .collect();
    }

    model
        .source_lines
        .get(start.min(model.source_lines.len())..end.min(model.source_lines.len()))
        .unwrap_or_default()
        .iter()
        .map(|line| DisplayLine {
            runs: vec![DisplayRun::unmapped(
                line.clone(),
                EditorRenderSpan::plain(""),
            )],
            block_index: None,
            no_wrap: true,
            column_start: 0,
        })
        .collect()
}

fn cached_source_line_ranges(model: &EditorViewModel) -> Option<&[EditorSourceRange]> {
    let source = model.source.as_deref()?;
    let ranges = model.source_line_ranges.as_slice();
    let valid = !ranges.is_empty()
        && ranges.first().is_some_and(|range| range.start == 0)
        && ranges.last().is_some_and(|range| range.end == source.len());
    valid.then_some(ranges)
}

fn source_line_count(source: &str) -> usize {
    let bytes = source.as_bytes();
    let mut lines = 1usize;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                lines = lines.saturating_add(1);
                index += 2;
            }
            b'\r' | b'\n' => {
                lines = lines.saturating_add(1);
                index += 1;
            }
            _ => index += 1,
        }
    }
    lines
}

fn source_display_line_ranges(source: &str) -> Vec<EditorSourceRange> {
    let bytes = source.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                lines.push(EditorSourceRange::new(start, index));
                index += 2;
                start = index;
            }
            b'\r' | b'\n' => {
                lines.push(EditorSourceRange::new(start, index));
                index += 1;
                start = index;
            }
            _ => index += 1,
        }
    }
    lines.push(EditorSourceRange::new(start, source.len()));
    lines
}

fn source_horizontal_content_width(source: &str, ranges: &[EditorSourceRange]) -> usize {
    ranges
        .iter()
        .filter_map(|range| source.get(range.start..range.end))
        .map(|line| Span::raw(terminal_safe_text(line)).width())
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .max(1)
}

fn accent_span() -> EditorRenderSpan {
    EditorRenderSpan {
        color: EditorSpanColor::Accent,
        bold: true,
        ..EditorRenderSpan::default()
    }
}

fn muted_span() -> EditorRenderSpan {
    EditorRenderSpan {
        color: EditorSpanColor::Muted,
        ..EditorRenderSpan::default()
    }
}

fn warning_span() -> EditorRenderSpan {
    EditorRenderSpan {
        color: EditorSpanColor::Warning,
        ..EditorRenderSpan::default()
    }
}

fn empty_display_line(block_index: Option<usize>) -> DisplayLine {
    empty_display_line_at(block_index, None)
}

fn empty_display_line_at(block_index: Option<usize>, byte_offset: Option<usize>) -> DisplayLine {
    DisplayLine {
        runs: vec![DisplayRun::virtual_text(
            "",
            EditorRenderSpan::default(),
            byte_offset,
        )],
        block_index,
        no_wrap: false,
        column_start: 0,
    }
}

fn inset(area: Rect, amount: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(amount),
        area.y.saturating_add(amount),
        area.width.saturating_sub(amount.saturating_mul(2)),
        area.height.saturating_sub(amount.saturating_mul(2)),
    )
}

fn contains(area: Rect, x: u16, y: u16) -> bool {
    area.width > 0
        && area.height > 0
        && x >= area.x
        && x < area.right()
        && y >= area.y
        && y < area.bottom()
}

fn to_u16(value: usize) -> u16 {
    min(value, usize::from(u16::MAX)) as u16
}

#[cfg(test)]
mod rich_layout_measurement_tests {
    use super::*;

    fn mapped(text: &str) -> EditorRenderSpan {
        EditorRenderSpan::plain(text).with_rich_range(RichRange::new(
            42,
            0,
            Span::raw(text).styled_graphemes(Style::default()).count(),
        ))
    }

    fn representative_blocks() -> Vec<EditorRenderBlock> {
        vec![
            EditorRenderBlock::Paragraph(Vec::new()),
            EditorRenderBlock::Paragraph(vec![EditorRenderSpan::plain("")]),
            EditorRenderBlock::Paragraph(vec![EditorRenderSpan::plain(
                "plain 好 e\u{301} 🙂 text that wraps",
            )]),
            EditorRenderBlock::Paragraph(vec![mapped("first\r\nsecond\rthird\n")]),
            EditorRenderBlock::Heading {
                level: 2,
                spans: vec![mapped("a heading that wraps")],
            },
            EditorRenderBlock::BulletListItem {
                depth: 2,
                checked: Some(false),
                spans: vec![mapped("task item with a continuation")],
            },
            EditorRenderBlock::OrderedListItem {
                depth: 1,
                number: 123,
                spans: vec![mapped("ordered item")],
            },
            EditorRenderBlock::Quote {
                depth: 2,
                spans: vec![mapped("quoted text\nwith a break")],
            },
            EditorRenderBlock::Footnote {
                label: "note".to_string(),
                spans: vec![mapped("footnote body")],
            },
            EditorRenderBlock::CodeBlock {
                language: Some("rust".to_string()),
                lines: vec!["fn main() {}".to_string(), "// line 2".to_string()],
            },
            EditorRenderBlock::Table {
                header: vec![EditorTableCell::text("A"), EditorTableCell::text("B")],
                rows: vec![vec![
                    EditorTableCell::text("one"),
                    EditorTableCell::text("two"),
                ]],
                alignments: vec![EditorTableAlignment::Left, EditorTableAlignment::Right],
            },
            EditorRenderBlock::Table {
                header: Vec::new(),
                rows: Vec::new(),
                alignments: Vec::new(),
            },
            EditorRenderBlock::HorizontalRule,
            EditorRenderBlock::RawHtml("<div>raw\nhtml</div>".to_string()),
            EditorRenderBlock::Image {
                markdown: "![preview](image.png)".to_string(),
            },
            EditorRenderBlock::Blank,
        ]
    }

    #[test]
    fn bounded_measurement_matches_materialized_block_line_counts() {
        for block in representative_blocks() {
            for width in 1..=24 {
                let expected = block_lines(&block, 0, width, None, None, None).len();
                for limit in 0..=expected.saturating_add(1) {
                    assert_eq!(
                        rich_block_line_count_up_to(&block, width, limit),
                        (expected <= limit).then_some(expected),
                        "block={block:?}, width={width}, limit={limit}"
                    );
                }
            }
        }
    }

    #[test]
    fn rich_scrollbar_layout_flattens_the_document_only_once() {
        let blocks: Arc<[EditorRenderBlock]> = (0..10_000)
            .map(|index| EditorRenderBlock::paragraph(format!("line {index}")))
            .collect::<Vec<_>>()
            .into();
        let mut model = EditorViewModel::new_shared("large.md", blocks);
        model.scroll_line = 5_000;

        RICH_FLATTEN_CALL_COUNT.with(|count| count.set(0));
        let layout = editor_layout(Rect::new(0, 0, 80, 12), &model);
        let flatten_calls = RICH_FLATTEN_CALL_COUNT.with(std::cell::Cell::get);

        assert_eq!(flatten_calls, 1);
        assert_eq!(layout.document_line_count, 10_000);
        assert_eq!(layout.visible_start, 5_000);
        assert!(layout.vertical_scrollbar.is_some());
        assert_eq!(layout.prepared_lines.len(), layout.visible_capacity);
    }

    #[test]
    fn scrollbar_measurement_preserves_the_wide_layout_fixed_point() {
        // At full width this is exactly one line. Reserving a scrollbar first
        // would wrap it and incorrectly make the scrollbar self-fulfilling.
        let fitting =
            EditorViewModel::new("fit.md", vec![EditorRenderBlock::paragraph("x".repeat(20))]);
        let fitting_layout = editor_layout(Rect::new(0, 0, 20, 4), &fitting);
        assert_eq!(fitting_layout.canvas.width, 20);
        assert_eq!(fitting_layout.document_line_count, 1);
        assert!(fitting_layout.vertical_scrollbar.is_none());

        let overflowing = EditorViewModel::new(
            "overflow.md",
            vec![EditorRenderBlock::paragraph("x".repeat(21))],
        );
        let overflowing_layout = editor_layout(Rect::new(0, 0, 20, 4), &overflowing);
        assert_eq!(overflowing_layout.canvas.width, 19);
        assert_eq!(overflowing_layout.document_line_count, 2);
        assert!(overflowing_layout.vertical_scrollbar.is_some());
    }
}

#[cfg(test)]
mod rich_table_identity_tests {
    use super::*;

    fn rich_table(table_id: NodeId) -> EditorRenderBlock {
        EditorRenderBlock::RichTable {
            table_id,
            header: vec![
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("Name").with_rich_range(RichRange::new(101, 0, 4)),
                    ],
                },
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("Value").with_rich_range(RichRange::new(102, 0, 5)),
                    ],
                },
            ],
            rows: vec![vec![
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("Tundra")
                            .with_rich_range(RichRange::new(201, 0, 6)),
                    ],
                },
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("3").with_rich_range(RichRange::new(202, 0, 1)),
                    ],
                },
            ]],
            alignments: vec![EditorTableAlignment::Left, EditorTableAlignment::Right],
        }
    }

    #[test]
    fn rich_table_handles_and_widths_follow_stable_table_id() {
        let table_id = NodeId::new(9001);
        let mut model = EditorViewModel::new(
            "table.md",
            vec![EditorRenderBlock::paragraph("before"), rich_table(table_id)],
        );
        model.rich_table_column_widths.insert(table_id, vec![9, 4]);

        let first_layout = editor_layout(Rect::new(0, 0, 80, 16), &model);
        let first_resize = first_layout
            .table_resize_handles
            .iter()
            .find(|handle| handle.table_id == Some(table_id) && handle.column_index == 0)
            .expect("Rich table resize handle");
        assert_eq!(first_resize.block_index, 1);
        assert_eq!(first_resize.width, 9);
        assert_eq!(
            first_layout.hit_test(first_resize.area.x, first_resize.area.y + 1),
            Some(EditorHitTarget::RichTableResize {
                table_id,
                column_index: 0,
                width: 9,
            })
        );

        let left_edge = first_layout
            .table_edge_handles
            .iter()
            .find(|handle| {
                handle.table_id == Some(table_id) && handle.edge == EditorTableEdge::Left
            })
            .expect("Rich table edge handle");
        assert_eq!(left_edge.source_range, None);
        assert_eq!(
            first_layout.hit_test(left_edge.area.x, left_edge.area.y + 1),
            Some(EditorHitTarget::RichTableEdge {
                table_id,
                edge: EditorTableEdge::Left,
            })
        );

        model
            .blocks
            .insert(0, EditorRenderBlock::paragraph("new leading block"));
        let shifted_layout = editor_layout(Rect::new(0, 0, 80, 18), &model);
        let shifted_resize = shifted_layout
            .table_resize_handles
            .iter()
            .find(|handle| handle.table_id == Some(table_id) && handle.column_index == 0)
            .expect("shifted Rich table resize handle");
        assert_eq!(shifted_resize.block_index, 2);
        assert_eq!(shifted_resize.width, 9);
    }
}

#[cfg(test)]
mod rich_newline_position_tests {
    use super::*;

    #[test]
    fn soft_break_second_line_hit_is_the_boundary_after_the_break() {
        let container = NodeId::new(77);
        let model = EditorViewModel::new(
            "soft-break.md",
            vec![EditorRenderBlock::Paragraph(vec![
                EditorRenderSpan::plain("a ").with_rich_range(RichRange::new(
                    container.get(),
                    0,
                    2,
                )),
                EditorRenderSpan::plain("\n").with_rich_range(RichRange::new(
                    container.get(),
                    2,
                    3,
                )),
            ])],
        );
        let layout = editor_layout(Rect::new(0, 0, 40, 10), &model);
        let after_break = RichPosition::in_node(container, 3);

        let visual = layout
            .visual_position_for_rich(after_break)
            .expect("soft-break boundary has a visual position");
        assert_eq!(visual, EditorTextPosition::new(1, 0));

        let hit = layout
            .hit_test_document(
                layout.canvas.x.saturating_add(to_u16(visual.column)),
                layout
                    .canvas
                    .y
                    .saturating_add(to_u16(visual.line.saturating_sub(layout.visible_start))),
            )
            .expect("second visual line is hittable");
        assert_eq!(hit.position, EditorDocumentPosition::Rich(after_break));
        assert!(hit.editable);
        assert_eq!(
            layout.visual_position_for_document(hit.position),
            Some(visual)
        );
    }

    #[test]
    fn source_newline_mapping_remains_byte_based() {
        let model = EditorViewModel::source("source.md", "a \nb");
        let layout = editor_layout(Rect::new(0, 0, 40, 10), &model);
        let after_newline = 3;
        let visual = layout
            .visual_position_for_source(after_newline)
            .expect("source byte boundary has a visual position");
        assert_eq!(visual, EditorTextPosition::new(1, 0));

        let hit = layout
            .hit_test_document(
                layout.canvas.x.saturating_add(to_u16(visual.column)),
                layout
                    .canvas
                    .y
                    .saturating_add(to_u16(visual.line.saturating_sub(layout.visible_start))),
            )
            .expect("second source line is hittable");
        assert_eq!(hit.position, EditorDocumentPosition::Source(after_newline));
        assert!(hit.editable);
    }
}

#[cfg(test)]
mod shared_rich_blocks_tests {
    use super::*;

    #[test]
    fn shared_projection_is_authoritative_and_reused_by_layout() {
        let blocks: Arc<[EditorRenderBlock]> = vec![
            EditorRenderBlock::paragraph("shared paragraph"),
            EditorRenderBlock::Image {
                markdown: "![shared](preview.png)".to_string(),
            },
        ]
        .into();
        let retained = Arc::clone(&blocks);
        let model = EditorViewModel::new_shared("shared.md", blocks);

        assert!(model.blocks.is_empty());
        assert!(Arc::ptr_eq(
            model.shared_blocks.as_ref().expect("shared projection"),
            &retained
        ));
        assert_eq!(model.render_blocks().len(), 2);
        let layout = editor_layout(Rect::new(0, 0, 80, 14), &model);
        assert_eq!(layout.document_line_count, 2);
        assert_eq!(layout.image_areas.len(), 1);
        assert_eq!(layout.image_areas[0].block_index, 1);
    }
}

#[cfg(test)]
mod source_virtualization_tests {
    use super::*;

    #[test]
    fn coalesced_source_run_preserves_unicode_byte_boundaries() {
        let source = "A好e\u{301}🙂";
        let model = EditorViewModel::source("unicode.log", source);
        let layout = editor_layout(Rect::new(0, 0, 60, 10), &model);

        assert_eq!(layout.prepared_lines.len(), 1);
        assert_eq!(layout.prepared_lines[0].runs.len(), 1);
        assert_eq!(
            layout.prepared_lines[0].runs[0].text,
            DisplayText::SourceRange(EditorSourceRange::new(0, source.len()))
        );
        let boundaries = &layout.source_line_maps[0].boundaries;
        assert!(
            boundaries
                .iter()
                .any(|boundary| { boundary.column == 3 && boundary.byte_offset == "A好".len() })
        );
        assert!(boundaries.iter().any(|boundary| {
            boundary.column == 4 && boundary.byte_offset == "A好e\u{301}".len()
        }));
        assert!(layout.rich_line_maps.is_empty());
    }

    #[test]
    fn long_source_line_prepares_one_run_and_bounded_hit_boundaries() {
        let source = "x".repeat(1_000_000);
        let model = EditorViewModel::source("single-line.log", &source);
        let layout = editor_layout(Rect::new(0, 0, 80, 12), &model);

        assert_eq!(layout.document_line_count, 1);
        assert_eq!(layout.prepared_lines.len(), 1);
        assert_eq!(layout.prepared_lines[0].runs.len(), 1);
        assert!(
            layout.source_line_maps[0].boundaries.len()
                <= usize::from(layout.canvas.width).saturating_add(2)
        );
    }

    #[test]
    fn many_source_lines_prepare_only_the_scrolled_viewport() {
        let source = (0..100_000)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut model = EditorViewModel::source("many-lines.log", source);
        model.scroll_line = 50_000;
        let layout = editor_layout(Rect::new(0, 0, 80, 12), &model);

        assert_eq!(layout.document_line_count, 100_000);
        assert_eq!(layout.visible_start, 50_000);
        assert_eq!(layout.prepared_lines.len(), layout.visible_capacity);
        assert!(layout.prepared_lines.len() < 100_000);
        assert_eq!(layout.source_line_maps.len(), layout.visible_capacity);
        assert!(
            layout
                .prepared_lines
                .iter()
                .all(|line| line.runs.len() == 1)
        );
        assert_eq!(
            layout
                .source_line_maps
                .first()
                .map(|line| line.document_line),
            Some(50_000)
        );
        assert!(layout.rich_line_maps.is_empty());
    }

    #[test]
    fn viewport_only_source_keeps_global_lines_columns_and_byte_offsets() {
        let first_line = 50_000;
        let first_byte = 900_000;
        let lines = (0..32)
            .map(|relative| {
                let text = if relative == 0 { "好x" } else { "row" };
                let start = first_byte + relative * 16;
                EditorSourceWindowLine::new(
                    EditorSourceRange::new(start, start + text.len()),
                    200,
                    text,
                )
            })
            .collect();
        let mut model = EditorViewModel::source_viewport("window.log", first_line, 100_000, lines);
        model.horizontal_scroll = 200;
        model.horizontal_content_width = 400;
        let layout = editor_layout(Rect::new(0, 0, 80, 12), &model);

        assert_eq!(layout.document_line_count, 100_000);
        assert_eq!(layout.visible_start, first_line);
        assert_eq!(layout.prepared_lines.len(), layout.visible_capacity);
        assert_eq!(layout.prepared_lines[0].column_start, 200);
        assert!(matches!(
            layout.prepared_lines[0].runs[0].text,
            DisplayText::Shared(_)
        ));
        let hit = layout
            .hit_test_source(layout.canvas.x + 2, layout.canvas.y)
            .expect("window line remains globally addressable");
        assert_eq!(hit.visual, EditorTextPosition::new(first_line, 202));
        assert_eq!(hit.byte_offset, first_byte + "好".len());
    }
}
