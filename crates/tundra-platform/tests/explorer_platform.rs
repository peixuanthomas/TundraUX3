use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_platform::mock::{MockCall, MockPlatform};
use tundra_platform::{
    AppPaths, DirectoryListing, ExecutableKind, FileAttributes, FileOpenPolicy, Platform,
    PlatformError, PlatformKind, UserDirs, cleanup_temp_path, default_file_open_policy,
};

#[test]
fn windows_classifies_every_launcher_managed_extension_case_insensitively() {
    let cases = [
        ("EXE", ExecutableKind::NativeBinary),
        ("com", ExecutableKind::NativeBinary),
        ("scr", ExecutableKind::NativeBinary),
        ("cpl", ExecutableKind::NativeBinary),
        ("pif", ExecutableKind::NativeBinary),
        ("msi", ExecutableKind::Installer),
        ("msp", ExecutableKind::Installer),
        ("msix", ExecutableKind::Installer),
        ("msixbundle", ExecutableKind::Installer),
        ("appx", ExecutableKind::Installer),
        ("appxbundle", ExecutableKind::Installer),
        ("bat", ExecutableKind::Script),
        ("cmd", ExecutableKind::Script),
        ("ps1", ExecutableKind::Script),
        ("psm1", ExecutableKind::Script),
        ("vbs", ExecutableKind::Script),
        ("vbe", ExecutableKind::Script),
        ("js", ExecutableKind::Script),
        ("jse", ExecutableKind::Script),
        ("wsf", ExecutableKind::Script),
        ("wsh", ExecutableKind::Script),
        ("hta", ExecutableKind::Script),
        ("jar", ExecutableKind::Script),
        ("py", ExecutableKind::Script),
        ("pyw", ExecutableKind::Script),
        ("rb", ExecutableKind::Script),
        ("pl", ExecutableKind::Script),
        ("lnk", ExecutableKind::Shortcut),
        ("url", ExecutableKind::Shortcut),
        ("reg", ExecutableKind::Shortcut),
        ("scf", ExecutableKind::Shortcut),
    ];

    for (extension, expected_kind) in cases {
        let path = PathBuf::from(format!("program.{extension}"));
        let policy =
            default_file_open_policy(PlatformKind::Windows, &path, &file_attributes(path.clone()));
        match policy {
            FileOpenPolicy::LauncherRequired { kind, reason } => {
                assert_eq!(kind, expected_kind, "incorrect kind for .{extension}");
                assert!(reason.contains("Launcher"));
            }
            other => panic!(".{extension} should require Launcher, got {other:?}"),
        }
    }
}

#[test]
fn open_policy_blocks_links_and_only_classifies_executable_files() {
    let linked_path = PathBuf::from("linked.exe");
    let mut linked = file_attributes(linked_path.clone());
    linked.symlink = true;
    assert!(matches!(
        default_file_open_policy(PlatformKind::Windows, &linked_path, &linked),
        FileOpenPolicy::Blocked { .. }
    ));

    let directory_path = PathBuf::from("directory.exe");
    let mut directory = file_attributes(directory_path.clone());
    directory.is_file = false;
    directory.is_dir = true;
    assert_eq!(
        default_file_open_policy(PlatformKind::Windows, &directory_path, &directory),
        FileOpenPolicy::SystemDefault
    );
    assert_eq!(
        default_file_open_policy(PlatformKind::Macos, &directory_path, &directory),
        FileOpenPolicy::SystemDefault
    );
    assert_eq!(
        default_file_open_policy(PlatformKind::Unsupported, &directory_path, &directory),
        FileOpenPolicy::SystemDefault
    );

    let document_path = PathBuf::from("notes.txt");
    assert_eq!(
        default_file_open_policy(
            PlatformKind::Windows,
            &document_path,
            &file_attributes(document_path.clone()),
        ),
        FileOpenPolicy::SystemDefault
    );
}

#[test]
fn exe_requires_launcher_on_non_windows_platforms() {
    let path = PathBuf::from("portable.ExE");
    let attributes = file_attributes(path.clone());

    for platform in [PlatformKind::Macos, PlatformKind::Unsupported] {
        assert!(matches!(
            default_file_open_policy(platform, &path, &attributes),
            FileOpenPolicy::LauncherRequired {
                kind: ExecutableKind::NativeBinary,
                ..
            }
        ));
    }
}

#[test]
fn macos_classifies_bundles_installers_and_mach_o_files() {
    let base = unique_temp_root("mach-o-policy");
    fs::create_dir_all(&base).expect("test root");
    let executable = base.join("native-tool");
    fs::write(&executable, [0xcf, 0xfa, 0xed, 0xfe, 0, 0, 0, 0]).expect("Mach-O fixture");

    assert!(matches!(
        default_file_open_policy(
            PlatformKind::Macos,
            &executable,
            &file_attributes(executable.clone()),
        ),
        FileOpenPolicy::LauncherRequired {
            kind: ExecutableKind::NativeBinary,
            ..
        }
    ));

    let application = base.join("Example.APP");
    let mut application_attributes = file_attributes(application.clone());
    application_attributes.is_file = false;
    application_attributes.is_dir = true;
    assert!(matches!(
        default_file_open_policy(PlatformKind::Macos, &application, &application_attributes),
        FileOpenPolicy::LauncherRequired {
            kind: ExecutableKind::ApplicationBundle,
            ..
        }
    ));

    let installer = base.join("Setup.MPKG");
    assert!(matches!(
        default_file_open_policy(
            PlatformKind::Macos,
            &installer,
            &file_attributes(installer.clone()),
        ),
        FileOpenPolicy::LauncherRequired {
            kind: ExecutableKind::Installer,
            ..
        }
    ));

    cleanup_temp_path(&base).expect("cleanup Mach-O fixture");
}

#[test]
fn directory_listing_keeps_entries_with_metadata_failures() {
    let base = unique_temp_root("partial-listing");
    fs::create_dir_all(&base).expect("test root");
    let readable = base.join("readable.txt");
    let unreadable = base.join("unreadable.txt");
    fs::write(&readable, b"ok").expect("readable fixture");
    fs::write(&unreadable, b"metadata error").expect("unreadable fixture");

    let platform = mock_platform(&base);
    platform.set_file_attributes_error(
        unreadable.clone(),
        PlatformError::Io {
            operation: "read file attributes",
            path: Some(unreadable.clone()),
            message: "injected metadata failure".to_string(),
        },
    );

    let listing = platform.read_directory(&base).expect("partial listing");
    assert_eq!(listing.entries.len(), 2);
    assert_eq!(listing.warnings.len(), 1);
    let failed = listing
        .entries
        .iter()
        .find(|entry| entry.path == unreadable)
        .expect("failed entry remains visible");
    assert!(failed.attributes.is_none());
    assert!(matches!(failed.open_policy, FileOpenPolicy::Blocked { .. }));

    cleanup_temp_path(&base).expect("cleanup partial listing fixture");
}

#[test]
fn mock_injects_directory_policy_and_cross_device_rename_results() {
    let base = unique_temp_root("mock-explorer-injection");
    let platform = mock_platform(&base);
    let virtual_directory = base.join("virtual");
    let listing = DirectoryListing {
        path: virtual_directory.clone(),
        entries: Vec::new(),
        warnings: Vec::new(),
    };
    platform.set_directory_listing(virtual_directory.clone(), listing.clone());
    assert_eq!(
        platform
            .read_directory(&virtual_directory)
            .expect("injected listing"),
        listing
    );

    let executable = virtual_directory.join("tool.bin");
    platform.set_file_open_policy(
        executable.clone(),
        FileOpenPolicy::launcher_required(
            ExecutableKind::NativeBinary,
            "injected executable classification",
        ),
    );
    assert!(
        platform
            .file_open_policy(&executable, &file_attributes(executable.clone()))
            .requires_launcher()
    );

    let target = base.join("other-volume").join("tool.bin");
    platform.set_cross_device_rename(executable.clone(), target.clone(), "not same device");
    assert!(matches!(
        platform.rename_path(&executable, &target),
        Err(PlatformError::CrossDevice { .. })
    ));
    assert!(platform.calls().contains(&MockCall::RenamePath {
        source: executable,
        target,
    }));
}

#[cfg(windows)]
#[test]
fn windows_directory_listing_returns_metadata_from_native_enumeration() {
    use tundra_platform::windows::WindowsPlatform;

    let base = unique_temp_root("windows-native-listing");
    fs::create_dir_all(&base).expect("test root");
    let file = base.join("sample.txt");
    fs::write(&file, b"native metadata").expect("file fixture");
    fs::create_dir(base.join("folder")).expect("directory fixture");

    let listing = WindowsPlatform
        .read_directory(&base)
        .expect("native directory listing");
    assert!(listing.warnings.is_empty());
    let file_entry = listing
        .entries
        .iter()
        .find(|entry| entry.path == file)
        .expect("file entry");
    let attributes = file_entry.attributes.as_ref().expect("native metadata");
    assert!(attributes.is_file);
    assert_eq!(attributes.len, 15);
    assert!(attributes.modified.is_some());
    assert!(listing.entries.iter().any(|entry| {
        entry
            .attributes
            .as_ref()
            .is_some_and(|attributes| attributes.is_dir)
    }));

    cleanup_temp_path(&base).expect("cleanup native listing fixture");
}

fn file_attributes(path: PathBuf) -> FileAttributes {
    FileAttributes {
        path,
        is_file: true,
        is_dir: false,
        len: 0,
        readonly: false,
        modified: None,
        hidden: false,
        system: false,
        archive: false,
        symlink: false,
        junction: false,
        reparse_point: false,
        shortcut: false,
    }
}

fn mock_platform(base: &Path) -> MockPlatform {
    let user_dirs = UserDirs::new(
        base.join("Desktop"),
        base.join("Documents"),
        base.join("Downloads"),
        base.join("Pictures"),
        base.join("Videos"),
        base.join("Music"),
        base.join("Roaming"),
    )
    .expect("fixture user directories");
    let app_paths = AppPaths::from_parts(
        base.join("config").join("config.toml"),
        base.join("data"),
        base.join("cache"),
        base.join("logs"),
        base.join("temp"),
    )
    .expect("fixture app paths");
    MockPlatform::new(user_dirs, app_paths)
}

fn unique_temp_root(case: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "tundra-platform-explorer-{}-{nanos}-{case}",
        std::process::id()
    ))
}
