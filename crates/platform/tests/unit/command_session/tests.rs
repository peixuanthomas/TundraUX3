use super::*;

fn run(session: &mut SystemCommandSession, command: &str, code: i32) {
    let result = session.run(command).unwrap();
    assert_eq!(result.exit_code, code, "{command}");
    assert!(
        result.state_error.is_none(),
        "{command}: {:?}",
        result.state_error
    );
}

#[cfg(unix)]
#[test]
fn unix_commands_retain_exports_directory_and_unsets_without_changing_the_host() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("folder with 'quotes' and 中文\n");
    std::fs::create_dir(&folder).unwrap();
    let folder = std::fs::canonicalize(folder).unwrap();
    let host_directory = std::env::current_dir().unwrap();
    let host_path = std::env::var_os("PATH");
    let mut session = SystemCommandSession::new().unwrap();
    session
        .environment
        .insert("DESTINATION".into(), folder.as_os_str().into());
    run(
        &mut session,
        "export RETAINED='first line\n第二行=value'; export EMPTY=; cd \"$DESTINATION\"",
        0,
    );
    run(
        &mut session,
        "test \"$RETAINED\" = 'first line\n第二行=value' && test \"${EMPTY+x}\" = x && test \"$PWD\" = \"$DESTINATION\" && printf saved > relative-file",
        0,
    );
    assert_eq!(
        std::fs::read(folder.join("relative-file")).unwrap(),
        b"saved"
    );
    let failed = session
        .run("export PATH=/nonexistent; unset RETAINED; cd /tundra-path-that-does-not-exist")
        .unwrap();
    assert_ne!(failed.exit_code, 0);
    assert!(failed.state_error.is_none());
    run(
        &mut session,
        "test \"$PATH\" = /nonexistent && test \"${RETAINED+x}\" != x && test \"$PWD\" = \"$DESTINATION\"",
        0,
    );
    run(&mut session, "cd ..", 0);
    run(&mut session, "cd -", 0);
    assert_eq!(session.directory, std::fs::canonicalize(&folder).unwrap());
    run(
        &mut session,
        "unset PATH; export AFTER_FAILURE=yes; false",
        1,
    );
    run(&mut session, "test \"$AFTER_FAILURE\" = yes", 0);
    assert!(!session.environment.contains_key(&OsString::from("PATH")));
    assert_eq!(std::env::current_dir().unwrap(), host_directory);
    assert_eq!(std::env::var_os("PATH"), host_path);
}

#[cfg(unix)]
#[test]
fn unix_exit_and_missing_snapshots_preserve_the_last_complete_state() {
    let mut session = SystemCommandSession::new().unwrap();
    run(&mut session, "export SAVED=before; exit 7", 7);
    run(&mut session, "test \"$SAVED\" = before", 0);
    for command in [
        "export SAVED=lost; exec /usr/bin/true",
        "export SAVED=lost; trap - EXIT",
        "export SAVED='unterminated",
    ] {
        let result = session.run(command).unwrap();
        // A shell may run EXIT even after a parse error. Either way, no
        // incomplete state or partial assignment may replace SAVED.
        if !command.ends_with("unterminated") {
            assert!(result.state_error.is_some());
        }
        run(&mut session, "test \"$SAVED\" = before", 0);
    }
    let mut other = SystemCommandSession::new().unwrap();
    run(&mut other, "test \"${SAVED+x}\" != x", 0);
}

#[cfg(unix)]
#[test]
fn unix_snapshots_preserve_non_utf8_environment_and_paths() {
    use std::os::unix::ffi::OsStringExt;
    let root = tempfile::tempdir().unwrap();
    #[cfg(target_os = "linux")]
    let folder = root
        .path()
        .join(OsString::from_vec(b"non-utf8-\xff\n".to_vec()));
    // macOS filesystems reject non-UTF-8 names; environment bytes still
    // exercise lossless capture on every Unix platform.
    #[cfg(not(target_os = "linux"))]
    let folder = root.path().join("unicode-中文\n");
    std::fs::create_dir(&folder).unwrap();
    let value = OsString::from_vec(b"one=two\n\xff".to_vec());
    let mut session = SystemCommandSession::new().unwrap();
    session.environment.insert("BYTES".into(), value.clone());
    session
        .environment
        .insert("DESTINATION".into(), folder.as_os_str().into());
    run(&mut session, "cd \"$DESTINATION\"", 0);
    assert_eq!(
        session.environment.get(&OsString::from("BYTES")),
        Some(&value)
    );
    assert_eq!(session.directory, std::fs::canonicalize(folder).unwrap());
}

#[cfg(windows)]
#[test]
fn windows_commands_retain_set_directory_and_unsets() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("folder with spaces 中文 ! & %");
    std::fs::create_dir(&folder).unwrap();
    let host_directory = std::env::current_dir().unwrap();
    let mut session = SystemCommandSession::new().unwrap();
    session
        .environment
        .insert("DESTINATION".into(), folder.as_os_str().into());
    run(&mut session, "set RETAINED=中文!=value", 0);
    run(&mut session, "cd /d \"%DESTINATION%\"", 0);
    run(
        &mut session,
        "if not \"%RETAINED%\"==\"中文!=value\" exit /b 9",
        0,
    );
    run(&mut session, "echo saved>relative-file", 0);
    assert!(folder.join("relative-file").exists());
    run(&mut session, "cmd /c exit 7", 7);
    run(&mut session, "set RETAINED=", 0);
    run(&mut session, "if defined RETAINED exit /b 9", 0);
    run(&mut session, "set PATH=", 0);
    run(&mut session, "cd /d Z:\\tundra-missing-path", 1);
    run(
        &mut session,
        "if /i not \"%CD%\"==\"%DESTINATION%\" exit /b 9",
        0,
    );
    run(&mut session, "for %i in (one two) do @echo %i", 0);
    assert_eq!(std::env::current_dir().unwrap(), host_directory);
}

#[cfg(windows)]
#[test]
fn windows_exit_without_snapshot_keeps_the_last_complete_state() {
    let mut session = SystemCommandSession::new().unwrap();
    run(&mut session, "set SAVED=before", 0);
    let result = session.run("set SAVED=lost&exit /b 7").unwrap();
    assert_eq!(result.exit_code, 7);
    assert!(result.state_error.is_some());
    run(&mut session, "if not \"%SAVED%\"==\"before\" exit /b 9", 0);
}
