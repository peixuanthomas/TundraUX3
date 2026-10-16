use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::*;

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
