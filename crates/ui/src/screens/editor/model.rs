use super::source::{source_display_line_ranges, source_horizontal_content_width};
use super::*;

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
    pub editable: bool,
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

    pub(super) const fn anchor(self) -> usize {
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
