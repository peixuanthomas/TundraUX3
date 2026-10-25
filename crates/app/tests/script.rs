use app::editor::{EditorCommand, EditorState, SourceLineChange, SourceRange};
use app::script::*;
use std::path::Path;

fn script_path(name: &str) -> std::path::PathBuf {
    std::env::current_dir().unwrap().join(name)
}

#[test]
fn interpreter_arguments_and_working_directory_preserve_script_semantics() {
    let path = script_path("中文 script.sh");
    let settings = ScriptSettings {
        arguments: "'a  b' '$HOME' '$(command)'".into(),
        ..Default::default()
    };
    let launch =
        build_launch_spec(&path, "#!/usr/bin/env -S bash -eu\necho ok\n", &settings).unwrap();
    assert!(launch.is_bash());
    assert_eq!(launch.interpreter, Path::new("bash"));
    assert_eq!(launch.interpreter_args, ["-eu"]);
    assert_eq!(launch.args, ["a  b", "$HOME", "$(command)"]);
    assert_eq!(launch.cwd, path.parent().unwrap());
    assert_eq!(
        build_launch_spec(&path, "echo ok", &ScriptSettings::default())
            .unwrap()
            .interpreter,
        Path::new("/bin/sh")
    );
    assert!(
        !build_launch_spec(&path, "#!/bin/dash\n", &ScriptSettings::default())
            .unwrap()
            .is_bash()
    );
    let settings = ScriptSettings {
        interpreter: "bash -x".into(),
        ..Default::default()
    };
    assert!(
        build_launch_spec(&path, "#!/bin/dash\n", &settings)
            .unwrap()
            .is_bash()
    );
}

#[test]
fn malformed_launch_settings_do_not_guess_an_interpreter() {
    let path = script_path("test.sh");
    assert!(
        build_launch_spec(
            &path,
            "#!/usr/bin/env -i bash\n",
            &ScriptSettings::default()
        )
        .is_err()
    );
    assert!(build_launch_spec(&path, "#!/broken 'quote", &ScriptSettings::default()).is_err());
    assert!(
        build_launch_spec(
            &path,
            "",
            &ScriptSettings {
                arguments: "'unterminated".into(),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        build_launch_spec(
            &path,
            "",
            &ScriptSettings {
                cwd: "relative-folder".into(),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(is_shell_script(None, "#!/usr/bin/env bash\n"));
    assert!(!is_shell_script(None, "#!/usr/bin/env python3\n"));
}

#[test]
fn stale_debugger_snapshots_cannot_replace_new_session_or_new_stop() {
    let mut state = ScriptState::default();
    state.begin(true);
    let first = state.session_id;
    state.apply(ScriptEvent {
        session_id: first,
        stop_id: 3,
        kind: ScriptEventKind::Stopped(ScriptLocation {
            path: script_path("s.sh"),
            line: 2,
        }),
    });
    assert!(!state.apply(ScriptEvent {
        session_id: first,
        stop_id: 2,
        kind: ScriptEventKind::Expression("old".into())
    }));
    state.begin(true);
    assert!(!state.apply(ScriptEvent {
        session_id: first,
        stop_id: 100,
        kind: ScriptEventKind::Finished(0)
    }));
    assert_eq!(state.status, ScriptStatus::Starting);
}

#[test]
fn breakpoint_rejection_is_distinct_and_resets_after_edit_or_new_run() {
    let path = script_path("rejected.sh");
    let other = script_path("other.sh");
    let mut state = ScriptState::default();
    state.toggle_breakpoint(&path, 2);
    state.toggle_breakpoint(&other, 2);
    assert!(
        state
            .breakpoints
            .iter()
            .all(|bp| !bp.verified && !bp.rejected)
    );
    for breakpoint in &mut state.breakpoints {
        breakpoint.rejected = true;
    }
    state.remap_breakpoints(&path, &[SourceLineChange::from_vim(1, 1, 1, 1)]);
    let moved = state.breakpoints.iter().find(|bp| bp.path == path).unwrap();
    assert_eq!(moved.line, 3);
    assert!(!moved.verified && !moved.rejected);
    assert!(
        state
            .breakpoints
            .iter()
            .find(|bp| bp.path == other)
            .unwrap()
            .rejected
    );
    state.begin(true);
    assert!(
        state
            .breakpoints
            .iter()
            .all(|bp| !bp.verified && !bp.rejected)
    );
}

#[test]
fn breakpoints_follow_native_edits_undo_redo_and_remove_deleted_lines() {
    let path = script_path("s.sh");
    let mut editor = EditorState::open(&path, b"one\ntwo\nthree\n").unwrap();
    editor.set_source_change_tracking(true);
    let mut state = ScriptState::default();
    state.toggle_breakpoint(&path, 2);
    editor.replace_source_range(SourceRange::new(0, 0), "first\n");
    state.remap_breakpoints(&path, &editor.take_source_line_changes());
    assert_eq!(state.breakpoints[0].line, 3);
    editor.apply(EditorCommand::Undo);
    state.remap_breakpoints(&path, &editor.take_source_line_changes());
    assert_eq!(state.breakpoints[0].line, 2);
    editor.apply(EditorCommand::Redo);
    state.remap_breakpoints(&path, &editor.take_source_line_changes());
    assert_eq!(state.breakpoints[0].line, 3);
    let range = editor.source_line_range(2).unwrap();
    editor.replace_source_range(SourceRange::new(range.start, range.end + 1), "");
    state.remap_breakpoints(&path, &editor.take_source_line_changes());
    assert!(state.breakpoints.is_empty());
}

#[test]
fn execution_lock_preserves_document_access_and_unmodified_source() {
    let path = script_path("s.sh");
    let mut editor = EditorState::open(&path, b"echo ok\n").unwrap();
    editor.apply(EditorCommand::SetExecutionReadOnly(true));
    editor.apply(EditorCommand::InsertText("changed".into()));
    assert_eq!(editor.source().as_ref(), "echo ok\n");
    assert!(editor.is_read_only());
    editor.apply(EditorCommand::SetExecutionReadOnly(false));
    assert!(!editor.is_read_only());
    let mut protected = EditorState::open_read_only(&path, b"echo ok\n").unwrap();
    protected.apply(EditorCommand::SetExecutionReadOnly(true));
    protected.apply(EditorCommand::SetExecutionReadOnly(false));
    assert!(protected.is_read_only());
}

#[test]
fn vim_deletions_remove_every_deleted_line_and_preserve_following_anchors() {
    let change = SourceLineChange::from_vim(2, 3, -3, 1);
    assert_eq!(change.remap_line(1), Some(1));
    assert_eq!(change.remap_line(2), None);
    assert_eq!(change.remap_line(4), None);
    assert_eq!(change.remap_line(5), Some(2));
    let insert = SourceLineChange::from_vim(2, 2, 2, 1);
    assert_eq!(insert.remap_line(2), Some(4));
}

#[test]
fn joining_lines_merges_breakpoints_instead_of_submitting_duplicates() {
    let path = script_path("s.sh");
    let mut editor = EditorState::open(&path, b"one\ntwo\n").unwrap();
    editor.set_source_change_tracking(true);
    let mut state = ScriptState::default();
    state.toggle_breakpoint(&path, 1);
    state.toggle_breakpoint(&path, 2);
    state.breakpoints[0].enabled = false;
    editor.replace_source_range(SourceRange::new(3, 4), "");
    state.remap_breakpoints(&path, &editor.take_source_line_changes());
    assert_eq!(state.breakpoints.len(), 1);
    assert_eq!(state.breakpoints[0].line, 1);
    assert!(state.breakpoints[0].enabled);
}

#[test]
fn multiline_indentation_and_line_ending_changes_keep_breakpoints() {
    let path = script_path("s.sh");
    let mut editor = EditorState::open(&path, b"one\ntwo\nthree\n").unwrap();
    editor.set_source_change_tracking(true);
    let mut state = ScriptState::default();
    state.toggle_breakpoint(&path, 2);
    editor.apply(EditorCommand::NormalizeLineEndings(
        app::editor::LineEnding::CrLf,
    ));
    state.remap_breakpoints(&path, &editor.take_source_line_changes());
    assert_eq!(state.breakpoints[0].line, 2);
    editor.apply(EditorCommand::SelectAll);
    editor.apply(EditorCommand::Indent {
        width: 2,
        tabs: false,
        outdent: false,
    });
    state.remap_breakpoints(&path, &editor.take_source_line_changes());
    assert_eq!(state.breakpoints[0].line, 2);
}

#[test]
fn batch_replacement_keeps_unchanged_middle_breakpoint_and_atomic_undo_redo() {
    use app::editor::search::{SearchEngine, SearchOptions};

    let original = "echo first\necho keep\necho last\n";
    for (query, replacement, expected, expected_line) in [
        ("^echo (first|last)\\n", "", "echo keep\n", 1),
        ("first|last", "x\nx", "echo x\nx\necho keep\necho x\nx\n", 3),
    ] {
        for track_before_replace in [true, false] {
            let path = script_path("batch.sh");
            let mut editor = EditorState::open(&path, original.as_bytes()).unwrap();
            editor.set_source_change_tracking(track_before_replace);
            let mut state = ScriptState::default();
            if track_before_replace {
                state.toggle_breakpoint(&path, 2);
            }
            let engine = SearchEngine::new(
                query,
                SearchOptions {
                    regex: true,
                    ..Default::default()
                },
            )
            .unwrap();
            let replacements = engine.preview(original, replacement);
            editor.apply(EditorCommand::ReplaceMatches {
                revision: editor.revision(),
                replacements,
            });
            assert_eq!(editor.source().as_ref(), expected);
            assert_eq!(editor.history_depth(), (1, 0));
            if track_before_replace {
                state.remap_breakpoints(&path, &editor.take_source_line_changes());
            } else {
                editor.set_source_change_tracking(true);
                state.toggle_breakpoint(&path, expected_line);
            }
            assert_eq!(state.breakpoints[0].line, expected_line);
            editor.apply(EditorCommand::Undo);
            state.remap_breakpoints(&path, &editor.take_source_line_changes());
            assert_eq!(editor.source().as_ref(), original);
            assert_eq!(state.breakpoints[0].line, 2);
            editor.apply(EditorCommand::Redo);
            state.remap_breakpoints(&path, &editor.take_source_line_changes());
            assert_eq!(editor.source().as_ref(), expected);
            assert_eq!(state.breakpoints[0].line, expected_line);
        }
    }
}

#[test]
fn crlf_formed_across_edit_boundaries_keeps_logical_breakpoint_rows() {
    for (original, range, replacement, expected, before_line, after_line) in [
        ("a\rb\nc\n", 2..2, "\n", "a\r\nb\nc\n", 2, 2),
        ("a\rX\nb\n", 2..3, "", "a\r\nb\n", 3, 2),
        ("a\rb\nc\r", 3..3, "\r", "a\rb\r\nc\r", 3, 3),
    ] {
        let path = script_path("mixed.sh");
        let mut editor = EditorState::open(&path, original.as_bytes()).unwrap();
        editor.set_source_change_tracking(true);
        let mut state = ScriptState::default();
        state.toggle_breakpoint(&path, before_line);
        editor.replace_source_range(SourceRange::new(range.start, range.end), replacement);
        state.remap_breakpoints(&path, &editor.take_source_line_changes());
        assert_eq!(editor.source().as_ref(), expected);
        assert_eq!(state.breakpoints[0].line, after_line);
        editor.apply(EditorCommand::Undo);
        state.remap_breakpoints(&path, &editor.take_source_line_changes());
        assert_eq!(editor.source().as_ref(), original);
        assert_eq!(state.breakpoints[0].line, before_line);
        editor.apply(EditorCommand::Redo);
        state.remap_breakpoints(&path, &editor.take_source_line_changes());
        assert_eq!(editor.source().as_ref(), expected);
        assert_eq!(state.breakpoints[0].line, after_line);
    }
}

#[test]
fn adjacent_batch_matches_use_the_previous_matchs_updated_crlf_context() {
    use app::editor::search::{SearchEngine, SearchOptions};
    let path = script_path("adjacent.sh");
    let original = "X\nkeep\n";
    let mut editor = EditorState::open(&path, original.as_bytes()).unwrap();
    editor.set_source_change_tracking(true);
    let mut state = ScriptState::default();
    state.toggle_breakpoint(&path, 2);
    let engine = SearchEngine::new(
        "X|\\n",
        SearchOptions {
            regex: true,
            ..Default::default()
        },
    )
    .unwrap();
    editor.apply(EditorCommand::ReplaceMatches {
        revision: editor.revision(),
        replacements: engine.preview(original, "\r"),
    });
    state.remap_breakpoints(&path, &editor.take_source_line_changes());
    assert_eq!(editor.source().as_ref(), "\r\rkeep\r");
    assert_eq!(state.breakpoints[0].line, 3);
    editor.apply(EditorCommand::Undo);
    state.remap_breakpoints(&path, &editor.take_source_line_changes());
    assert_eq!(editor.source().as_ref(), original);
    assert_eq!(state.breakpoints[0].line, 2);
    editor.apply(EditorCommand::Redo);
    state.remap_breakpoints(&path, &editor.take_source_line_changes());
    assert_eq!(state.breakpoints[0].line, 3);
}
