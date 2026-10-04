use super::PackageRecord;
use crate::management::ManagementError;
use std::collections::BTreeMap;

fn record(name: String, architecture: String, version: String, summary: String) -> PackageRecord {
    PackageRecord {
        name,
        architecture,
        version,
        summary,
        description: String::new(),
        repository: String::new(),
        installed_version: None,
        details: Vec::new(),
    }
}

pub(super) fn dpkg_records(output: &str) -> Result<Vec<PackageRecord>, ManagementError> {
    let mut records = Vec::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<_> = line.splitn(5, '\t').collect();
        if fields.len() != 5 {
            return Err(ManagementError::Failed(
                "Unrecognized dpkg-query output".into(),
            ));
        }
        // The first letter is the desired action (including held packages),
        // the second is the actual state, and the third is the error flag.
        if fields[3].as_bytes().get(1) != Some(&b'i') {
            continue;
        }
        let name = fields[0].split(':').next().unwrap_or(fields[0]);
        let mut item = record(
            name.into(),
            fields[2].into(),
            fields[1].into(),
            fields[4].into(),
        );
        item.installed_version = Some(item.version.clone());
        item.details
            .push(("Package status".into(), fields[3].into()));
        records.push(item);
    }
    Ok(records)
}

pub(super) fn apt_records(output: &str) -> Result<Vec<PackageRecord>, ManagementError> {
    let mut records = Vec::new();
    let mut paragraph = BTreeMap::<String, String>::new();
    let mut current = String::new();
    for line in output.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if !paragraph.is_empty() {
                let name = paragraph.get("Package").cloned().ok_or_else(|| {
                    ManagementError::Failed("APT metadata is missing Package".into())
                })?;
                let version = paragraph.get("Version").cloned().ok_or_else(|| {
                    ManagementError::Failed("APT metadata is missing Version".into())
                })?;
                let architecture = paragraph.get("Architecture").cloned().unwrap_or_default();
                let description = paragraph
                    .get("Description")
                    .cloned()
                    .or_else(|| {
                        paragraph
                            .iter()
                            .find(|(key, _)| {
                                key.starts_with("Description-") && key.as_str() != "Description-md5"
                            })
                            .map(|(_, value)| value.clone())
                    })
                    .unwrap_or_default();
                let summary = description.lines().next().unwrap_or("").to_owned();
                let mut item = record(name, architecture, version, summary);
                item.description = description;
                item.repository = paragraph.get("Origin").cloned().unwrap_or_default();
                for key in [
                    "Homepage",
                    "Depends",
                    "Pre-Depends",
                    "Recommends",
                    "Suggests",
                    "Conflicts",
                    "Breaks",
                    "Replaces",
                    "Essential",
                    "Priority",
                    "Section",
                    "Size",
                    "Installed-Size",
                ] {
                    if let Some(value) = paragraph.get(key) {
                        item.details.push((key.into(), value.clone()));
                    }
                }
                records.push(item);
                paragraph.clear();
                current.clear();
            }
        } else if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(value) = paragraph.get_mut(&current) {
                value.push('\n');
                value.push_str(if line.trim() == "." {
                    ""
                } else {
                    line.trim_start()
                });
            } else {
                return Err(ManagementError::Failed(
                    "APT metadata contains a continuation without a field".into(),
                ));
            }
        } else if let Some((key, value)) = line.split_once(':') {
            current = key.into();
            paragraph.insert(key.into(), value.trim_start().into());
        } else {
            return Err(ManagementError::Failed("Unrecognized APT metadata".into()));
        }
    }
    Ok(records)
}

/// This is only the updates-list preview. Its output is never used to authorize
/// a later modification or to answer the running package manager's question.
pub(super) fn apt_upgrades(output: &str) -> Result<Vec<(String, String)>, ManagementError> {
    let mut updates = Vec::new();
    for line in output.lines().filter(|line| line.starts_with("Inst ")) {
        let name = line.split_whitespace().nth(1).ok_or_else(|| {
            ManagementError::Failed("APT upgrade preview is missing a package name".into())
        })?;
        let version = line
            .split_once('(')
            .and_then(|(_, rest)| rest.split_whitespace().next())
            .ok_or_else(|| {
                ManagementError::Failed("APT upgrade preview is missing a package version".into())
            })?;
        updates.push((
            name.split(':').next().unwrap_or(name).into(),
            version.into(),
        ));
    }
    Ok(updates)
}

pub(super) fn rpm_records(output: &str) -> Result<Vec<PackageRecord>, ManagementError> {
    let mut records = Vec::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<_> = line.splitn(4, '\t').collect();
        if fields.len() != 4
            || fields[..3]
                .iter()
                .any(|field| field.is_empty() || field.contains("%{"))
        {
            return Err(ManagementError::Failed("Unrecognized RPM/DNF query output; package metadata was not treated as an empty list".into()));
        }
        let version = fields[2].strip_prefix("0:").unwrap_or(fields[2]);
        records.push(record(
            fields[0].into(),
            fields[1].into(),
            version.into(),
            fields[3].into(),
        ));
    }
    Ok(records)
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ObservedPrompt {
    Confirm {
        text: String,
    },
    Conffile {
        text: String,
    },
    Progress {
        message: String,
        percent: Option<u8>,
    },
}

/// Prompts are observed from the currently running package manager. No prompt
/// is inferred from a previous simulation. Unrecognized interaction remains
/// visible in the embedded terminal and is never answered automatically.
#[derive(Default)]
pub(super) struct PromptParser {
    pub applying: bool,
    pending: String,
    transcript: String,
    transcript_overflow: bool,
}

impl PromptParser {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<ObservedPrompt> {
        let text = String::from_utf8_lossy(bytes);
        self.pending.push_str(&text);
        self.transcript.push_str(&text);
        self.transcript_overflow |= self.transcript.len() > 64 * 1024;
        bound_text(&mut self.transcript, 64 * 1024);
        let mut events = Vec::new();
        while let Some(end) = self.pending.find(['\r', '\n']) {
            let line = self.pending[..end].to_owned();
            self.pending.drain(..=end);
            if let Some(event) = self.line(&line) {
                events.push(event);
            }
        }
        let prompt = self.pending.trim();
        let dependency_prompt = prompt.ends_with("Do you want to continue? [Y/n]")
            || prompt.ends_with("Do you want to continue? [y/N]")
            || prompt.ends_with("Is this ok [y/N]:")
            || prompt.ends_with("Is this ok [Y/n]:");
        // A script can print arbitrary text. Standard dependency confirmation
        // belongs only to the package manager before it starts applying changes.
        if dependency_prompt && !self.applying {
            if self.transcript_overflow {
                // Never present a clipped dependency list as a complete native
                // confirmation. The full output has already reached the PTY UI.
                events.push(ObservedPrompt::Progress { message: "The actual package plan is too large for a confirmation form. Review and answer the running package manager in the embedded terminal.".into(), percent: None });
            } else {
                events.push(ObservedPrompt::Confirm {
                    text: self.transcript.clone(),
                });
            }
            self.pending.clear();
        }
        bound_text(&mut self.pending, 16 * 1024);
        events
    }

    fn line(&mut self, line: &str) -> Option<ObservedPrompt> {
        if let Some(rest) = line.strip_prefix("processing: ") {
            self.applying = true;
            return Some(ObservedPrompt::Progress {
                message: format!("dpkg: {rest}"),
                percent: None,
            });
        }
        if let Some(rest) = line.strip_prefix("status: ") {
            self.applying = true;
            if let Some((file, question)) = rest.split_once(" : conffile-prompt : ") {
                return Some(ObservedPrompt::Conffile {
                    text: format!("{file}: {question}"),
                });
            }
            return Some(ObservedPrompt::Progress {
                message: format!("dpkg: {rest}"),
                percent: None,
            });
        }
        if line.contains("Running transaction")
            || line.starts_with("Preparing to unpack ")
            || line.starts_with("Unpacking ")
            || line.starts_with("Setting up ")
            || line.starts_with("Removing ")
        {
            self.applying = true;
            return Some(ObservedPrompt::Progress {
                message: line.into(),
                percent: None,
            });
        }
        None
    }
}

fn bound_text(text: &mut String, max: usize) {
    if text.len() <= max {
        return;
    }
    let mut end = text.len() - max;
    while !text.is_char_boundary(end) {
        end += 1;
    }
    text.drain(..end);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_held_packages_and_architecture_are_preserved() {
        let records = dpkg_records(
            "libc6:amd64\t2.39\tamd64\thi \tC library\nremoved\t1\tall\trc \tOld package\n",
        )
        .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "libc6");
        assert_eq!(records[0].installed_version.as_deref(), Some("2.39"));
    }

    #[test]
    fn apt_description_and_dependency_details_are_preserved() {
        let records = apt_records("Package: demo\nArchitecture: all\nVersion: 2\nDepends: base (>= 1)\nDescription-en: Demo\n continued text\n .\n final text\n\n").unwrap();
        assert_eq!(records[0].description, "Demo\ncontinued text\n\nfinal text");
        assert!(
            records[0]
                .details
                .contains(&("Depends".into(), "base (>= 1)".into()))
        );
        assert!(apt_records("Package: incomplete\n").is_err());
    }

    #[test]
    fn apt_updates_are_preview_only() {
        let records = apt_upgrades(
            "Inst libc6:amd64 [2.38] (2.39 Ubuntu [amd64])\nConf libc6 (2.39 Ubuntu)\n",
        )
        .unwrap();
        assert_eq!(records, vec![("libc6".into(), "2.39".into())]);
    }

    #[test]
    fn rpm_newlines_and_epoch_formats_are_normalized() {
        let records = rpm_records("bash\tx86_64\t0:5.2-1.fc43\tShell\n\n").unwrap();
        assert_eq!(records[0].version, "5.2-1.fc43");
        assert!(rpm_records("Error: missing repository cache").is_err());
    }

    #[test]
    fn split_real_confirmation_contains_current_dependencies() {
        let mut parser = PromptParser::default();
        assert!(parser.push(b"The following packages will be REMOVED:\r\n dangerous-old\r\nDo you want to con").is_empty());
        let events = parser.push(b"tinue? [Y/n] ");
        assert!(
            matches!(&events[..], [ObservedPrompt::Confirm {text}] if text.contains("dangerous-old"))
        );
        assert!(parser.push(b"").is_empty());
    }

    #[test]
    fn configuration_prompt_handles_architecture_and_colons_in_paths() {
        let mut parser = PromptParser::default();
        let events = parser.push(
            b"status: /etc/file:a : conffile-prompt : '/etc/file:a' '/etc/file:a.dpkg-new' 1 1\n",
        );
        assert_eq!(
            events,
            vec![ObservedPrompt::Conffile {
                text: "/etc/file:a: '/etc/file:a' '/etc/file:a.dpkg-new' 1 1".into()
            }]
        );
        assert!(parser.applying);
        assert!(parser.push(b"Do you want to continue? [Y/n] ").is_empty());
    }

    #[test]
    fn dpkg_status_stream_emits_native_configuration_question_and_real_phase() {
        let mut parser = PromptParser::default();
        let events = parser.push(b"processing: configure: demo:amd64\nstatus: /etc/demo:part.conf : conffile-prompt : '/etc/demo:part.conf' '/etc/demo:part.conf.dpkg-new' 1 1\n");
        assert!(
            matches!(&events[..], [ObservedPrompt::Progress{percent:None,..},ObservedPrompt::Conffile{text}] if text.contains("/etc/demo:part.conf.dpkg-new"))
        );
        assert!(parser.applying);
    }

    #[test]
    fn oversized_actual_plan_is_never_shown_as_a_complete_confirmation() {
        let mut parser = PromptParser::default();
        let plan = format!(
            "{}Do you want to continue? [Y/n] ",
            "dependency ".repeat(8_000)
        );
        let events = parser.push(plan.as_bytes());
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ObservedPrompt::Confirm { .. }))
        );
        assert!(events.iter().any(|event| matches!(event, ObservedPrompt::Progress{message,..} if message.contains("too large"))));
    }
}
