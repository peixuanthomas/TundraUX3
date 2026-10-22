use super::*;

#[test]
fn expired_idle_connection_is_closed_without_waiting_for_input() {
    let root = tempfile::tempdir().unwrap();
    let mut server = Server::start(&root.path().join("control")).unwrap();
    let _client = connect(&server.info).unwrap();
    assert!(server.poll().is_empty());
    assert_eq!(server.clients.len(), 1);
    server.clients.values_mut().next().unwrap().since = Instant::now() - CLIENT_TIMEOUT;
    assert!(server.poll().is_empty());
    assert!(server.clients.is_empty());
}

#[cfg(windows)]
#[test]
fn windows_registry_and_pipe_grant_only_current_user_and_deny_network() {
    use interprocess::os::windows::security_descriptor::AsSecurityDescriptor;
    use std::os::windows::io::{AsHandle, AsRawHandle};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::{
        Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
            GetSecurityInfo, SE_FILE_OBJECT, SE_KERNEL_OBJECT,
        },
        DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    };
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(&root.path().join("control")).unwrap();
    let info = DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION;
    let check = |descriptor: windows_sys::Win32::Security::PSECURITY_DESCRIPTOR| {
        let mut text = std::ptr::null_mut();
        // SAFETY: the descriptor remains live; Windows allocates the output string.
        let sddl = unsafe {
            assert_ne!(
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    descriptor,
                    1,
                    info,
                    &mut text,
                    std::ptr::null_mut()
                ),
                0
            );
            let sddl = widestring::U16CStr::from_ptr_str(text).to_string_lossy();
            LocalFree(text.cast());
            sddl
        };
        assert_eq!(sddl.matches("(A;").count(), 1);
        assert_eq!(sddl.matches("(D;").count(), 1);
        assert!(sddl.contains(";;;NU)"));
        let owner = sddl.strip_prefix("O:").unwrap().split("D:").next().unwrap();
        assert!(sddl.ends_with(&format!(";;;{owner})")));
    };
    check(windows::descriptor().unwrap().as_sd().cast_mut());
    let stream = connect(&server.info).unwrap();
    let Stream::NamedPipe(pipe) = &stream;
    let mut pipe_descriptor = std::ptr::null_mut();
    // SAFETY: the live pipe handle is queried and Windows allocates the descriptor.
    unsafe {
        assert_eq!(
            GetSecurityInfo(
                pipe.as_handle().as_raw_handle(),
                SE_KERNEL_OBJECT,
                info,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut pipe_descriptor
            ),
            0
        );
        check(pipe_descriptor);
        LocalFree(pipe_descriptor);
    }
    for path in [root.path().join("control"), server.registration.clone()] {
        let path = widestring::U16CString::from_os_str(path).unwrap();
        let mut descriptor = std::ptr::null_mut();
        // SAFETY: Windows allocates the descriptor and owns its embedded pointers.
        unsafe {
            assert_eq!(
                GetNamedSecurityInfoW(
                    path.as_ptr(),
                    SE_FILE_OBJECT,
                    info,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut descriptor
                ),
                0
            );
            check(descriptor);
            LocalFree(descriptor);
        }
    }
}
