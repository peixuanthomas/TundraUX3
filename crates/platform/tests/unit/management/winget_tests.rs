use super::*;
use crate::ProcessStream;

fn table(headers: &[&str], widths: &[usize], rows: &[Vec<&str>]) -> String {
    let line = |values: &[&str]| -> String {
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                if index + 1 == values.len() {
                    value.to_string()
                } else {
                    format!(
                        "{value}{}",
                        " ".repeat(widths[index] - display_width(value))
                    )
                }
            })
            .collect::<String>()
    };
    let heading = line(headers);
    let mut output = format!("{heading}\n{}\n", "-".repeat(display_width(&heading)));
    for values in rows {
        output.push_str(&line(values));
        output.push('\n');
    }
    output
}

fn package() -> Record {
    Record {
        name: "Git".into(),
        id: "Git.Git".into(),
        installed: "2.50.0".into(),
        available: "2.51.0".into(),
    }
}

fn command(action: &str, package: &Record) -> ManagementCommand {
    ManagementCommand {
        kind: ManagementKind::Packages,
        action: action.into(),
        target: Some(package.id.clone()),
        values: BTreeMap::new(),
        identity: row(package.clone(), "updates").identity,
    }
}

fn output(code: u32, text: &str) -> ProcessExit {
    ProcessExit {
        code: Some(code as i32),
        stdout: ProcessStream::from_bytes(text.as_bytes().to_vec()),
        stderr: ProcessStream::from_bytes(Vec::new()),
    }
}

#[test]
fn reads_english_installed_tables_with_optional_columns_absent() {
    let text = table(
        &["Name", "Id", "Version"],
        &[20, 24, 10],
        &[vec!["Git", "Git.Git", "2.50.0"]],
    );
    let records = parse_table(&text, "installed").unwrap();
    assert_eq!(records[0].id, "Git.Git");
    assert_eq!(records[0].installed, "2.50.0");
    assert!(records[0].available.is_empty());
    assert_eq!(row(records[0].clone(), "installed").actions.len(), 1);
}

#[test]
fn reads_literal_english_and_chinese_winget_layouts() {
    let english = "Name                Id          Version  Available\n\
-------------------------------------------------\n\
Git                 Git.Git     2.50.0   2.51.0\n\
1 upgrades available.\n";
    let chinese = "名称      ID          版本    可用\n\
--------------------------------\n\
编辑器    App.Editor  1.0     2.0\n\
1 升级可用。\n";
    let english = parse_table(english, "updates").unwrap();
    assert_eq!(english, vec![package()]);
    let chinese = parse_table(chinese, "updates").unwrap();
    assert_eq!(chinese[0].name, "编辑器");
    assert_eq!(chinese[0].id, "App.Editor");
    assert_eq!(chinese[0].installed, "1.0");
    assert_eq!(chinese[0].available, "2.0");
}

#[test]
fn spinner_overwritten_by_header_does_not_hide_or_change_package_rows() {
    let text = "   -\r   \\\r   |\rName                Id          Version  Available\n\
-------------------------------------------------\n\
Git                 Git.Git     2.50.0   2.51.0\n";
    assert_eq!(parse_table(text, "updates").unwrap(), vec![package()]);
    let changed_row = text.replace(
        "Git                 Git.Git",
        "Fake\rGit                 Git.Git",
    );
    assert!(parse_table(&changed_row, "updates").is_err());
}

#[test]
fn chinese_names_and_graphemes_do_not_shift_package_identity() {
    let text = table(
        &["名称", "ID", "版本", "可用"],
        &[24, 26, 12, 12],
        &[vec![
            "编辑器 👩‍💻 e\u{301}",
            "Microsoft.VisualStudioCode",
            "1.100",
            "1.101",
        ]],
    );
    let records = parse_table(&text, "updates").unwrap();
    assert_eq!(records[0].name, "编辑器 👩‍💻 e\u{301}");
    assert_eq!(records[0].id, "Microsoft.VisualStudioCode");
    assert_eq!(records[0].installed, "1.100");
    assert_eq!(records[0].available, "1.101");
}

#[test]
fn search_match_is_not_a_version_and_source_is_checked() {
    let text = table(
        &["Name", "Id", "Version", "Match", "Source"],
        &[16, 22, 12, 20, 10],
        &[vec!["Git", "Git.Git", "2.51.0", "Moniker: git", "winget"]],
    );
    let records = parse_table(&text, "search").unwrap();
    assert_eq!(records[0].available, "2.51.0");
    assert!(records[0].installed.is_empty());
    assert!(parse_table(&text.replace("winget", "msstore"), "search").is_err());
}

#[test]
fn upgrade_summaries_and_pinned_tables_use_their_own_column_widths() {
    let mut text = table(
        &["Name", "Id", "Version", "Available"],
        &[16, 22, 12, 12],
        &[vec!["Git", "Git.Git", "2.50.0", "2.51.0"]],
    );
    text.push_str("1 upgrades available.\n\nThe following packages have an upgrade available, but require explicit targeting for upgrade:\n");
    text.push_str(&table(
        &["Name", "Id", "Version", "Available"],
        &[24, 28, 16, 12],
        &[vec![
            "Visual Studio Code",
            "Microsoft.VisualStudioCode",
            "1.100",
            "1.101",
        ]],
    ));
    let records = parse_table(&text, "updates").unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[1].id, "Microsoft.VisualStudioCode");
}

#[test]
fn unknown_layouts_and_ambiguous_installed_ids_are_rejected() {
    assert!(
        parse_table(
            "Nom Identifiant Version\n-----------------------\nGit Git.Git 2.50.0",
            "installed"
        )
        .is_err()
    );
    assert!(parse_table("", "installed").is_err());
    let text = table(
        &["Name", "Id", "Version"],
        &[16, 22, 12],
        &[
            vec!["Git", "Git.Git", "2.50.0"],
            vec!["Git x86", "Git.Git", "2.49.0"],
        ],
    );
    assert!(parse_table(&text, "installed").is_err());
}

#[test]
fn no_results_are_scope_specific_and_write_errors_never_become_success() {
    for text in [
        "No installed package found matching input criteria.",
        "找不到与输入条件匹配的程序包。",
    ] {
        assert!(parse_table(text, "search").unwrap().is_empty());
    }
    assert!(
        verified_output(&output(NO_APPLICATIONS, "no match"), "installed")
            .unwrap()
            .is_none()
    );
    assert!(
        verified_output(&output(NO_UPDATES, "no updates"), "updates")
            .unwrap()
            .is_none()
    );
    assert!(verified_output(&output(NO_APPLICATIONS, "no match"), "write").is_err());
    assert!(verified_output(&output(NO_UPDATES, "no updates"), "write").is_err());
    assert!(verified_output(&output(0x8a15_0046, "source agreements"), "search").is_err());
}

#[test]
fn shortened_identities_are_displayed_but_never_modifiable() {
    for (id, installed, available) in [
        ("Microsoft.Visual…", "1", "2"),
        ("Git.Git", "1…", "2"),
        ("Git.Git", "1", "2…"),
    ] {
        let package = Record {
            name: "Package".into(),
            id: id.into(),
            installed: installed.into(),
            available: available.into(),
        };
        assert!(
            row(package.clone(), "updates")
                .actions
                .iter()
                .all(|action| action.disabled_reason.is_some())
        );
        assert!(validate_identity(&command("upgrade", &package), &package).is_err());
    }
}

#[test]
fn changed_identity_or_versions_require_a_new_confirmation() {
    let original = package();
    let request = command("upgrade", &original);
    validate_identity(&request, &original).unwrap();
    for changed in [
        Record {
            installed: "2.49.0".into(),
            ..original.clone()
        },
        Record {
            available: "2.52.0".into(),
            ..original.clone()
        },
        Record {
            id: "Other.Package".into(),
            ..original.clone()
        },
    ] {
        assert!(matches!(
            validate_identity(&request, &changed),
            Err(ManagementError::Conflict(_))
        ));
    }
    let mut missing_identity = request;
    missing_identity.identity.clear();
    assert!(validate_identity(&missing_identity, &original).is_err());
}

#[test]
fn writes_use_exact_ids_pinned_versions_and_no_shell_or_automatic_agreement() {
    for (action, verb) in [
        ("install", "install"),
        ("upgrade", "upgrade"),
        ("remove", "uninstall"),
    ] {
        let spec = build_command(Path::new("winget.exe"), &command(action, &package())).unwrap();
        assert_eq!(spec.program(), Path::new("winget.exe"));
        assert_eq!(spec.args_slice()[0], verb);
        assert!(
            spec.args_slice()
                .windows(3)
                .any(|args| args == ["--id", "Git.Git", "--exact"])
        );
        assert!(
            !spec
                .args_slice()
                .iter()
                .any(|arg| arg.starts_with("--accept-"))
        );
        assert_eq!(
            spec.args_slice().iter().any(|arg| arg == "--version"),
            action != "remove"
        );
    }
    for id in [
        "--all",
        "Git.Git & calc",
        "a;cmd",
        "C:\\evil",
        "*",
        "Git.…",
        "Git...",
    ] {
        let mut request = command("remove", &package());
        request.target = Some(id.into());
        assert!(build_command(Path::new("winget.exe"), &request).is_err());
    }
}

#[test]
fn upgrade_all_requires_the_reviewed_update_plan() {
    let package = package();
    let mut request = ManagementCommand {
        kind: ManagementKind::Packages,
        action: "upgrade_all".into(),
        target: None,
        values: BTreeMap::new(),
        identity: BTreeMap::new(),
    };
    assert!(build_command(Path::new("winget.exe"), &request).is_err());
    request
        .values
        .insert("winget_plan".into(), update_plan(&[package.clone()]));
    let spec = build_command(Path::new("winget.exe"), &request).unwrap();
    assert!(spec.args_slice().iter().any(|arg| arg == "--all"));
    assert_ne!(
        update_plan(&[Record {
            available: "3.0".into(),
            ..package.clone()
        }]),
        update_plan(&[package])
    );
    request
        .values
        .insert("arbitrary_option".into(), "--force".into());
    assert!(build_command(Path::new("winget.exe"), &request).is_err());
}

#[test]
fn cancelled_query_does_not_launch_or_lookup_winget() {
    assert!(matches!(
        query(
            &ManagementQuery::new(ManagementKind::Packages),
            &AtomicBool::new(true)
        ),
        Err(ManagementError::Cancelled)
    ));
}

#[cfg(windows)]
#[test]
#[ignore = "read-only live winget queries; run explicitly on Windows with source agreements already accepted"]
fn windows_winget_readonly_smoke() {
    let cancelled = AtomicBool::new(false);
    let program = executable().unwrap();
    let version = read(ProcessSpec::new(&program).arg("--version"), &cancelled).unwrap();
    assert_eq!(version.code, Some(0));
    assert!(!version.stdout.utf8_lossy().trim().is_empty());
    let mut request = ManagementQuery::new(ManagementKind::Packages);
    for scope in ["installed", "updates", "search"] {
        request.scope = scope.into();
        request.filter = if scope == "search" {
            "Microsoft.PowerShell".into()
        } else {
            String::new()
        };
        let snapshot = query(&request, &cancelled).unwrap();
        assert_eq!(snapshot.backend, "winget");
        if scope == "search" {
            let package = snapshot
                .rows
                .first()
                .expect("PowerShell must exist in the community source");
            request.target = Some(package.id.clone());
            let details = query(&request, &cancelled).unwrap();
            assert!(details.details_complete);
            assert_eq!(details.rows[0].id, package.id);
        }
    }
}
