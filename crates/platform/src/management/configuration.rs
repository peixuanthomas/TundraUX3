use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[cfg(target_os = "linux")]
pub(crate) fn confirm_preview(
    id: &str,
    summary: &str,
    lines: &[String],
    io: &mut dyn super::OperationInteraction,
) -> Result<(), super::ManagementError> {
    let pages = lines.len().max(1).div_ceil(100);
    let mut page = 0;
    loop {
        let apply = i18n::tr!("permissions-confirm");
        let cancel = i18n::tr!("permissions-cancel");
        let next = i18n::tr!("permissions-next");
        let previous = i18n::tr!("permissions-previous");
        let mut choices = vec![cancel.clone(), apply.clone()];
        if page + 1 < pages {
            choices.push(next.clone());
        }
        if page > 0 {
            choices.push(previous.clone());
        }
        let prompt = format!(
            "{summary}\n{}\n{}",
            i18n::tr!(
                "permissions-page",
                page = (page + 1) as i64,
                pages = pages as i64
            ),
            lines
                .iter()
                .skip(page * 100)
                .take(100)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
        let answer = io.ask(id, &prompt, &choices, false)?;
        if answer == apply {
            return Ok(());
        }
        if answer == next && page + 1 < pages {
            page += 1;
        } else if answer == previous && page > 0 {
            page -= 1;
        } else {
            return Err(super::ManagementError::Cancelled);
        }
    }
}

/// Delivered over the private operation connection, never rendered as a log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigDocument {
    pub path: PathBuf,
    pub content: String,
    pub version: String,
    pub existed: bool,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub validator: String,
    pub check: ConfigCheck,
    pub backup_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigCheck {
    #[default]
    NotChecked,
    Passed,
    Unavailable(String),
    Failed(String),
}
