use super::*;

#[test]
fn common_dates_are_validated_and_never_silently_become_daily() {
    let mut values = BTreeMap::from([
        ("schedule_kind".into(), "daily".into()),
        ("hour".into(), "9".into()),
        ("minute".into(), "5".into()),
    ]);
    assert_eq!(calendar(&values).unwrap(), "*-*-* 09:05:00");
    values.insert("schedule_kind".into(), "weekly".into());
    assert!(calendar(&values).is_err());
    values.insert("day_0".into(), "true".into());
    values.insert("day_4".into(), "true".into());
    assert_eq!(calendar(&values).unwrap(), "Mon,Fri *-*-* 09:05:00");
    assert_eq!(cron_calendar(&values).unwrap(), "5 9 * * 1,5");
    values.extend([
        ("schedule_kind".into(), "once".into()),
        ("year".into(), "2028".into()),
        ("month".into(), "2".into()),
        ("day".into(), "29".into()),
    ]);
    assert_eq!(calendar(&values).unwrap(), "2028-02-29 09:05:00");
    values.insert("year".into(), "2027".into());
    assert!(calendar(&values).is_err());
    values.insert("persistent".into(), "true".into());
    assert!(persistent(&values).unwrap());
    assert!(cron_calendar(&values).is_err());
}

#[test]
fn advanced_expression_keeps_its_original_rule() {
    let values = BTreeMap::from([("schedule".into(), "Mon..Fri 09:00".into())]);
    assert_eq!(calendar(&values).unwrap(), "Mon..Fri 09:00");
    assert!(!persistent(&values).unwrap());
}

#[test]
fn selected_unicode_script_and_arguments_are_passed_without_shell_expansion() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("部署脚本.sh");
    fs::write(&path, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
    let values = BTreeMap::from([
        ("task_type".into(), "script".into()),
        ("script_path".into(), path.display().to_string()),
        ("arguments".into(), "'含 空格' '$(do-not-run)'".into()),
    ]);
    let (command, system) = operation(&values, &AtomicBool::new(false)).unwrap();
    assert!(!system);
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "含 空格\n$(do-not-run)\n"
    );
}

#[test]
fn scheduled_package_template_uses_no_input_and_propagates_tool_failures() {
    use super::super::super::packages::{PackageBackend, detect_backend};
    use std::os::unix::fs::PermissionsExt;
    let backend = detect_backend(&AtomicBool::new(false)).unwrap();
    let program = match backend {
        PackageBackend::Apt => "/usr/bin/apt-get",
        PackageBackend::Dnf4 => "/usr/bin/dnf",
        PackageBackend::Dnf5 => "/usr/bin/dnf5",
        PackageBackend::Pacman => "/usr/bin/pacman",
    };
    let directory = tempfile::tempdir().unwrap();
    let fake = directory.path().join("fake-package-manager");
    let log = directory.path().join("arguments");
    let content = format!(
        "#!/bin/sh\n[ \"$DEBIAN_FRONTEND\" = noninteractive ] || exit 91\nif read -r input; then exit 92; fi\nprintf '%s\\n' \"$*\" >> {}\ncase \"$*\" in *upgrade*|*-Syu*) exit 37;; esac\nexit 0\n",
        cron::shell_quote(&log.display().to_string())
    );
    fs::write(&fake, content).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let values = BTreeMap::from([("task_type".into(), "packages_update".into())]);
    let (command, root) = operation(&values, &AtomicBool::new(false)).unwrap();
    assert!(root);
    let command = command.replace(program, fake.to_str().unwrap());
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(&command)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(37));
    let calls = fs::read_to_string(&log).unwrap();
    if backend == PackageBackend::Pacman {
        assert_eq!(calls.lines().count(), 1);
        assert!(calls.contains("-Syu"));
        assert!(calls.contains("--noconfirm"));
    } else {
        assert_eq!(calls.lines().count(), 2);
        assert!(calls.lines().nth(1).unwrap().contains("upgrade"));
        if backend == PackageBackend::Apt {
            assert!(calls.contains("--force-confold"));
        }
        fs::write(&fake, "#!/bin/sh\nexit 23\n").unwrap();
        let failed_refresh = Command::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(failed_refresh.status.code(), Some(23));
    }
}
