use std::path::PathBuf;
use std::sync::Arc;

use app::script::{ScriptBreakpoint, ScriptLocation, ScriptState, ScriptStatus, ScriptVariable};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::{
    EditorViewModel, RenderContext, ScriptAction, ScriptHitTarget, ScriptPane, ScriptViewModel,
    TundraTheme, render_script_contextual, script_editor_area, script_editor_area_with_state,
    script_layout,
};

fn model() -> ScriptViewModel {
    let mut editor = EditorViewModel::source(
        "test.sh",
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$value\"\n",
    );
    editor.path_hint = Some("/tmp/test.sh".into());
    ScriptViewModel::new(editor, ScriptState::default())
}

#[test]
fn detail_motion_draws_borders_then_restores_source_and_details() {
    let mut model = model();
    model.detail = ScriptPane::Expression;
    model.state.expression_log.push("count = 42".into());
    let context = RenderContext::default();
    let mut terminal = Terminal::new(TestBackend::new(140, 36)).unwrap();
    for progress in [500, 999, 1_000, 500, 0] {
        model.detail_expansion = Some(progress);
        let layout = script_layout(Rect::new(0, 0, 140, 36), &model);
        terminal
            .draw(|frame| {
                render_script_contextual(frame, frame.area(), &model, &context);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert_eq!(
            buffer[(layout.editor_area.x, layout.editor_area.y)].symbol(),
            "╭"
        );
        if progress > 0 && progress < 1_000 {
            assert!(!text.contains("#!/usr/bin/env bash"));
            assert!(!text.contains("count = 42"));
            assert!(
                layout
                    .editor_area
                    .inner(ratatui::layout::Margin::new(1, 1))
                    .positions()
                    .all(|position| buffer[position].symbol() == " ")
            );
        } else {
            assert!(text.contains("#!/usr/bin/env bash"));
            if progress == 1_000 {
                assert!(text.contains("count = 42"));
            }
        }
    }
}

#[test]
fn script_launcher_reuses_editor_art_in_existing_themes() {
    let assets = ui::RuntimeAsciiAssets::load_default().unwrap();
    assert!(std::ptr::eq(
        assets.launcher_icon("builtin.editor").unwrap(),
        assets.launcher_icon("builtin.script-editor").unwrap()
    ));
    assert_eq!(
        assets.launcher_icon_image_bytes("builtin.editor"),
        assets.launcher_icon_image_bytes("builtin.script-editor")
    );
}

#[test]
fn wide_workspace_defaults_to_source_and_output_and_opens_one_detail_at_a_time() {
    let mut model = model();
    let area = Rect::new(4, 3, 140, 36);
    let layout = script_layout(area, &model);
    assert!(layout.wide);
    assert_eq!(layout.editor_area, script_editor_area(area));
    assert_eq!(layout.panes.len(), 1);
    assert!(!layout.pane(ScriptPane::Output).is_empty());
    assert_eq!(layout.tabs.len(), 5);
    let default_source = layout.editor_area;
    for pane in ScriptPane::ALL.into_iter().skip(2) {
        model.detail = pane;
        let expanded = script_layout(area, &model);
        assert_eq!(expanded.panes.len(), 2);
        assert!(!expanded.pane(pane).is_empty());
        assert!(!expanded.pane(ScriptPane::Output).is_empty());
        assert!(expanded.editor_area.width < default_source.width);
        assert!(!expanded.pane(pane).intersects(expanded.editor_area));
        assert!(
            !expanded
                .pane(pane)
                .intersects(expanded.pane(ScriptPane::Output))
        );
    }
    for (_, pane) in &layout.panes {
        assert!(!pane.is_empty());
        assert!(area.contains((pane.x, pane.y).into()));
        assert!(pane.right() <= area.right() && pane.bottom() <= layout.status.y);
        assert!(!pane.intersects(layout.editor_area));
    }
    for (action, rect) in &layout.actions {
        assert!(!rect.is_empty());
        assert_eq!(
            layout.hit_test(rect.x, rect.y),
            Some(ScriptHitTarget::Action(*action))
        );
    }
}

#[test]
fn compact_workspace_keeps_all_details_accessible_after_resize() {
    let mut model = model();
    for pane in ScriptPane::ALL.into_iter().skip(1) {
        model.detail = pane;
        let layout = script_layout(Rect::new(0, 0, 108, 20), &model);
        assert!(!layout.wide);
        assert_eq!(layout.tabs.len(), 5);
        assert!(layout.pane(pane).height >= 3);
        assert_eq!(layout.editor_area, script_editor_area(layout.area));
        assert!(
            layout
                .actions
                .iter()
                .any(|(action, _)| *action == ScriptAction::Run)
        );
        for (tab, area) in &layout.tabs {
            assert_eq!(
                layout.hit_test(area.x, area.y),
                Some(ScriptHitTarget::Tab(*tab))
            );
        }
    }
}

#[test]
fn primary_actions_follow_runtime_state_and_share_source_geometry_after_wrapping() {
    let mut model = model();
    for (status, expected) in [
        (
            ScriptStatus::Idle,
            vec![
                ScriptAction::Run,
                ScriptAction::Debug,
                ScriptAction::Settings,
            ],
        ),
        (
            ScriptStatus::Finished,
            vec![
                ScriptAction::Run,
                ScriptAction::Debug,
                ScriptAction::Settings,
            ],
        ),
        (
            ScriptStatus::Failed,
            vec![
                ScriptAction::Run,
                ScriptAction::Debug,
                ScriptAction::Settings,
            ],
        ),
        (ScriptStatus::Running, vec![ScriptAction::Stop]),
        (ScriptStatus::Saving, vec![ScriptAction::Stop]),
        (ScriptStatus::Starting, vec![ScriptAction::Stop]),
        (ScriptStatus::Stopping, vec![ScriptAction::Stop]),
        (
            ScriptStatus::Paused,
            vec![
                ScriptAction::Continue,
                ScriptAction::Next,
                ScriptAction::Step,
                ScriptAction::Finish,
                ScriptAction::Stop,
            ],
        ),
    ] {
        model.state.status = status;
        model.editor.dirty = true;
        for (width, height) in [(108, 20), (140, 36), (48, 20)] {
            let area = Rect::new(3, 2, width, height);
            let layout = script_layout(area, &model);
            assert_eq!(
                layout
                    .actions
                    .iter()
                    .map(|(action, _)| *action)
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(
                layout.editor_area,
                script_editor_area_with_state(area, status, true, ScriptPane::Output, None)
            );
            for (action, rect) in &layout.actions {
                assert!(rect.y > layout.status.y);
                assert!(rect.bottom() <= area.bottom());
                assert!(rect.right() <= area.right());
                assert_eq!(
                    layout.hit_test(rect.x, rect.y),
                    Some(ScriptHitTarget::Action(*action))
                );
            }
        }
    }
}

#[test]
fn breakpoint_gutter_tracks_logical_lines_when_source_wraps() {
    let mut model = model();
    let source = format!("{}\nprintf done\n", "abcdef好".repeat(40));
    let state =
        app::editor::EditorState::open(PathBuf::from("/tmp/test.sh"), source.as_bytes()).unwrap();
    model.editor = EditorViewModel::source("test.sh", &source);
    model.editor.path_hint = Some("/tmp/test.sh".into());
    model.editor.soft_wrap = true;
    model.editor.wrap_layout = state.source_wrap_layout(30);
    model.state.breakpoints.push(ScriptBreakpoint {
        path: "/tmp/test.sh".into(),
        line: 1,
        enabled: true,
        verified: true,
        rejected: false,
    });
    model.state.location = Some(ScriptLocation {
        path: "/tmp/test.sh".into(),
        line: 1,
    });
    let layout = script_layout(Rect::new(0, 0, 140, 36), &model);
    assert_eq!(
        layout
            .breakpoint_lines
            .iter()
            .filter(|(line, _)| *line == 1)
            .count(),
        1
    );
    let (_, rect) = layout.breakpoint_lines[0];
    assert_eq!(
        layout.hit_test(rect.x, rect.y),
        Some(ScriptHitTarget::BreakpointLine(1))
    );
    assert_eq!(model.decorated_editor().script_execution_line, Some(1));
}

#[test]
fn variable_expansion_exposes_children_without_losing_parent_values() {
    let mut model = model();
    model.state.variables.push(ScriptVariable {
        name: "items".into(),
        value: "array[1]".into(),
        children: vec![ScriptVariable {
            name: "[0]".into(),
            value: "\"中文 and spaces\"".into(),
            children: vec![],
        }],
    });
    let key = model.variable_rows()[0].key.clone();
    assert_eq!(model.variable_rows().len(), 1);
    model.expanded_variables.insert(key);
    let rows = model.variable_rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].depth, 1);
    assert_eq!(rows[1].variable.value, "\"中文 and spaces\"");
}

#[test]
fn rejected_pending_and_verified_breakpoints_have_distinct_markers_and_labels() {
    let mut model = model();
    model.detail = ScriptPane::Breakpoints;
    model.editor =
        EditorViewModel::source("test.sh", "#!/usr/bin/env bash\nprintf one\nprintf two\n");
    model.editor.path_hint = Some("/tmp/test.sh".into());
    model.state.breakpoints = [(1, false, true), (2, false, false), (3, true, false)]
        .into_iter()
        .map(|(line, verified, rejected)| ScriptBreakpoint {
            path: "/tmp/test.sh".into(),
            line,
            enabled: true,
            verified,
            rejected,
        })
        .collect();
    let context = RenderContext::default();
    let area = Rect::new(0, 0, 140, 36);
    let layout = script_layout(area, &model);
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal
        .draw(|frame| {
            render_script_contextual(frame, area, &model, &context);
        })
        .unwrap();
    for (line, symbol, color) in [
        (1, "!", context.theme.danger),
        (2, "?", context.theme.warning),
        (3, "B", context.theme.accent),
    ] {
        let (_, rect) = layout
            .breakpoint_lines
            .iter()
            .find(|(candidate, _)| *candidate == line)
            .unwrap();
        let cell = &terminal.backend().buffer()[(rect.x, rect.y)];
        assert_eq!(cell.symbol(), symbol);
        assert_eq!(cell.fg, color);
    }
    let rendered: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    for key in [
        "script-breakpoint-rejected",
        "script-breakpoint-unverified",
        "script-breakpoint-verified",
    ] {
        assert!(rendered.contains(&i18n::tr!(key)));
    }
    assert!(
        model
            .decorated_editor()
            .script_breakpoints
            .as_ref()
            .unwrap()[0]
            .rejected
    );
}

#[test]
fn expression_actions_are_disabled_until_a_nonempty_paused_input_exists() {
    let mut model = model();
    model.expression.set_value("count + 1");
    assert!(!model.action_enabled(ScriptAction::Examine));
    model.state.status = ScriptStatus::Paused;
    assert!(model.action_enabled(ScriptAction::Examine));
    assert!(model.action_enabled(ScriptAction::Evaluate));
    assert!(!model.action_enabled(ScriptAction::Run));
    model.expression.set_value("");
    assert!(!model.action_enabled(ScriptAction::Evaluate));
}

#[test]
fn long_directory_paths_keep_breakpoint_status_and_stack_line_visible() {
    let mut model = model();
    let path = PathBuf::from(format!(
        "/tmp/{}/run script.sh",
        "中文 空格长目录/".repeat(12)
    ));
    model.state.breakpoints = [(1234, true), (1235, false)]
        .into_iter()
        .map(|(line, rejected)| ScriptBreakpoint {
            path: path.clone(),
            line,
            enabled: true,
            verified: false,
            rejected,
        })
        .collect();
    model.state.stack.push(app::script::ScriptStackFrame {
        index: 0,
        name: "bump".into(),
        path,
        line: 2345,
    });
    let area = Rect::new(0, 0, 140, 36);
    let context = RenderContext::default();
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    for (pane, index, number, status) in [
        (
            ScriptPane::Breakpoints,
            0,
            "1234",
            Some("script-breakpoint-rejected"),
        ),
        (
            ScriptPane::Breakpoints,
            1,
            "1235",
            Some("script-breakpoint-unverified"),
        ),
        (ScriptPane::Stack, 0, "bump:2345", None),
    ] {
        model.detail = pane;
        let layout = script_layout(area, &model);
        terminal
            .draw(|frame| {
                render_script_contextual(frame, area, &model, &context);
            })
            .unwrap();
        let (_, _, rect) = layout
            .rows
            .iter()
            .find(|(row_pane, row_index, _)| *row_pane == pane && *row_index == index)
            .unwrap();
        let row: String = (rect.x..rect.right())
            .map(|x| terminal.backend().buffer()[(x, rect.y)].symbol())
            .collect();
        assert!(row.contains(number), "{row}");
        assert!(row.contains("run script.sh"), "{row}");
        if let Some(key) = status {
            assert!(row.contains(&i18n::tr!(key)), "{row}");
        }
        assert!(!row.contains("/tmp/"), "{row}");
    }
}

#[test]
fn multiline_expression_results_wrap_and_output_scrollbar_uses_terminal_offset() {
    let mut model = model();
    model.detail = ScriptPane::Expression;
    model
        .state
        .expression_log
        .push("first\nsecond\u{1b}[31m".into());
    let mut output = ui::CommandLineTerminalSnapshot::blank(40, 9);
    output.scrollback_rows = 100;
    output.scrollback_offset = 75;
    model.output = Some(Arc::new(output));
    let layout = script_layout(Rect::new(0, 0, 140, 36), &model);
    assert_eq!(layout.expression_lines, ["first", "second[31m"]);
    assert!(
        layout
            .expression_lines
            .iter()
            .all(|line| !line.chars().any(char::is_control))
    );
    let mut terminal = Terminal::new(TestBackend::new(140, 36)).unwrap();
    terminal
        .draw(|frame| {
            render_script_contextual(frame, frame.area(), &model, &RenderContext::default());
        })
        .unwrap();
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .all(|cell| !cell.symbol().chars().any(char::is_control))
    );
    let (_, scrollbar) = layout
        .scrollbars
        .iter()
        .find(|(pane, _)| *pane == ScriptPane::Output)
        .unwrap();
    let expected = ui::components::Scrollbar::new(109, scrollbar.track.height as usize, 25);
    let (start, len) = expected.thumb_range(scrollbar.track);
    assert_eq!(scrollbar.thumb.y, scrollbar.track.y + start);
    assert_eq!(scrollbar.thumb.height, len);
}

#[test]
fn rendering_preserves_source_and_expression_content_in_both_sizes() {
    let mut model = model();
    model.detail = ScriptPane::Expression;
    model.focus = ScriptPane::Expression;
    model.state.status = ScriptStatus::Paused;
    model.state.expression_log.push("count = 42".into());
    model.expression.set_value("count + 1");
    model.output = Some(Arc::new(ui::CommandLineTerminalSnapshot::blank(20, 4)));
    let theme = TundraTheme::default_dark();
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    for (width, height) in [(140, 36), (108, 20)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                render_script_contextual(frame, frame.area(), &model, &context);
            })
            .unwrap();
        let output: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(output.contains("count + 1"));
        assert!(output.contains("count = 42"));
        assert!(output.contains("#!/usr/bin/env bash"));
    }
}
