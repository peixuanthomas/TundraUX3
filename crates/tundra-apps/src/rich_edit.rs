//! Semantic editing operations for [`RichDocument`](crate::rich_document::RichDocument).
//!
//! No operation in this module reads or writes Markdown delimiters. Markdown
//! only re-enters the flow through `markdown_codec` at an explicit boundary.

use unicode_segmentation::UnicodeSegmentation;

use crate::editor::{CursorMove, FormatCommand, TableColumnEdge, TableColumnEdit};
use crate::rich_document::{
    InlineContent, InlineMarks, InlineNode, LinkAttributes, NodeId, ProjectedBlockKind,
    RewriteState, RichBlock, RichBlockKind, RichDocument, RichListItem, RichListKind, RichPosition,
    RichProjection, RichTableAlignment, RichTableCell, RichText,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RichSelection {
    pub anchor: RichPosition,
    pub focus: RichPosition,
}

impl RichSelection {
    pub const fn new(anchor: RichPosition, focus: RichPosition) -> Self {
        Self { anchor, focus }
    }

    pub fn is_collapsed(self) -> bool {
        self.anchor == self.focus
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichEditor {
    pub document: RichDocument,
    pub cursor: Option<RichPosition>,
    pub selection: Option<RichSelection>,
    preferred_column: Option<usize>,
}

impl RichEditor {
    pub fn new(document: RichDocument) -> Self {
        let cursor = document.first_editable_position();
        Self {
            document,
            cursor,
            selection: None,
            preferred_column: None,
        }
    }

    pub fn projection(&self) -> RichProjection {
        self.document.project()
    }

    pub fn is_empty(&self) -> bool {
        self.document.blocks.is_empty()
            || self
                .ordered_containers()
                .iter()
                .all(|container| container.len == 0)
    }

    pub fn word_count(&self) -> usize {
        self.plain_text().unicode_words().count()
    }

    pub fn plain_text(&self) -> String {
        let projection = self.projection();
        projected_plain_text(&projection)
    }

    pub fn selected_text(&self) -> Option<String> {
        let selection = self
            .selection
            .filter(|selection| !selection.is_collapsed())?;
        let segments = self.selection_segments(selection)?;
        let mut output = String::new();
        for (index, segment) in segments.iter().enumerate() {
            if index > 0 {
                output.push('\n');
            }
            output.push_str(&container_grapheme_slice(
                &self.document,
                segment.id,
                segment.start,
                segment.end,
            ));
        }
        Some(output)
    }

    pub fn has_selection(&self) -> bool {
        self.selection
            .is_some_and(|selection| !selection.is_collapsed())
    }

    pub fn contains_position(&self, position: RichPosition) -> bool {
        container_len(&self.document, position.container_id)
            .is_some_and(|len| position.grapheme_offset <= len)
    }

    pub fn select_all(&mut self) {
        let containers = self.ordered_containers();
        let (Some(first), Some(last)) = (containers.first(), containers.last()) else {
            return;
        };
        let anchor = RichPosition::new(first.id, 0);
        let focus = RichPosition::new(last.id, last.len);
        self.cursor = Some(focus);
        self.selection = Some(RichSelection::new(anchor, focus));
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub fn move_to(&mut self, position: RichPosition, extend_selection: bool) -> bool {
        let Some(len) = container_len(&self.document, position.container_id) else {
            return false;
        };
        let target = RichPosition::new(position.container_id, position.grapheme_offset.min(len));
        let old = self.cursor.unwrap_or(target);
        if extend_selection {
            let anchor = self.selection.map_or(old, |selection| selection.anchor);
            self.selection = (anchor != target).then_some(RichSelection::new(anchor, target));
        } else {
            self.selection = None;
        }
        self.cursor = Some(target);
        self.preferred_column = None;
        true
    }

    pub fn move_cursor(&mut self, movement: CursorMove, extend_selection: bool) {
        let containers = self.ordered_containers();
        if containers.is_empty() {
            return;
        }
        let current = self
            .cursor
            .unwrap_or(RichPosition::new(containers[0].id, 0));
        let index = containers
            .iter()
            .position(|container| container.id == current.container_id)
            .unwrap_or(0);
        let current_len = containers[index].len;
        let target = match movement {
            CursorMove::Left => {
                if current.grapheme_offset > 0 {
                    RichPosition::new(current.container_id, current.grapheme_offset - 1)
                } else if index > 0 {
                    RichPosition::new(containers[index - 1].id, containers[index - 1].len)
                } else {
                    current
                }
            }
            CursorMove::Right => {
                if current.grapheme_offset < current_len {
                    RichPosition::new(current.container_id, current.grapheme_offset + 1)
                } else if index + 1 < containers.len() {
                    RichPosition::new(containers[index + 1].id, 0)
                } else {
                    current
                }
            }
            CursorMove::WordLeft => {
                let text = container_text(&self.document, current.container_id);
                RichPosition::new(
                    current.container_id,
                    previous_word_grapheme(&text, current.grapheme_offset),
                )
            }
            CursorMove::WordRight => {
                let text = container_text(&self.document, current.container_id);
                RichPosition::new(
                    current.container_id,
                    next_word_grapheme(&text, current.grapheme_offset),
                )
            }
            CursorMove::LineStart => RichPosition::new(
                current.container_id,
                logical_line_start(
                    &container_text(&self.document, current.container_id),
                    current.grapheme_offset,
                ),
            ),
            CursorMove::LineEnd => RichPosition::new(
                current.container_id,
                logical_line_end(
                    &container_text(&self.document, current.container_id),
                    current.grapheme_offset,
                ),
            ),
            CursorMove::DocumentStart => RichPosition::new(containers[0].id, 0),
            CursorMove::DocumentEnd => {
                let last = containers.last().expect("non-empty containers");
                RichPosition::new(last.id, last.len)
            }
            CursorMove::Up => {
                let desired = self.preferred_column.unwrap_or(current.grapheme_offset);
                self.preferred_column = Some(desired);
                if index > 0 {
                    RichPosition::new(
                        containers[index - 1].id,
                        desired.min(containers[index - 1].len),
                    )
                } else {
                    RichPosition::new(current.container_id, 0)
                }
            }
            CursorMove::Down => {
                let desired = self.preferred_column.unwrap_or(current.grapheme_offset);
                self.preferred_column = Some(desired);
                if index + 1 < containers.len() {
                    RichPosition::new(
                        containers[index + 1].id,
                        desired.min(containers[index + 1].len),
                    )
                } else {
                    RichPosition::new(current.container_id, current_len)
                }
            }
        };
        let keep_preferred = matches!(movement, CursorMove::Up | CursorMove::Down);
        self.move_to(target, extend_selection);
        if keep_preferred {
            self.preferred_column = Some(target.grapheme_offset);
        }
    }

    pub fn insert_text(&mut self, text: &str) -> bool {
        if text.is_empty() && !self.has_selection() {
            return false;
        }
        if self.has_selection() {
            self.delete_selection();
        }
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let Some(position) = self.ensure_cursor() else {
            return false;
        };
        let inserted_len = normalized.graphemes(true).count();
        let inserted = inserted_atoms(&normalized);
        if let Some(content) = find_inline_mut(&mut self.document.blocks, position.container_id) {
            let mut atoms = inline_atoms(content);
            let offset = position.grapheme_offset.min(atoms.len());
            atoms.splice(offset..offset, inserted);
            *content = atoms_to_inline(atoms);
        } else if let Some(code) = find_code_mut(&mut self.document.blocks, position.container_id) {
            replace_grapheme_range(
                code,
                position.grapheme_offset,
                position.grapheme_offset,
                &normalized,
            );
        } else {
            return false;
        }
        self.document
            .mark_containing_block_dirty(position.container_id);
        self.cursor = Some(RichPosition::new(
            position.container_id,
            position.grapheme_offset + inserted_len,
        ));
        self.selection = None;
        true
    }

    pub fn backspace(&mut self) -> bool {
        if self.has_selection() {
            return self.delete_selection();
        }
        let Some(position) = self.cursor else {
            return false;
        };
        if position.grapheme_offset == 0 {
            let containers = self.ordered_containers();
            let Some(index) = containers
                .iter()
                .position(|container| container.id == position.container_id)
            else {
                return false;
            };
            if index == 0 || containers[index - 1].len == 0 {
                return false;
            }
            let previous = &containers[index - 1];
            self.selection = Some(RichSelection::new(
                RichPosition::new(previous.id, previous.len - 1),
                RichPosition::new(previous.id, previous.len),
            ));
            return self.delete_selection();
        }
        self.selection = Some(RichSelection::new(
            RichPosition::new(position.container_id, position.grapheme_offset - 1),
            position,
        ));
        self.delete_selection()
    }

    pub fn delete_forward(&mut self) -> bool {
        if self.has_selection() {
            return self.delete_selection();
        }
        let Some(position) = self.cursor else {
            return false;
        };
        let Some(len) = container_len(&self.document, position.container_id) else {
            return false;
        };
        if position.grapheme_offset >= len {
            return false;
        }
        self.selection = Some(RichSelection::new(
            position,
            RichPosition::new(position.container_id, position.grapheme_offset + 1),
        ));
        self.delete_selection()
    }

    pub fn delete_selection(&mut self) -> bool {
        let Some(selection) = self.selection.filter(|selection| !selection.is_collapsed()) else {
            return false;
        };
        let Some(segments) = self.selection_segments(selection) else {
            return false;
        };
        let Some(first) = segments.first().copied() else {
            return false;
        };
        for segment in segments.iter().rev() {
            replace_container_range(
                &mut self.document,
                segment.id,
                segment.start,
                segment.end,
                Vec::new(),
                "",
            );
            self.document.mark_containing_block_dirty(segment.id);
        }
        self.cursor = Some(RichPosition::new(first.id, first.start));
        self.selection = None;
        true
    }

    pub fn apply_format(&mut self, format: &FormatCommand) -> bool {
        match format {
            FormatCommand::Bold => self.toggle_mark(MarkKind::Bold),
            FormatCommand::Italic => self.toggle_mark(MarkKind::Italic),
            FormatCommand::Strikethrough => self.toggle_mark(MarkKind::Strikethrough),
            FormatCommand::InlineCode => self.toggle_mark(MarkKind::Code),
            FormatCommand::Heading(level) => self.set_heading(*level),
            FormatCommand::Paragraph => self.set_heading(0),
            FormatCommand::Quote => self.toggle_quote(),
            FormatCommand::BulletList => self.toggle_list(RichListKind::Bullet),
            FormatCommand::OrderedList => self.toggle_list(RichListKind::Ordered),
            FormatCommand::TaskList => self.toggle_list(RichListKind::Task),
            FormatCommand::Link { url, title } => self.apply_link(url, title.clone()),
            FormatCommand::Image { url, alt, title } => {
                self.insert_image(url.clone(), alt.clone(), title.clone())
            }
            FormatCommand::Table { columns, rows } => self.insert_table(*columns, *rows),
        }
    }

    pub fn edit_table_column(
        &mut self,
        table_id: NodeId,
        edge: TableColumnEdge,
        edit: TableColumnEdit,
    ) -> bool {
        let columns = find_table(&self.document.blocks, table_id)
            .map(|(header, rows)| {
                header
                    .len()
                    .max(rows.iter().map(Vec::len).max().unwrap_or(0))
            })
            .unwrap_or(0);
        if columns == 0 || (edit == TableColumnEdit::Remove && columns <= 1) {
            return false;
        }
        let row_count = find_table(&self.document.blocks, table_id)
            .map(|(_, rows)| rows.len())
            .unwrap_or(0);
        let mut ids = Vec::with_capacity(row_count + 1);
        if edit == TableColumnEdit::Insert {
            for _ in 0..=row_count {
                ids.push(self.document.allocate_node_id());
            }
        }
        let Some((header, rows)) = find_table_mut(&mut self.document.blocks, table_id) else {
            return false;
        };
        match edit {
            TableColumnEdit::Insert => {
                let mut ids = ids.into_iter();
                insert_cell(header, edge, ids.next().expect("header id"));
                for row in rows {
                    insert_cell(row, edge, ids.next().expect("row id"));
                }
            }
            TableColumnEdit::Remove => {
                remove_cell(header, edge);
                for row in rows {
                    remove_cell(row, edge);
                }
            }
        }
        self.document.mark_containing_block_dirty(table_id);
        true
    }

    fn ensure_cursor(&mut self) -> Option<RichPosition> {
        if let Some(cursor) = self.cursor {
            return Some(cursor);
        }
        let id = self.document.allocate_node_id();
        self.document.blocks.push(RichBlock::new(
            id,
            RichBlockKind::Paragraph {
                content: InlineContent::default(),
            },
        ));
        let cursor = RichPosition::new(id, 0);
        self.cursor = Some(cursor);
        Some(cursor)
    }

    fn ordered_containers(&self) -> Vec<ContainerInfo> {
        let mut containers = Vec::new();
        collect_containers(&self.document.blocks, &mut containers);
        containers
    }

    fn selection_segments(&self, selection: RichSelection) -> Option<Vec<SelectionSegment>> {
        let containers = self.ordered_containers();
        let anchor_index = containers
            .iter()
            .position(|container| container.id == selection.anchor.container_id)?;
        let focus_index = containers
            .iter()
            .position(|container| container.id == selection.focus.container_id)?;
        let (start_position, end_position, start_index, end_index) = if anchor_index < focus_index
            || (anchor_index == focus_index
                && selection.anchor.grapheme_offset <= selection.focus.grapheme_offset)
        {
            (selection.anchor, selection.focus, anchor_index, focus_index)
        } else {
            (selection.focus, selection.anchor, focus_index, anchor_index)
        };
        Some(
            containers[start_index..=end_index]
                .iter()
                .enumerate()
                .map(|(relative, container)| SelectionSegment {
                    id: container.id,
                    start: if relative == 0 {
                        start_position.grapheme_offset.min(container.len)
                    } else {
                        0
                    },
                    end: if start_index + relative == end_index {
                        end_position.grapheme_offset.min(container.len)
                    } else {
                        container.len
                    },
                })
                .collect(),
        )
    }

    fn toggle_mark(&mut self, mark: MarkKind) -> bool {
        let Some(selection) = self.selection.filter(|selection| !selection.is_collapsed()) else {
            return false;
        };
        let Some(segments) = self.selection_segments(selection) else {
            return false;
        };
        let enable = !segments.iter().all(|segment| {
            range_has_mark(&self.document, segment.id, segment.start, segment.end, mark)
        });
        for segment in &segments {
            if let Some(content) = find_inline_mut(&mut self.document.blocks, segment.id) {
                let mut atoms = inline_atoms(content);
                for atom in atoms.iter_mut().take(segment.end).skip(segment.start) {
                    if let Atom::Text { marks, .. } = atom {
                        set_mark(marks, mark, enable);
                    }
                }
                *content = atoms_to_inline(atoms);
                self.document.mark_containing_block_dirty(segment.id);
            }
        }
        self.cursor = Some(selection.focus);
        self.selection = None;
        true
    }

    fn apply_link(&mut self, url: &str, title: Option<String>) -> bool {
        let Some(selection) = self.selection.filter(|selection| !selection.is_collapsed()) else {
            return false;
        };
        let Some(segments) = self.selection_segments(selection) else {
            return false;
        };
        for segment in &segments {
            if let Some(content) = find_inline_mut(&mut self.document.blocks, segment.id) {
                let mut atoms = inline_atoms(content);
                for atom in atoms.iter_mut().take(segment.end).skip(segment.start) {
                    if let Atom::Text { link, .. } = atom {
                        *link = Some(LinkAttributes {
                            url: url.to_owned(),
                            title: title.clone(),
                        });
                    }
                }
                *content = atoms_to_inline(atoms);
                self.document.mark_containing_block_dirty(segment.id);
            }
        }
        self.cursor = Some(selection.focus);
        self.selection = None;
        true
    }

    fn insert_image(&mut self, url: String, alt: String, title: Option<String>) -> bool {
        if self.has_selection() {
            self.delete_selection();
        }
        let Some(position) = self.ensure_cursor() else {
            return false;
        };
        let Some(content) = find_inline_mut(&mut self.document.blocks, position.container_id)
        else {
            return false;
        };
        let mut atoms = inline_atoms(content);
        let offset = position.grapheme_offset.min(atoms.len());
        atoms.insert(offset, Atom::Image { alt, url, title });
        *content = atoms_to_inline(atoms);
        self.document
            .mark_containing_block_dirty(position.container_id);
        self.cursor = Some(RichPosition::new(position.container_id, offset + 1));
        self.selection = None;
        true
    }

    fn set_heading(&mut self, level: u8) -> bool {
        if level > 6 {
            return false;
        }
        let Some(id) = self.cursor.map(|cursor| cursor.container_id) else {
            return false;
        };
        let changed = change_block_kind(&mut self.document.blocks, id, |kind| match kind {
            RichBlockKind::Paragraph { content } if level > 0 => {
                Some(RichBlockKind::Heading { level, content })
            }
            RichBlockKind::Heading { content, .. } if level == 0 => {
                Some(RichBlockKind::Paragraph { content })
            }
            RichBlockKind::Heading { content, .. } => {
                Some(RichBlockKind::Heading { level, content })
            }
            other => Some(other),
        });
        if changed {
            self.document.mark_containing_block_dirty(id);
        }
        changed
    }

    fn toggle_quote(&mut self) -> bool {
        let Some(id) = self.cursor.map(|cursor| cursor.container_id) else {
            return false;
        };
        let Some(index) = self
            .document
            .blocks
            .iter()
            .position(|block| block.contains_node(id))
        else {
            return false;
        };
        let current = self.document.blocks.remove(index);
        let replacement = match current.kind {
            RichBlockKind::Quote { mut blocks } if blocks.len() == 1 => {
                let mut block = blocks.remove(0);
                block.leading_trivia = current.leading_trivia;
                block.original_raw = None;
                block.rewrite = RewriteState::Dirty;
                block
            }
            kind => {
                let nested = RichBlock {
                    id: current.id,
                    leading_trivia: String::new(),
                    original_raw: None,
                    rewrite: RewriteState::Dirty,
                    kind,
                };
                let wrapper_id = self.document.allocate_node_id();
                RichBlock {
                    id: wrapper_id,
                    leading_trivia: current.leading_trivia,
                    original_raw: None,
                    rewrite: RewriteState::Dirty,
                    kind: RichBlockKind::Quote {
                        blocks: vec![nested],
                    },
                }
            }
        };
        self.document.blocks.insert(index, replacement);
        true
    }

    fn toggle_list(&mut self, target: RichListKind) -> bool {
        let Some(id) = self.cursor.map(|cursor| cursor.container_id) else {
            return false;
        };
        let Some(index) = self
            .document
            .blocks
            .iter()
            .position(|block| block.contains_node(id))
        else {
            return false;
        };
        let current = self.document.blocks.remove(index);
        let leading = current.leading_trivia.clone();
        let replacement = match current.kind {
            RichBlockKind::List { kind, items, .. } if kind == target => {
                let mut nodes = Vec::new();
                for (item_index, item) in items.into_iter().enumerate() {
                    if item_index > 0 {
                        nodes.push(InlineNode::SoftBreak);
                    }
                    if let Some(block) = item.blocks.into_iter().next() {
                        match block.kind {
                            RichBlockKind::Paragraph { content }
                            | RichBlockKind::Heading { content, .. } => nodes.extend(content.0),
                            _ => {}
                        }
                    }
                }
                RichBlock {
                    id: current.id,
                    leading_trivia: leading,
                    original_raw: None,
                    rewrite: RewriteState::Dirty,
                    kind: RichBlockKind::Paragraph {
                        content: InlineContent(nodes),
                    },
                }
            }
            RichBlockKind::Paragraph { content } | RichBlockKind::Heading { content, .. } => {
                let lines = split_inline_lines(content);
                let mut items = Vec::new();
                for line in lines {
                    let item_id = self.document.allocate_node_id();
                    let paragraph_id = self.document.allocate_node_id();
                    items.push(RichListItem {
                        id: item_id,
                        checked: (target == RichListKind::Task).then_some(false),
                        blocks: vec![RichBlock {
                            id: paragraph_id,
                            leading_trivia: String::new(),
                            original_raw: None,
                            rewrite: RewriteState::Dirty,
                            kind: RichBlockKind::Paragraph { content: line },
                        }],
                    });
                }
                let wrapper_id = self.document.allocate_node_id();
                RichBlock {
                    id: wrapper_id,
                    leading_trivia: leading,
                    original_raw: None,
                    rewrite: RewriteState::Dirty,
                    kind: RichBlockKind::List {
                        kind: target,
                        start: 1,
                        tight: true,
                        items,
                    },
                }
            }
            kind => RichBlock {
                id: current.id,
                leading_trivia: leading,
                original_raw: current.original_raw,
                rewrite: current.rewrite,
                kind,
            },
        };
        let cursor = replacement.first_editable_position();
        self.document.blocks.insert(index, replacement);
        if let Some(cursor) = cursor {
            self.cursor = Some(cursor);
        }
        true
    }

    fn insert_table(&mut self, columns: usize, rows: usize) -> bool {
        if columns == 0 || columns > 32 || rows > 256 {
            return false;
        }
        let table_id = self.document.allocate_node_id();
        let mut header = Vec::with_capacity(columns);
        for column in 1..=columns {
            let id = self.document.allocate_node_id();
            header.push(RichTableCell {
                id,
                content: InlineContent::plain(format!("Column {column}")),
            });
        }
        let mut body = Vec::with_capacity(rows);
        for _ in 0..rows {
            let mut row = Vec::with_capacity(columns);
            for _ in 0..columns {
                row.push(RichTableCell {
                    id: self.document.allocate_node_id(),
                    content: InlineContent::default(),
                });
            }
            body.push(row);
        }
        let cursor_id = header.first().map(|cell| cell.id);
        let insert_index = self
            .cursor
            .and_then(|cursor| {
                self.document
                    .blocks
                    .iter()
                    .position(|block| block.contains_node(cursor.container_id))
            })
            .map_or(self.document.blocks.len(), |index| index + 1);
        let leading_trivia = (!self.document.blocks.is_empty())
            .then(|| {
                let newline = self.document.preferred_line_ending.as_str();
                format!("{newline}{newline}")
            })
            .unwrap_or_default();
        self.document.blocks.insert(
            insert_index,
            RichBlock {
                id: table_id,
                leading_trivia,
                original_raw: None,
                rewrite: RewriteState::Dirty,
                kind: RichBlockKind::Table {
                    alignments: vec![RichTableAlignment::None; columns],
                    header,
                    rows: body,
                },
            },
        );
        let paragraph_id = self.document.allocate_node_id();
        let newline = self.document.preferred_line_ending.as_str();
        self.document.blocks.insert(
            insert_index + 1,
            RichBlock {
                id: paragraph_id,
                leading_trivia: format!("{newline}{newline}"),
                original_raw: None,
                rewrite: RewriteState::Dirty,
                kind: RichBlockKind::Paragraph {
                    content: InlineContent::default(),
                },
            },
        );
        self.cursor = cursor_id.map(|id| RichPosition::new(id, 0));
        self.selection = None;
        true
    }
}

#[derive(Debug, Clone, Copy)]
struct ContainerInfo {
    id: NodeId,
    len: usize,
}

#[derive(Debug, Clone, Copy)]
struct SelectionSegment {
    id: NodeId,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Atom {
    Text {
        grapheme: String,
        marks: InlineMarks,
        link: Option<LinkAttributes>,
    },
    Image {
        alt: String,
        url: String,
        title: Option<String>,
    },
    SoftBreak,
    HardBreak,
}

#[derive(Debug, Clone, Copy)]
enum MarkKind {
    Bold,
    Italic,
    Strikethrough,
    Code,
}

fn collect_containers(blocks: &[RichBlock], output: &mut Vec<ContainerInfo>) {
    for block in blocks {
        match &block.kind {
            RichBlockKind::Paragraph { content } | RichBlockKind::Heading { content, .. } => {
                output.push(ContainerInfo {
                    id: block.id,
                    len: content.grapheme_len(),
                });
            }
            RichBlockKind::CodeBlock { code, .. } => output.push(ContainerInfo {
                id: block.id,
                len: code.graphemes(true).count(),
            }),
            RichBlockKind::Quote { blocks } => collect_containers(blocks, output),
            RichBlockKind::List { items, .. } => {
                for item in items {
                    collect_containers(&item.blocks, output);
                }
            }
            RichBlockKind::Table { header, rows, .. } => {
                output.extend(header.iter().chain(rows.iter().flatten()).map(|cell| {
                    ContainerInfo {
                        id: cell.id,
                        len: cell.content.grapheme_len(),
                    }
                }));
            }
            RichBlockKind::Rule | RichBlockKind::OpaqueMarkdown { .. } => {}
        }
    }
}

fn find_inline_mut(blocks: &mut [RichBlock], id: NodeId) -> Option<&mut InlineContent> {
    for block in blocks {
        let block_id = block.id;
        match &mut block.kind {
            RichBlockKind::Paragraph { content } | RichBlockKind::Heading { content, .. } => {
                if block_id == id {
                    return Some(content);
                }
            }
            RichBlockKind::Quote { blocks } => {
                if let Some(content) = find_inline_mut(blocks, id) {
                    return Some(content);
                }
            }
            RichBlockKind::List { items, .. } => {
                for item in items {
                    if let Some(content) = find_inline_mut(&mut item.blocks, id) {
                        return Some(content);
                    }
                }
            }
            RichBlockKind::Table { header, rows, .. } => {
                if let Some(cell) = header
                    .iter_mut()
                    .chain(rows.iter_mut().flatten())
                    .find(|cell| cell.id == id)
                {
                    return Some(&mut cell.content);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_inline(blocks: &[RichBlock], id: NodeId) -> Option<&InlineContent> {
    for block in blocks {
        if block.id == id {
            match &block.kind {
                RichBlockKind::Paragraph { content } | RichBlockKind::Heading { content, .. } => {
                    return Some(content);
                }
                _ => {}
            }
        }
        match &block.kind {
            RichBlockKind::Quote { blocks } => {
                if let Some(content) = find_inline(blocks, id) {
                    return Some(content);
                }
            }
            RichBlockKind::List { items, .. } => {
                for item in items {
                    if let Some(content) = find_inline(&item.blocks, id) {
                        return Some(content);
                    }
                }
            }
            RichBlockKind::Table { header, rows, .. } => {
                if let Some(cell) = header
                    .iter()
                    .chain(rows.iter().flatten())
                    .find(|cell| cell.id == id)
                {
                    return Some(&cell.content);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_code_mut(blocks: &mut [RichBlock], id: NodeId) -> Option<&mut String> {
    for block in blocks {
        let block_id = block.id;
        match &mut block.kind {
            RichBlockKind::CodeBlock { code, .. } if block_id == id => return Some(code),
            RichBlockKind::Quote { blocks } => {
                if let Some(code) = find_code_mut(blocks, id) {
                    return Some(code);
                }
            }
            RichBlockKind::List { items, .. } => {
                for item in items {
                    if let Some(code) = find_code_mut(&mut item.blocks, id) {
                        return Some(code);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn find_code(blocks: &[RichBlock], id: NodeId) -> Option<&str> {
    for block in blocks {
        if block.id == id
            && let RichBlockKind::CodeBlock { code, .. } = &block.kind
        {
            return Some(code);
        }
        match &block.kind {
            RichBlockKind::Quote { blocks } => {
                if let Some(code) = find_code(blocks, id) {
                    return Some(code);
                }
            }
            RichBlockKind::List { items, .. } => {
                for item in items {
                    if let Some(code) = find_code(&item.blocks, id) {
                        return Some(code);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn find_table(
    blocks: &[RichBlock],
    id: NodeId,
) -> Option<(&[RichTableCell], &[Vec<RichTableCell>])> {
    for block in blocks {
        if block.id == id
            && let RichBlockKind::Table { header, rows, .. } = &block.kind
        {
            return Some((header, rows));
        }
        match &block.kind {
            RichBlockKind::Quote { blocks } => {
                if let Some(table) = find_table(blocks, id) {
                    return Some(table);
                }
            }
            RichBlockKind::List { items, .. } => {
                for item in items {
                    if let Some(table) = find_table(&item.blocks, id) {
                        return Some(table);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn find_table_mut(
    blocks: &mut [RichBlock],
    id: NodeId,
) -> Option<(&mut Vec<RichTableCell>, &mut Vec<Vec<RichTableCell>>)> {
    for block in blocks {
        let block_id = block.id;
        match &mut block.kind {
            RichBlockKind::Table { header, rows, .. } if block_id == id => {
                return Some((header, rows));
            }
            RichBlockKind::Quote { blocks } => {
                if let Some(table) = find_table_mut(blocks, id) {
                    return Some(table);
                }
            }
            RichBlockKind::List { items, .. } => {
                for item in items {
                    if let Some(table) = find_table_mut(&mut item.blocks, id) {
                        return Some(table);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn container_len(document: &RichDocument, id: NodeId) -> Option<usize> {
    find_inline(&document.blocks, id)
        .map(InlineContent::grapheme_len)
        .or_else(|| find_code(&document.blocks, id).map(|code| code.graphemes(true).count()))
}

fn container_text(document: &RichDocument, id: NodeId) -> String {
    find_inline(&document.blocks, id)
        .map(InlineContent::plain_text)
        .or_else(|| find_code(&document.blocks, id).map(str::to_owned))
        .unwrap_or_default()
}

fn container_grapheme_slice(
    document: &RichDocument,
    id: NodeId,
    start: usize,
    end: usize,
) -> String {
    if let Some(content) = find_inline(&document.blocks, id) {
        return inline_atoms(content)
            .into_iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .map(|atom| match atom {
                Atom::Text { grapheme, .. } => grapheme,
                Atom::Image { alt, .. } => alt,
                Atom::SoftBreak | Atom::HardBreak => "\n".to_owned(),
            })
            .collect();
    }
    find_code(&document.blocks, id)
        .map(|code| {
            code.graphemes(true)
                .skip(start)
                .take(end.saturating_sub(start))
                .collect()
        })
        .unwrap_or_default()
}

fn inline_atoms(content: &InlineContent) -> Vec<Atom> {
    let mut atoms = Vec::new();
    for node in &content.0 {
        match node {
            InlineNode::Text(text) => {
                atoms.extend(text.text.graphemes(true).map(|grapheme| Atom::Text {
                    grapheme: grapheme.to_owned(),
                    marks: text.marks,
                    link: text.link.clone(),
                }))
            }
            InlineNode::Image { alt, url, title } => atoms.push(Atom::Image {
                alt: alt.clone(),
                url: url.clone(),
                title: title.clone(),
            }),
            InlineNode::SoftBreak => atoms.push(Atom::SoftBreak),
            InlineNode::HardBreak => atoms.push(Atom::HardBreak),
        }
    }
    atoms
}

fn atoms_to_inline(atoms: Vec<Atom>) -> InlineContent {
    let mut nodes = Vec::new();
    for atom in atoms {
        match atom {
            Atom::Text {
                grapheme,
                marks,
                link,
            } => match nodes.last_mut() {
                Some(InlineNode::Text(previous))
                    if previous.marks == marks && previous.link == link =>
                {
                    previous.text.push_str(&grapheme);
                }
                _ => nodes.push(InlineNode::Text(RichText {
                    text: grapheme,
                    marks,
                    link,
                })),
            },
            Atom::Image { alt, url, title } => nodes.push(InlineNode::Image { alt, url, title }),
            Atom::SoftBreak => nodes.push(InlineNode::SoftBreak),
            Atom::HardBreak => nodes.push(InlineNode::HardBreak),
        }
    }
    InlineContent(nodes)
}

fn inserted_atoms(text: &str) -> Vec<Atom> {
    text.graphemes(true)
        .map(|grapheme| {
            if grapheme == "\n" {
                Atom::SoftBreak
            } else {
                Atom::Text {
                    grapheme: grapheme.to_owned(),
                    marks: InlineMarks::default(),
                    link: None,
                }
            }
        })
        .collect()
}

fn replace_container_range(
    document: &mut RichDocument,
    id: NodeId,
    start: usize,
    end: usize,
    replacement: Vec<Atom>,
    code_replacement: &str,
) {
    if let Some(content) = find_inline_mut(&mut document.blocks, id) {
        let mut atoms = inline_atoms(content);
        let start = start.min(atoms.len());
        let end = end.min(atoms.len()).max(start);
        atoms.splice(start..end, replacement);
        *content = atoms_to_inline(atoms);
    } else if let Some(code) = find_code_mut(&mut document.blocks, id) {
        replace_grapheme_range(code, start, end, code_replacement);
    }
}

fn replace_grapheme_range(text: &mut String, start: usize, end: usize, replacement: &str) {
    let byte_start = grapheme_byte_offset(text, start);
    let byte_end = grapheme_byte_offset(text, end);
    text.replace_range(byte_start..byte_end, replacement);
}

fn grapheme_byte_offset(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .nth(offset)
        .map_or(text.len(), |(index, _)| index)
}

fn range_has_mark(
    document: &RichDocument,
    id: NodeId,
    start: usize,
    end: usize,
    mark: MarkKind,
) -> bool {
    let Some(content) = find_inline(&document.blocks, id) else {
        return false;
    };
    let selected = inline_atoms(content)
        .into_iter()
        .skip(start)
        .take(end.saturating_sub(start))
        .filter_map(|atom| match atom {
            Atom::Text { marks, .. } => Some(mark_value(marks, mark)),
            _ => None,
        })
        .collect::<Vec<_>>();
    !selected.is_empty() && selected.into_iter().all(|value| value)
}

fn mark_value(marks: InlineMarks, mark: MarkKind) -> bool {
    match mark {
        MarkKind::Bold => marks.bold,
        MarkKind::Italic => marks.italic,
        MarkKind::Strikethrough => marks.strikethrough,
        MarkKind::Code => marks.code,
    }
}

fn set_mark(marks: &mut InlineMarks, mark: MarkKind, enabled: bool) {
    match mark {
        MarkKind::Bold => marks.bold = enabled,
        MarkKind::Italic => marks.italic = enabled,
        MarkKind::Strikethrough => marks.strikethrough = enabled,
        MarkKind::Code => {
            marks.code = enabled;
            if enabled {
                marks.bold = false;
                marks.italic = false;
                marks.strikethrough = false;
            }
        }
    }
}

fn change_block_kind(
    blocks: &mut [RichBlock],
    id: NodeId,
    transform: impl FnOnce(RichBlockKind) -> Option<RichBlockKind> + Copy,
) -> bool {
    for block in blocks {
        if block.id == id {
            let placeholder = RichBlockKind::Rule;
            let current = std::mem::replace(&mut block.kind, placeholder);
            let original = current.clone();
            if let Some(replacement) = transform(current) {
                let changed = replacement != original;
                block.kind = replacement;
                if changed {
                    block.rewrite = RewriteState::Dirty;
                }
                return changed;
            }
            block.kind = original;
            return false;
        }
        match &mut block.kind {
            RichBlockKind::Quote { blocks } => {
                if change_block_kind(blocks, id, transform) {
                    return true;
                }
            }
            RichBlockKind::List { items, .. } => {
                for item in items {
                    if change_block_kind(&mut item.blocks, id, transform) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn split_inline_lines(content: InlineContent) -> Vec<InlineContent> {
    let mut lines = vec![Vec::new()];
    for node in content.0 {
        match node {
            InlineNode::SoftBreak | InlineNode::HardBreak => lines.push(Vec::new()),
            other => lines.last_mut().expect("at least one line").push(other),
        }
    }
    lines.into_iter().map(InlineContent).collect()
}

fn insert_cell(row: &mut Vec<RichTableCell>, edge: TableColumnEdge, id: NodeId) {
    let cell = RichTableCell {
        id,
        content: InlineContent::default(),
    };
    match edge {
        TableColumnEdge::Left => row.insert(0, cell),
        TableColumnEdge::Right => row.push(cell),
    }
}

fn remove_cell(row: &mut Vec<RichTableCell>, edge: TableColumnEdge) {
    if row.is_empty() {
        return;
    }
    match edge {
        TableColumnEdge::Left => {
            row.remove(0);
        }
        TableColumnEdge::Right => {
            row.pop();
        }
    }
}

fn previous_word_grapheme(text: &str, current: usize) -> usize {
    let prefix = text.graphemes(true).take(current).collect::<String>();
    let trimmed = prefix.trim_end_matches(char::is_whitespace);
    trimmed
        .grapheme_indices(true)
        .rev()
        .find(|(_, grapheme)| grapheme.chars().all(char::is_whitespace))
        .map_or(0, |(byte, grapheme)| {
            trimmed[..byte + grapheme.len()].graphemes(true).count()
        })
}

fn next_word_grapheme(text: &str, current: usize) -> usize {
    let graphemes = text.graphemes(true).collect::<Vec<_>>();
    let mut index = current.min(graphemes.len());
    while index < graphemes.len() && !graphemes[index].chars().all(char::is_whitespace) {
        index += 1;
    }
    while index < graphemes.len() && graphemes[index].chars().all(char::is_whitespace) {
        index += 1;
    }
    index
}

fn logical_line_start(text: &str, current: usize) -> usize {
    text.graphemes(true)
        .take(current)
        .collect::<Vec<_>>()
        .iter()
        .rposition(|grapheme| *grapheme == "\n")
        .map_or(0, |index| index + 1)
}

fn logical_line_end(text: &str, current: usize) -> usize {
    let graphemes = text.graphemes(true).collect::<Vec<_>>();
    graphemes[current.min(graphemes.len())..]
        .iter()
        .position(|grapheme| *grapheme == "\n")
        .map_or(graphemes.len(), |offset| current + offset)
}

fn projected_plain_text(projection: &RichProjection) -> String {
    fn block_text(kind: &ProjectedBlockKind) -> String {
        match kind {
            ProjectedBlockKind::Paragraph { content }
            | ProjectedBlockKind::Heading { content, .. } => {
                content.iter().map(|span| span.text.as_str()).collect()
            }
            ProjectedBlockKind::Quote { blocks } => blocks
                .iter()
                .map(|block| block_text(&block.kind))
                .collect::<Vec<_>>()
                .join("\n"),
            ProjectedBlockKind::CodeBlock { code, .. } => code.clone(),
            ProjectedBlockKind::List { items, .. } => items
                .iter()
                .flat_map(|item| item.blocks.iter())
                .map(|block| block_text(&block.kind))
                .collect::<Vec<_>>()
                .join("\n"),
            ProjectedBlockKind::Table { header, rows, .. } => header
                .iter()
                .chain(rows.iter().flatten())
                .map(|cell| {
                    cell.content
                        .iter()
                        .map(|span| span.text.as_str())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\t"),
            ProjectedBlockKind::Rule => String::new(),
            ProjectedBlockKind::OpaqueMarkdown { raw, .. } => raw.clone(),
        }
    }
    projection
        .blocks
        .iter()
        .map(|block| block_text(&block.kind))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown_codec::{
        MarkdownCodec, parse_count_for_tests, reset_parse_count_for_tests,
    };

    fn assert_single_paragraph(editor: &RichEditor) {
        let projection = editor.projection();
        assert_eq!(projection.blocks.len(), 1);
        assert!(matches!(
            projection.blocks[0].kind,
            ProjectedBlockKind::Paragraph { .. }
        ));
    }

    #[test]
    fn formatting_and_navigation_never_create_markdown_delimiters() {
        let imported = MarkdownCodec::import("text").unwrap();
        let mut editor = RichEditor::new(imported.document);
        let id = editor.cursor.unwrap().container_id;
        editor.selection = Some(RichSelection::new(
            RichPosition::new(id, 0),
            RichPosition::new(id, 4),
        ));
        assert!(editor.apply_format(&FormatCommand::Bold));
        editor.move_cursor(CursorMove::Left, false);
        editor.backspace();

        assert_eq!(editor.plain_text(), "tet");
        let projection = editor.projection();
        let ProjectedBlockKind::Paragraph { content } = &projection.blocks[0].kind else {
            panic!("paragraph")
        };
        assert!(content.iter().all(|span| !span.text.contains("**")));
        assert!(content.iter().any(|span| span.marks.bold));
    }

    #[test]
    fn markdown_punctuation_is_plain_text_until_export() {
        let mut editor = RichEditor::new(RichDocument::new());
        editor.insert_text("**** # | ` literal");
        assert_eq!(editor.plain_text(), "**** # | ` literal");
        assert!(matches!(
            editor.projection().blocks[0].kind,
            ProjectedBlockKind::Paragraph { .. }
        ));
        let markdown = MarkdownCodec::export(&editor.document).unwrap().markdown;
        assert!(markdown.contains("\\*\\*\\*\\*"));
    }

    #[test]
    fn unicode_positions_count_graphemes() {
        let mut editor = RichEditor::new(RichDocument::new());
        editor.insert_text("A👨‍👩‍👧‍👦e\u{301}🇨🇳好");
        assert_eq!(editor.cursor.unwrap().grapheme_offset, 5);
        editor.backspace();
        assert_eq!(editor.plain_text(), "A👨‍👩‍👧‍👦e\u{301}🇨🇳");
        editor.backspace();
        assert_eq!(editor.plain_text(), "A👨‍👩‍👧‍👦e\u{301}");
        editor.backspace();
        assert_eq!(editor.plain_text(), "A👨‍👩‍👧‍👦");
    }

    #[test]
    fn punctuation_typed_one_key_at_a_time_never_changes_structure() {
        let mut editor = RichEditor::new(RichDocument::new());
        let mut expected = String::new();
        for key in [
            "*", "*", "*", "*", "`", "#", " ", "-", "-", "-", "|", "x", "|",
        ] {
            assert!(editor.insert_text(key));
            expected.push_str(key);
            assert_eq!(editor.plain_text(), expected);
            assert_single_paragraph(&editor);
            let projection = editor.projection();
            let ProjectedBlockKind::Paragraph { content } = &projection.blocks[0].kind else {
                unreachable!()
            };
            assert!(
                content
                    .iter()
                    .all(|span| span.marks == InlineMarks::default())
            );
        }

        assert!(editor.insert_text("\n| cell | value |"));
        assert_eq!(editor.document.blocks.len(), 1);
        assert_single_paragraph(&editor);
    }

    #[test]
    fn nested_marks_and_deleting_the_last_marked_grapheme_stay_semantic() {
        let imported = MarkdownCodec::import("abcdef").unwrap();
        let mut editor = RichEditor::new(imported.document);
        let id = editor.cursor.unwrap().container_id;

        editor.selection = Some(RichSelection::new(
            RichPosition::new(id, 0),
            RichPosition::new(id, 6),
        ));
        assert!(editor.apply_format(&FormatCommand::Bold));
        editor.selection = Some(RichSelection::new(
            RichPosition::new(id, 1),
            RichPosition::new(id, 5),
        ));
        assert!(editor.apply_format(&FormatCommand::Italic));
        editor.selection = Some(RichSelection::new(
            RichPosition::new(id, 2),
            RichPosition::new(id, 4),
        ));
        assert!(editor.apply_format(&FormatCommand::Strikethrough));

        let projection = editor.projection();
        let ProjectedBlockKind::Paragraph { content } = &projection.blocks[0].kind else {
            panic!("paragraph")
        };
        assert_eq!(
            content
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            "abcdef"
        );
        assert!(content.iter().all(|span| span.marks.bold));
        assert!(
            content
                .iter()
                .any(|span| span.marks.bold && span.marks.italic)
        );
        assert!(
            content
                .iter()
                .any(|span| { span.marks.bold && span.marks.italic && span.marks.strikethrough })
        );
        assert!(
            content
                .iter()
                .all(|span| { !span.text.contains("**") && !span.text.contains("~~") })
        );

        // Remove the final grapheme of the innermost formatted range. No
        // delimiter can be exposed because delimiters do not exist in memory.
        editor.selection = Some(RichSelection::new(
            RichPosition::new(id, 3),
            RichPosition::new(id, 4),
        ));
        assert!(editor.delete_selection());
        assert_eq!(editor.plain_text(), "abcef");
        assert_single_paragraph(&editor);
        assert!(editor.projection().blocks.iter().all(|block| {
            match &block.kind {
                ProjectedBlockKind::Paragraph { content } => content
                    .iter()
                    .all(|span| !span.text.contains("**") && !span.text.contains("~~")),
                _ => false,
            }
        }));
    }

    #[test]
    fn cross_container_edits_preserve_opaque_nodes_and_reject_opaque_positions() {
        let source = "alpha\n\n<section data-x=\"keep\">raw</section>\n\nomega";
        let imported = MarkdownCodec::import(source).unwrap();
        let mut editor = RichEditor::new(imported.document);
        assert_eq!(editor.document.blocks.len(), 3);
        let first_id = editor.document.blocks[0].id;
        let opaque_id = editor.document.blocks[1].id;
        let second_id = editor.document.blocks[2].id;
        assert!(matches!(
            &editor.document.blocks[1].kind,
            RichBlockKind::OpaqueMarkdown { .. }
        ));

        let original_cursor = editor.cursor;
        assert!(!editor.move_to(RichPosition::new(opaque_id, 0), false));
        assert_eq!(editor.cursor, original_cursor);

        editor.selection = Some(RichSelection::new(
            RichPosition::new(first_id, 2),
            RichPosition::new(second_id, 3),
        ));
        assert!(editor.apply_format(&FormatCommand::Bold));
        assert!(matches!(
            &editor.document.blocks[1].kind,
            RichBlockKind::OpaqueMarkdown { raw, .. }
                if raw == "<section data-x=\"keep\">raw</section>"
        ));

        editor.selection = Some(RichSelection::new(
            RichPosition::new(first_id, 4),
            RichPosition::new(second_id, 1),
        ));
        assert!(editor.delete_selection());
        assert_eq!(container_text(&editor.document, first_id), "alph");
        assert_eq!(container_text(&editor.document, second_id), "mega");
        assert!(matches!(
            &editor.document.blocks[1].kind,
            RichBlockKind::OpaqueMarkdown { raw, .. }
                if raw == "<section data-x=\"keep\">raw</section>"
        ));
    }

    #[test]
    fn typing_table_syntax_inside_a_cell_does_not_add_columns_or_rows() {
        let mut editor = RichEditor::new(RichDocument::new());
        assert!(editor.apply_format(&FormatCommand::Table {
            columns: 3,
            rows: 2,
        }));
        let table_id = editor.document.blocks[0].id;
        for key in ["|", " ", "#", " ", "`", "-", "-", "-", " ", "|"] {
            assert!(editor.insert_text(key));
        }

        let RichBlockKind::Table { header, rows, .. } = &editor.document.blocks[0].kind else {
            panic!("table")
        };
        assert_eq!(editor.document.blocks[0].id, table_id);
        assert_eq!(header.len(), 3);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.len() == 3));
        assert_eq!(header[0].content.plain_text(), "| # `--- |Column 1");
    }

    #[test]
    fn ordinary_rich_edits_and_render_projection_parse_markdown_zero_times() {
        let imported = MarkdownCodec::import("editable text").unwrap();
        let mut editor = RichEditor::new(imported.document);
        reset_parse_count_for_tests();

        let id = editor.cursor.unwrap().container_id;
        assert!(editor.move_to(RichPosition::new(id, 13), false));
        for key in ["*", "#", "`", "|", " ", "👨‍👩‍👧‍👦"] {
            assert!(editor.insert_text(key));
            let _ = editor.projection();
            let _ = editor.plain_text();
        }
        editor.selection = Some(RichSelection::new(
            RichPosition::new(id, 0),
            RichPosition::new(id, 8),
        ));
        assert!(editor.apply_format(&FormatCommand::Bold));
        editor.move_cursor(CursorMove::Left, false);
        assert!(editor.backspace());
        let _ = MarkdownCodec::export(&editor.document).unwrap();

        assert_eq!(parse_count_for_tests(), 0);
        let _ = MarkdownCodec::import("explicit boundary").unwrap();
        assert_eq!(parse_count_for_tests(), 1);
    }
}
