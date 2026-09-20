use super::*;
#[test]
fn captured_platform_error_keeps_native_code_and_chain() {
    let native = std::io::Error::from_raw_os_error(13);
    let error = crate::PlatformError::from_io("copy", Some("/private/source".into()), &native);
    assert_eq!(error.raw_os_error(), Some(13));
    let source = error.source().unwrap();
    assert_eq!(source.to_string(), native.to_string());
    assert_eq!(
        source
            .downcast_ref::<CapturedIoError>()
            .unwrap()
            .os_error_code,
        Some(13)
    );
    let nested = std::io::Error::other(error);
    let captured = CapturedIoError::capture(&nested);
    assert!(captured.source().is_some());
}
