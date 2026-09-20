use super::*;

#[test]
fn role_defaults_select_privileged_widget_and_cover_both_profiles() {
    let admin = SystemStatusDashboardConfig::for_role("Admin");
    let user = SystemStatusDashboardConfig::for_role("User");
    assert!(
        admin
            .widgets
            .contains(&SystemStatusWidgetKind::TopProcesses)
    );
    assert!(!admin.widgets.contains(&SystemStatusWidgetKind::Diagnostics));
    assert!(user.widgets.contains(&SystemStatusWidgetKind::Diagnostics));
    assert!(!user.widgets.contains(&SystemStatusWidgetKind::TopProcesses));
    for config in [&admin, &user] {
        assert_eq!(config.widgets.len(), config.wide.placements.len());
        assert_eq!(config.widgets.len(), config.narrow.placements.len());
        for kind in &config.widgets {
            assert_eq!(
                config
                    .wide
                    .placements
                    .iter()
                    .filter(|p| p.kind == *kind)
                    .count(),
                1
            );
            assert_eq!(
                config
                    .narrow
                    .placements
                    .iter()
                    .filter(|p| p.kind == *kind)
                    .count(),
                1
            );
        }
    }
}

#[test]
fn normalization_deduplicates_clamps_and_resolves_overlap_without_compacting_gaps() {
    let mut config = SystemStatusDashboardConfig {
        widgets: vec![
            SystemStatusWidgetKind::Cpu,
            SystemStatusWidgetKind::Cpu,
            SystemStatusWidgetKind::Memory,
            SystemStatusWidgetKind::Storage,
        ],
        wide: DashboardLayout {
            placements: vec![
                placement(
                    SystemStatusWidgetKind::Cpu,
                    99,
                    5,
                    SystemStatusWidgetSize::Large,
                ),
                placement(
                    SystemStatusWidgetKind::Cpu,
                    0,
                    0,
                    SystemStatusWidgetSize::Small,
                ),
                placement(
                    SystemStatusWidgetKind::Memory,
                    6,
                    5,
                    SystemStatusWidgetSize::Small,
                ),
                placement(
                    SystemStatusWidgetKind::Storage,
                    99,
                    10,
                    SystemStatusWidgetSize::Small,
                ),
            ],
        },
        narrow: DashboardLayout::default(),
    };
    config.normalize();
    assert_eq!(
        config.widgets,
        vec![
            SystemStatusWidgetKind::Cpu,
            SystemStatusWidgetKind::Memory,
            SystemStatusWidgetKind::Storage
        ]
    );
    assert_eq!(
        (
            config.wide.placements[0].column,
            config.wide.placements[0].row
        ),
        (0, 0)
    );
    assert_eq!(
        (
            config.wide.placements[1].column,
            config.wide.placements[1].row
        ),
        (6, 5)
    );
    assert_eq!(
        (
            config.wide.placements[2].column,
            config.wide.placements[2].row
        ),
        (6, 10)
    );
    assert_eq!(config.narrow.placements.len(), 3);
}

#[test]
fn target_first_move_cascades_collisions_deterministically() {
    let mut config = SystemStatusDashboardConfig {
        widgets: vec![
            SystemStatusWidgetKind::Cpu,
            SystemStatusWidgetKind::Memory,
            SystemStatusWidgetKind::Storage,
        ],
        wide: DashboardLayout {
            placements: vec![
                placement(
                    SystemStatusWidgetKind::Cpu,
                    0,
                    0,
                    SystemStatusWidgetSize::Small,
                ),
                placement(
                    SystemStatusWidgetKind::Memory,
                    2,
                    0,
                    SystemStatusWidgetSize::Wide,
                ),
                placement(
                    SystemStatusWidgetKind::Storage,
                    4,
                    0,
                    SystemStatusWidgetSize::Large,
                ),
            ],
        },
        narrow: DashboardLayout::default(),
    };
    assert!(config.move_widget(
        DashboardProfile::Wide,
        SystemStatusWidgetKind::Storage,
        0,
        0
    ));
    let position = |kind| {
        config
            .wide
            .placements
            .iter()
            .find(|p| p.kind == kind)
            .map(|p| (p.column, p.row))
            .unwrap()
    };
    assert_eq!(position(SystemStatusWidgetKind::Storage), (0, 0));
    assert_eq!(position(SystemStatusWidgetKind::Cpu), (4, 0));
    assert_eq!(position(SystemStatusWidgetKind::Memory), (4, 2));
}

#[test]
fn profile_edits_are_separate_while_catalog_add_remove_is_shared() {
    let mut config = SystemStatusDashboardConfig::for_role("User");
    let narrow_before = config.narrow.clone();
    config.move_widget(DashboardProfile::Wide, SystemStatusWidgetKind::Cpu, 0, 20);
    assert_eq!(config.narrow, narrow_before);
    assert!(config.add_widget(SystemStatusWidgetKind::Logs));
    assert!(
        config
            .wide
            .placements
            .iter()
            .any(|p| p.kind == SystemStatusWidgetKind::Logs)
    );
    assert!(
        config
            .narrow
            .placements
            .iter()
            .any(|p| p.kind == SystemStatusWidgetKind::Logs)
    );
    assert!(config.remove_widget(SystemStatusWidgetKind::Logs));
    assert!(!config.widgets.contains(&SystemStatusWidgetKind::Logs));
}

#[test]
fn first_fit_at_maximum_row_wraps_once_instead_of_looping() {
    let occupied = vec![
        placement(
            SystemStatusWidgetKind::Cpu,
            0,
            u16::MAX,
            SystemStatusWidgetSize::Small,
        ),
        placement(
            SystemStatusWidgetKind::Memory,
            2,
            u16::MAX,
            SystemStatusWidgetSize::Small,
        ),
    ];
    assert_eq!(
        first_fit(&occupied, SystemStatusWidgetSize::Small, 4, 0, u16::MAX),
        (0, 0)
    );
}
