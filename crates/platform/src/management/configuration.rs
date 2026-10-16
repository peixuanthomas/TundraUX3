use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigCandidate {
    pub path: PathBuf,
    pub expected_version: String,
    pub content: String,
}

#[cfg(target_os = "linux")]
pub(crate) fn confirm_preview(
    id: &str,
    summary: &str,
    lines: &[String],
    io: &mut dyn super::OperationInteraction,
) -> Result<(), super::ManagementError> {
    let mut pages = vec![String::new()];
    let mut rows = 0;
    for text in lines {
        let text = runtime_log::sanitize_text(text);
        for line in text.split('\n') {
            let mut remaining = line;
            loop {
                let mut end = remaining.len().min(48 * 1024);
                while !remaining.is_char_boundary(end) {
                    end -= 1;
                }
                let part = &remaining[..end];
                if rows == 100 || pages.last().unwrap().len() + part.len() + 1 > 48 * 1024 {
                    pages.push(String::new());
                    rows = 0;
                }
                pages.last_mut().unwrap().push_str(part);
                pages.last_mut().unwrap().push('\n');
                rows += 1;
                if end == remaining.len() {
                    break;
                }
                remaining = &remaining[end..];
            }
        }
    }
    let mut page = 0;
    loop {
        let apply = i18n::tr!("permissions-confirm");
        let cancel = i18n::tr!("permissions-cancel");
        let next = i18n::tr!("permissions-next");
        let previous = i18n::tr!("permissions-previous");
        let mut choices = vec![cancel.clone(), apply.clone()];
        if page + 1 < pages.len() {
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
                pages = pages.len() as i64
            ),
            pages[page]
        );
        let answer = io.ask(id, &prompt, &choices, false)?;
        if answer == apply {
            return Ok(());
        }
        if answer == next && page + 1 < pages.len() {
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
