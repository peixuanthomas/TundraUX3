use platform::{PlatformError, PlatformIcon};

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

#[cfg(windows)]
#[test]
fn windows_platform_extracts_an_rgba_icon_for_the_current_executable() {
    let executable = std::env::current_exe().expect("current executable path");
    let icon = platform::windows::WindowsPlatform
        .file_icon(&executable, 32)
        .expect("Windows icon lookup should not fail")
        .expect("current executable should have a Shell icon");

    assert_eq!((icon.width(), icon.height()), (32, 32));
    assert_eq!(icon.rgba().len(), 32 * 32 * 4);
}
