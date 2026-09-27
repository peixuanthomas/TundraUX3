use std::sync::Arc;

use app::editor::line_changes::{LineChange, LineMarker};
use app::editor::{EditorCommand, EditorEffect, EditorState, SourceRange};

fn compare(old: &str, new: &str) -> Arc<[LineMarker]> {
    let mut editor = EditorState::open("note.txt", old.as_bytes()).unwrap();
    editor.replace_source_range(SourceRange::new(0, old.len()), new);
    editor.source_line_markers()
}

#[test]
fn separate_insert_replace_and_delete_keep_unchanged_lines_unmarked() {
    let markers = compare(
        "one\ntwo\nthree\nfour\nfive\nsix",
        "one\nnew\ntwo\nTHREE\nfive\nsix",
    );
    assert_eq!(
        markers.iter().map(|m| m.change).collect::<Vec<_>>(),
        vec![
            LineChange::Unchanged,
            LineChange::Added,
            LineChange::Unchanged,
            LineChange::Modified,
            LineChange::Unchanged,
            LineChange::Unchanged,
        ]
    );
    assert_eq!(markers[4].deleted_before, 1);
    assert_eq!(
        markers
            .iter()
            .map(|m| m.deleted_before + m.deleted_after)
            .sum::<usize>(),
        1
    );
}

#[test]
fn deletion_markers_cover_start_end_and_entire_document() {
    assert_eq!(compare("first\nkeep", "keep")[0].deleted_before, 1);
    assert_eq!(compare("keep\nlast", "keep\n")[1].deleted_before, 0);
    // Deletion at EOF with the preceding line unchanged.
    let markers = compare("keep\nlast\n", "keep\n");
    assert_eq!(markers[1].deleted_before, 1);
    let markers = compare("keep\nlast", "keep");
    assert_eq!(markers[0].change, LineChange::Modified);
    assert_eq!(markers[0].deleted_after, 1);
    let markers = compare("a\nb", "");
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].deleted_before, 2);
}

#[test]
fn unicode_mixed_line_endings_and_repeated_lines_are_compared_without_normalization() {
    let old = "标题\r\na\r\na\r\n尾\rend";
    let new = "标题\r\na\r\n新增\r\na\r\n尾\rend";
    let markers = compare(old, new);
    assert_eq!(markers[2].change, LineChange::Added);
    assert_eq!(
        markers
            .iter()
            .filter(|m| m.change != LineChange::Unchanged)
            .count(),
        1
    );
    assert_eq!(compare("a\r\nb", "a\nb")[0].change, LineChange::Modified);
    assert!(
        compare("", "new")
            .iter()
            .all(|m| m.change == LineChange::Added)
    );
}

#[test]
fn markers_follow_undo_redo_successful_saves_and_in_flight_snapshots() {
    let mut editor = EditorState::open("note.txt", b"one\ntwo").unwrap();
    assert!(editor.source_line_markers().is_empty());
    editor.replace_source_range(SourceRange::new(0, 3), "ONE");
    let before = editor.clone();
    let first = editor.source_line_markers();
    assert_eq!(editor, before);
    assert!(Arc::ptr_eq(&first, &editor.source_line_markers()));
    editor.apply(EditorCommand::Undo);
    assert!(editor.source_line_markers().is_empty());
    editor.apply(EditorCommand::Redo);
    assert_eq!(editor.source_line_markers(), first);

    let effects = editor.apply(EditorCommand::RequestSave);
    let [EditorEffect::SaveFile { snapshot, .. }] = effects.as_slice() else {
        panic!()
    };
    // An unsuccessful save has no acknowledgement and must retain the markers.
    assert_eq!(editor.source_line_markers(), first);
    editor.replace_source_range(SourceRange::new(4, 7), "TWO");
    editor.apply(EditorCommand::MarkSaved {
        path: None,
        revision: snapshot.revision,
    });
    let markers = editor.source_line_markers();
    assert_eq!(markers[0].change, LineChange::Unchanged);
    assert_eq!(markers[1].change, LineChange::Modified);

    let effects = editor.apply(EditorCommand::RequestSave);
    let [EditorEffect::SaveFile { snapshot, .. }] = effects.as_slice() else {
        panic!()
    };
    editor.apply(EditorCommand::MarkSaved {
        path: None,
        revision: snapshot.revision,
    });
    assert!(editor.source_line_markers().is_empty());
    editor.apply(EditorCommand::Undo);
    assert!(
        editor
            .source_line_markers()
            .iter()
            .any(|m| m.change == LineChange::Modified)
    );
}

#[test]
fn distant_edits_in_large_files_preserve_the_lines_between_them() {
    let old = (0..20_000)
        .map(|i| format!("line {i}\n"))
        .collect::<String>();
    let new = old
        .replace("line 10\n", "changed 10\n")
        .replace("line 19000\n", "added\nline 19000\n");
    let markers = compare(&old, &new);
    assert_eq!(markers[10].change, LineChange::Modified);
    assert_eq!(markers[19_000].change, LineChange::Added);
    assert_eq!(
        markers
            .iter()
            .filter(|m| m.change != LineChange::Unchanged)
            .count(),
        2
    );
}
