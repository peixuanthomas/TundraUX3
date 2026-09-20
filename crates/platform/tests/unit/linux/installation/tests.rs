use super::*;
#[test]
fn package_ownership_never_falls_through_to_portable() {
    let rpm = RpmIdentity {
        name: PACKAGE_NAME.into(),
        version: "1-1".into(),
        architecture: "x86_64".into(),
    };
    let installed = classify_installation(
        Path::new("/tmp/user-owned"),
        Ok(Some(rpm.clone())),
        true,
        true,
        || panic!("RPM installation entered portable path"),
    )
    .unwrap();
    assert_eq!(installed.backend, UpdateBackend::SystemRpm);
    assert!(
        classify_installation(
            Path::new("/usr/bin"),
            Ok(Some(rpm)),
            true,
            false,
            || panic!("Unsupported distribution entered portable path")
        )
        .is_err()
    );
    assert!(
        classify_installation(
            Path::new("/tmp"),
            Err(ServiceError::Timeout),
            true,
            true,
            || panic!("Unknown RPM ownership entered portable path")
        )
        .is_err()
    );
    assert!(
        classify_installation(Path::new("/usr/bin"), Ok(None), true, true, || Err(
            ServiceError::Unsupported
        ))
        .is_err()
    );
    let portable =
        classify_installation(Path::new("/home/user/Tundra"), Ok(None), true, true, || {
            Ok(())
        })
        .unwrap();
    assert_eq!(portable.backend, UpdateBackend::PortableUser);
    assert!(portable.rpm.is_none());
}

#[test]
fn writable_directory_without_marker_is_not_a_portable_installation() {
    use std::os::unix::fs::symlink;
    let uid = super::super::identity::LinuxUserContext::current()
        .unwrap()
        .process
        .uid;
    let temp_root =
        std::env::temp_dir().join(format!("tundra-installation-test-{}", std::process::id()));
    let root = crate::create_temp_dir(&temp_root, "installation").unwrap();
    for name in ["tundra-shell", "tundra-cli"] {
        std::fs::write(root.join(name), b"fixture").unwrap();
    }
    assert!(validate_portable_directory(&root, uid).is_err());
    std::fs::write(
        root.join(PORTABLE_MARKER),
        crate::installation::PORTABLE_MARKER_CONTENT,
    )
    .unwrap();
    assert!(validate_portable_directory(&root, uid).is_ok());
    std::fs::remove_file(root.join("tundra-cli")).unwrap();
    symlink("tundra-shell", root.join("tundra-cli")).unwrap();
    assert!(validate_portable_directory(&root, uid).is_err());
    std::fs::remove_dir_all(temp_root).unwrap();
}

#[test]
fn rpm_identity_requires_one_complete_record() {
    assert_eq!(
        parse_rpm("tundraux3\t1:1.3.1-1.fc43\tx86_64\n")
            .unwrap()
            .version,
        "1:1.3.1-1.fc43"
    );
    for value in [
        "",
        "tundraux3",
        "tundraux3\t\tx86_64",
        "tundraux3\t1\tx86_64\nother\t1\tx86_64",
    ] {
        assert!(parse_rpm(value).is_err());
    }
}
