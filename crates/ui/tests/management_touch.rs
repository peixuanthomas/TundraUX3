use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::*;

#[test]
fn compact_toolbar_keeps_every_basic_touch_action_visible() {
    let model = ManagementViewModel {
        actions: vec![("Start selected service".into(), true)],
        ..Default::default()
    };
    for size in [(64, 14), (60, 18), (80, 24)] {
        let area = Rect::new(0, 0, size.0, size.1);
        let layout = management_layout(area, &model);
        assert_eq!(layout.controls.len(), 6);
        assert_eq!(layout.controls[0].0, ManagementControl::Refresh);
        assert_eq!(layout.controls[0].1.x, area.x);
        assert!(layout.controls.iter().all(|(_, rect)| rect.width > 0
            && rect.height > 0
            && rect.right() <= area.right()
            && rect.bottom() <= area.bottom()));
        assert!(!layout.actions.is_empty());
    }
}

#[test]
fn overflowing_lists_text_and_actions_have_matching_drag_geometry() {
    let model = ManagementViewModel {
        columns: vec!["Name".into()],
        rows: (0..80).map(|i| vec![format!("row-{i}")]).collect(),
        details: "long description\n".repeat(80),
        actions: (0..40).map(|i| (format!("Action {i}"), true)).collect(),
        ..Default::default()
    };
    let layout = management_layout(Rect::new(0, 0, 120, 30), &model);
    for target in [
        ManagementScrollTarget::Rows,
        ManagementScrollTarget::Details,
        ManagementScrollTarget::Actions,
    ] {
        let bar = layout
            .scrollbars
            .iter()
            .find(|bar| bar.target == target)
            .unwrap();
        assert_eq!(bar.offset_at((bar.track.x, bar.track.y), 0), 0);
        assert_eq!(
            bar.offset_at((bar.track.x, bar.track.bottom()), 0),
            bar.content_len - bar.viewport_len
        );
    }
}

#[test]
fn long_form_fields_messages_and_choices_remain_scrollable() {
    let mut model = ManagementViewModel {
        form: Some(ManagementForm {
            message: "dependency\n".repeat(100),
            fields: (0..30)
                .map(|_| ManagementFormField {
                    label: "Field".into(),
                    value: "A value".into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let area = Rect::new(0, 0, 64, 18);
    let layout = management_layout(area, &model);
    for target in [
        ManagementScrollTarget::FormMessage,
        ManagementScrollTarget::FormFields,
    ] {
        assert!(layout.scrollbars.iter().any(|bar| bar.target == target));
    }
    model.form.as_mut().unwrap().choice = Some(ManagementChoices {
        values: (0..80)
            .map(|i| format!("A very long choice path /directory/{i}/{}", "x".repeat(100)))
            .collect(),
        ..Default::default()
    });
    let layout = management_layout(area, &model);
    assert!(
        layout
            .scrollbars
            .iter()
            .any(|bar| bar.target == ManagementScrollTarget::Choices)
    );
    assert!(
        layout
            .scrollbars
            .iter()
            .any(|bar| bar.target == ManagementScrollTarget::ChoiceColumns)
    );
    assert!(layout.choice_cancel.width > 0);
}

#[test]
fn management_panels_and_form_inherit_square_and_rounded_borders() {
    for (shape, corner) in [(BorderShape::Square, "┌"), (BorderShape::Rounded, "╭")] {
        let theme = TundraTheme::default_dark().with_border_shape(shape);
        let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        let mut model = ManagementViewModel::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        let area = Rect::new(0, 0, 120, 36);
        let layout = management_layout(area, &model);
        terminal
            .draw(|frame| render_management_content(frame, area, &model, &context))
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(layout.list.x, layout.list.y)].symbol(),
            corner
        );
        assert_eq!(
            terminal.backend().buffer()[(layout.details.x, layout.details.y)].symbol(),
            corner
        );
        model.form = Some(ManagementForm {
            title: "Confirm".into(),
            ..Default::default()
        });
        let layout = management_layout(area, &model);
        terminal
            .draw(|frame| render_management_overlay(frame, area, &model, &context))
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(layout.form.x, layout.form.y)].symbol(),
            corner
        );
    }
}

#[test]
fn management_rendered_button_ids_match_capture_regions_and_show_pressed_color() {
    use ui::components::ButtonFrame;
    let model = ManagementViewModel {
        scope_id: "services".into(),
        action_ids: vec!["management.action.services.start.row-42".into()],
        actions: vec![("Start".into(), true)],
        ..Default::default()
    };
    let area = Rect::new(0, 0, 120, 36);
    let region = management_button_regions(area, &model)
        .into_iter()
        .find(|region| region.id.as_str() == "management.action.services.start.row-42")
        .unwrap();
    let theme = TundraTheme::default_dark();
    let buttons = ButtonFrame::new(Some(region.clone()), Some(region.clone()), &theme);
    let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    context.buttons = Some(buttons.clone());
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    terminal
        .draw(|frame| render_management_content(frame, area, &model, &context))
        .unwrap();
    assert!(buttons.regions().contains(&region));
    assert_eq!(
        terminal.backend().buffer()[(region.area.x, region.area.y)].fg,
        theme.accent_color
    );
    let mut model = model;
    model.form = Some(ManagementForm {
        identity: "question-1".into(),
        fields: vec![ManagementFormField {
            id: "answer".into(),
            label: "Answer".into(),
            value: "Yes".into(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let capture = management_button_regions(area, &model);
    let buttons = ButtonFrame::new(None, None, &theme);
    context.buttons = Some(buttons.clone());
    terminal
        .draw(|frame| render_management_overlay(frame, area, &model, &context))
        .unwrap();
    for region in capture {
        assert!(buttons.regions().contains(&region), "missing {:?}", region);
    }
    model.form.as_mut().unwrap().choice = Some(ManagementChoices {
        values: vec!["No".into(), "Yes".into()],
        ..Default::default()
    });
    let capture = management_button_regions(area, &model);
    let buttons = ButtonFrame::new(None, None, &theme);
    context.buttons = Some(buttons.clone());
    terminal
        .draw(|frame| render_management_overlay(frame, area, &model, &context))
        .unwrap();
    for region in capture {
        assert!(buttons.regions().contains(&region), "missing {:?}", region);
    }
}
