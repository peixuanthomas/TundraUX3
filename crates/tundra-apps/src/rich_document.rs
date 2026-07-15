//! Serializable, Markdown-independent document model used by the rich editor.
//!
//! Positions in this module are logical grapheme offsets. They deliberately do
//! not expose byte offsets into a Markdown serialization.

use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

/// Stable identity for a block, list item, or table cell.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct NodeId(u64);

impl NodeId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A caret position within one editable rich container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RichPosition {
    pub container_id: NodeId,
    pub grapheme_offset: usize,
}

impl RichPosition {
    pub const fn new(container_id: NodeId, grapheme_offset: usize) -> Self {
        Self {
            container_id,
            grapheme_offset,
        }
    }
}

/// A range whose endpoints are rich-document positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RichRange {
    pub start: RichPosition,
    pub end: RichPosition,
}

impl RichRange {
    pub const fn new(start: RichPosition, end: RichPosition) -> Self {
        Self { start, end }
    }

    pub fn is_collapsed(self) -> bool {
        self.start.container_id == self.end.container_id
            && self.start.grapheme_offset == self.end.grapheme_offset
    }
}

/// Preferred newline for newly serialized or modified blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RichLineEnding {
    #[default]
    Lf,
    CrLf,
    Cr,
}

impl RichLineEnding {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }
}

/// Whether a top-level block may be emitted from its exact imported bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RewriteState {
    #[default]
    Clean,
    Dirty,
}

/// A rich document. All editing state lives here rather than in Markdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RichDocument {
    pub blocks: Vec<RichBlock>,
    pub trailing_trivia: String,
    pub utf8_bom: bool,
    pub preferred_line_ending: RichLineEnding,
    #[serde(default = "default_next_node_id")]
    next_node_id: u64,
}

const fn default_next_node_id() -> u64 {
    1
}

impl Default for RichDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl RichDocument {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            trailing_trivia: String::new(),
            utf8_bom: false,
            preferred_line_ending: RichLineEnding::Lf,
            next_node_id: 1,
        }
    }

    pub fn allocate_node_id(&mut self) -> NodeId {
        let id = NodeId::new(self.next_node_id.max(1));
        self.next_node_id = id.get().saturating_add(1);
        id
    }

    /// Repairs the allocator after deserializing older or hand-authored data.
    pub fn repair_node_id_allocator(&mut self) {
        let max_id = self
            .blocks
            .iter()
            .map(RichBlock::max_node_id)
            .max()
            .unwrap_or(0);
        self.next_node_id = self.next_node_id.max(max_id.saturating_add(1)).max(1);
    }

    /// Marks the top-level block containing `node_id` for normalized export.
    pub fn mark_containing_block_dirty(&mut self, node_id: NodeId) -> bool {
        let Some(block) = self
            .blocks
            .iter_mut()
            .find(|block| block.contains_node(node_id))
        else {
            return false;
        };
        block.rewrite = RewriteState::Dirty;
        true
    }

    pub fn is_dirty(&self) -> bool {
        self.blocks
            .iter()
            .any(|block| block.rewrite == RewriteState::Dirty)
    }

    pub fn contains_node(&self, node_id: NodeId) -> bool {
        self.blocks.iter().any(|block| block.contains_node(node_id))
    }

    /// Produces renderer-ready spans without parsing Markdown.
    pub fn project(&self) -> RichProjection {
        RichProjection {
            blocks: self.blocks.iter().map(RichBlock::project).collect(),
        }
    }

    pub fn first_editable_position(&self) -> Option<RichPosition> {
        self.blocks
            .iter()
            .find_map(RichBlock::first_editable_position)
    }
}

/// A block plus preservation data owned by its top-level ancestor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RichBlock {
    pub id: NodeId,
    pub leading_trivia: String,
    pub original_raw: Option<String>,
    pub rewrite: RewriteState,
    pub kind: RichBlockKind,
}

impl RichBlock {
    pub fn new(id: NodeId, kind: RichBlockKind) -> Self {
        Self {
            id,
            leading_trivia: String::new(),
            original_raw: None,
            rewrite: RewriteState::Dirty,
            kind,
        }
    }

    pub fn imported(
        id: NodeId,
        leading_trivia: String,
        original_raw: String,
        kind: RichBlockKind,
    ) -> Self {
        Self {
            id,
            leading_trivia,
            original_raw: Some(original_raw),
            rewrite: RewriteState::Clean,
            kind,
        }
    }

    pub fn is_opaque(&self) -> bool {
        matches!(self.kind, RichBlockKind::OpaqueMarkdown { .. })
    }

    pub fn contains_node(&self, node_id: NodeId) -> bool {
        self.id == node_id
            || match &self.kind {
                RichBlockKind::Quote { blocks } => {
                    blocks.iter().any(|block| block.contains_node(node_id))
                }
                RichBlockKind::List { items, .. } => items.iter().any(|item| {
                    item.id == node_id
                        || item.blocks.iter().any(|block| block.contains_node(node_id))
                }),
                RichBlockKind::Table { header, rows, .. } => header
                    .iter()
                    .chain(rows.iter().flatten())
                    .any(|cell| cell.id == node_id),
                _ => false,
            }
    }

    fn max_node_id(&self) -> u64 {
        let descendants = match &self.kind {
            RichBlockKind::Quote { blocks } => {
                blocks.iter().map(RichBlock::max_node_id).max().unwrap_or(0)
            }
            RichBlockKind::List { items, .. } => items
                .iter()
                .map(|item| {
                    item.blocks
                        .iter()
                        .map(RichBlock::max_node_id)
                        .max()
                        .unwrap_or(0)
                        .max(item.id.get())
                })
                .max()
                .unwrap_or(0),
            RichBlockKind::Table { header, rows, .. } => header
                .iter()
                .chain(rows.iter().flatten())
                .map(|cell| cell.id.get())
                .max()
                .unwrap_or(0),
            _ => 0,
        };
        self.id.get().max(descendants)
    }

    pub(crate) fn first_editable_position(&self) -> Option<RichPosition> {
        match &self.kind {
            RichBlockKind::Paragraph { .. }
            | RichBlockKind::Heading { .. }
            | RichBlockKind::CodeBlock { .. } => Some(RichPosition::new(self.id, 0)),
            RichBlockKind::Quote { blocks } => {
                blocks.iter().find_map(RichBlock::first_editable_position)
            }
            RichBlockKind::List { items, .. } => items.iter().find_map(|item| {
                item.blocks
                    .iter()
                    .find_map(RichBlock::first_editable_position)
            }),
            RichBlockKind::Table { header, rows, .. } => header
                .iter()
                .chain(rows.iter().flatten())
                .next()
                .map(|cell| RichPosition::new(cell.id, 0)),
            RichBlockKind::Rule | RichBlockKind::OpaqueMarkdown { .. } => None,
        }
    }

    fn project(&self) -> ProjectedBlock {
        let kind = match &self.kind {
            RichBlockKind::Paragraph { content } => ProjectedBlockKind::Paragraph {
                content: project_inline(self.id, content),
            },
            RichBlockKind::Heading { level, content } => ProjectedBlockKind::Heading {
                level: *level,
                content: project_inline(self.id, content),
            },
            RichBlockKind::Quote { blocks } => ProjectedBlockKind::Quote {
                blocks: blocks.iter().map(RichBlock::project).collect(),
            },
            RichBlockKind::CodeBlock { language, code } => {
                let end = code.graphemes(true).count();
                ProjectedBlockKind::CodeBlock {
                    language: language.clone(),
                    code: code.clone(),
                    range: RichRange::new(
                        RichPosition::new(self.id, 0),
                        RichPosition::new(self.id, end),
                    ),
                }
            }
            RichBlockKind::List {
                kind,
                start,
                tight,
                items,
            } => ProjectedBlockKind::List {
                kind: *kind,
                start: *start,
                tight: *tight,
                items: items
                    .iter()
                    .map(|item| ProjectedListItem {
                        id: item.id,
                        checked: item.checked,
                        blocks: item.blocks.iter().map(RichBlock::project).collect(),
                    })
                    .collect(),
            },
            RichBlockKind::Table {
                alignments,
                header,
                rows,
            } => ProjectedBlockKind::Table {
                alignments: alignments.clone(),
                header: header.iter().map(RichTableCell::project).collect(),
                rows: rows
                    .iter()
                    .map(|row| row.iter().map(RichTableCell::project).collect())
                    .collect(),
            },
            RichBlockKind::Rule => ProjectedBlockKind::Rule,
            RichBlockKind::OpaqueMarkdown { raw, reason } => ProjectedBlockKind::OpaqueMarkdown {
                raw: raw.clone(),
                reason: reason.clone(),
            },
        };
        ProjectedBlock { id: self.id, kind }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RichBlockKind {
    Paragraph {
        content: InlineContent,
    },
    Heading {
        level: u8,
        content: InlineContent,
    },
    Quote {
        blocks: Vec<RichBlock>,
    },
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    List {
        kind: RichListKind,
        start: usize,
        tight: bool,
        items: Vec<RichListItem>,
    },
    Table {
        alignments: Vec<RichTableAlignment>,
        header: Vec<RichTableCell>,
        rows: Vec<Vec<RichTableCell>>,
    },
    Rule,
    /// Unsupported Markdown is visible but never editable in Rich mode.
    OpaqueMarkdown {
        raw: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RichListKind {
    Bullet,
    Ordered,
    Task,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RichListItem {
    pub id: NodeId,
    pub checked: Option<bool>,
    pub blocks: Vec<RichBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RichTableAlignment {
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RichTableCell {
    pub id: NodeId,
    pub content: InlineContent,
}

impl RichTableCell {
    fn project(&self) -> ProjectedTableCell {
        ProjectedTableCell {
            id: self.id,
            content: project_inline(self.id, &self.content),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct InlineContent(pub Vec<InlineNode>);

impl InlineContent {
    pub fn plain(text: impl Into<String>) -> Self {
        Self(vec![InlineNode::Text(RichText::plain(text))])
    }

    pub fn plain_text(&self) -> String {
        self.0
            .iter()
            .map(InlineNode::display_text)
            .collect::<String>()
    }

    pub fn grapheme_len(&self) -> usize {
        self.0.iter().map(InlineNode::grapheme_len).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InlineNode {
    Text(RichText),
    Image {
        alt: String,
        url: String,
        title: Option<String>,
    },
    SoftBreak,
    HardBreak,
}

impl InlineNode {
    pub fn display_text(&self) -> &str {
        match self {
            Self::Text(text) => &text.text,
            Self::Image { alt, .. } => alt,
            Self::SoftBreak | Self::HardBreak => "\n",
        }
    }

    pub fn grapheme_len(&self) -> usize {
        match self {
            Self::Image { .. } => 1,
            _ => self.display_text().graphemes(true).count(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RichText {
    pub text: String,
    pub marks: InlineMarks,
    pub link: Option<LinkAttributes>,
}

impl RichText {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            marks: InlineMarks::default(),
            link: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct InlineMarks {
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub code: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkAttributes {
    pub url: String,
    pub title: Option<String>,
}

/// A cheap projection used directly by renderers and hit testing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichProjection {
    pub blocks: Vec<ProjectedBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedBlock {
    pub id: NodeId,
    pub kind: ProjectedBlockKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectedBlockKind {
    Paragraph {
        content: Vec<ProjectedInline>,
    },
    Heading {
        level: u8,
        content: Vec<ProjectedInline>,
    },
    Quote {
        blocks: Vec<ProjectedBlock>,
    },
    CodeBlock {
        language: Option<String>,
        code: String,
        range: RichRange,
    },
    List {
        kind: RichListKind,
        start: usize,
        tight: bool,
        items: Vec<ProjectedListItem>,
    },
    Table {
        alignments: Vec<RichTableAlignment>,
        header: Vec<ProjectedTableCell>,
        rows: Vec<Vec<ProjectedTableCell>>,
    },
    Rule,
    OpaqueMarkdown {
        raw: String,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedListItem {
    pub id: NodeId,
    pub checked: Option<bool>,
    pub blocks: Vec<ProjectedBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedTableCell {
    pub id: NodeId,
    pub content: Vec<ProjectedInline>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedInline {
    pub text: String,
    pub marks: InlineMarks,
    pub link: Option<LinkAttributes>,
    pub image: Option<ImageAttributes>,
    pub hard_break: bool,
    pub range: RichRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageAttributes {
    pub url: String,
    pub title: Option<String>,
}

fn project_inline(container_id: NodeId, content: &InlineContent) -> Vec<ProjectedInline> {
    let mut offset = 0usize;
    content
        .0
        .iter()
        .map(|node| {
            let start = offset;
            offset += node.grapheme_len();
            let (marks, link, image, hard_break) = match node {
                InlineNode::Text(text) => (text.marks, text.link.clone(), None, false),
                InlineNode::Image { url, title, .. } => (
                    InlineMarks::default(),
                    None,
                    Some(ImageAttributes {
                        url: url.clone(),
                        title: title.clone(),
                    }),
                    false,
                ),
                InlineNode::SoftBreak => (InlineMarks::default(), None, None, false),
                InlineNode::HardBreak => (InlineMarks::default(), None, None, true),
            };
            ProjectedInline {
                text: node.display_text().to_owned(),
                marks,
                link,
                image,
                hard_break,
                range: RichRange::new(
                    RichPosition::new(container_id, start),
                    RichPosition::new(container_id, offset),
                ),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_uses_grapheme_offsets_and_never_markdown_offsets() {
        let mut document = RichDocument::new();
        let id = document.allocate_node_id();
        document.blocks.push(RichBlock::new(
            id,
            RichBlockKind::Paragraph {
                content: InlineContent(vec![InlineNode::Text(RichText {
                    text: "a👨‍👩‍👧‍👦b".to_owned(),
                    marks: InlineMarks {
                        bold: true,
                        ..InlineMarks::default()
                    },
                    link: None,
                })]),
            },
        ));

        let projection = document.project();
        let ProjectedBlockKind::Paragraph { content } = &projection.blocks[0].kind else {
            panic!("expected paragraph")
        };
        assert_eq!(content[0].range.end, RichPosition::new(id, 3));
        assert!(content[0].marks.bold);
    }

    #[test]
    fn serde_round_trip_repairs_stable_id_allocator() {
        let mut document = RichDocument::new();
        document.blocks.push(RichBlock::new(
            NodeId::new(42),
            RichBlockKind::Paragraph {
                content: InlineContent::plain("hello"),
            },
        ));
        let json = serde_json::to_string(&document).unwrap();
        let mut restored: RichDocument = serde_json::from_str(&json).unwrap();
        restored.repair_node_id_allocator();
        assert_eq!(restored.allocate_node_id(), NodeId::new(43));
    }
}
