use std::collections::HashSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SystemStatusWidgetKind {
    SystemOverview,
    Cpu,
    Memory,
    Storage,
    Network,
    Temperature,
    Battery,
    UptimeLoad,
    TopProcesses,
    Diagnostics,
    Activity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SystemStatusWidgetSize {
    Small,
    Wide,
    Large,
}

impl SystemStatusWidgetSize {
    pub const fn columns(self) -> u8 {
        match self {
            Self::Small => 2,
            Self::Wide | Self::Large => 4,
        }
    }

    pub const fn rows(self) -> u16 {
        match self {
            Self::Small | Self::Wide => 2,
            Self::Large => 4,
        }
    }

    pub const fn cycle(self) -> Self {
        match self {
            Self::Small => Self::Wide,
            Self::Wide => Self::Large,
            Self::Large => Self::Small,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WidgetPlacement {
    pub kind: SystemStatusWidgetKind,
    pub column: u8,
    pub row: u16,
    pub size: SystemStatusWidgetSize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardLayout {
    #[serde(default)]
    pub placements: Vec<WidgetPlacement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardProfile {
    Wide,
    Narrow,
}

impl DashboardProfile {
    pub const fn columns(self) -> u8 {
        match self {
            Self::Wide => 8,
            Self::Narrow => 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemStatusDashboardConfig {
    #[serde(default)]
    pub widgets: Vec<SystemStatusWidgetKind>,
    #[serde(default)]
    pub wide: DashboardLayout,
    #[serde(default)]
    pub narrow: DashboardLayout,
}

impl Default for SystemStatusDashboardConfig {
    fn default() -> Self {
        Self::for_role("User")
    }
}

impl SystemStatusDashboardConfig {
    pub fn for_role(role: &str) -> Self {
        let process_kind = if role.eq_ignore_ascii_case("Admin") {
            SystemStatusWidgetKind::TopProcesses
        } else {
            SystemStatusWidgetKind::Diagnostics
        };
        let widgets = vec![
            SystemStatusWidgetKind::SystemOverview,
            SystemStatusWidgetKind::Cpu,
            SystemStatusWidgetKind::Memory,
            SystemStatusWidgetKind::Storage,
            SystemStatusWidgetKind::Network,
            process_kind,
            SystemStatusWidgetKind::UptimeLoad,
        ];
        let wide = DashboardLayout {
            placements: vec![
                placement(
                    SystemStatusWidgetKind::SystemOverview,
                    0,
                    0,
                    SystemStatusWidgetSize::Wide,
                ),
                placement(
                    SystemStatusWidgetKind::Cpu,
                    4,
                    0,
                    SystemStatusWidgetSize::Small,
                ),
                placement(
                    SystemStatusWidgetKind::Memory,
                    6,
                    0,
                    SystemStatusWidgetSize::Small,
                ),
                placement(
                    SystemStatusWidgetKind::Storage,
                    0,
                    2,
                    SystemStatusWidgetSize::Small,
                ),
                placement(
                    SystemStatusWidgetKind::Network,
                    2,
                    2,
                    SystemStatusWidgetSize::Small,
                ),
                placement(process_kind, 4, 2, SystemStatusWidgetSize::Large),
                placement(
                    SystemStatusWidgetKind::UptimeLoad,
                    0,
                    4,
                    SystemStatusWidgetSize::Wide,
                ),
            ],
        };
        let mut config = Self {
            widgets,
            wide,
            narrow: DashboardLayout::default(),
        };
        config.normalize();
        config
    }

    pub fn normalize(&mut self) {
        let mut seen = HashSet::new();
        self.widgets.retain(|kind| seen.insert(*kind));
        normalize_layout(
            &mut self.wide,
            &self.widgets,
            DashboardProfile::Wide.columns(),
        );
        normalize_layout(
            &mut self.narrow,
            &self.widgets,
            DashboardProfile::Narrow.columns(),
        );
    }

    pub fn layout(&self, profile: DashboardProfile) -> &DashboardLayout {
        match profile {
            DashboardProfile::Wide => &self.wide,
            DashboardProfile::Narrow => &self.narrow,
        }
    }

    pub fn add_widget(&mut self, kind: SystemStatusWidgetKind) -> bool {
        if self.widgets.contains(&kind) {
            return false;
        }
        self.widgets.push(kind);
        for profile in [DashboardProfile::Wide, DashboardProfile::Narrow] {
            let layout = self.layout_mut(profile);
            let (column, row) = first_fit(
                &layout.placements,
                SystemStatusWidgetSize::Small,
                profile.columns(),
                0,
                0,
            );
            layout
                .placements
                .push(placement(kind, column, row, SystemStatusWidgetSize::Small));
        }
        true
    }

    pub fn remove_widget(&mut self, kind: SystemStatusWidgetKind) -> bool {
        let old_len = self.widgets.len();
        self.widgets.retain(|candidate| *candidate != kind);
        self.wide
            .placements
            .retain(|placement| placement.kind != kind);
        self.narrow
            .placements
            .retain(|placement| placement.kind != kind);
        old_len != self.widgets.len()
    }

    pub fn move_widget(
        &mut self,
        profile: DashboardProfile,
        kind: SystemStatusWidgetKind,
        column: u8,
        row: u16,
    ) -> bool {
        self.edit_placement(profile, kind, |placement| {
            placement.column = column;
            placement.row = row;
        })
    }

    pub fn resize_widget(
        &mut self,
        profile: DashboardProfile,
        kind: SystemStatusWidgetKind,
        size: SystemStatusWidgetSize,
    ) -> bool {
        self.edit_placement(profile, kind, |placement| placement.size = size)
    }

    fn layout_mut(&mut self, profile: DashboardProfile) -> &mut DashboardLayout {
        match profile {
            DashboardProfile::Wide => &mut self.wide,
            DashboardProfile::Narrow => &mut self.narrow,
        }
    }

    fn edit_placement(
        &mut self,
        profile: DashboardProfile,
        kind: SystemStatusWidgetKind,
        edit: impl FnOnce(&mut WidgetPlacement),
    ) -> bool {
        let columns = profile.columns();
        let layout = self.layout_mut(profile);
        let Some(index) = layout
            .placements
            .iter()
            .position(|placement| placement.kind == kind)
        else {
            return false;
        };
        let mut target = layout.placements.remove(index);
        edit(&mut target);
        target.column = target
            .column
            .min(columns.saturating_sub(target.size.columns()));

        let mut prior = std::mem::take(&mut layout.placements);
        prior.sort_by_key(|placement| (placement.row, placement.column, placement.kind));
        let mut rebuilt = vec![target];
        for mut placement in prior {
            placement.column = placement
                .column
                .min(columns.saturating_sub(placement.size.columns()));
            if overlaps_any(&placement, &rebuilt) {
                (placement.column, placement.row) = first_fit(
                    &rebuilt,
                    placement.size,
                    columns,
                    placement.column,
                    placement.row,
                );
            }
            rebuilt.push(placement);
        }
        layout.placements = rebuilt;
        true
    }
}

fn normalize_layout(layout: &mut DashboardLayout, widgets: &[SystemStatusWidgetKind], columns: u8) {
    let enabled: HashSet<_> = widgets.iter().copied().collect();
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    let mut input = std::mem::take(&mut layout.placements);
    input.sort_by_key(|placement| (placement.row, placement.column, placement.kind));
    for placement in input {
        if !enabled.contains(&placement.kind) || !seen.insert(placement.kind) {
            continue;
        }
        let mut placement = placement;
        placement.column = placement
            .column
            .min(columns.saturating_sub(placement.size.columns()));
        if overlaps_any(&placement, &normalized) {
            (placement.column, placement.row) = first_fit(
                &normalized,
                placement.size,
                columns,
                placement.column,
                placement.row,
            );
        }
        normalized.push(placement);
    }
    for kind in widgets.iter().copied().filter(|kind| !seen.contains(kind)) {
        let (column, row) = first_fit(&normalized, SystemStatusWidgetSize::Small, columns, 0, 0);
        normalized.push(placement(kind, column, row, SystemStatusWidgetSize::Small));
    }
    layout.placements = normalized;
}

fn first_fit(
    placed: &[WidgetPlacement],
    size: SystemStatusWidgetSize,
    columns: u8,
    start_column: u8,
    start_row: u16,
) -> (u8, u16) {
    let max_column = columns.saturating_sub(size.columns());
    for row in start_row..=u16::MAX {
        let first_column = if row == start_row {
            start_column.min(max_column)
        } else {
            0
        };
        for column in first_column..=max_column {
            let candidate = placement(SystemStatusWidgetKind::Activity, column, row, size);
            if !overlaps_any(&candidate, placed) {
                return (column, row);
            }
        }
    }
    // A placement at the last representable row cannot scan farther forward.
    // Wrap once so malformed persisted input still normalizes deterministically.
    for row in 0..start_row {
        for column in 0..=max_column {
            let candidate = placement(SystemStatusWidgetKind::Activity, column, row, size);
            if !overlaps_any(&candidate, placed) {
                return (column, row);
            }
        }
    }
    // The persisted coordinate type has no representable free position when the
    // complete grid is occupied. Keep the result bounded and deterministic.
    (0, start_row)
}

fn overlaps_any(candidate: &WidgetPlacement, placed: &[WidgetPlacement]) -> bool {
    placed.iter().any(|other| {
        u16::from(candidate.column) < u16::from(other.column) + u16::from(other.size.columns())
            && u16::from(other.column)
                < u16::from(candidate.column) + u16::from(candidate.size.columns())
            && u32::from(candidate.row) < u32::from(other.row) + u32::from(other.size.rows())
            && u32::from(other.row) < u32::from(candidate.row) + u32::from(candidate.size.rows())
    })
}

fn placement(
    kind: SystemStatusWidgetKind,
    column: u8,
    row: u16,
    size: SystemStatusWidgetSize,
) -> WidgetPlacement {
    WidgetPlacement {
        kind,
        column,
        row,
        size,
    }
}

#[cfg(test)]
mod tests {
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
    fn serde_is_snake_case_and_round_trips() {
        let config = SystemStatusDashboardConfig::for_role("Admin");
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("system_overview"));
        assert!(json.contains("top_processes"));
        assert_eq!(
            serde_json::from_str::<SystemStatusDashboardConfig>(&json).unwrap(),
            config
        );
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
        assert!(config.add_widget(SystemStatusWidgetKind::Activity));
        assert!(
            config
                .wide
                .placements
                .iter()
                .any(|p| p.kind == SystemStatusWidgetKind::Activity)
        );
        assert!(
            config
                .narrow
                .placements
                .iter()
                .any(|p| p.kind == SystemStatusWidgetKind::Activity)
        );
        assert!(config.remove_widget(SystemStatusWidgetKind::Activity));
        assert!(!config.widgets.contains(&SystemStatusWidgetKind::Activity));
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
}
