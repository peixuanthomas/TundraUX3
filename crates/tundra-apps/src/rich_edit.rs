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

    /// Returns whether the current non-empty selection touches at least one
    /// paragraph or heading that supports a block format.
    pub fn can_apply_block_format_to_selection(&self) -> bool {
        self.has_selection() && !self.selected_block_format_targets().is_empty()
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
        if normalized.contains('\n')
            && self.cursor.is_some_and(|cursor| {
                is_heading_container(&self.document.blocks, cursor.container_id)
            })
        {
            return self.insert_multiline_heading_text(&normalized);
        }
        self.insert_normalized_text(&normalized)
    }

    fn insert_normalized_text(&mut self, normalized: &str) -> bool {
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

    fn insert_multiline_heading_text(&mut self, normalized: &str) -> bool {
        let Some((heading_text, paragraph_text)) = normalized.split_once('\n') else {
            return self.insert_normalized_text(normalized);
        };
        if !heading_text.is_empty() && !self.insert_normalized_text(heading_text) {
            return false;
        }
        if !self.split_heading_at_cursor() {
            return false;
        }
        if !paragraph_text.is_empty() {
            self.insert_normalized_text(paragraph_text);
        }
        true
    }

    pub(crate) fn insert_newline(&mut self) -> bool {
        if self.has_selection() {
            self.delete_selection();
        }
        if self.split_heading_at_cursor() {
            return true;
        }
        self.insert_normalized_text("\n")
    }

    /// Enter inside a heading creates a paragraph at the caret boundary
    /// instead of inserting an inline soft break into the heading container.
    fn split_heading_at_cursor(&mut self) -> bool {
        let Some(position) = self.cursor else {
            return false;
        };
        if !is_heading_container(&self.document.blocks, position.container_id) {
            return false;
        }

        let heading_len = container_len(&self.document, position.container_id).unwrap_or(0);
        let paragraph_id = if heading_len == 0 {
            position.container_id
        } else {
            self.document.allocate_node_id()
        };
        let block_separator = self.document.preferred_line_ending.as_str().repeat(2);
        if !split_heading_block(
            &mut self.document.blocks,
            position.container_id,
            position.grapheme_offset,
            paragraph_id,
            &block_separator,
        ) {
            return false;
        }

        self.document.mark_containing_block_dirty(paragraph_id);
        self.cursor = Some(RichPosition::new(paragraph_id, 0));
        self.selection = None;
        self.preferred_column = None;
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
            if let Some(heading_id) = remove_empty_paragraph_before_heading(
                &mut self.document.blocks,
                position.container_id,
            ) {
                self.cursor = Some(RichPosition::new(heading_id, 0));
                self.selection = None;
                self.preferred_column = None;
                return true;
            }
            let containers = self.ordered_containers();
            let Some(index) = containers
                .iter()
                .position(|container| container.id == position.container_id)
            else {
                return false;
            };
            if index == 0 {
                return false;
            }
            let previous = containers[index - 1];
            if merge_heading_with_following_paragraph(
                &mut self.document.blocks,
                previous.id,
                position.container_id,
            ) {
                self.document.mark_containing_block_dirty(previous.id);
                self.cursor = Some(RichPosition::new(previous.id, previous.len));
                self.selection = None;
                self.preferred_column = None;
                return true;
            }
            if previous.len == 0 {
                return false;
            }
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
            FormatCommand::InlineCode => self.toggle_code_on_current_line(),
            FormatCommand::Heading(level) => self.set_heading(*level),
            FormatCommand::Paragraph => self.set_paragraph(),
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

    /// Inline code is exposed as a line-oriented toolbar action. Preserve a
    /// precise selection when it stays on the caret's logical line, but never
    /// let a cross-line selection carry code formatting into adjacent lines.
    fn toggle_code_on_current_line(&mut self) -> bool {
        let Some(cursor) = self.cursor else {
            return false;
        };
        let Some(content) = find_inline(&self.document.blocks, cursor.container_id) else {
            return false;
        };
        let text = content.plain_text();
        if let Some(selection) = self.selection.filter(|selection| {
            !selection.is_collapsed()
                && selection.anchor.container_id == cursor.container_id
                && selection.focus.container_id == cursor.container_id
        }) {
            let start = selection
                .anchor
                .grapheme_offset
                .min(selection.focus.grapheme_offset);
            let end = selection
                .anchor
                .grapheme_offset
                .max(selection.focus.grapheme_offset);
            let selected_line_end = logical_line_end(&text, start);
            let trailing_break_end = selected_line_end
                .saturating_add((selected_line_end < content.grapheme_len()) as usize);
            if end <= trailing_break_end && start < selected_line_end {
                self.selection = Some(RichSelection::new(
                    RichPosition::new(cursor.container_id, start),
                    RichPosition::new(cursor.container_id, end.min(selected_line_end)),
                ));
                return self.toggle_mark(MarkKind::Code);
            }
        }

        let line_start = logical_line_start(&text, cursor.grapheme_offset);
        let line_end = logical_line_end(&text, cursor.grapheme_offset);
        if line_start == line_end {
            return false;
        }
        self.selection = Some(RichSelection::new(
            RichPosition::new(cursor.container_id, line_start),
            RichPosition::new(cursor.container_id, line_end),
        ));
        self.toggle_mark(MarkKind::Code)
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
        let Some(id) = self.isolate_cursor_line() else {
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

    fn set_paragraph(&mut self) -> bool {
        let Some(id) = self.isolate_cursor_line() else {
            return false;
        };
        let extracted = self.extract_current_top_level_line(id);
        let changed = change_block_kind(&mut self.document.blocks, id, |kind| match kind {
            RichBlockKind::Heading { content, .. } => Some(RichBlockKind::Paragraph { content }),
            other => Some(other),
        });
        if changed {
            self.document.mark_containing_block_dirty(id);
        }
        extracted || changed
    }

    /// Splits the caret's visual/logical line out of an inline container while
    /// retaining the target container id. Block formatting can then operate on
    /// exactly that line without changing its neighbors.
    fn isolate_cursor_line(&mut self) -> Option<NodeId> {
        let cursor = self.cursor?;
        let content = find_inline(&self.document.blocks, cursor.container_id)?;
        let text = content.plain_text();
        let line_start = logical_line_start(&text, cursor.grapheme_offset);
        let line_end = logical_line_end(&text, cursor.grapheme_offset);
        let len = content.grapheme_len();
        self.selection = None;
        self.preferred_column = None;
        if line_start == 0 && line_end == len {
            return Some(cursor.container_id);
        }

        let before_id = (line_start > 0).then(|| self.document.allocate_node_id());
        let after_id = (line_end < len).then(|| self.document.allocate_node_id());
        if !split_inline_block_line(
            &mut self.document.blocks,
            cursor.container_id,
            line_start,
            line_end,
            before_id,
            after_id,
        ) {
            return None;
        }
        self.cursor = Some(RichPosition::new(
            cursor.container_id,
            cursor.grapheme_offset.saturating_sub(line_start),
        ));
        self.document
            .mark_containing_block_dirty(cursor.container_id);
        Some(cursor.container_id)
    }

    fn current_top_level_line_context(&self, id: NodeId) -> Option<TopLevelLineContext> {
        self.document
            .blocks
            .iter()
            .find_map(|block| match &block.kind {
                RichBlockKind::Quote { blocks } if blocks.iter().any(|nested| nested.id == id) => {
                    Some(TopLevelLineContext::Quote)
                }
                RichBlockKind::List { kind, items, .. }
                    if items
                        .iter()
                        .any(|item| item.blocks.iter().any(|nested| nested.id == id)) =>
                {
                    Some(TopLevelLineContext::List(*kind))
                }
                _ if block.id == id => Some(TopLevelLineContext::Direct),
                _ => None,
            })
    }

    /// Pulls one isolated line out of a top-level quote or list, splitting the
    /// surrounding container so preceding and following lines retain their
    /// original format.
    fn extract_current_top_level_line(&mut self, id: NodeId) -> bool {
        let Some(index) = self.document.blocks.iter().position(|block| {
            matches!(
                &block.kind,
                RichBlockKind::Quote { blocks }
                    if blocks.iter().any(|nested| nested.id == id)
            ) || matches!(
                &block.kind,
                RichBlockKind::List { items, .. }
                    if items
                        .iter()
                        .any(|item| item.blocks.iter().any(|nested| nested.id == id))
            )
        }) else {
            return false;
        };

        let wrapper_ids = [
            self.document.allocate_node_id(),
            self.document.allocate_node_id(),
        ];
        let split_item_id = self.document.allocate_node_id();
        let current = self.document.blocks.remove(index);
        let Some(mut replacements) =
            extract_line_from_top_level_container(current.clone(), id, wrapper_ids, split_item_id)
        else {
            self.document.blocks.insert(index, current);
            return false;
        };
        if let Some(first) = replacements.first_mut() {
            first.leading_trivia = current.leading_trivia;
        }
        self.document.blocks.splice(index..index, replacements);
        self.document.mark_containing_block_dirty(id);
        true
    }

    fn selected_block_format_targets(&self) -> Vec<NodeId> {
        let Some(selection) = self.selection.filter(|selection| !selection.is_collapsed()) else {
            return Vec::new();
        };
        let containers = self.ordered_containers();
        let Some(anchor_index) = containers
            .iter()
            .position(|container| container.id == selection.anchor.container_id)
        else {
            return Vec::new();
        };
        let Some(focus_index) = containers
            .iter()
            .position(|container| container.id == selection.focus.container_id)
        else {
            return Vec::new();
        };
        let (start, end, start_index, end_index) = if anchor_index < focus_index
            || (anchor_index == focus_index
                && selection.anchor.grapheme_offset <= selection.focus.grapheme_offset)
        {
            (selection.anchor, selection.focus, anchor_index, focus_index)
        } else {
            (selection.focus, selection.anchor, focus_index, anchor_index)
        };
        containers[start_index..=end_index]
            .iter()
            .enumerate()
            .filter_map(|(relative, container)| {
                let index = start_index + relative;
                let selected = if start_index == end_index {
                    true
                } else if index == start_index {
                    start.grapheme_offset < container.len
                } else if index == end_index {
                    end.grapheme_offset > 0
                } else {
                    true
                };
                (selected && is_block_format_container(&self.document.blocks, container.id))
                    .then_some(container.id)
            })
            .collect()
    }

    fn toggle_quote(&mut self) -> bool {
        let Some(id) = self.isolate_cursor_line() else {
            return false;
        };
        if matches!(
            self.current_top_level_line_context(id),
            Some(TopLevelLineContext::Quote)
        ) {
            return self.extract_current_top_level_line(id);
        }
        self.extract_current_top_level_line(id);
        let Some(index) = self.document.blocks.iter().position(|block| block.id == id) else {
            return false;
        };
        let current = self.document.blocks.remove(index);
        let nested = RichBlock {
            id: current.id,
            leading_trivia: String::new(),
            original_raw: None,
            rewrite: RewriteState::Dirty,
            kind: current.kind,
        };
        let wrapper_id = self.document.allocate_node_id();
        self.document.blocks.insert(
            index,
            RichBlock {
                id: wrapper_id,
                leading_trivia: current.leading_trivia,
                original_raw: None,
                rewrite: RewriteState::Dirty,
                kind: RichBlockKind::Quote {
                    blocks: vec![nested],
                },
            },
        );
        true
    }

    fn toggle_list(&mut self, target: RichListKind) -> bool {
        let Some(id) = self.isolate_cursor_line() else {
            return false;
        };
        if matches!(
            self.current_top_level_line_context(id),
            Some(TopLevelLineContext::List(kind)) if kind == target
        ) {
            return self.extract_current_top_level_line(id);
        }
        self.extract_current_top_level_line(id);
        let Some(index) = self.document.blocks.iter().position(|block| block.id == id) else {
            return false;
        };
        let current = self.document.blocks.remove(index);
        if !matches!(
            current.kind,
            RichBlockKind::Paragraph { .. } | RichBlockKind::Heading { .. }
        ) {
            self.document.blocks.insert(index, current);
            return false;
        }
        let leading = current.leading_trivia;
        let item_id = self.document.allocate_node_id();
        let wrapper_id = self.document.allocate_node_id();
        let nested = RichBlock {
            id: current.id,
            leading_trivia: String::new(),
            original_raw: None,
            rewrite: RewriteState::Dirty,
            kind: current.kind,
        };
        self.document.blocks.insert(
            index,
            RichBlock {
                id: wrapper_id,
                leading_trivia: leading,
                original_raw: None,
                rewrite: RewriteState::Dirty,
                kind: RichBlockKind::List {
                    kind: target,
                    start: 1,
                    tight: true,
                    items: vec![RichListItem {
                        id: item_id,
                        checked: (target == RichListKind::Task).then_some(false),
                        blocks: vec![nested],
                    }],
                },
            },
        );
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TopLevelLineContext {
    Direct,
    Quote,
    List(RichListKind),
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

#[derive(Debug, Clone, Copy)]
enum InlineBlockStyle {
    Paragraph,
    Heading(u8),
}

fn split_inline_block_line(
    blocks: &mut Vec<RichBlock>,
    id: NodeId,
    line_start: usize,
    line_end: usize,
    before_id: Option<NodeId>,
    after_id: Option<NodeId>,
) -> bool {
    for index in 0..blocks.len() {
        if blocks[index].id == id {
            let current = blocks.remove(index);
            let (style, content) = match current.kind {
                RichBlockKind::Paragraph { content } => (InlineBlockStyle::Paragraph, content),
                RichBlockKind::Heading { level, content } => {
                    (InlineBlockStyle::Heading(level), content)
                }
                kind => {
                    blocks.insert(index, RichBlock { kind, ..current });
                    return false;
                }
            };
            let atoms = inline_atoms(&content);
            let start = line_start.min(atoms.len());
            let end = line_end.min(atoms.len()).max(start);
            let before_end =
                start.saturating_sub((start > 0 && is_break_atom(&atoms[start - 1])) as usize);
            let after_start =
                end.saturating_add((end < atoms.len() && is_break_atom(&atoms[end])) as usize);
            let mut replacements = Vec::with_capacity(3);
            if let Some(before_id) = before_id {
                replacements.push(inline_line_block(
                    before_id,
                    style,
                    atoms[..before_end].to_vec(),
                ));
            }
            replacements.push(inline_line_block(id, style, atoms[start..end].to_vec()));
            if let Some(after_id) = after_id {
                replacements.push(inline_line_block(
                    after_id,
                    style,
                    atoms[after_start.min(atoms.len())..].to_vec(),
                ));
            }
            if let Some(first) = replacements.first_mut() {
                first.leading_trivia = current.leading_trivia;
            }
            blocks.splice(index..index, replacements);
            return true;
        }

        match &mut blocks[index].kind {
            RichBlockKind::Quote { blocks } => {
                if split_inline_block_line(blocks, id, line_start, line_end, before_id, after_id) {
                    return true;
                }
            }
            RichBlockKind::List { items, .. } => {
                for item in items {
                    if split_inline_block_line(
                        &mut item.blocks,
                        id,
                        line_start,
                        line_end,
                        before_id,
                        after_id,
                    ) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn is_break_atom(atom: &Atom) -> bool {
    matches!(atom, Atom::SoftBreak | Atom::HardBreak)
}

fn inline_line_block(id: NodeId, style: InlineBlockStyle, atoms: Vec<Atom>) -> RichBlock {
    let content = atoms_to_inline(atoms);
    let kind = match style {
        InlineBlockStyle::Paragraph => RichBlockKind::Paragraph { content },
        InlineBlockStyle::Heading(level) => RichBlockKind::Heading { level, content },
    };
    RichBlock {
        id,
        leading_trivia: String::new(),
        original_raw: None,
        rewrite: RewriteState::Dirty,
        kind,
    }
}

fn extract_line_from_top_level_container(
    current: RichBlock,
    id: NodeId,
    wrapper_ids: [NodeId; 2],
    split_item_id: NodeId,
) -> Option<Vec<RichBlock>> {
    match current.kind {
        RichBlockKind::Quote { mut blocks } => {
            let index = blocks.iter().position(|block| block.id == id)?;
            let after = blocks.split_off(index + 1);
            let mut target = blocks.pop()?;
            target.leading_trivia.clear();
            target.original_raw = None;
            target.rewrite = RewriteState::Dirty;

            let mut replacements = Vec::with_capacity(3);
            if !blocks.is_empty() {
                replacements.push(quote_block(wrapper_ids[0], blocks));
            }
            replacements.push(target);
            if !after.is_empty() {
                replacements.push(quote_block(wrapper_ids[1], after));
            }
            Some(replacements)
        }
        RichBlockKind::List {
            kind,
            start,
            tight,
            mut items,
        } => {
            let (item_index, block_index) =
                items.iter().enumerate().find_map(|(item, candidate)| {
                    candidate
                        .blocks
                        .iter()
                        .position(|block| block.id == id)
                        .map(|block| (item, block))
                })?;
            let mut after_items = items.split_off(item_index + 1);
            let mut target_item = items.pop()?;
            let mut after_blocks = target_item.blocks.split_off(block_index + 1);
            let mut target = target_item.blocks.pop()?;
            target.leading_trivia.clear();
            target.original_raw = None;
            target.rewrite = RewriteState::Dirty;

            if !target_item.blocks.is_empty() {
                items.push(RichListItem {
                    id: target_item.id,
                    checked: target_item.checked,
                    blocks: target_item.blocks,
                });
            }
            let prefix_len = items.len();
            if !after_blocks.is_empty() {
                after_items.insert(
                    0,
                    RichListItem {
                        id: if prefix_len > item_index {
                            split_item_id
                        } else {
                            target_item.id
                        },
                        checked: target_item.checked,
                        blocks: std::mem::take(&mut after_blocks),
                    },
                );
            }

            let mut replacements = Vec::with_capacity(3);
            if !items.is_empty() {
                replacements.push(list_block(wrapper_ids[0], kind, start, tight, items));
            }
            replacements.push(target);
            if !after_items.is_empty() {
                replacements.push(list_block(
                    wrapper_ids[1],
                    kind,
                    start.saturating_add(prefix_len).saturating_add(1),
                    tight,
                    after_items,
                ));
            }
            Some(replacements)
        }
        _ => None,
    }
}

fn quote_block(id: NodeId, blocks: Vec<RichBlock>) -> RichBlock {
    RichBlock {
        id,
        leading_trivia: String::new(),
        original_raw: None,
        rewrite: RewriteState::Dirty,
        kind: RichBlockKind::Quote { blocks },
    }
}

fn list_block(
    id: NodeId,
    kind: RichListKind,
    start: usize,
    tight: bool,
    items: Vec<RichListItem>,
) -> RichBlock {
    RichBlock {
        id,
        leading_trivia: String::new(),
        original_raw: None,
        rewrite: RewriteState::Dirty,
        kind: RichBlockKind::List {
            kind,
            start,
            tight,
            items,
        },
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

fn is_heading_container(blocks: &[RichBlock], id: NodeId) -> bool {
    for block in blocks {
        if block.id == id {
            return matches!(&block.kind, RichBlockKind::Heading { .. });
        }
        match &block.kind {
            RichBlockKind::Quote { blocks } => {
                if is_heading_container(blocks, id) {
                    return true;
                }
            }
            RichBlockKind::List { items, .. } => {
                if items
                    .iter()
                    .any(|item| is_heading_container(&item.blocks, id))
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn split_heading_block(
    blocks: &mut Vec<RichBlock>,
    id: NodeId,
    grapheme_offset: usize,
    paragraph_id: NodeId,
    block_separator: &str,
) -> bool {
    for index in 0..blocks.len() {
        if blocks[index].id == id {
            let RichBlockKind::Heading { content, .. } = &mut blocks[index].kind else {
                return false;
            };
            let mut left = inline_atoms(content);
            let split_at = grapheme_offset.min(left.len());
            if left.is_empty() {
                blocks[index].kind = RichBlockKind::Paragraph {
                    content: InlineContent::default(),
                };
                blocks[index].original_raw = None;
                blocks[index].rewrite = RewriteState::Dirty;
                return true;
            }
            if split_at == 0 {
                let leading_trivia = std::mem::take(&mut blocks[index].leading_trivia);
                blocks[index].leading_trivia = block_separator.to_owned();
                blocks.insert(
                    index,
                    RichBlock {
                        id: paragraph_id,
                        leading_trivia,
                        original_raw: None,
                        rewrite: RewriteState::Dirty,
                        kind: RichBlockKind::Paragraph {
                            content: InlineContent::default(),
                        },
                    },
                );
                return true;
            }
            let right = left.split_off(split_at);
            *content = atoms_to_inline(left);
            blocks[index].original_raw = None;
            blocks[index].rewrite = RewriteState::Dirty;
            blocks.insert(
                index + 1,
                RichBlock::new(
                    paragraph_id,
                    RichBlockKind::Paragraph {
                        content: atoms_to_inline(right),
                    },
                ),
            );
            return true;
        }

        match &mut blocks[index].kind {
            RichBlockKind::Quote { blocks } => {
                if split_heading_block(blocks, id, grapheme_offset, paragraph_id, block_separator) {
                    return true;
                }
            }
            RichBlockKind::List { items, .. } => {
                for item in items {
                    if split_heading_block(
                        &mut item.blocks,
                        id,
                        grapheme_offset,
                        paragraph_id,
                        block_separator,
                    ) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn merge_heading_with_following_paragraph(
    blocks: &mut Vec<RichBlock>,
    heading_id: NodeId,
    paragraph_id: NodeId,
) -> bool {
    for index in 0..blocks.len() {
        if index + 1 < blocks.len()
            && blocks[index].id == heading_id
            && blocks[index + 1].id == paragraph_id
            && matches!(&blocks[index].kind, RichBlockKind::Heading { .. })
            && matches!(&blocks[index + 1].kind, RichBlockKind::Paragraph { .. })
        {
            let RichBlockKind::Paragraph { content: right } =
                std::mem::replace(&mut blocks[index + 1].kind, RichBlockKind::Rule)
            else {
                unreachable!("paragraph kind checked above");
            };
            let RichBlockKind::Heading { content: left, .. } = &mut blocks[index].kind else {
                unreachable!("heading kind checked above");
            };
            let mut atoms = inline_atoms(left);
            atoms.extend(inline_atoms(&right));
            *left = atoms_to_inline(atoms);
            blocks[index].original_raw = None;
            blocks[index].rewrite = RewriteState::Dirty;
            blocks.remove(index + 1);
            return true;
        }

        match &mut blocks[index].kind {
            RichBlockKind::Quote { blocks } => {
                if merge_heading_with_following_paragraph(blocks, heading_id, paragraph_id) {
                    return true;
                }
            }
            RichBlockKind::List { items, .. } => {
                for item in items {
                    if merge_heading_with_following_paragraph(
                        &mut item.blocks,
                        heading_id,
                        paragraph_id,
                    ) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn remove_empty_paragraph_before_heading(
    blocks: &mut Vec<RichBlock>,
    paragraph_id: NodeId,
) -> Option<NodeId> {
    for index in 0..blocks.len() {
        if index + 1 < blocks.len()
            && blocks[index].id == paragraph_id
            && matches!(
                &blocks[index].kind,
                RichBlockKind::Paragraph { content } if content.grapheme_len() == 0
            )
            && matches!(&blocks[index + 1].kind, RichBlockKind::Heading { .. })
        {
            let heading_id = blocks[index + 1].id;
            let leading_trivia = std::mem::take(&mut blocks[index].leading_trivia);
            blocks[index + 1].leading_trivia = leading_trivia;
            blocks.remove(index);
            return Some(heading_id);
        }

        match &mut blocks[index].kind {
            RichBlockKind::Quote { blocks } => {
                if let Some(heading_id) =
                    remove_empty_paragraph_before_heading(blocks, paragraph_id)
                {
                    return Some(heading_id);
                }
            }
            RichBlockKind::List { items, .. } => {
                for item in items {
                    if let Some(heading_id) =
                        remove_empty_paragraph_before_heading(&mut item.blocks, paragraph_id)
                    {
                        return Some(heading_id);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn is_block_format_container(blocks: &[RichBlock], id: NodeId) -> bool {
    for block in blocks {
        if block.id == id {
            return matches!(
                &block.kind,
                RichBlockKind::Paragraph { .. } | RichBlockKind::Heading { .. }
            );
        }
        match &block.kind {
            RichBlockKind::Quote { blocks } => {
                if is_block_format_container(blocks, id) {
                    return true;
                }
            }
            RichBlockKind::List { items, .. } => {
                if items
                    .iter()
                    .any(|item| is_block_format_container(&item.blocks, id))
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
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
