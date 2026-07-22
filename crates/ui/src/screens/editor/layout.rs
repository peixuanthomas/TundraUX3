use super::document::*;
use super::source::*;
use super::*;

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
    pub(super) prepared_lines: Vec<DisplayLine>,
    pub(super) prepared_start: usize,
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

pub(super) fn menu_layout(area: Rect) -> (Vec<EditorMenuLayout>, Vec<EditorModeLayout>) {
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

pub(super) fn menu_popup_layout(
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

pub(super) fn settings_layout(
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

pub(super) fn quick_menu_layout(
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

pub(super) fn menu_actions(menu: EditorMenu) -> Vec<EditorMenuAction> {
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

pub(super) fn quick_action_label(action: EditorQuickAction) -> &'static str {
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

pub(super) fn toolbar_layout(
    area: Rect,
    model: &EditorViewModel,
) -> (Vec<EditorToolbarItemLayout>, bool) {
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

pub(super) fn toolbar_specs() -> Vec<(EditorToolbarAction, &'static str, u16)> {
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

pub(super) fn toolbar_spec(
    action: EditorToolbarAction,
) -> (EditorToolbarAction, &'static str, u16) {
    let label = toolbar_label(action);
    (action, label, to_u16(label.chars().count()))
}

pub(super) fn toolbar_label(action: EditorToolbarAction) -> &'static str {
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

pub(super) fn menu_label(menu: EditorMenu) -> &'static str {
    match menu {
        EditorMenu::File => "File",
        EditorMenu::Edit => "Edit",
        EditorMenu::Insert => "Insert",
        EditorMenu::Format => "Format",
        EditorMenu::View => "View",
        EditorMenu::Settings => "Settings",
    }
}

pub(super) fn menu_action_label(action: EditorMenuAction) -> &'static str {
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

pub(super) fn mode_label(mode: EditorMode) -> &'static str {
    match mode {
        EditorMode::Rich => "Rich",
        EditorMode::Source => "Source",
    }
}

pub(super) fn scrollbar_layout(
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

pub(super) fn horizontal_scrollbar_layout(
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

pub(super) fn inset(area: Rect, amount: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(amount),
        area.y.saturating_add(amount),
        area.width.saturating_sub(amount.saturating_mul(2)),
        area.height.saturating_sub(amount.saturating_mul(2)),
    )
}

pub(super) fn contains(area: Rect, x: u16, y: u16) -> bool {
    area.width > 0
        && area.height > 0
        && x >= area.x
        && x < area.right()
        && y >= area.y
        && y < area.bottom()
}

pub(super) fn to_u16(value: usize) -> u16 {
    min(value, usize::from(u16::MAX)) as u16
}
