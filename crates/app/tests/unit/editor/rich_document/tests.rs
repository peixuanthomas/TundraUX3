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
