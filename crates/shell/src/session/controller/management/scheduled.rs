use super::*;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScheduleStep {
    Operation,
    File,
    Details,
    Kind,
    Time,
    Review,
}

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/management/scheduled.rs"]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScheduledWizard {
    step: ScheduleStep,
    values: BTreeMap<String, String>,
    action: ManagementAction,
    row: Option<ManagementRow>,
    ready: bool,
    preview_id: Option<u64>,
    preview_values: Option<BTreeMap<String, String>>,
    preview_after: Option<Instant>,
    preview_message: String,
}

fn text(id: &str, label: &str, value: String, required: bool) -> ManagementField {
    ManagementField {
        id: id.into(),
        label: i18n::tr!(label),
        value,
        required,
        ..Default::default()
    }
}
fn choice(id: &str, label: &str, value: String, choices: Vec<String>) -> ManagementField {
    ManagementField {
        choices,
        control: FieldControl::SearchableChoice,
        ..text(id, label, value, true)
    }
}
fn boolean(id: &str, label: &str, value: String) -> ManagementField {
    ManagementField {
        control: FieldControl::Checkbox,
        ..text(id, label, value, false)
    }
}
fn operation_labels() -> Vec<String> {
    ["script", "service_restart", "packages_update", "command"]
        .map(str::to_owned)
        .to_vec()
}
fn kind_labels() -> Vec<String> {
    ["daily", "weekly", "once", "advanced"]
        .map(str::to_owned)
        .to_vec()
}

pub(super) fn choice_label(field: &str, value: &str) -> Option<String> {
    let key = match (field, value) {
        ("task_type", "script") => "schedule-action-script",
        ("task_type", "service_restart") => "schedule-action-service",
        ("task_type", "packages_update") => "schedule-action-packages",
        ("task_type", "command") => "schedule-action-command",
        ("schedule_kind", "daily") => "schedule-daily",
        ("schedule_kind", "weekly") => "schedule-weekly",
        ("schedule_kind", "once") => "schedule-once",
        ("schedule_kind", "advanced") => "schedule-advanced",
        _ => return None,
    };
    Some(i18n::tr!(key))
}

fn seed_existing_schedule(values: &mut BTreeMap<String, String>, cron: bool) {
    use chrono::{Datelike, Timelike};
    let original = values.get("schedule").cloned().unwrap_or_default();
    values.insert("schedule_kind".into(), "advanced".into());
    let expression = match (cron, original.as_str()) {
        (true, "@daily" | "@midnight") => "0 0 * * *",
        (true, "@weekly") => "0 0 * * 0",
        (false, "daily") => "*-*-* 00:00:00",
        (false, "weekly") => "Mon *-*-* 00:00:00",
        _ => original.as_str(),
    };
    let words = expression.split_whitespace().collect::<Vec<_>>();
    let (hour, minute, days, date) = if cron {
        if words.len() != 5 || words[2] != "*" || words[3] != "*" {
            return;
        }
        let (Ok(minute), Ok(hour)) = (words[0].parse::<u8>(), words[1].parse::<u8>()) else {
            return;
        };
        let days = if words[4] == "*" {
            None
        } else {
            let mut days = Vec::new();
            for part in words[4].split(',') {
                let parse = |value: &str| {
                    value
                        .parse::<u8>()
                        .ok()
                        .filter(|value| *value <= 7)
                        .or_else(|| {
                            ["sun", "mon", "tue", "wed", "thu", "fri", "sat"]
                                .iter()
                                .position(|day| *day == value.to_ascii_lowercase())
                                .map(|index| index as u8)
                        })
                };
                if let Some((first, last)) = part.split_once('-') {
                    let (Some(first), Some(last)) = (parse(first), parse(last)) else {
                        return;
                    };
                    if first > last {
                        return;
                    }
                    days.extend((first..=last).map(|day| ((day + 6) % 7) as usize));
                } else {
                    let Some(day) = parse(part) else {
                        return;
                    };
                    days.push(((day + 6) % 7) as usize);
                }
            }
            Some(days)
        };
        (hour, minute, days, None)
    } else {
        let Some(raw_time) = words.last() else {
            return;
        };
        let time = chrono::NaiveTime::parse_from_str(raw_time, "%H:%M:%S")
            .or_else(|_| chrono::NaiveTime::parse_from_str(raw_time, "%H:%M"));
        let Ok(time) = time else {
            return;
        };
        if time.second() != 0 {
            return;
        }
        let date = if words.len() == 2
            && words[0].starts_with(|character: char| character.is_ascii_digit())
        {
            match chrono::NaiveDate::parse_from_str(words[0], "%Y-%m-%d") {
                Ok(date) => Some(date),
                Err(_) => return,
            }
        } else {
            None
        };
        let days = if words.len() == 3 && words[1] == "*-*-*"
            || words.len() == 2 && date.is_none() && words[0] != "*-*-*"
        {
            let mut days = Vec::new();
            let names = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
            let parse = |day: &str| {
                names
                    .iter()
                    .position(|name| *name == day.to_ascii_lowercase())
            };
            for part in words[0].split(',') {
                if let Some((first, last)) = part.split_once("..") {
                    let (Some(first), Some(last)) = (parse(first), parse(last)) else {
                        return;
                    };
                    if first > last {
                        return;
                    }
                    days.extend(first..=last);
                } else {
                    let Some(day) = parse(part) else {
                        return;
                    };
                    days.push(day);
                }
            }
            Some(days)
        } else if words.len() == 1 || words.len() == 2 {
            None
        } else {
            return;
        };
        (time.hour() as u8, time.minute() as u8, days, date)
    };
    if hour >= 24 || minute >= 60 {
        return;
    }
    values.insert("hour".into(), format!("{hour:02}"));
    values.insert("minute".into(), format!("{minute:02}"));
    if let Some(date) = date {
        values.insert("schedule_kind".into(), "once".into());
        values.insert("year".into(), date.year().to_string());
        values.insert("month".into(), format!("{:02}", date.month()));
        values.insert("day".into(), format!("{:02}", date.day()));
    } else if let Some(days) = days {
        values.insert("schedule_kind".into(), "weekly".into());
        for index in 0..7 {
            values.insert(format!("day_{index}"), days.contains(&index).to_string());
        }
    } else {
        values.insert("schedule_kind".into(), "daily".into());
    }
}

impl ScheduledWizard {
    fn first_step(&self) -> ScheduleStep {
        if self.get("__source").is_empty() {
            ScheduleStep::Operation
        } else {
            ScheduleStep::Kind
        }
    }
    pub(super) fn submit_label(&self) -> String {
        i18n::tr!(if self.step == ScheduleStep::Review {
            if !self.ready {
                "schedule-working"
            } else if self.action.id == "edit_task" {
                "schedule-save"
            } else if self.action.id == "preview_task" {
                "scheduled-task-preview"
            } else {
                "schedule-create"
            }
        } else {
            "schedule-next"
        })
    }
    pub(super) fn submit_disabled(&self) -> bool {
        self.step == ScheduleStep::Review && !self.ready
    }
    pub(super) fn back_label(&self) -> Option<String> {
        (self.step != self.first_step()).then(|| i18n::tr!("schedule-back"))
    }
    fn get(&self, key: &str) -> String {
        self.values.get(key).cloned().unwrap_or_default()
    }
    fn collect(&mut self, fields: Vec<ManagementField>) {
        for field in fields {
            if field.id == "task_type" {
                if let Some(index) = operation_labels()
                    .iter()
                    .position(|value| value == &field.value)
                {
                    self.values.insert(
                        field.id,
                        ["script", "service_restart", "packages_update", "command"][index].into(),
                    );
                }
            } else if field.id == "schedule_kind" {
                if let Some(index) = kind_labels().iter().position(|value| value == &field.value) {
                    self.values.insert(
                        field.id,
                        ["daily", "weekly", "once", "advanced"][index].into(),
                    );
                }
            } else {
                self.values.insert(field.id, field.value);
            }
        }
    }
}

impl ShellSession {
    pub(super) fn begin_scheduled_wizard(
        &mut self,
        action: ManagementAction,
        row: Option<ManagementRow>,
    ) {
        let native_edit = action.id == "edit_task" && action.values.is_empty();
        let mut values = action.values.clone();
        if !values.contains_key("service_choices") {
            if let Some(choices) = self
                .pages
                .management
                .management_state
                .snapshot
                .actions
                .iter()
                .find(|action| action.id == "create_task")
                .and_then(|action| action.values.get("service_choices"))
            {
                values.insert("service_choices".into(), choices.clone());
            }
        }
        for field in &action.fields {
            values
                .entry(field.id.clone())
                .or_insert_with(|| field.value.clone());
        }
        if native_edit && let Some(row) = &row {
            let cron = matches!(
                row.identity.get("source").map(String::as_str),
                Some("crontab" | "cron-file")
            );
            values.insert(
                "__source".into(),
                if cron {
                    "cron".into()
                } else {
                    "native-timer".into()
                },
            );
            values
                .entry("name".into())
                .or_insert_with(|| row.cells.first().cloned().unwrap_or_default());
            values
                .entry("user".into())
                .or_insert_with(|| row.cells.get(2).cloned().unwrap_or_default());
            values
                .entry("working_directory".into())
                .or_insert("/".into());
            if cron {
                if let Some(zone) = row
                    .detail
                    .iter()
                    .flat_map(|(_, value)| value.lines())
                    .find_map(|line| line.strip_prefix("CRON_TZ="))
                {
                    values.insert("cron_tz".into(), zone.into());
                }
            }
            seed_existing_schedule(&mut values, cron);
        }
        if action.id == "edit_task" && !values.contains_key("schedule_kind") {
            seed_existing_schedule(&mut values, false);
        }
        values.entry("task_type".into()).or_insert_with(|| {
            if action.id == "edit_task" {
                "command".into()
            } else {
                "script".into()
            }
        });
        values.entry("schedule_kind".into()).or_insert_with(|| {
            if action.id == "edit_task" {
                "advanced".into()
            } else {
                "daily".into()
            }
        });
        values.entry("hour".into()).or_insert("09".into());
        values.entry("minute".into()).or_insert("00".into());
        values.entry("persistent".into()).or_insert("false".into());
        let mut wizard = ScheduledWizard {
            step: ScheduleStep::Operation,
            values,
            action,
            row,
            ready: false,
            preview_id: None,
            preview_values: None,
            preview_after: None,
            preview_message: String::new(),
        };
        wizard.step = wizard.first_step();
        self.show_scheduled_step(wizard, String::new());
    }

    fn show_scheduled_step(&mut self, wizard: ScheduledWizard, message: String) {
        let mut fields = Vec::new();
        let mut message = message;
        let title = match wizard.step {
            ScheduleStep::Operation => {
                let index = ["script", "service_restart", "packages_update", "command"]
                    .iter()
                    .position(|value| *value == wizard.get("task_type"))
                    .unwrap_or(0);
                fields.push(text(
                    "name",
                    "scheduled-task-field-name",
                    wizard.get("name"),
                    true,
                ));
                fields.push(choice(
                    "task_type",
                    "schedule-operation",
                    operation_labels()[index].clone(),
                    operation_labels(),
                ));
                "schedule-step-operation"
            }
            ScheduleStep::File => {
                let mut path = PathBuf::from(wizard.get("script_path"));
                if !path.is_dir() {
                    path = PathBuf::from(wizard.get("working_directory"));
                }
                let mut choices = path
                    .parent()
                    .map(|parent| parent.display().to_string())
                    .into_iter()
                    .collect::<Vec<_>>();
                match std::fs::read_dir(&path) {
                    Ok(entries) => {
                        let mut unknown_name = false;
                        let mut entries = entries
                            .filter_map(Result::ok)
                            .filter(|entry| {
                                entry
                                    .file_type()
                                    .is_ok_and(|kind| kind.is_dir() || kind.is_file())
                            })
                            .filter_map(|entry| match entry.path().into_os_string().into_string() {
                                Ok(path) if !path.chars().any(char::is_control) => Some(path),
                                Ok(_) | Err(_) => {
                                    unknown_name = true;
                                    None
                                }
                            })
                            .collect::<Vec<_>>();
                        entries.sort();
                        choices.extend(entries);
                        if unknown_name {
                            message = i18n::tr!("schedule-unrepresentable-name");
                        }
                    }
                    Err(error) => message = error.to_string(),
                }
                fields.push(choice(
                    "script_path",
                    "schedule-script",
                    path.display().to_string(),
                    choices,
                ));
                if message.is_empty() {
                    message = i18n::tr!("schedule-file-help");
                }
                "schedule-step-file"
            }
            ScheduleStep::Details => {
                match wizard.get("task_type").as_str() {
                    "script" => fields.push(text(
                        "arguments",
                        "schedule-arguments",
                        wizard.get("arguments"),
                        false,
                    )),
                    "service_restart" => {
                        let choices = wizard
                            .values
                            .get("service_choices")
                            .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
                            .unwrap_or_default();
                        fields.push(choice(
                            "service_unit",
                            "schedule-service",
                            wizard.get("service_unit"),
                            choices,
                        ));
                    }
                    "packages_update" => {
                        message = i18n::tr!("schedule-packages-help");
                    }
                    _ => fields.push(text(
                        "command",
                        "scheduled-task-field-command",
                        wizard.get("command"),
                        true,
                    )),
                }
                if !matches!(
                    wizard.get("task_type").as_str(),
                    "service_restart" | "packages_update"
                ) {
                    fields.push(text(
                        "working_directory",
                        "scheduled-task-field-directory",
                        wizard.get("working_directory"),
                        true,
                    ));
                }
                let users = self
                    .pages
                    .management
                    .management_state
                    .snapshot
                    .actions
                    .iter()
                    .find(|action| action.id == "create_task")
                    .and_then(|action| action.fields.iter().find(|field| field.id == "user"))
                    .map(|field| field.choices.clone())
                    .unwrap_or_else(|| vec![wizard.get("user")]);
                fields.push(choice(
                    "user",
                    "scheduled-task-field-user",
                    wizard.get("user"),
                    users,
                ));
                "schedule-step-details"
            }
            ScheduleStep::Kind => {
                let index = ["daily", "weekly", "once", "advanced"]
                    .iter()
                    .position(|value| *value == wizard.get("schedule_kind"))
                    .unwrap_or(0);
                let mut choices = kind_labels();
                if wizard.get("__source") == "cron" {
                    choices.remove(2);
                }
                fields.push(choice(
                    "schedule_kind",
                    "schedule-kind",
                    kind_labels()[index].clone(),
                    choices,
                ));
                message = i18n::tr!("schedule-time-help");
                "schedule-step-time"
            }
            ScheduleStep::Time => {
                match wizard.get("schedule_kind").as_str() {
                    "advanced" => fields.push(text(
                        "schedule",
                        "scheduled-task-field-schedule",
                        wizard.get("schedule"),
                        true,
                    )),
                    kind => {
                        if kind == "weekly" {
                            for (index, label) in [
                                "schedule-mon",
                                "schedule-tue",
                                "schedule-wed",
                                "schedule-thu",
                                "schedule-fri",
                                "schedule-sat",
                                "schedule-sun",
                            ]
                            .into_iter()
                            .enumerate()
                            {
                                let key = format!("day_{index}");
                                fields.push(boolean(
                                    &key,
                                    label,
                                    wizard.values.get(&key).cloned().unwrap_or("false".into()),
                                ));
                            }
                        } else if kind == "once" {
                            let date = chrono::Local::now();
                            let default_date = (
                                date.format("%Y").to_string(),
                                date.format("%m").to_string(),
                                date.format("%d").to_string(),
                            );
                            fields.push(text(
                                "year",
                                "schedule-year",
                                wizard.values.get("year").cloned().unwrap_or(default_date.0),
                                true,
                            ));
                            fields.push(choice(
                                "month",
                                "schedule-month",
                                wizard
                                    .values
                                    .get("month")
                                    .cloned()
                                    .unwrap_or(default_date.1),
                                (1..=12).map(|n| format!("{n:02}")).collect(),
                            ));
                            fields.push(choice(
                                "day",
                                "schedule-day",
                                wizard.values.get("day").cloned().unwrap_or(default_date.2),
                                (1..=31).map(|n| format!("{n:02}")).collect(),
                            ));
                        }
                        fields.push(choice(
                            "hour",
                            "schedule-hour",
                            wizard.get("hour"),
                            (0..24).map(|n| format!("{n:02}")).collect(),
                        ));
                        fields.push(choice(
                            "minute",
                            "schedule-minute",
                            wizard.get("minute"),
                            (0..60).map(|n| format!("{n:02}")).collect(),
                        ));
                    }
                }
                if wizard.get("__source") != "cron" {
                    fields.push(boolean(
                        "persistent",
                        "schedule-catch-up",
                        wizard.get("persistent"),
                    ));
                }
                "schedule-step-time"
            }
            ScheduleStep::Review => {
                if wizard.action.id == "edit_task" {
                    "schedule-step-review-edit"
                } else if wizard.action.id == "preview_task" {
                    "schedule-step-preview"
                } else {
                    "schedule-step-review"
                }
            }
        };
        self.reset_management_form_view();
        self.pages.management.management_state.form = Some(ManagementEditor {
            instance: next_management_form_instance(),
            input_states: Default::default(),
            title: i18n::tr!(title),
            message,
            message_scroll: 0,
            fields,
            selected: 0,
            purpose: FormPurpose::Scheduled(wizard),
        });
    }

    pub(super) fn submit_scheduled_step(
        &mut self,
        mut wizard: ScheduledWizard,
        fields: Vec<ManagementField>,
    ) {
        wizard.collect(fields);
        match wizard.step {
            ScheduleStep::Operation => {
                if matches!(
                    wizard.get("task_type").as_str(),
                    "service_restart" | "packages_update"
                ) {
                    wizard.values.insert("user".into(), "root".into());
                    wizard.values.insert("working_directory".into(), "/".into());
                }
                if wizard.get("task_type") == "script" {
                    wizard.step = ScheduleStep::File;
                } else {
                    wizard.step = ScheduleStep::Details;
                }
            }
            ScheduleStep::File => {
                let path = PathBuf::from(wizard.get("script_path"));
                if path.is_dir() {
                    self.show_scheduled_step(wizard, String::new());
                    return;
                }
                if !path.is_file() {
                    self.show_scheduled_step(wizard, i18n::tr!("schedule-invalid-file-choice"));
                    return;
                }
                wizard.values.insert(
                    "working_directory".into(),
                    path.parent()
                        .unwrap_or(Path::new("/"))
                        .display()
                        .to_string(),
                );
                wizard.step = ScheduleStep::Details;
            }
            ScheduleStep::Details => wizard.step = ScheduleStep::Kind,
            ScheduleStep::Kind => wizard.step = ScheduleStep::Time,
            ScheduleStep::Time => {
                if wizard.ready && wizard.preview_values.as_ref() == Some(&wizard.values) {
                    wizard.step = ScheduleStep::Review;
                    let message = wizard.preview_message.clone();
                    self.show_scheduled_step(wizard, message);
                    return;
                }
                wizard.step = ScheduleStep::Review;
                self.begin_scheduled_preview(wizard, false);
                return;
            }
            ScheduleStep::Review => {
                if !wizard.ready {
                    return;
                }
                let mut action = wizard.action;
                action.values = wizard.values;
                action.fields.clear();
                self.perform_management_action(action, wizard.row);
                return;
            }
        }
        self.show_scheduled_step(wizard, String::new());
    }

    pub(super) fn cancel_scheduled_step(&mut self) -> bool {
        let Some(form) = self.pages.management.management_state.form.as_ref() else {
            return false;
        };
        let FormPurpose::Scheduled(mut wizard) = form.purpose.clone() else {
            return false;
        };
        if wizard.step == wizard.first_step() {
            return false;
        }
        wizard.collect(form.fields.clone());
        if let Some(job) = self
            .pages
            .management
            .management_state
            .scheduled_preview
            .take()
        {
            job.cancel();
            self.tasks.complete(TaskDomain::ManagementDraft, job.0.id);
        }
        wizard.step = match wizard.step {
            ScheduleStep::File | ScheduleStep::Details => ScheduleStep::Operation,
            ScheduleStep::Kind => ScheduleStep::Details,
            ScheduleStep::Time => ScheduleStep::Kind,
            ScheduleStep::Review => ScheduleStep::Time,
            ScheduleStep::Operation => ScheduleStep::Operation,
        };
        self.show_scheduled_step(wizard, String::new());
        true
    }

    fn begin_scheduled_preview(&mut self, mut wizard: ScheduledWizard, in_place: bool) {
        let Some(group) = self.settings_task_runtime.shared.task_group.clone() else {
            wizard.step = ScheduleStep::Time;
            wizard.preview_after = None;
            if in_place {
                if let Some(form) = self.pages.management.management_state.form.as_mut() {
                    form.purpose = FormPurpose::Scheduled(wizard);
                    form.message = i18n::tr!("management-worker-unavailable");
                }
            } else {
                self.show_scheduled_step(wizard, i18n::tr!("management-worker-unavailable"));
            }
            return;
        };
        let (job, _) = self.management_job();
        let output = Arc::downgrade(&job.0);
        let cancelled = job.0.cancelled.clone();
        let values = wizard.values.clone();
        let language = self.language.clone();
        #[cfg(target_os = "linux")]
        let uid = unsafe { libc::getuid() };
        let task = TaskId::new(format!("schedule-preview-{}", job.0.id)).expect("bounded task id");
        match group.spawn_thread(TaskSpec::one_shot(task), move || {
            let _language = i18n::enter_snapshot(language.clone());
            #[cfg(target_os = "linux")]
            let result =
                platform::management::scheduled_tasks::preview_plan(&values, uid, &cancelled);
            #[cfg(not(target_os = "linux"))]
            let result: Result<String, ManagementError> = {
                let _ = (&values, &cancelled);
                Err(ManagementError::Unavailable(i18n::tr!(
                    "management-action-not-available"
                )))
            };
            if let Some(output) = output.upgrade() {
                *output.snapshot.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(result.map(|message| ManagementSnapshot {
                        details: vec![(String::new(), message)],
                        ..Default::default()
                    }));
            }
        }) {
            Ok(worker) => {
                *job.0.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker);
                self.register_management_job(&job, TaskDomain::ManagementDraft, TaskClass::Query);
                wizard.preview_id = Some(job.0.id);
                wizard.preview_after = None;
                wizard.preview_values = Some(wizard.values.clone());
                wizard.ready = false;
                self.pages.management.management_state.scheduled_preview = Some(job);
                if in_place {
                    if let Some(form) = self.pages.management.management_state.form.as_mut() {
                        form.purpose = FormPurpose::Scheduled(wizard);
                        form.message = i18n::tr!("schedule-working");
                    }
                } else {
                    self.show_scheduled_step(wizard, i18n::tr!("schedule-working"));
                }
            }
            Err(error) => {
                wizard.step = ScheduleStep::Time;
                wizard.preview_after = None;
                self.show_scheduled_step(wizard, error.to_string());
            }
        }
    }

    pub(super) fn poll_scheduled_preview(&mut self) {
        if let Some(form) = self.pages.management.management_state.form.as_ref()
            && let FormPurpose::Scheduled(mut wizard) = form.purpose.clone()
            && wizard.step == ScheduleStep::Time
        {
            wizard.collect(form.fields.clone());
            if wizard.preview_values.as_ref() != Some(&wizard.values) {
                if let Some(job) = self
                    .pages
                    .management
                    .management_state
                    .scheduled_preview
                    .take()
                {
                    job.cancel();
                    self.tasks.complete(TaskDomain::ManagementDraft, job.0.id);
                }
                wizard.preview_values = Some(wizard.values.clone());
                wizard.preview_after = Some(Instant::now() + Duration::from_millis(250));
                wizard.preview_id = None;
                wizard.ready = false;
                if let Some(form) = self.pages.management.management_state.form.as_mut() {
                    form.purpose = FormPurpose::Scheduled(wizard);
                    form.message = i18n::tr!("schedule-working");
                }
                return;
            }
            if wizard.preview_id.is_none()
                && wizard.preview_after.is_some_and(|at| Instant::now() >= at)
            {
                self.begin_scheduled_preview(wizard, true);
                return;
            }
        }
        let state = &mut self.pages.management.management_state;
        let Some(job) = state.scheduled_preview.clone() else {
            return;
        };
        let result = job
            .0
            .snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let Some(result) = result else {
            return;
        };
        self.tasks.complete(TaskDomain::ManagementDraft, job.0.id);
        let state = &mut self.pages.management.management_state;
        state.scheduled_preview = None;
        let Some(form) = state.form.as_mut() else {
            return;
        };
        let FormPurpose::Scheduled(wizard) = &mut form.purpose else {
            return;
        };
        if wizard.preview_id != Some(job.0.id) {
            return;
        }
        match result {
            Ok(snapshot) => {
                form.message = snapshot
                    .details
                    .into_iter()
                    .map(|(_, message)| message)
                    .collect::<Vec<_>>()
                    .join("\n");
                wizard.preview_message = form.message.clone();
                wizard.ready = true;
            }
            Err(error) => {
                form.message = error.to_string();
                wizard.ready = false;
            }
        }
        self.cancel_management_pointer_gesture();
        self.interaction.button_capture = None;
    }
}
