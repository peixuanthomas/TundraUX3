use super::*;
use chrono::{Days, Local, NaiveDate, TimeZone};

const SOURCES: &[&str] = &[
    "",
    "system-config",
    "system-settings",
    "users",
    "network",
    "firewall",
    "ssh",
    "scheduled-tasks",
    "restore",
    "legacy",
];

fn source_label(source: &str) -> String {
    match source {
        "" => i18n::tr!("backup-filter-all-sources"),
        "system-config" => i18n::tr!("platform-backups-source-config"),
        "system-settings" => i18n::tr!("system-settings-title"),
        "users" => i18n::tr!("platform-backups-source-users"),
        "network" => i18n::tr!("platform-backups-source-network"),
        "firewall" => i18n::tr!("platform-backups-source-firewall"),
        "ssh" => i18n::tr!("platform-backups-source-ssh"),
        "scheduled-tasks" => i18n::tr!("platform-backups-source-scheduled"),
        "restore" => i18n::tr!("platform-backups-source-restore"),
        _ => i18n::tr!("platform-backups-source-legacy"),
    }
}

fn date_option(query: Option<&ManagementQuery>, key: &str) -> String {
    query
        .and_then(|query| query.options.get(key))
        .and_then(|value| value.parse().ok())
        .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0))
        .map(|time| time.with_timezone(&Local).format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

impl ShellSession {
    pub(super) fn merge_backup_details(
        &self,
        snapshot: &mut ManagementSnapshot,
        query: Option<&ManagementQuery>,
    ) {
        if self.pages.management.management_state.kind != Some(ManagementKind::Backups) {
            return;
        }
        let Some(id) = query.and_then(|query| query.target.as_deref()) else {
            return;
        };
        if snapshot.rows.len() != 1 || snapshot.rows[0].id != id {
            return;
        }
        let mut list = self.pages.management.management_state.snapshot.clone();
        let Some(row) = list.rows.iter_mut().find(|row| row.id == id) else {
            return;
        };
        *row = snapshot.rows.remove(0);
        list.details_complete = false;
        *snapshot = list;
    }

    pub(super) fn backup_filter_action(&self) -> ManagementAction {
        let query = self.pages.management.management_state.query.as_ref();
        let source = query
            .and_then(|query| query.options.get("source"))
            .map_or("", String::as_str);
        ManagementAction {
            id: "filter_backups".into(),
            label: i18n::tr!("backup-filter-title"),
            fields: vec![
                ManagementField {
                    id: "from_date".into(),
                    label: i18n::tr!("backup-filter-from"),
                    value: date_option(query, "from_timestamp"),
                    ..Default::default()
                },
                ManagementField {
                    id: "to_date".into(),
                    label: i18n::tr!("backup-filter-to"),
                    value: date_option(query, "to_timestamp"),
                    ..Default::default()
                },
                ManagementField {
                    id: "source".into(),
                    label: i18n::tr!("backup-filter-source"),
                    value: source_label(source),
                    choices: SOURCES.iter().map(|source| source_label(source)).collect(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    pub(super) fn handle_backup_view_action(&mut self, action: &ManagementAction) -> bool {
        if self.pages.management.management_state.kind != Some(ManagementKind::Backups) {
            return false;
        }
        let state = &mut self.pages.management.management_state;
        match action.id.as_str() {
            "backups_next_page" | "backups_first_page" => {
                state.query = Some(ManagementQuery::from_inspection(&ManagementCommand {
                    kind: ManagementKind::Backups,
                    action: action.id.clone(),
                    target: None,
                    values: action.values.clone(),
                    identity: Default::default(),
                }));
            }
            "preview_cleanup" | "cancel_cleanup_preview" => {
                if let Some(query) = &mut state.query {
                    query.scope = if action.id == "preview_cleanup" {
                        "cleanup-preview".into()
                    } else {
                        String::new()
                    };
                    query.target = None;
                    query.options.remove("cursor");
                    query.options.remove("anchor");
                }
            }
            "filter_backups" => {
                let value = |id: &str| {
                    action
                        .fields
                        .iter()
                        .find(|field| field.id == id)
                        .map_or("", |field| field.value.trim())
                };
                let parse = |text: &str, end: bool| -> Result<Option<i64>, ()> {
                    if text.is_empty() {
                        return Ok(None);
                    }
                    let mut date = NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| ())?;
                    if end {
                        date = date.checked_add_days(Days::new(1)).ok_or(())?;
                    }
                    let timestamp = Local
                        .from_local_datetime(&date.and_time(chrono::NaiveTime::MIN))
                        .earliest()
                        .ok_or(())?
                        .timestamp();
                    Ok(Some(timestamp - i64::from(end)))
                };
                let range = parse(value("from_date"), false)
                    .and_then(|from| parse(value("to_date"), true).map(|to| (from, to)))
                    .and_then(|(from, to)| {
                        if from.zip(to).is_some_and(|(from, to)| from > to) {
                            Err(())
                        } else {
                            Ok((from, to))
                        }
                    });
                let Ok((from, to)) = range else {
                    state.form = Some(ManagementEditor {
                        instance: next_management_form_instance(),
                        title: action.label.clone(),
                        message: i18n::tr!("backup-filter-invalid-date"),
                        message_scroll: 0,
                        fields: action.fields.clone(),
                        selected: 0,
                        purpose: FormPurpose::Action(action.clone(), None),
                    });
                    return true;
                };
                if let Some(query) = &mut state.query {
                    for (key, value) in [("from_timestamp", from), ("to_timestamp", to)] {
                        if let Some(value) = value {
                            query.options.insert(key.into(), value.to_string());
                        } else {
                            query.options.remove(key);
                        }
                    }
                    let source = SOURCES
                        .iter()
                        .find(|source| source_label(source) == value("source"))
                        .copied()
                        .unwrap_or("");
                    if source.is_empty() {
                        query.options.remove("source");
                    } else {
                        query.options.insert("source".into(), source.into());
                    }
                    query.options.remove("cursor");
                    query.options.remove("anchor");
                    query.scope.clear();
                    query.target = None;
                }
            }
            _ => return false,
        }
        state.details_only = false;
        self.refresh_management();
        true
    }
}
