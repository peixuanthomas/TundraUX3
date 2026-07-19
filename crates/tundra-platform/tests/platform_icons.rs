use std::path::Path;

use tundra_platform::mock::{MockCall, MockPlatform, UnsupportedPlatform};
use tundra_platform::{Platform, PlatformError, PlatformIcon, UserDirs, build_windows_app_paths};

#[test]
fn platform_icon_requires_nonzero_dimensions_and_exact_rgba_length() {
    let icon = PlatformIcon::new(2, 3, vec![7; 24]).expect("valid RGBA icon");
    assert_eq!(icon.width(), 2);
    assert_eq!(icon.height(), 3);
    assert_eq!(icon.rgba(), vec![7; 24]);

    assert!(matches!(
        PlatformIcon::new(0, 3, Vec::new()),
        Err(PlatformError::InvalidInput { .. })
    ));
    assert!(matches!(
        PlatformIcon::new(2, 3, vec![0; 23]),
        Err(PlatformError::InvalidInput { .. })
    ));
}

#[test]
fn mock_platform_injects_and_records_file_icon_requests() {
    let base = std::env::temp_dir().join("tundra-platform-icon-mock");
    let platform = mock_platform(&base);
    let path = base.join("approved.exe");
    let icon = PlatformIcon::new(1, 1, vec![1, 2, 3, 4]).expect("valid icon");
    platform.set_file_icon(path.clone(), Some(icon.clone()));

    assert_eq!(platform.file_icon(&path, 64), Ok(Some(icon)));
    assert_eq!(platform.file_icon(&base.join("missing.exe"), 32), Ok(None),);
    assert!(platform.calls().contains(&MockCall::FileIcon {
        path: path.clone(),
        preferred_size: 64,
    }));
    assert!(platform.calls().contains(&MockCall::FileIcon {
        path: base.join("missing.exe"),
        preferred_size: 32,
    }));
}

#[test]
fn unsupported_platform_returns_no_file_icon() {
    assert_eq!(
        UnsupportedPlatform.file_icon(Path::new("unsupported"), 64),
        Ok(None)
    );
}

#[cfg(windows)]
#[test]
fn windows_platform_extracts_an_rgba_icon_for_the_current_executable() {
    let executable = std::env::current_exe().expect("current executable path");
    let icon = tundra_platform::windows::WindowsPlatform
        .file_icon(&executable, 32)
        .expect("Windows icon lookup should not fail")
        .expect("current executable should have a Shell icon");

    assert_eq!((icon.width(), icon.height()), (32, 32));
    assert_eq!(icon.rgba().len(), 32 * 32 * 4);
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
    .expect("fixture user directories should resolve");
    let app_paths =
        build_windows_app_paths(base.join("Roaming"), base.join("Local"), base.join("Temp"))
            .expect("fixture app paths should resolve");
    MockPlatform::new(user_dirs, app_paths)
}
