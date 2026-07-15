//! Import/export boundary between Markdown files and the rich editor model.
//!
//! Markdown is parsed only when a file is opened or Source mode is explicitly
//! converted back to Rich mode. Rendering and ordinary Rich edits operate on
//! [`RichDocument`] directly.

use std::ops::Range;

use comrak::nodes::{AstNode, ListType, NodeValue, TableAlignment as ComrakTableAlignment};
use comrak::{Arena, Options, parse_document};
use unicode_segmentation::UnicodeSegmentation;

#[cfg(test)]
thread_local! {
    /// Test-only instrumentation proving that Rich editing and projection do
    /// not cross the Markdown parsing boundary. A thread-local counter keeps
    /// parallel unit tests independent.
    static MARKDOWN_PARSE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

use crate::rich_document::{
    InlineContent, InlineMarks, InlineNode, LinkAttributes, NodeId, RewriteState, RichBlock,
    RichBlockKind, RichDocument, RichLineEnding, RichListItem, RichListKind, RichPosition,
    RichTableAlignment, RichTableCell, RichText,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownPositionEntry {
    pub rich: RichPosition,
    pub source_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MarkdownPositionMap {
    pub entries: Vec<MarkdownPositionEntry>,
}

impl MarkdownPositionMap {
    pub fn source_offset_for(&self, position: RichPosition) -> Option<usize> {
        self.entries
            .iter()
            .filter(|entry| entry.rich.container_id == position.container_id)
            .min_by_key(|entry| {
                entry
                    .rich
                    .grapheme_offset
                    .abs_diff(position.grapheme_offset)
            })
            .map(|entry| entry.source_offset)
    }

    pub fn rich_position_for(&self, source_offset: usize) -> Option<RichPosition> {
        self.entries
            .iter()
            .min_by_key(|entry| {
                (
                    entry.source_offset.abs_diff(source_offset),
                    usize::from(entry.source_offset > source_offset),
                )
            })
            .map(|entry| entry.rich)
    }

    fn record(&mut self, rich: RichPosition, source_offset: usize) {
        if let Some(existing) = self.entries.iter_mut().find(|entry| entry.rich == rich) {
            // A logical boundary shared by two differently styled inline nodes
            // has two reasonable source affinities. Prefer the later one: it is
            // after the previous node's closing markers and the next node's
            // opening markers, so Source-mode insertion keeps the latter style.
            existing.source_offset = existing.source_offset.max(source_offset);
        } else {
            self.entries.push(MarkdownPositionEntry {
                rich,
                source_offset,
            });
        }
    }

    fn append_shifted(&mut self, other: &Self, shift: usize) {
        for entry in &other.entries {
            self.record(entry.rich, entry.source_offset.saturating_add(shift));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownImport {
    pub document: RichDocument,
    pub positions: MarkdownPositionMap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownExport {
    pub markdown: String,
    pub positions: MarkdownPositionMap,
    block_fragments: Vec<String>,
}

impl MarkdownExport {
    pub fn to_bytes(&self, utf8_bom: bool) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.markdown.len() + usize::from(utf8_bom) * 3);
        if utf8_bom {
            bytes.extend_from_slice(b"\xEF\xBB\xBF");
        }
        bytes.extend_from_slice(self.markdown.as_bytes());
        bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownCodecError {
    message: String,
}

impl MarkdownCodecError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for MarkdownCodecError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for MarkdownCodecError {}

#[derive(Debug, Clone, Copy, Default)]
pub struct MarkdownCodec;

impl MarkdownCodec {
    pub fn import(source: &str) -> Result<MarkdownImport, MarkdownCodecError> {
        Self::import_with_metadata(source, false, detect_line_ending(source))
    }

    pub fn import_with_metadata(
        source: &str,
        utf8_bom: bool,
        preferred_line_ending: RichLineEnding,
    ) -> Result<MarkdownImport, MarkdownCodecError> {
        #[cfg(test)]
        MARKDOWN_PARSE_COUNT.with(|count| count.set(count.get().saturating_add(1)));
        let arena = Arena::new();
        let options = markdown_options();
        let root = parse_document(&arena, source, &options);
        let mut builder = ImportBuilder {
            document: RichDocument::new(),
            positions: MarkdownPositionMap::default(),
        };
        builder.document.utf8_bom = utf8_bom;
        builder.document.preferred_line_ending = preferred_line_ending;

        let mut previous_end = 0usize;
        for node in root.children() {
            let range = node_source_range(node, source);
            if range.start < previous_end || range.end > source.len() {
                return Err(MarkdownCodecError::invalid(
                    "Markdown parser returned an invalid top-level source range",
                ));
            }
            let leading_trivia = source[previous_end..range.start].to_owned();
            let raw = source[range.clone()].to_owned();
            let id = builder.document.allocate_node_id();
            let kind = builder.import_block_kind(node).unwrap_or_else(|reason| {
                RichBlockKind::OpaqueMarkdown {
                    raw: raw.clone(),
                    reason,
                }
            });
            let block = RichBlock::imported(id, leading_trivia, raw, kind);
            record_block_positions_from_ast(&block, node, source, &mut builder.positions);
            builder.document.blocks.push(block);
            previous_end = range.end;
        }
        builder.document.trailing_trivia = source[previous_end..].to_owned();
        builder.document.repair_node_id_allocator();
        Ok(MarkdownImport {
            document: builder.document,
            positions: builder.positions,
        })
    }

    pub fn export(document: &RichDocument) -> Result<MarkdownExport, MarkdownCodecError> {
        let newline = document.preferred_line_ending.as_str();
        let mut markdown = String::new();
        let mut block_fragments = Vec::with_capacity(document.blocks.len());
        let mut positions = MarkdownPositionMap::default();

        for (index, block) in document.blocks.iter().enumerate() {
            if index > 0 && block.leading_trivia.is_empty() {
                ensure_blank_block_separator(&mut markdown, newline);
            } else {
                markdown.push_str(&block.leading_trivia);
            }
            let source_start = markdown.len();
            let (fragment, local_positions) = if block.rewrite == RewriteState::Clean {
                if let Some(raw) = block.original_raw.clone() {
                    let mut local_positions = MarkdownPositionMap::default();
                    record_block_positions_lexically(block, &raw, &mut local_positions);
                    (raw, local_positions)
                } else {
                    let serialized = serialize_block_mapped(block, newline);
                    (serialized.text, serialized.positions)
                }
            } else {
                let serialized = serialize_block_mapped(block, newline);
                (serialized.text, serialized.positions)
            };
            positions.append_shifted(&local_positions, source_start);
            markdown.push_str(&fragment);
            block_fragments.push(fragment);
        }
        markdown.push_str(&document.trailing_trivia);
        Ok(MarkdownExport {
            markdown,
            positions,
            block_fragments,
        })
    }

    /// Rebases round-trip preservation data after the corresponding export was
    /// successfully written. This never reparses or replaces semantic nodes.
    pub fn accept_export(
        document: &mut RichDocument,
        export: &MarkdownExport,
    ) -> Result<(), MarkdownCodecError> {
        if document.blocks.len() != export.block_fragments.len() {
            return Err(MarkdownCodecError::invalid(
                "export does not belong to the current rich document",
            ));
        }
        for (block, raw) in document
            .blocks
            .iter_mut()
            .zip(export.block_fragments.iter())
        {
            block.original_raw = Some(raw.clone());
            block.rewrite = RewriteState::Clean;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn reset_parse_count_for_tests() {
    MARKDOWN_PARSE_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn parse_count_for_tests() -> usize {
    MARKDOWN_PARSE_COUNT.with(std::cell::Cell::get)
}

struct ImportBuilder {
    document: RichDocument,
    positions: MarkdownPositionMap,
}

impl ImportBuilder {
    fn import_block_kind<'a>(&mut self, node: &'a AstNode<'a>) -> Result<RichBlockKind, String> {
        let value = node.data().value.clone();
        match value {
            NodeValue::Paragraph | NodeValue::TaskItem(_) => Ok(RichBlockKind::Paragraph {
                content: self.import_inline_children(node)?,
            }),
            NodeValue::Heading(heading) => Ok(RichBlockKind::Heading {
                level: heading.level.clamp(1, 6),
                content: self.import_inline_children(node)?,
            }),
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => Ok(RichBlockKind::Quote {
                blocks: node
                    .children()
                    .map(|child| self.import_nested_block(child))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            NodeValue::CodeBlock(code) => Ok(RichBlockKind::CodeBlock {
                language: code
                    .info
                    .split_whitespace()
                    .next()
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned),
                code: code.literal,
            }),
            NodeValue::List(list) => {
                let mut items = Vec::new();
                for item in node.children() {
                    let item_id = self.document.allocate_node_id();
                    let checked = item.descendants().find_map(|descendant| {
                        let data = descendant.data();
                        match data.value {
                            NodeValue::TaskItem(task) => Some(task.symbol.is_some()),
                            _ => None,
                        }
                    });
                    let blocks = item
                        .children()
                        .map(|child| self.import_nested_block(child))
                        .collect::<Result<Vec<_>, _>>()?;
                    items.push(RichListItem {
                        id: item_id,
                        checked,
                        blocks,
                    });
                }
                let kind = if list.is_task_list || items.iter().any(|item| item.checked.is_some()) {
                    RichListKind::Task
                } else if list.list_type == ListType::Ordered {
                    RichListKind::Ordered
                } else {
                    RichListKind::Bullet
                };
                Ok(RichBlockKind::List {
                    kind,
                    start: list.start,
                    tight: list.tight,
                    items,
                })
            }
            NodeValue::Table(table) => {
                let alignments = table
                    .alignments
                    .iter()
                    .map(|alignment| match alignment {
                        ComrakTableAlignment::None => RichTableAlignment::None,
                        ComrakTableAlignment::Left => RichTableAlignment::Left,
                        ComrakTableAlignment::Center => RichTableAlignment::Center,
                        ComrakTableAlignment::Right => RichTableAlignment::Right,
                    })
                    .collect();
                let mut header = Vec::new();
                let mut rows = Vec::new();
                for row in node.children() {
                    let is_header = matches!(row.data().value, NodeValue::TableRow(true));
                    let mut cells = Vec::new();
                    for cell in row.children() {
                        let id = self.document.allocate_node_id();
                        let content = self.import_inline_children(cell)?;
                        cells.push(RichTableCell { id, content });
                    }
                    if is_header {
                        header = cells;
                    } else {
                        rows.push(cells);
                    }
                }
                Ok(RichBlockKind::Table {
                    alignments,
                    header,
                    rows,
                })
            }
            NodeValue::ThematicBreak => Ok(RichBlockKind::Rule),
            unsupported => Err(format!(
                "{} is preserved as read-only Markdown",
                unsupported.xml_node_name()
            )),
        }
    }

    fn import_nested_block<'a>(&mut self, node: &'a AstNode<'a>) -> Result<RichBlock, String> {
        let id = self.document.allocate_node_id();
        let kind = self.import_block_kind(node)?;
        Ok(RichBlock {
            id,
            leading_trivia: String::new(),
            original_raw: None,
            rewrite: RewriteState::Clean,
            kind,
        })
    }

    fn import_inline_children<'a>(
        &mut self,
        node: &'a AstNode<'a>,
    ) -> Result<InlineContent, String> {
        let mut nodes = Vec::new();
        for child in node.children() {
            self.import_inline_node(child, InlineMarks::default(), None, &mut nodes)?;
        }
        Ok(InlineContent(nodes))
    }

    fn import_inline_node<'a>(
        &mut self,
        node: &'a AstNode<'a>,
        marks: InlineMarks,
        link: Option<LinkAttributes>,
        output: &mut Vec<InlineNode>,
    ) -> Result<(), String> {
        let value = node.data().value.clone();
        match value {
            NodeValue::Text(text) => output.push(InlineNode::Text(RichText {
                text: text.into_owned(),
                marks,
                link,
            })),
            NodeValue::Code(code) => output.push(InlineNode::Text(RichText {
                text: code.literal,
                marks: InlineMarks {
                    code: true,
                    ..InlineMarks::default()
                },
                link,
            })),
            NodeValue::SoftBreak => output.push(InlineNode::SoftBreak),
            NodeValue::LineBreak => output.push(InlineNode::HardBreak),
            NodeValue::Strong => {
                let mut nested = marks;
                nested.bold = true;
                for child in node.children() {
                    self.import_inline_node(child, nested, link.clone(), output)?;
                }
            }
            NodeValue::Emph => {
                let mut nested = marks;
                nested.italic = true;
                for child in node.children() {
                    self.import_inline_node(child, nested, link.clone(), output)?;
                }
            }
            NodeValue::Strikethrough => {
                let mut nested = marks;
                nested.strikethrough = true;
                for child in node.children() {
                    self.import_inline_node(child, nested, link.clone(), output)?;
                }
            }
            NodeValue::Link(link_node) => {
                let nested_link = Some(LinkAttributes {
                    url: link_node.url,
                    title: (!link_node.title.is_empty()).then_some(link_node.title),
                });
                for child in node.children() {
                    self.import_inline_node(child, marks, nested_link.clone(), output)?;
                }
            }
            NodeValue::Image(link_node) => {
                let alt = node
                    .descendants()
                    .filter_map(|descendant| match descendant.data().value.clone() {
                        NodeValue::Text(text) => Some(text.into_owned()),
                        _ => None,
                    })
                    .collect::<String>();
                output.push(InlineNode::Image {
                    alt,
                    url: link_node.url,
                    title: (!link_node.title.is_empty()).then_some(link_node.title),
                });
            }
            NodeValue::TaskItem(_) => {
                for child in node.children() {
                    self.import_inline_node(child, marks, link.clone(), output)?;
                }
            }
            unsupported => {
                return Err(format!(
                    "inline {} cannot be edited safely",
                    unsupported.xml_node_name()
                ));
            }
        }
        Ok(())
    }
}

fn record_block_positions_from_ast<'a>(
    block: &RichBlock,
    node: &'a AstNode<'a>,
    source: &str,
    positions: &mut MarkdownPositionMap,
) {
    match (&block.kind, &node.data().value) {
        (RichBlockKind::Paragraph { content }, _) | (RichBlockKind::Heading { content, .. }, _) => {
            record_inline_container_from_ast(block.id, content, node, source, positions);
        }
        (RichBlockKind::Quote { blocks }, _) => {
            for (nested, child) in blocks.iter().zip(node.children()) {
                record_block_positions_from_ast(nested, child, source, positions);
            }
        }
        (RichBlockKind::CodeBlock { code, .. }, _) => {
            let range = code_content_search_range(node, source);
            record_display_text_boundaries(block.id, 0, code, range, source, positions);
        }
        (RichBlockKind::List { items, .. }, _) => {
            for (item, item_node) in items.iter().zip(node.children()) {
                for (nested, child) in item.blocks.iter().zip(item_node.children()) {
                    record_block_positions_from_ast(nested, child, source, positions);
                }
            }
        }
        (RichBlockKind::Table { header, rows, .. }, NodeValue::Table(_)) => {
            let mut ast_rows = node.children();
            if let Some(header_node) = ast_rows.next() {
                for (cell, cell_node) in header.iter().zip(header_node.children()) {
                    record_inline_container_from_ast(
                        cell.id,
                        &cell.content,
                        cell_node,
                        source,
                        positions,
                    );
                }
            }
            for (row, row_node) in rows.iter().zip(ast_rows) {
                for (cell, cell_node) in row.iter().zip(row_node.children()) {
                    record_inline_container_from_ast(
                        cell.id,
                        &cell.content,
                        cell_node,
                        source,
                        positions,
                    );
                }
            }
        }
        _ => {}
    }
}

fn record_inline_container_from_ast<'a>(
    container_id: NodeId,
    expected: &InlineContent,
    node: &'a AstNode<'a>,
    source: &str,
    positions: &mut MarkdownPositionMap,
) {
    let mut logical_offset = 0usize;
    for child in node.children() {
        record_inline_node_from_ast(container_id, child, source, &mut logical_offset, positions);
    }

    let expected_len = expected.grapheme_len();
    let container_range = node_source_range(node, source);
    if expected_len == 0 {
        positions.record(
            RichPosition::new(container_id, 0),
            editable_insertion_offset(source, container_range),
        );
        return;
    }

    // The outer inline container includes closing emphasis/link markers. Map
    // the final caret after them so a Source-mode insertion does not split a
    // delimiter pair. Interior boundaries still come from precise leaf
    // source positions.
    if logical_offset == expected_len {
        positions.record(
            RichPosition::new(container_id, expected_len),
            container_range.end,
        );
    }

    ensure_dense_container_boundaries(container_id, expected, source, container_range, positions);
}

fn record_inline_node_from_ast<'a>(
    container_id: NodeId,
    node: &'a AstNode<'a>,
    source: &str,
    logical_offset: &mut usize,
    positions: &mut MarkdownPositionMap,
) {
    match node.data().value.clone() {
        NodeValue::Text(text) => {
            let text = text.into_owned();
            let range = node_source_range(node, source);
            record_display_text_boundaries(
                container_id,
                *logical_offset,
                &text,
                range,
                source,
                positions,
            );
            *logical_offset = logical_offset.saturating_add(text.graphemes(true).count());
        }
        NodeValue::Code(code) => {
            let range = inline_code_content_search_range(node, source);
            record_display_text_boundaries(
                container_id,
                *logical_offset,
                &code.literal,
                range,
                source,
                positions,
            );
            *logical_offset = logical_offset.saturating_add(code.literal.graphemes(true).count());
        }
        NodeValue::SoftBreak | NodeValue::LineBreak => {
            let range = node_source_range(node, source);
            positions.record(
                RichPosition::new(container_id, *logical_offset),
                range.start,
            );
            *logical_offset = logical_offset.saturating_add(1);
            positions.record(RichPosition::new(container_id, *logical_offset), range.end);
        }
        NodeValue::Image(_) => {
            let range = node_source_range(node, source);
            positions.record(
                RichPosition::new(container_id, *logical_offset),
                range.start,
            );
            *logical_offset = logical_offset.saturating_add(1);
            positions.record(RichPosition::new(container_id, *logical_offset), range.end);
        }
        NodeValue::Strong
        | NodeValue::Emph
        | NodeValue::Strikethrough
        | NodeValue::Link(_)
        | NodeValue::TaskItem(_) => {
            for child in node.children() {
                record_inline_node_from_ast(container_id, child, source, logical_offset, positions);
            }
        }
        _ => {}
    }
}

fn record_display_text_boundaries(
    container_id: NodeId,
    logical_start: usize,
    display: &str,
    source_range: Range<usize>,
    source: &str,
    positions: &mut MarkdownPositionMap,
) {
    let range = normalize_range(source, source_range);
    let raw = &source[range.clone()];
    let graphemes = display.graphemes(true).collect::<Vec<_>>();
    if graphemes.is_empty() {
        positions.record(RichPosition::new(container_id, logical_start), range.start);
        return;
    }

    let fallback = proportional_boundaries(raw, graphemes.len());
    let mut search_from = 0usize;
    let mut first_boundary = None;
    for (index, grapheme) in graphemes.iter().enumerate() {
        let found = raw[search_from..]
            .find(grapheme)
            .map(|relative| search_from + relative)
            .filter(|position| raw.is_char_boundary(*position));
        let start = found.unwrap_or(fallback[index]).min(raw.len());
        if first_boundary.is_none() {
            first_boundary = Some(start);
            positions.record(
                RichPosition::new(container_id, logical_start),
                range.start + start,
            );
        }
        let end = found
            .map(|position| position.saturating_add(grapheme.len()))
            .unwrap_or(fallback[index + 1])
            .min(raw.len());
        positions.record(
            RichPosition::new(container_id, logical_start + index + 1),
            range.start + normalize_end_boundary(raw, end),
        );
        search_from = end;
    }
}

fn ensure_dense_container_boundaries(
    container_id: NodeId,
    content: &InlineContent,
    source: &str,
    source_range: Range<usize>,
    positions: &mut MarkdownPositionMap,
) {
    let range = normalize_range(source, source_range);
    let fallback = proportional_boundaries(&source[range.clone()], content.grapheme_len());
    for (offset, relative) in fallback.into_iter().enumerate() {
        let rich = RichPosition::new(container_id, offset);
        if !positions.entries.iter().any(|entry| entry.rich == rich) {
            positions.record(rich, range.start + relative);
        }
    }
}

fn proportional_boundaries(source: &str, count: usize) -> Vec<usize> {
    if count == 0 {
        return vec![0];
    }
    let valid = std::iter::once(0)
        .chain(source.char_indices().map(|(index, _)| index).skip(1))
        .chain(std::iter::once(source.len()))
        .collect::<Vec<_>>();
    (0..=count)
        .map(|index| {
            let candidate = index.saturating_mul(source.len()) / count;
            valid
                .iter()
                .copied()
                .min_by_key(|boundary| boundary.abs_diff(candidate))
                .unwrap_or(0)
        })
        .collect()
}

fn editable_insertion_offset(source: &str, range: Range<usize>) -> usize {
    let range = normalize_range(source, range);
    let raw = &source[range.clone()];
    range.start
        + raw
            .char_indices()
            .find_map(|(index, character)| (!character.is_whitespace()).then_some(index))
            .unwrap_or(raw.len())
}

fn inline_code_content_search_range(node: &AstNode<'_>, source: &str) -> Range<usize> {
    let range = node_source_range(node, source);
    let raw = &source[range.clone()];
    let fence_len = raw.bytes().take_while(|byte| *byte == b'`').count();
    if fence_len == 0 || raw.len() < fence_len.saturating_mul(2) {
        return range;
    }
    let mut start = fence_len;
    let mut end = raw.len().saturating_sub(fence_len);
    if raw[start..end].starts_with(' ') && raw[start..end].ends_with(' ') && end > start + 1 {
        start += 1;
        end -= 1;
    }
    (range.start + start)..(range.start + end)
}

fn code_content_search_range(node: &AstNode<'_>, source: &str) -> Range<usize> {
    let range = node_source_range(node, source);
    let raw = &source[range.clone()];
    let first_line_end = raw
        .find(['\n', '\r'])
        .map(|index| {
            if raw.as_bytes().get(index) == Some(&b'\r')
                && raw.as_bytes().get(index + 1) == Some(&b'\n')
            {
                index + 2
            } else {
                index + 1
            }
        })
        .unwrap_or(0);
    if raw.starts_with("```") || raw.starts_with("~~~") {
        range.start + first_line_end..range.end
    } else {
        range
    }
}

fn normalize_range(source: &str, range: Range<usize>) -> Range<usize> {
    let start = normalize_boundary(source, range.start);
    let end = normalize_end_boundary(source, range.end.max(start));
    start..end
}

fn markdown_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.footnotes = true;
    options
}

fn serialize_block(block: &RichBlock, newline: &str) -> String {
    let lf = match &block.kind {
        RichBlockKind::Paragraph { content } => serialize_inline(content),
        RichBlockKind::Heading { level, content } => {
            format!(
                "{} {}",
                "#".repeat(usize::from((*level).clamp(1, 6))),
                serialize_inline(content)
            )
        }
        RichBlockKind::Quote { blocks } => {
            let inner = blocks
                .iter()
                .map(|block| serialize_block(block, "\n"))
                .collect::<Vec<_>>()
                .join("\n\n");
            inner
                .split('\n')
                .map(|line| {
                    if line.is_empty() {
                        ">".to_owned()
                    } else {
                        format!("> {line}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        RichBlockKind::CodeBlock { language, code } => serialize_code_block(language, code),
        RichBlockKind::List {
            kind, start, items, ..
        } => serialize_list(*kind, *start, items),
        RichBlockKind::Table {
            alignments,
            header,
            rows,
        } => serialize_table(alignments, header, rows),
        RichBlockKind::Rule => "---".to_owned(),
        RichBlockKind::OpaqueMarkdown { raw, .. } => raw.clone(),
    };
    if newline == "\n" {
        lf
    } else {
        lf.replace('\n', newline)
    }
}

struct MappedBlock {
    text: String,
    positions: MarkdownPositionMap,
}

fn serialize_block_mapped(block: &RichBlock, newline: &str) -> MappedBlock {
    let text = serialize_block(block, newline);
    let mut positions = MarkdownPositionMap::default();
    record_block_positions_lexically(block, &text, &mut positions);
    MappedBlock { text, positions }
}

enum EditableContainer<'a> {
    Inline(NodeId, &'a InlineContent),
    Code(NodeId, &'a str),
}

impl EditableContainer<'_> {
    fn id(&self) -> NodeId {
        match self {
            Self::Inline(id, _) | Self::Code(id, _) => *id,
        }
    }

    fn grapheme_len(&self) -> usize {
        match self {
            Self::Inline(_, content) => content.grapheme_len(),
            Self::Code(_, code) => code.graphemes(true).count(),
        }
    }
}

fn record_block_positions_lexically(
    block: &RichBlock,
    source: &str,
    positions: &mut MarkdownPositionMap,
) {
    let mut containers = Vec::new();
    collect_editable_containers(block, &mut containers);
    let only_container = containers.len() == 1;
    let mut search_from = 0usize;

    for container in containers {
        let id = container.id();
        let length = container.grapheme_len();
        let is_code = matches!(container, EditableContainer::Code(_, _));
        match container {
            EditableContainer::Inline(_, content) => {
                record_inline_content_lexically(id, content, source, &mut search_from, positions)
            }
            EditableContainer::Code(_, code) => {
                record_code_lexically(id, code, source, &mut search_from, positions)
            }
        }

        if only_container && !is_code {
            positions.record(RichPosition::new(id, length), source.len());
            search_from = source.len();
        }
        ensure_dense_lexical_boundaries(id, length, source, positions);
    }
}

fn collect_editable_containers<'a>(block: &'a RichBlock, output: &mut Vec<EditableContainer<'a>>) {
    match &block.kind {
        RichBlockKind::Paragraph { content } | RichBlockKind::Heading { content, .. } => {
            output.push(EditableContainer::Inline(block.id, content));
        }
        RichBlockKind::Quote { blocks } => {
            for block in blocks {
                collect_editable_containers(block, output);
            }
        }
        RichBlockKind::CodeBlock { code, .. } => {
            output.push(EditableContainer::Code(block.id, code));
        }
        RichBlockKind::List { items, .. } => {
            for item in items {
                for block in &item.blocks {
                    collect_editable_containers(block, output);
                }
            }
        }
        RichBlockKind::Table { header, rows, .. } => {
            output.extend(
                header
                    .iter()
                    .chain(rows.iter().flatten())
                    .map(|cell| EditableContainer::Inline(cell.id, &cell.content)),
            );
        }
        RichBlockKind::Rule | RichBlockKind::OpaqueMarkdown { .. } => {}
    }
}

fn record_inline_content_lexically(
    container_id: NodeId,
    content: &InlineContent,
    source: &str,
    search_from: &mut usize,
    positions: &mut MarkdownPositionMap,
) {
    let mut logical = 0usize;
    for node in &content.0 {
        match node {
            InlineNode::Text(text) => {
                record_lexical_graphemes(
                    container_id,
                    &mut logical,
                    &text.text,
                    source,
                    search_from,
                    positions,
                );
            }
            InlineNode::Image { .. } => {
                let start = source[*search_from..]
                    .find("![")
                    .map(|relative| *search_from + relative)
                    .unwrap_or(*search_from)
                    .min(source.len());
                positions.record(RichPosition::new(container_id, logical), start);
                let end = source[start..]
                    .find(')')
                    .map(|relative| start + relative + 1)
                    .unwrap_or(start)
                    .min(source.len());
                logical = logical.saturating_add(1);
                positions.record(RichPosition::new(container_id, logical), end);
                *search_from = end;
            }
            InlineNode::SoftBreak | InlineNode::HardBreak => {
                let start = source[*search_from..]
                    .find(['\n', '\r'])
                    .map(|relative| *search_from + relative)
                    .unwrap_or(*search_from)
                    .min(source.len());
                positions.record(RichPosition::new(container_id, logical), start);
                let end = if source.as_bytes().get(start) == Some(&b'\r')
                    && source.as_bytes().get(start + 1) == Some(&b'\n')
                {
                    start + 2
                } else {
                    (start + 1).min(source.len())
                };
                logical = logical.saturating_add(1);
                positions.record(RichPosition::new(container_id, logical), end);
                *search_from = end;
            }
        }
    }

    // Pull the final caret over closing inline delimiters. This keeps a
    // Rich-to-Source switch at the end of `**text**` out of the closing `**`.
    let mut end = (*search_from).min(source.len());
    while let Some(character) = source[end..].chars().next() {
        if matches!(character, '*' | '_' | '~' | '`') {
            end += character.len_utf8();
        } else {
            break;
        }
    }
    positions.record(RichPosition::new(container_id, logical), end);
    *search_from = end;
}

fn record_code_lexically(
    container_id: NodeId,
    code: &str,
    source: &str,
    search_from: &mut usize,
    positions: &mut MarkdownPositionMap,
) {
    let mut logical = 0usize;
    let mut content_start = (*search_from).min(source.len());
    let remaining = &source[content_start..];
    let trimmed =
        remaining.trim_start_matches(|character| matches!(character, ' ' | '\t' | '>' | '-'));
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        if let Some(line_end) = remaining.find(['\n', '\r']) {
            content_start += line_end;
            if source.as_bytes().get(content_start) == Some(&b'\r')
                && source.as_bytes().get(content_start + 1) == Some(&b'\n')
            {
                content_start += 2;
            } else {
                content_start += 1;
            }
        }
    }
    *search_from = content_start;
    record_lexical_graphemes(
        container_id,
        &mut logical,
        code,
        source,
        search_from,
        positions,
    );
}

fn record_lexical_graphemes(
    container_id: NodeId,
    logical: &mut usize,
    display: &str,
    source: &str,
    search_from: &mut usize,
    positions: &mut MarkdownPositionMap,
) {
    for grapheme in display.graphemes(true) {
        let start = source[*search_from..]
            .find(grapheme)
            .map(|relative| *search_from + relative)
            .unwrap_or(*search_from)
            .min(source.len());
        positions.record(RichPosition::new(container_id, *logical), start);
        let end = start.saturating_add(grapheme.len()).min(source.len());
        *logical = logical.saturating_add(1);
        positions.record(RichPosition::new(container_id, *logical), end);
        *search_from = end;
    }
    if display.is_empty() {
        positions.record(
            RichPosition::new(container_id, *logical),
            (*search_from).min(source.len()),
        );
    }
}

fn ensure_dense_lexical_boundaries(
    container_id: NodeId,
    grapheme_len: usize,
    source: &str,
    positions: &mut MarkdownPositionMap,
) {
    let fallback = proportional_boundaries(source, grapheme_len);
    for (offset, source_offset) in fallback.into_iter().enumerate() {
        let rich = RichPosition::new(container_id, offset);
        if !positions.entries.iter().any(|entry| entry.rich == rich) {
            positions.record(rich, source_offset);
        }
    }
}

fn serialize_inline(content: &InlineContent) -> String {
    let mut output = String::new();
    for node in &content.0 {
        match node {
            InlineNode::Text(text) => output.push_str(&serialize_text(text)),
            InlineNode::Image { alt, url, title } => {
                output.push_str("![");
                output.push_str(&escape_label(alt));
                output.push_str("](");
                output.push_str(&escape_destination(url));
                append_title(&mut output, title.as_deref());
                output.push(')');
            }
            InlineNode::SoftBreak => output.push('\n'),
            InlineNode::HardBreak => output.push_str("  \n"),
        }
    }
    output
}

fn serialize_text(text: &RichText) -> String {
    let mut value = if text.marks.code {
        code_span(&text.text)
    } else {
        let mut value = escape_text(&text.text);
        if text.marks.italic {
            value = format!("_{value}_");
        }
        if text.marks.bold {
            value = format!("**{value}**");
        }
        if text.marks.strikethrough {
            value = format!("~~{value}~~");
        }
        value
    };
    if let Some(link) = &text.link {
        let mut wrapped = format!("[{value}]({}", escape_destination(&link.url));
        append_title(&mut wrapped, link.title.as_deref());
        wrapped.push(')');
        value = wrapped;
    }
    value
}

fn serialize_code_block(language: &Option<String>, code: &str) -> String {
    let fence_len = longest_run(code, '`').saturating_add(1).max(3);
    let fence = "`".repeat(fence_len);
    let mut result = fence.clone();
    if let Some(language) = language.as_deref().filter(|value| !value.is_empty()) {
        result.push_str(language.trim());
    }
    result.push('\n');
    result.push_str(code);
    if !code.ends_with('\n') {
        result.push('\n');
    }
    result.push_str(&fence);
    result
}

fn serialize_list(kind: RichListKind, start: usize, items: &[RichListItem]) -> String {
    let mut lines = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let marker = match kind {
            RichListKind::Bullet => "- ".to_owned(),
            RichListKind::Ordered => format!("{}. ", start.saturating_add(index)),
            RichListKind::Task => format!(
                "- [{}] ",
                if item.checked == Some(true) { 'x' } else { ' ' }
            ),
        };
        let body = item
            .blocks
            .iter()
            .map(|block| serialize_block(block, "\n"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let indent = " ".repeat(marker.chars().count());
        for (line_index, line) in body.split('\n').enumerate() {
            if line_index == 0 {
                lines.push(format!("{marker}{line}"));
            } else {
                lines.push(format!("{indent}{line}"));
            }
        }
        if body.is_empty() {
            lines.push(marker.trim_end().to_owned());
        }
    }
    lines.join("\n")
}

fn serialize_table(
    alignments: &[RichTableAlignment],
    header: &[RichTableCell],
    rows: &[Vec<RichTableCell>],
) -> String {
    let columns = header
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or_default())
        .max(1);
    let row = |cells: &[RichTableCell]| {
        let values = (0..columns)
            .map(|index| {
                cells
                    .get(index)
                    .map(|cell| escape_table_cell(&serialize_inline(&cell.content)))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        format!("| {} |", values.join(" | "))
    };
    let separator = (0..columns)
        .map(|index| {
            match alignments
                .get(index)
                .copied()
                .unwrap_or(RichTableAlignment::None)
            {
                RichTableAlignment::None => "---",
                RichTableAlignment::Left => ":---",
                RichTableAlignment::Center => ":---:",
                RichTableAlignment::Right => "---:",
            }
        })
        .collect::<Vec<_>>();
    let mut output = row(header);
    output.push('\n');
    output.push_str(&format!("| {} |", separator.join(" | ")));
    for cells in rows {
        output.push('\n');
        output.push_str(&row(cells));
    }
    output
}

fn ensure_blank_block_separator(output: &mut String, newline: &str) {
    if output.is_empty() {
        return;
    }
    if output.ends_with(&format!("{newline}{newline}")) {
        return;
    }
    if !output.ends_with(newline) {
        output.push_str(newline);
    }
    output.push_str(newline);
}

fn append_title(output: &mut String, title: Option<&str>) {
    if let Some(title) = title.filter(|value| !value.is_empty()) {
        output.push_str(" \"");
        output.push_str(&title.replace('\\', "\\\\").replace('"', "\\\""));
        output.push('"');
    }
}

fn escape_text(text: &str) -> String {
    const SPECIAL: &str = "\\`*{}_[]<>()#+-.!|>~";
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if SPECIAL.contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn escape_label(text: &str) -> String {
    text.replace('\\', "\\\\").replace(']', "\\]")
}

fn escape_destination(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn escape_table_cell(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut preceding_backslashes = 0usize;
    for character in text.chars() {
        match character {
            '\n' => {
                output.push_str("<br>");
                preceding_backslashes = 0;
            }
            '|' => {
                // `serialize_text` already escapes literal pipes. Images and
                // other atoms may not, so only add an escape when the pipe is
                // not already preceded by an odd run of backslashes.
                if preceding_backslashes % 2 == 0 {
                    output.push('\\');
                }
                output.push('|');
                preceding_backslashes = 0;
            }
            '\\' => {
                output.push('\\');
                preceding_backslashes += 1;
            }
            other => {
                output.push(other);
                preceding_backslashes = 0;
            }
        }
    }
    output
}

fn code_span(text: &str) -> String {
    let fence = "`".repeat(longest_run(text, '`').saturating_add(1).max(1));
    if text.starts_with(['`', ' ']) || text.ends_with(['`', ' ']) {
        format!("{fence} {text} {fence}")
    } else {
        format!("{fence}{text}{fence}")
    }
}

fn longest_run(text: &str, needle: char) -> usize {
    text.chars()
        .fold((0usize, 0usize), |(best, current), character| {
            if character == needle {
                (best.max(current + 1), current + 1)
            } else {
                (best, 0)
            }
        })
        .0
}

fn detect_line_ending(source: &str) -> RichLineEnding {
    let crlf = source.match_indices("\r\n").count();
    let without_crlf = source.replace("\r\n", "");
    let lf = without_crlf.bytes().filter(|byte| *byte == b'\n').count();
    let cr = without_crlf.bytes().filter(|byte| *byte == b'\r').count();
    [
        (lf, RichLineEnding::Lf),
        (crlf, RichLineEnding::CrLf),
        (cr, RichLineEnding::Cr),
    ]
    .into_iter()
    .max_by_key(|(count, _)| *count)
    .filter(|(count, _)| *count > 0)
    .map_or(RichLineEnding::Lf, |(_, ending)| ending)
}

fn node_source_range(node: &AstNode<'_>, source: &str) -> Range<usize> {
    source_range_from_position(source, node.data().sourcepos)
}

fn source_range_from_position(source: &str, position: comrak::nodes::Sourcepos) -> Range<usize> {
    if position.start.line == 0 || position.end.line == 0 {
        return 0..0;
    }
    let lines = line_ranges(source);
    let Some(start_line) = lines.get(position.start.line.saturating_sub(1)) else {
        return source.len()..source.len();
    };
    let Some(end_line) = lines.get(position.end.line.saturating_sub(1)) else {
        return start_line.start..source.len();
    };
    let start = normalize_boundary(
        source,
        (start_line.start + position.start.column.saturating_sub(1)).min(start_line.end),
    );
    let end = normalize_end_boundary(
        source,
        (end_line.start + position.end.column).min(end_line.end),
    );
    start..end
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

fn normalize_boundary(source: &str, mut position: usize) -> usize {
    position = position.min(source.len());
    while position > 0 && !source.is_char_boundary(position) {
        position -= 1;
    }
    position
}

fn normalize_end_boundary(source: &str, mut position: usize) -> usize {
    position = position.min(source.len());
    while position < source.len() && !source.is_char_boundary(position) {
        position += 1;
    }
    position
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_dense_positions(
        map: &MarkdownPositionMap,
        container_id: NodeId,
        grapheme_len: usize,
        source: &str,
    ) {
        for grapheme_offset in 0..=grapheme_len {
            let rich = RichPosition::new(container_id, grapheme_offset);
            let entry = map
                .entries
                .iter()
                .find(|entry| entry.rich == rich)
                .unwrap_or_else(|| panic!("missing mapping for grapheme {grapheme_offset}"));
            assert!(
                source.is_char_boundary(entry.source_offset),
                "source offset {} is not a UTF-8 boundary",
                entry.source_offset
            );
        }
    }

    #[test]
    fn unedited_import_exports_byte_for_byte() {
        let source = "# Title\r\n\r\nText with **bold**.\n\n<aside>keep</aside>\n";
        let imported = MarkdownCodec::import(source).unwrap();
        let exported = MarkdownCodec::export(&imported.document).unwrap();
        assert_eq!(exported.markdown, source);
        assert!(matches!(
            imported.document.blocks.last().map(|block| &block.kind),
            Some(RichBlockKind::OpaqueMarkdown { .. })
        ));
    }

    #[test]
    fn dirty_block_is_serialized_from_semantics_while_other_blocks_stay_raw() {
        let source = "__old spelling__\n\n<custom untouched>\n";
        let mut imported = MarkdownCodec::import(source).unwrap();
        let first = &mut imported.document.blocks[0];
        first.kind = RichBlockKind::Paragraph {
            content: InlineContent(vec![InlineNode::Text(RichText {
                text: "changed * literally".to_owned(),
                marks: InlineMarks {
                    bold: true,
                    ..InlineMarks::default()
                },
                link: None,
            })]),
        };
        first.rewrite = RewriteState::Dirty;
        let exported = MarkdownCodec::export(&imported.document).unwrap();
        assert!(exported.markdown.starts_with("**changed \\* literally**"));
        assert!(exported.markdown.ends_with("<custom untouched>\n"));
    }

    #[test]
    fn supported_export_reimports_with_the_same_semantics() {
        let mut document = RichDocument::new();
        let id = document.allocate_node_id();
        document.blocks.push(RichBlock::new(
            id,
            RichBlockKind::Heading {
                level: 2,
                content: InlineContent(vec![InlineNode::Text(RichText {
                    text: "Bold and italic".to_owned(),
                    marks: InlineMarks {
                        bold: true,
                        italic: true,
                        ..InlineMarks::default()
                    },
                    link: None,
                })]),
            },
        ));
        let markdown = MarkdownCodec::export(&document).unwrap().markdown;
        let reparsed = MarkdownCodec::import(&markdown).unwrap();
        let RichBlockKind::Heading { content, .. } = &reparsed.document.blocks[0].kind else {
            panic!("expected heading")
        };
        let InlineNode::Text(text) = &content.0[0] else {
            panic!("expected text")
        };
        assert!(text.marks.bold);
        assert!(text.marks.italic);
    }

    #[test]
    fn unedited_mixed_line_endings_and_bom_are_byte_identical() {
        let source = "# Title\r\n\r\nfirst\n\nsecond\r";
        let imported =
            MarkdownCodec::import_with_metadata(source, true, RichLineEnding::CrLf).unwrap();
        let exported = MarkdownCodec::export(&imported.document).unwrap();
        let mut expected = b"\xEF\xBB\xBF".to_vec();
        expected.extend_from_slice(source.as_bytes());

        assert_eq!(exported.to_bytes(imported.document.utf8_bom), expected);
    }

    #[test]
    fn changing_one_block_preserves_front_matter_html_footnotes_and_definitions() {
        let source = concat!(
            "---\n",
            "title: \"Tundra\"\n",
            "tags: [editor, markdown]\n",
            "---\n\n",
            "<section data-note=\"keep | exactly\">\n",
            "  <b>raw html</b>\n",
            "</section>\n\n",
            "Paragraph _using its original spelling_.\n\n",
            "[^note]: Footnote with **raw markers**.\n\n",
            "[reference]: https://example.com/a_(b) \"Raw title\"\n",
        );
        let mut imported = MarkdownCodec::import(source).unwrap();
        let target = imported
            .document
            .blocks
            .iter_mut()
            .find(|block| match &block.kind {
                RichBlockKind::Paragraph { content } => {
                    content.plain_text().starts_with("Paragraph ")
                }
                _ => false,
            })
            .expect("editable paragraph in preservation corpus");
        target.kind = RichBlockKind::Paragraph {
            content: InlineContent::plain("Only this paragraph changed."),
        };
        target.rewrite = RewriteState::Dirty;

        let exported = MarkdownCodec::export(&imported.document).unwrap();
        let expected = source.replace(
            "Paragraph _using its original spelling_.",
            "Only this paragraph changed\\.",
        );
        assert_eq!(exported.markdown, expected);
    }

    #[test]
    fn table_pipes_code_backticks_and_link_image_escapes_reimport_semantically() {
        let mut document = RichDocument::new();
        let table_id = document.allocate_node_id();
        let header_id = document.allocate_node_id();
        let body_id = document.allocate_node_id();
        document.blocks.push(RichBlock::new(
            table_id,
            RichBlockKind::Table {
                alignments: vec![RichTableAlignment::None],
                header: vec![RichTableCell {
                    id: header_id,
                    content: InlineContent::plain("head|pipe"),
                }],
                rows: vec![vec![RichTableCell {
                    id: body_id,
                    content: InlineContent::plain("body|pipe"),
                }]],
            },
        ));
        let paragraph_id = document.allocate_node_id();
        document.blocks.push(RichBlock::new(
            paragraph_id,
            RichBlockKind::Paragraph {
                content: InlineContent(vec![
                    InlineNode::Text(RichText {
                        text: "a `tick` and ``pair``".to_owned(),
                        marks: InlineMarks {
                            code: true,
                            ..InlineMarks::default()
                        },
                        link: None,
                    }),
                    InlineNode::Text(RichText {
                        text: " linked]label".to_owned(),
                        marks: InlineMarks::default(),
                        link: Some(LinkAttributes {
                            url: "https://example.com/a_(b)".to_owned(),
                            title: Some("a \"quoted\" title".to_owned()),
                        }),
                    }),
                    InlineNode::Image {
                        alt: "image]alt|pipe".to_owned(),
                        url: "https://example.com/image_(1).png".to_owned(),
                        title: Some("image \"title\"".to_owned()),
                    },
                ]),
            },
        ));

        let exported = MarkdownCodec::export(&document).unwrap();
        assert!(exported.markdown.contains("head\\|pipe"));
        assert!(!exported.markdown.contains("head\\\\|pipe"));
        let reparsed = MarkdownCodec::import(&exported.markdown).unwrap();

        let RichBlockKind::Table { header, rows, .. } = &reparsed.document.blocks[0].kind else {
            panic!("expected table")
        };
        assert_eq!(header[0].content.plain_text(), "head|pipe");
        assert_eq!(rows[0][0].content.plain_text(), "body|pipe");
        let RichBlockKind::Paragraph { content } = &reparsed.document.blocks[1].kind else {
            panic!("expected paragraph")
        };
        assert_eq!(
            content.plain_text(),
            "a `tick` and ``pair`` linked]labelimage]alt|pipe"
        );
        let code = content
            .0
            .iter()
            .find_map(|node| match node {
                InlineNode::Text(text) if text.marks.code => Some(text),
                _ => None,
            })
            .expect("code span");
        assert_eq!(code.text, "a `tick` and ``pair``");
        let linked = content
            .0
            .iter()
            .find_map(|node| match node {
                InlineNode::Text(text) if text.link.is_some() => Some(text),
                _ => None,
            })
            .expect("linked text");
        assert_eq!(
            linked.link.as_ref().unwrap().url,
            "https://example.com/a_(b)"
        );
        assert!(content.0.iter().any(|node| matches!(
            node,
            InlineNode::Image { alt, url, .. }
                if alt == "image]alt|pipe" && url == "https://example.com/image_(1).png"
        )));
    }

    #[test]
    fn formatted_unicode_maps_every_grapheme_boundary_on_import_and_export() {
        let source = "# A **好👨‍👩‍👧‍👦e\u{301}** Z\n";
        let imported = MarkdownCodec::import(source).unwrap();
        let block = &imported.document.blocks[0];
        let RichBlockKind::Heading { content, .. } = &block.kind else {
            panic!("heading")
        };
        assert_dense_positions(
            &imported.positions,
            block.id,
            content.grapheme_len(),
            source,
        );

        let logical_before_unicode = "A ".graphemes(true).count();
        let good_start = source.find('好').unwrap();
        assert_eq!(
            imported
                .positions
                .source_offset_for(RichPosition::new(block.id, logical_before_unicode)),
            Some(good_start)
        );
        assert_eq!(
            imported
                .positions
                .source_offset_for(RichPosition::new(block.id, logical_before_unicode + 1)),
            Some(good_start + '好'.len_utf8())
        );

        let exported = MarkdownCodec::export(&imported.document).unwrap();
        assert_eq!(exported.markdown, source);
        assert_dense_positions(
            &exported.positions,
            block.id,
            content.grapheme_len(),
            &exported.markdown,
        );
        assert_eq!(
            exported
                .positions
                .rich_position_for(good_start + '好'.len_utf8()),
            Some(RichPosition::new(block.id, logical_before_unicode + 1))
        );
    }

    #[test]
    fn quote_list_and_code_descendants_have_dense_position_maps() {
        let source = concat!(
            "> 引用 **好**\n",
            "\n",
            "- item 👩‍👩‍👧‍👦\n",
            "\n",
            "```rs\n",
            "let 名 = 1;\n",
            "```\n",
        );
        let imported = MarkdownCodec::import(source).unwrap();

        let RichBlockKind::Quote { blocks } = &imported.document.blocks[0].kind else {
            panic!("quote")
        };
        let quote_paragraph = &blocks[0];
        let RichBlockKind::Paragraph {
            content: quote_content,
        } = &quote_paragraph.kind
        else {
            panic!("quote paragraph")
        };

        let RichBlockKind::List { items, .. } = &imported.document.blocks[1].kind else {
            panic!("list")
        };
        let list_paragraph = &items[0].blocks[0];
        let RichBlockKind::Paragraph {
            content: list_content,
        } = &list_paragraph.kind
        else {
            panic!("list paragraph")
        };

        let code_block = &imported.document.blocks[2];
        let RichBlockKind::CodeBlock { code, .. } = &code_block.kind else {
            panic!("code block")
        };

        let containers = [
            (quote_paragraph.id, quote_content.grapheme_len()),
            (list_paragraph.id, list_content.grapheme_len()),
            (code_block.id, code.graphemes(true).count()),
        ];
        for (id, length) in containers {
            assert_dense_positions(&imported.positions, id, length, source);
        }

        let exported = MarkdownCodec::export(&imported.document).unwrap();
        assert_eq!(exported.markdown, source);
        for (id, length) in containers {
            assert_dense_positions(&exported.positions, id, length, &exported.markdown);
        }

        let mut dirty_document = imported.document.clone();
        for block in &mut dirty_document.blocks {
            block.rewrite = RewriteState::Dirty;
        }
        let normalized = MarkdownCodec::export(&dirty_document).unwrap();
        for (id, length) in containers {
            assert_dense_positions(&normalized.positions, id, length, &normalized.markdown);
        }
    }

    #[test]
    fn table_cells_map_unicode_and_escaped_pipe_boundaries() {
        let source = concat!(
            "| 名字 | 值\\|pipe |\n",
            "| --- | --- |\n",
            "| e\u{301} | 👨‍👩‍👧‍👦 |\n",
        );
        let imported = MarkdownCodec::import(source).unwrap();
        let RichBlockKind::Table { header, rows, .. } = &imported.document.blocks[0].kind else {
            panic!("table")
        };
        let cells = header
            .iter()
            .chain(rows.iter().flatten())
            .collect::<Vec<_>>();
        for cell in &cells {
            assert_dense_positions(
                &imported.positions,
                cell.id,
                cell.content.grapheme_len(),
                source,
            );
        }

        let escaped_pipe_cell = &header[1];
        let pipe_logical = "值".graphemes(true).count();
        // The caret before a literal pipe belongs before the complete escape
        // sequence, not between `\\` and `|`.
        let pipe_source = source.find("\\|").unwrap();
        assert_eq!(
            imported
                .positions
                .source_offset_for(RichPosition::new(escaped_pipe_cell.id, pipe_logical)),
            Some(pipe_source)
        );

        let exported = MarkdownCodec::export(&imported.document).unwrap();
        assert_eq!(exported.markdown, source);
        for cell in cells {
            assert_dense_positions(
                &exported.positions,
                cell.id,
                cell.content.grapheme_len(),
                &exported.markdown,
            );
        }
    }

    #[test]
    fn unsupported_inline_in_nested_container_promotes_the_top_level_block() {
        let source = "> quote with a footnote[^nested]\n\n[^nested]: definition\n";
        let imported = MarkdownCodec::import(source).unwrap();
        assert!(matches!(
            imported.document.blocks[0].kind,
            RichBlockKind::OpaqueMarkdown { .. }
        ));
        assert!(
            imported
                .positions
                .entries
                .iter()
                .all(|entry| entry.rich.container_id != imported.document.blocks[0].id)
        );
        assert_eq!(
            MarkdownCodec::export(&imported.document).unwrap().markdown,
            source
        );
    }
}
