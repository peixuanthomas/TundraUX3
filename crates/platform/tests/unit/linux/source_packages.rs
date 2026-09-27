use super::*;

#[test]
fn only_ubuntu_and_arch_use_local_package_builds() {
    assert_eq!(
        distribution_backend("ID=ubuntu\nID_LIKE=debian"),
        Some(UpdateBackend::SystemDeb)
    );
    assert_eq!(
        distribution_backend("ID=\"arch\""),
        Some(UpdateBackend::SystemArch)
    );
    for os in ["ID=fedora", "ID=debian", "ID=other\nID_LIKE=arch"] {
        assert_eq!(distribution_backend(os), None);
    }
    assert!(!UpdateBackend::SystemRpm.uses_source_updates());
    assert!(UpdateBackend::PortableUser.uses_source_updates());
}

#[test]
fn native_install_preserves_confirmation_and_keeps_the_path_one_argument() {
    let root = crate::create_temp_dir(
        &std::env::temp_dir().join(format!("tundra-package-tests-{}", std::process::id())),
        "package-args",
    )
    .unwrap();
    let path = root.join("package with spaces;literal.deb");
    std::fs::write(&path, b"fixture").unwrap();
    for (backend, expected) in [
        (
            UpdateBackend::SystemDeb,
            vec![
                "--",
                "/usr/bin/apt-get",
                "--no-remove",
                "--no-install-recommends",
                "install",
                "--",
            ],
        ),
        (
            UpdateBackend::SystemArch,
            vec!["--", "/usr/bin/pacman", "-U", "--"],
        ),
    ] {
        let command = install_command(backend, &path).unwrap();
        assert_eq!(command.get_program(), "/usr/bin/sudo");
        let args: Vec<_> = command
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert_eq!(&args[..args.len() - 1], &expected);
        assert_eq!(args.last().unwrap(), &path.to_string_lossy());
    }
    assert!(install_command(UpdateBackend::PortableUser, &path).is_err());
    let link = root.join("link.deb");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(install_command(UpdateBackend::SystemDeb, &link).is_err());
    assert!(install_command(UpdateBackend::SystemDeb, Path::new("relative.deb")).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn source_detection_accepts_the_users_program_and_preserves_portable_installations() {
    let Some(expected) = distribution_backend(&std::fs::read_to_string("/etc/os-release").unwrap())
    else {
        return;
    };
    let uid = super::super::identity::LinuxUserContext::current()
        .unwrap()
        .process
        .uid;
    let root = crate::create_temp_dir(
        &std::env::temp_dir().join(format!("tundra-package-tests-{}", std::process::id())),
        "source-install-detection",
    )
    .unwrap();
    let executable = root.join("tundra-shell");
    std::fs::copy("/usr/bin/true", &executable).unwrap();
    assert_eq!(detect(&executable, uid).unwrap().unwrap().backend, expected);
    let other = root.join("unrelated-program");
    std::fs::copy(&executable, &other).unwrap();
    assert!(detect(&other, uid).is_err());
    std::fs::write(
        root.join(PORTABLE_MARKER),
        crate::installation::PORTABLE_MARKER_CONTENT,
    )
    .unwrap();
    assert!(detect(&executable, uid).unwrap().is_none());
    std::fs::remove_dir_all(root).unwrap();
}
