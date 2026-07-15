use std::borrow::Cow;
use std::cmp::{max, min};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::TundraTheme;

/// The two representations exposed by the editor. Both edit the same document.
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
    pub source_range: Option<EditorSourceRange>,
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

    pub fn is_enabled(&self, action: EditorToolbarAction, read_only: bool) -> bool {
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
            | EditorToolbarAction::ParagraphStyle => !read_only,
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
    pub mode: EditorMode,
    pub focus: EditorFocus,
    pub open_menu: Option<EditorMenu>,
    pub selected_toolbar_action: Option<EditorToolbarAction>,
    pub blocks: Vec<EditorRenderBlock>,
    pub source_lines: Vec<String>,
    pub scroll_line: usize,
    pub horizontal_scroll: usize,
    pub cursor: Option<EditorTextPosition>,
    pub selection: Option<EditorSelection>,
    /// Canonical source used only for visual-to-byte mapping. `None` preserves
    /// the legacy visual line/column contract.
    pub source: Option<String>,
    /// Parallel to `blocks`. Missing entries simply disable source mapping for
    /// the corresponding block.
    pub block_sources: Vec<EditorBlockSourceMap>,
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
            mode: EditorMode::Rich,
            focus: EditorFocus::Canvas,
            open_menu: None,
            selected_toolbar_action: None,
            blocks,
            source_lines: vec![String::new()],
            scroll_line: 0,
            horizontal_scroll: 0,
            cursor: Some(EditorTextPosition::default()),
            selection: None,
            source: None,
            block_sources: Vec::new(),
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
        model.source_lines = source_display_lines(source);
        if model.source_lines.is_empty() {
            model.source_lines.push(String::new());
        }
        model.word_count = source.split_whitespace().count();
        model
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorMenuLayout {
    pub menu: EditorMenu,
    pub area: Rect,
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
pub enum EditorHitTarget {
    Menu(EditorMenu),
    Toolbar(EditorToolbarAction),
    Mode(EditorMode),
    Canvas(EditorTextPosition),
    VerticalScrollbar,
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
    pub area: Rect,
    pub menu_bar: Rect,
    pub toolbar: Rect,
    pub canvas_panel: Rect,
    /// Text cells only; borders and the optional scrollbar are excluded.
    pub canvas: Rect,
    pub status_bar: Rect,
    pub menus: Vec<EditorMenuLayout>,
    pub toolbar_items: Vec<EditorToolbarItemLayout>,
    pub modes: Vec<EditorModeLayout>,
    pub line_areas: Vec<EditorLineLayout>,
    pub block_areas: Vec<EditorBlockArea>,
    /// Subset of `block_areas` that can receive a ratatui-image overlay.
    pub image_areas: Vec<EditorBlockArea>,
    pub vertical_scrollbar: Option<EditorScrollbarLayout>,
    pub visible_start: usize,
    pub visible_capacity: usize,
    pub document_line_count: usize,
    pub horizontal_scroll: usize,
    pub toolbar_overflow: bool,
    pub canvas_framed: bool,
    pub source_line_maps: Vec<EditorSourceLineMap>,
}

impl EditorLayout {
    pub fn hit_test(&self, x: u16, y: u16) -> Option<EditorHitTarget> {
        if let Some(item) = self.modes.iter().find(|item| contains(item.area, x, y)) {
            return Some(EditorHitTarget::Mode(item.mode));
        }
        if let Some(item) = self.menus.iter().find(|item| contains(item.area, x, y)) {
            return Some(EditorHitTarget::Menu(item.menu));
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
    let mut display_lines = flatten_document(model, usize::from(base_canvas.width.max(1)));
    let mut canvas = base_canvas;
    if display_lines.len() > usize::from(base_canvas.height) && base_canvas.width > 1 {
        canvas.width = canvas.width.saturating_sub(1);
        display_lines = flatten_document(model, usize::from(canvas.width.max(1)));
    }
    let document_line_count = display_lines.len().max(1);
    let visible_capacity = usize::from(canvas.height);
    let max_start = document_line_count.saturating_sub(visible_capacity);
    let visible_start = model.scroll_line.min(max_start);
    let visible_end = visible_start
        .saturating_add(visible_capacity)
        .min(document_line_count);
    let line_areas = (visible_start..visible_end)
        .map(|document_line| EditorLineLayout {
            document_line,
            block_index: display_lines
                .get(document_line)
                .and_then(|line| line.block_index),
            horizontally_scrollable: display_lines
                .get(document_line)
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
                model.blocks.get(entry.block_index),
                Some(EditorRenderBlock::Image { .. })
            )
        })
        .collect();

    let vertical_scrollbar = (document_line_count > visible_capacity && base_canvas.width > 1)
        .then(|| {
            scrollbar_layout(
                base_canvas,
                document_line_count,
                visible_start,
                visible_capacity,
            )
        });
    let (menus, modes) = menu_layout(menu_bar);
    let (toolbar_items, toolbar_overflow) = toolbar_layout(toolbar, model);
    let source_line_maps = display_lines
        .iter()
        .enumerate()
        .filter_map(|(document_line, line)| {
            let boundaries = display_line_source_boundaries(line);
            (!boundaries.is_empty()).then_some(EditorSourceLineMap {
                document_line,
                boundaries,
            })
        })
        .collect();

    EditorLayout {
        area,
        menu_bar,
        toolbar,
        canvas_panel,
        canvas,
        status_bar,
        menus,
        toolbar_items,
        modes,
        line_areas,
        block_areas,
        image_areas,
        vertical_scrollbar,
        visible_start,
        visible_capacity,
        document_line_count,
        horizontal_scroll: model.horizontal_scroll,
        toolbar_overflow,
        canvas_framed,
        source_line_maps,
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
    layout
}

#[derive(Debug, Clone)]
struct DisplayRun {
    text: String,
    style: EditorRenderSpan,
    source: DisplaySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplaySource {
    Unmapped,
    Range(EditorSourceRange),
    Virtual(usize),
}

impl DisplayRun {
    fn unmapped(text: impl Into<String>, style: EditorRenderSpan) -> Self {
        Self {
            text: text.into(),
            style,
            source: DisplaySource::Unmapped,
        }
    }

    fn virtual_text(
        text: impl Into<String>,
        style: EditorRenderSpan,
        byte_offset: Option<usize>,
    ) -> Self {
        Self {
            text: text.into(),
            style,
            source: byte_offset
                .map(DisplaySource::Virtual)
                .unwrap_or(DisplaySource::Unmapped),
        }
    }
}

#[derive(Debug, Clone)]
struct DisplayLine {
    runs: Vec<DisplayRun>,
    block_index: Option<usize>,
    no_wrap: bool,
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
        let active = model.open_menu == Some(item.menu);
        let style = if active {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else if model.focus == EditorFocus::MenuBar {
            Style::default().fg(theme.accent).bg(Color::DarkGray)
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
                .bg(theme.accent)
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
                .bg(theme.accent)
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

    let display_lines = flatten_document(model, usize::from(layout.canvas.width.max(1)));
    for line_layout in &layout.line_areas {
        let Some(display_line) = display_lines.get(line_layout.document_line) else {
            continue;
        };
        let line = styled_line(
            display_line,
            line_layout.document_line,
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
                Paragraph::new("│").style(theme.muted_style()),
                Rect::new(scrollbar.track.x, y, 1, 1),
            );
        }
        for y in scrollbar.thumb.y..scrollbar.thumb.bottom() {
            frame.render_widget(
                Paragraph::new("█").style(Style::default().fg(theme.accent)),
                Rect::new(scrollbar.thumb.x, y, 1, 1),
            );
        }
    }

    if model.focus == EditorFocus::Canvas
        && let Some(cursor) = effective_cursor(layout, model)
        && cursor.line >= layout.visible_start
        && cursor.line < layout.visible_start.saturating_add(layout.visible_capacity)
    {
        let horizontal_scroll = display_lines
            .get(cursor.line)
            .filter(|line| model.mode == EditorMode::Source || line.no_wrap)
            .map_or(0, |_| model.horizontal_scroll);
        let cursor_column = cursor.column.saturating_sub(horizontal_scroll);
        if cursor_column < usize::from(layout.canvas.width) {
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
    let left = model
        .status_message
        .as_deref()
        .unwrap_or(if model.read_only {
            "Read only"
        } else {
            "Ready"
        });
    let right = format!(
        "{}  Ln {}, Col {}  {} words  {}/{}  {}",
        mode,
        cursor.line.saturating_add(1),
        cursor.column.saturating_add(1),
        model.word_count,
        model.encoding,
        model.line_ending,
        image
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
        Style::default().fg(theme.background).bg(theme.accent)
    } else {
        Style::default().fg(theme.foreground).bg(Color::DarkGray)
    };
    frame.render_widget(Paragraph::new(text).style(style), layout.status_bar);
}

fn styled_line(
    line: &DisplayLine,
    document_line: usize,
    model: &EditorViewModel,
    theme: &TundraTheme,
    width: usize,
) -> Line<'static> {
    let scroll = if model.mode == EditorMode::Source || line.no_wrap {
        model.horizontal_scroll
    } else {
        0
    };
    let mut output = Vec::new();
    let mut column = 0usize;
    let mut visible_width = 0usize;
    for run in &line.runs {
        let base_style = span_style(&run.style, theme);
        let run_span = Span::raw(run.text.clone());
        for grapheme in run_span.styled_graphemes(Style::default()) {
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
            let selected = model.selection_offsets.map_or_else(
                || {
                    model
                        .selection
                        .is_some_and(|selection| selection.contains(position))
                },
                |selection| source_run_is_selected(run, selection),
            );
            let style = if selected {
                base_style
                    .fg(theme.background)
                    .bg(theme.accent)
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
    model
        .cursor_offset
        .and_then(|offset| layout.visual_position_for_source(offset))
        .or(model.cursor)
}

fn source_run_is_selected(run: &DisplayRun, selection: EditorSourceSelection) -> bool {
    let selected = selection.normalized();
    if selected.is_empty() {
        return false;
    }
    match run.source {
        DisplaySource::Range(range) => range.start < selected.end && selected.start < range.end,
        DisplaySource::Unmapped | DisplaySource::Virtual(_) => false,
    }
}

fn span_style(span: &EditorRenderSpan, theme: &TundraTheme) -> Style {
    let foreground = match span.color {
        EditorSpanColor::Normal => theme.foreground,
        EditorSpanColor::Accent => theme.accent,
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

fn flatten_document(model: &EditorViewModel, width: usize) -> Vec<DisplayLine> {
    let width = width.max(1);
    if model.mode == EditorMode::Source {
        let mut lines = if let Some(source) = model.source.as_deref() {
            source_display_line_ranges(source)
                .into_iter()
                .map(|range| DisplayLine {
                    runs: mapped_text_runs(
                        source.get(range.start..range.end).unwrap_or_default(),
                        EditorRenderSpan::plain(""),
                        Some(range),
                        Some(source),
                    ),
                    block_index: None,
                    no_wrap: true,
                })
                .collect::<Vec<_>>()
        } else {
            model
                .source_lines
                .iter()
                .map(|line| DisplayLine {
                    runs: vec![DisplayRun::unmapped(
                        line.clone(),
                        EditorRenderSpan::plain(""),
                    )],
                    block_index: None,
                    no_wrap: true,
                })
                .collect::<Vec<_>>()
        };
        if lines.is_empty() {
            lines.push(empty_display_line(None));
        }
        return lines;
    }

    let mut output = Vec::new();
    for (block_index, block) in model.blocks.iter().enumerate() {
        let lines = block_lines(
            block,
            block_index,
            width,
            model.block_sources.get(block_index).copied(),
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
    source: Option<&str>,
) -> Vec<DisplayLine> {
    let anchor = block_source.map(EditorBlockSourceMap::anchor);
    match block {
        EditorRenderBlock::Paragraph(spans) => {
            wrap_runs(Vec::new(), spans.clone(), width, block_index, false, source)
        }
        EditorRenderBlock::Heading { level, spans } => {
            let level = (*level).clamp(1, 6);
            let styled = spans
                .iter()
                .cloned()
                .map(|mut span| {
                    span.bold = true;
                    span.color = if level <= 2 {
                        EditorSpanColor::Accent
                    } else {
                        EditorSpanColor::Normal
                    };
                    span.underlined |= level == 1;
                    span
                })
                .collect();
            wrap_runs(Vec::new(), styled, width, block_index, false, source)
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
                spans.clone(),
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
                spans.clone(),
                width,
                block_index,
                false,
                source,
            )
        }
        EditorRenderBlock::Quote { depth, spans } => {
            let prefix = "│ ".repeat(usize::from((*depth).max(1)));
            let quote_spans = spans
                .iter()
                .cloned()
                .map(|mut span| {
                    span.italic = true;
                    span
                })
                .collect();
            wrap_runs(
                vec![DisplayRun::virtual_text(prefix, accent_span(), anchor)],
                quote_spans,
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
            });
            output
        }
        EditorRenderBlock::Table {
            header,
            rows,
            alignments,
        } => table_lines(header, rows, alignments, block_index, anchor),
        EditorRenderBlock::HorizontalRule => vec![DisplayLine {
            runs: vec![DisplayRun::virtual_text(
                "─".repeat(width),
                accent_span(),
                anchor,
            )],
            block_index: Some(block_index),
            no_wrap: true,
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
        }],
        EditorRenderBlock::Footnote { label, spans } => wrap_runs(
            vec![DisplayRun::virtual_text(
                format!("[^{label}] "),
                accent_span(),
                anchor,
            )],
            spans.clone(),
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
    block_index: usize,
    anchor: Option<usize>,
) -> Vec<DisplayLine> {
    let columns = max(
        header.len(),
        rows.iter().map(Vec::len).max().unwrap_or_default(),
    );
    if columns == 0 {
        return vec![empty_display_line_at(Some(block_index), anchor)];
    }
    let mut widths = vec![1usize; columns];
    for row in std::iter::once(header).chain(rows.iter().map(Vec::as_slice)) {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell_text(cell).chars().count().min(24));
        }
    }
    let border = |left: char, middle: char, right: char| {
        let mut value = String::new();
        value.push(left);
        for (index, width) in widths.iter().enumerate() {
            value.push_str(&"─".repeat(width.saturating_add(2)));
            value.push(if index + 1 == widths.len() {
                right
            } else {
                middle
            });
        }
        value
    };
    let mut output = vec![table_display_line(
        border('┌', '┬', '┐'),
        block_index,
        false,
        anchor,
    )];
    if !header.is_empty() {
        output.push(table_row_line(
            header,
            &widths,
            alignments,
            block_index,
            true,
            anchor,
        ));
        output.push(table_display_line(
            border('├', '┼', '┤'),
            block_index,
            false,
            anchor,
        ));
    }
    output.extend(
        rows.iter()
            .map(|row| table_row_line(row, &widths, alignments, block_index, false, anchor)),
    );
    output.push(table_display_line(
        border('└', '┴', '┘'),
        block_index,
        false,
        anchor,
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
) -> DisplayLine {
    let mut text = String::from("│");
    for (column, width) in widths.iter().enumerate() {
        let value = row.get(column).map(cell_text).unwrap_or_default();
        text.push(' ');
        text.push_str(&align_text(
            &fit_text(&value, *width),
            *width,
            alignments.get(column).copied().unwrap_or_default(),
        ));
        text.push(' ');
        text.push('│');
    }
    table_display_line(text, block_index, header, anchor)
}

fn table_display_line(
    text: String,
    block_index: usize,
    header: bool,
    anchor: Option<usize>,
) -> DisplayLine {
    let mut style = EditorRenderSpan::plain("");
    style.color = EditorSpanColor::Accent;
    style.bold = header;
    DisplayLine {
        runs: vec![DisplayRun::virtual_text(text, style, anchor)],
        block_index: Some(block_index),
        no_wrap: true,
    }
}

fn wrap_runs(
    prefix: Vec<DisplayRun>,
    spans: Vec<EditorRenderSpan>,
    width: usize,
    block_index: usize,
    no_wrap: bool,
    source: Option<&str>,
) -> Vec<DisplayLine> {
    let prefix_width = runs_width(&prefix);
    let continuation_anchor = prefix.iter().find_map(display_run_anchor);
    let continuation_prefix = if prefix_width == 0 {
        Vec::new()
    } else {
        vec![DisplayRun::virtual_text(
            " ".repeat(prefix_width),
            EditorRenderSpan::plain(""),
            continuation_anchor,
        )]
    };
    let mut lines = Vec::new();
    let mut current = prefix.clone();
    let mut current_width = prefix_width;
    for span in spans {
        let span_range = span.source_range;
        for run in mapped_text_runs(&span.text, span.clone(), span_range, source) {
            if is_display_newline(&run.text) {
                if let Some(before) = display_run_start(&run) {
                    current.push(DisplayRun::virtual_text(
                        "",
                        EditorRenderSpan::plain(""),
                        Some(before),
                    ));
                }
                lines.push(DisplayLine {
                    runs: std::mem::take(&mut current),
                    block_index: Some(block_index),
                    no_wrap,
                });
                current = continuation_prefix.clone();
                if let Some(after) = display_run_end(&run) {
                    current.push(DisplayRun::virtual_text(
                        "",
                        EditorRenderSpan::plain(""),
                        Some(after),
                    ));
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
            enabled: model.toolbar.is_enabled(action, model.read_only),
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

fn align_text(text: &str, width: usize, alignment: EditorTableAlignment) -> String {
    let used = text.chars().count().min(width);
    let remaining = width.saturating_sub(used);
    let (left, right) = match alignment {
        EditorTableAlignment::Left => (0, remaining),
        EditorTableAlignment::Center => (remaining / 2, remaining - remaining / 2),
        EditorTableAlignment::Right => (remaining, 0),
    };
    format!("{}{}{}", " ".repeat(left), text, " ".repeat(right))
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
    Span::raw(terminal_safe_text(&run.text)).width()
}

fn display_run_anchor(run: &DisplayRun) -> Option<usize> {
    match run.source {
        DisplaySource::Unmapped => None,
        DisplaySource::Range(range) => Some(range.start),
        DisplaySource::Virtual(offset) => Some(offset),
    }
}

fn display_run_start(run: &DisplayRun) -> Option<usize> {
    display_run_anchor(run)
}

fn display_run_end(run: &DisplayRun) -> Option<usize> {
    match run.source {
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
    let span = Span::raw(text.to_owned());
    let mut relative = 0usize;
    let mut runs = span
        .styled_graphemes(Style::default())
        .map(|grapheme| {
            let start = relative;
            relative = relative.saturating_add(grapheme.symbol.len());
            let mapped = match (exact, source_range) {
                (Some(range), _) => DisplaySource::Range(EditorSourceRange::new(
                    range.start.saturating_add(start),
                    range.start.saturating_add(relative).min(range.end),
                )),
                // Decoded entities, generated labels and other non-exact text
                // must never pretend that every visible grapheme covers the
                // whole source range. Keep an anchor for Source-mode fallback,
                // but mark it virtual/non-editable.
                (None, Some(range)) => DisplaySource::Virtual(range.start),
                (None, None) => DisplaySource::Unmapped,
            };
            DisplayRun {
                text: grapheme.symbol.to_owned(),
                style: style.clone(),
                source: mapped,
            }
        })
        .collect::<Vec<_>>();
    if runs.is_empty() {
        runs.push(DisplayRun {
            text: String::new(),
            style,
            source: exact
                .map(DisplaySource::Range)
                .or_else(|| source_range.map(|range| DisplaySource::Virtual(range.start)))
                .unwrap_or(DisplaySource::Unmapped),
        });
    }
    runs
}

fn display_line_source_boundaries(line: &DisplayLine) -> Vec<EditorSourceBoundary> {
    let mut boundaries = Vec::new();
    let mut column = 0usize;
    for run in &line.runs {
        if run.text.is_empty() {
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
        let span = Span::raw(run.text.clone());
        for grapheme in span.styled_graphemes(Style::default()) {
            if let Some(offset) = display_run_start(run) {
                push_source_boundary(
                    &mut boundaries,
                    column,
                    offset,
                    matches!(run.source, DisplaySource::Range(_)),
                );
            }
            let safe = terminal_safe_text(grapheme.symbol);
            column = column.saturating_add(Span::raw(safe).width().max(1));
            if let Some(offset) = display_run_end(run) {
                push_source_boundary(
                    &mut boundaries,
                    column,
                    offset,
                    matches!(run.source, DisplaySource::Range(_)),
                );
            }
        }
    }
    boundaries
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

fn source_display_lines(source: &str) -> Vec<String> {
    source_display_line_ranges(source)
        .into_iter()
        .map(|range| {
            source
                .get(range.start..range.end)
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
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
