use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::*;
use unicode_width::UnicodeWidthStr;

#[test]
fn permission_titles_and_painted_checkboxes_share_columns_in_both_matrices() {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for language in ["en-US", "zh-CN"] {
        let snapshot = i18n::LanguageSnapshot::load(&assets, language, 1)
            .unwrap()
            .snapshot;
        let _language = i18n::enter_snapshot(std::sync::Arc::new(snapshot));
        for (width, height) in [(116, 41), (108, 20), (50, 16)] {
            let mut model = ManagementViewModel {
                form: Some(ManagementForm {
                    identity: "aligned-permissions".into(),
                    title: i18n::tr!("permissions-title"),
                    message: i18n::tr!("permissions-help"),
                    field_scroll: Some(0),
                    fields: [
                        ("files", "permissions-files", "1m0000000"),
                        ("directories", "permissions-directories", "010101010"),
                    ]
                    .into_iter()
                    .map(|(id, label, value)| ManagementFormField {
                        id: id.into(),
                        label: i18n::tr!(label),
                        value: value.into(),
                        control: ManagementFormControl::Permissions,
                        selected_cell: Some(0),
                        ..Default::default()
                    })
                    .collect(),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let theme = TundraTheme::default_dark();
            let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
            let main = ShellFrameLayout::new(Rect::new(0, 0, width, height), None, &context).main;
            let initial = management_layout(main, &model);
            let total_rows = initial
                .field_ranges
                .iter()
                .map(|(_, height)| height)
                .sum::<usize>();
            let mut seen = [[false; 9]; 2];
            let mut heading_centers = [[None; 3]; 2];
            for offset in 0..total_rows {
                model.form.as_mut().unwrap().field_scroll = Some(offset);
                let layout = management_layout(main, &model);
                let regions = management_button_regions(main, &model);
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| render_management_overlay(frame, main, &model, &context))
                    .unwrap();
                let form = model.form.as_ref().unwrap();
                for (index, area) in &layout.fields {
                    let field = &form.fields[*index];
                    let skip = layout
                        .field_start
                        .saturating_sub(layout.field_ranges[*index].0);
                    let cells = management_permission_cells(field, *area, skip);
                    if skip <= 1 && 1 - skip < usize::from(area.height) {
                        let heading_y = area.y + (1 - skip) as u16;
                        for (column, title) in [
                            i18n::tr!("permissions-read"),
                            i18n::tr!("permissions-write"),
                            i18n::tr!("permissions-execute"),
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            let Some((_, cell)) = cells.iter().find(|(cell, _)| cell % 3 == column)
                            else {
                                continue;
                            };
                            let painted_x = (cell.x..cell.right())
                                .find(|x| {
                                    !terminal.backend().buffer()[(*x, heading_y)]
                                        .symbol()
                                        .trim()
                                        .is_empty()
                                })
                                .expect("the permission title must be visible above its column");
                            let title_width = title.width() as u16;
                            assert!(
                                title_width <= cell.width,
                                "the fixture must fit the complete localized title"
                            );
                            assert_eq!(
                                painted_x,
                                cell.x + cell.width / 2 - title_width / 2,
                                "{language} {width}x{height} field {index} column {column}: title is offset from the centered checkbox"
                            );
                            heading_centers[*index][column] = Some(painted_x * 2 + title_width - 1);
                        }
                    }
                    for (cell, area) in cells {
                        let painted_x = (area.x..area.right())
                            .find(|x| terminal.backend().buffer()[(*x, area.y)].symbol() == "[")
                            .expect("the checkbox must be visible inside its button target");
                        assert_eq!(painted_x, area.x + area.width / 2 - 1);
                        if let Some(center) = heading_centers[*index][cell % 3] {
                            assert!(center.abs_diff((painted_x + 1) * 2) <= 1);
                        }
                        assert!(regions.iter().any(|region| {
                            region.id.as_str() == management_permission_id(form, *index, cell)
                                && region.area == area
                                && region.area.contains((painted_x + 1, area.y).into())
                        }));
                        seen[*index][cell] = true;
                    }
                }
            }
            assert!(
                seen.into_iter().flatten().all(|seen| seen),
                "{language} {width}x{height}: every checkbox must remain visible while scrolling"
            );
            assert!(
                heading_centers
                    .into_iter()
                    .flatten()
                    .all(|center| center.is_some()),
                "{language} {width}x{height}: both matrices must expose each aligned title"
            );
        }
    }
}

#[test]
fn keyboard_selection_keeps_each_permission_cell_visible_in_short_forms() {
    let main = Rect::new(0, 0, 108, 14);
    for cell in 0..9 {
        let field = ManagementFormField {
            id: "files".into(),
            value: "mmmmmmmmm".into(),
            control: ManagementFormControl::Permissions,
            selected_cell: Some(cell),
            ..Default::default()
        };
        let model = ManagementViewModel {
            form: Some(ManagementForm {
                identity: "short".into(),
                message: "Long explanation ".repeat(40),
                fields: vec![
                    field.clone(),
                    ManagementFormField {
                        id: "directories".into(),
                        ..field
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        };
        let regions = management_button_regions(main, &model);
        assert!(
            regions
                .iter()
                .any(|region| region.id.as_str()
                    == format!("management.field.short.files.cell.{cell}")),
            "cell {cell} is hidden"
        );
    }
}

#[test]
fn permission_matrix_has_nine_distinct_visible_button_targets() {
    for (width, height) in [(50, 16), (108, 20), (140, 40)] {
        let main = Rect::new(0, 0, width, height);
        let model = ManagementViewModel {
            form: Some(ManagementForm {
                identity: "permissions".into(),
                title: "权限 / Permissions".into(),
                fields: vec![ManagementFormField {
                    id: "files".into(),
                    label: "文件 / Files".into(),
                    value: "1m0000000".into(),
                    control: ManagementFormControl::Permissions,
                    selected_cell: Some(1),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let regions = management_button_regions(main, &model);
        let cells: Vec<_> = regions
            .iter()
            .filter(|region| region.id.as_str().contains(".cell."))
            .collect();
        assert_eq!(cells.len(), 9, "{width}x{height}");
        for (index, region) in cells.iter().enumerate() {
            assert_eq!(region.area.intersection(main), region.area);
            assert!(
                !cells
                    .iter()
                    .skip(index + 1)
                    .any(|other| other.area.intersects(region.area))
            );
        }
        let theme = TundraTheme::default_dark();
        let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render_management_overlay(frame, main, &model, &context))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("[x]"));
        assert!(text.contains("[-]"));
        assert!(text.contains("[ ]"));
    }
}
