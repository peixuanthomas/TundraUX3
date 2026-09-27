use super::*;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn fixture() -> (PathBuf, PreparedUpdate, PathBuf) {
    let root = platform::create_temp_dir(
        &std::env::temp_dir().join(format!("tundra-package-tests-{}", std::process::id())),
        "package-build-test",
    )
    .unwrap();
    let source = root.join("source");
    fs::create_dir_all(source.join("crates/ascii-assets/assets")).unwrap();
    fs::create_dir_all(source.join("packaging/debian")).unwrap();
    fs::create_dir_all(source.join("crates/weathr")).unwrap();
    for (name, text) in [
        ("Cargo.toml", "[workspace.package]\nversion = '1.3.1'\n"),
        ("crates/ascii-assets/assets/test.txt", "test asset"),
        (
            "packaging/debian/tundraux3.desktop",
            "[Desktop Entry]\nName=TundraUX3\nExec=tundra-shell\nType=Application\n",
        ),
        ("LICENSE", "fixture license"),
        ("crates/weathr/LICENSE.weathr", "fixture license"),
    ] {
        fs::write(source.join(name), text).unwrap();
    }
    let prepared = PreparedUpdate {
        work_dir: root.clone(),
        target_sha: SHA.into(),
        shell_exe: PathBuf::from("/usr/bin/true"),
        cli_exe: PathBuf::from("/usr/bin/true"),
    };
    (root, prepared, source)
}

#[test]
fn package_versions_track_the_commit_and_reject_recipe_injection() {
    let (root, _, source) = fixture();
    assert_eq!(
        package_version(&source, SHA, UpdateBackend::SystemDeb, 123).unwrap(),
        format!("1.3.1+git123.{SHA}")
    );
    assert_eq!(
        package_version(&source, SHA, UpdateBackend::SystemArch, 123).unwrap(),
        format!("1.3.1.r123.g{SHA}-1")
    );
    assert!(
        package_version(
            &source,
            "$(touch /tmp/oops)",
            UpdateBackend::SystemArch,
            123
        )
        .is_err()
    );
    assert!(package_version(&source, SHA, UpdateBackend::SystemRpm, 123).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn package_payload_rejects_links() {
    let (root, _, source) = fixture();
    std::os::unix::fs::symlink(
        "/etc/passwd",
        source.join("crates/ascii-assets/assets/link"),
    )
    .unwrap();
    assert!(
        copy_tree(
            &source.join("crates/ascii-assets/assets"),
            &root.join("output")
        )
        .is_err()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn private_build_directories_do_not_restrict_installed_system_directories() {
    let (root, _, _) = fixture();
    let payload = root.join("payload");
    fs::create_dir_all(payload.join("usr/bin")).unwrap();
    for path in [&payload, &payload.join("usr"), &payload.join("usr/bin")] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    directory_modes(&payload).unwrap();
    for path in [&payload, &payload.join("usr"), &payload.join("usr/bin")] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires Ubuntu dpkg-dev; builds a real package without installing it"]
fn ubuntu_builds_real_deb_with_version_ownership_assets_and_dependencies() {
    let (root, prepared, source) = fixture();
    let native = platform::native_platform();
    let package = build(
        native.as_ref(),
        &prepared,
        &source,
        UpdateBackend::SystemDeb,
    )
    .unwrap();
    let fields = std::process::Command::new("dpkg-deb")
        .args(["-f"])
        .arg(&package.path)
        .output()
        .unwrap();
    assert!(fields.status.success());
    let fields = String::from_utf8(fields.stdout).unwrap();
    assert!(fields.contains(&format!("Version: {}", package.version)));
    assert!(fields.contains("libc6"));
    let listing = std::process::Command::new("dpkg-deb")
        .arg("-c")
        .arg(&package.path)
        .output()
        .unwrap();
    assert!(listing.status.success());
    let listing = String::from_utf8(listing.stdout).unwrap();
    assert!(listing.contains("root/root"));
    for file in [
        "./usr/bin/tundra-shell",
        "./usr/bin/tundra-cli",
        "./usr/share/tundraux3/assets/test.txt",
    ] {
        assert!(listing.contains(file), "{listing}");
    }
    assert!(!listing.contains("tundra-installation.json"));
    assert!(!listing.contains("/home/"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires Arch makepkg and fakeroot; run as an ordinary user"]
fn arch_builds_real_pacman_package() {
    let (root, prepared, source) = fixture();
    let native = platform::native_platform();
    let package = build(
        native.as_ref(),
        &prepared,
        &source,
        UpdateBackend::SystemArch,
    )
    .unwrap();
    let query = std::process::Command::new("pacman")
        .arg("-Qp")
        .arg(&package.path)
        .output()
        .unwrap();
    assert!(query.status.success());
    assert_eq!(
        String::from_utf8(query.stdout).unwrap().trim(),
        format!("tundraux3 {}", package.version)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "exports a DEB for an explicit installation smoke test; requires TUNDRA_PACKAGE_TEST_BIN and TUNDRA_PACKAGE_TEST_OUTPUT"]
fn ubuntu_export_current_build_for_install_smoke() {
    let binaries =
        PathBuf::from(std::env::var_os("TUNDRA_PACKAGE_TEST_BIN").expect("binary directory"));
    let output =
        PathBuf::from(std::env::var_os("TUNDRA_PACKAGE_TEST_OUTPUT").expect("output directory"));
    assert!(output.is_absolute());
    fs::create_dir_all(&output).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let root = platform::create_temp_dir(
        &std::env::temp_dir().join(format!("tundra-package-tests-{}", std::process::id())),
        "package-install-smoke",
    )
    .unwrap();
    let sha = current_build_identity()
        .commit_sha
        .expect("workspace commit");
    let prepared = PreparedUpdate {
        work_dir: root.clone(),
        target_sha: sha.clone(),
        shell_exe: binaries.join("tundra-shell"),
        cli_exe: binaries.join("tundra-cli"),
    };
    validate_update_probe(&prepared.shell_exe, &sha).unwrap();
    validate_update_probe(&prepared.cli_exe, &sha).unwrap();
    let native = platform::native_platform();
    let package = build(
        native.as_ref(),
        &prepared,
        &source,
        UpdateBackend::SystemDeb,
    )
    .unwrap();
    fs::copy(&package.path, output.join("tundraux3.deb")).unwrap();
    fs::write(
        output.join("expected.json"),
        serde_json::to_vec(&serde_json::json!({"version": package.version, "commit": sha}))
            .unwrap(),
    )
    .unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires the exported smoke package to be installed; TUNDRA_PACKAGE_TEST_OUTPUT selects its expected metadata"]
fn ubuntu_verifies_installed_smoke_package() {
    let output =
        PathBuf::from(std::env::var_os("TUNDRA_PACKAGE_TEST_OUTPUT").expect("output directory"));
    let expected: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("expected.json")).unwrap()).unwrap();
    let mut package = PreparedPackage {
        work_dir: output.clone(),
        path: output.join("tundraux3.deb"),
        backend: UpdateBackend::SystemDeb,
        version: expected["version"].as_str().unwrap().into(),
        target_sha: expected["commit"].as_str().unwrap().into(),
    };
    verify_installed(&package).unwrap();
    package.target_sha = "0000000000000000000000000000000000000000".into();
    assert!(
        verify_installed(&package).is_err(),
        "wrong program commit must not report success"
    );
    package.version = "0.0.0".into();
    assert!(
        verify_installed(&package).is_err(),
        "wrong installed package version must not report success"
    );
}
