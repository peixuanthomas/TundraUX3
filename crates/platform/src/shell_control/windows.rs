use interprocess::local_socket::{Stream, prelude::*};
use interprocess::os::windows::named_pipe::{PipeListener, PipeListenerOptions, pipe_mode::Bytes};
use interprocess::os::windows::security_descriptor::{AsSecurityDescriptor, SecurityDescriptor};
use std::io::{self, Read};
use std::os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, ERROR_INSUFFICIENT_BUFFER, LocalFree},
    Security::{
        Authorization::{ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT},
        DACL_SECURITY_INFORMATION, GetTokenInformation, OWNER_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, SECURITY_ATTRIBUTES, SetFileSecurityW, TOKEN_QUERY,
        TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::CreateDirectoryW,
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub(super) struct Listener(PipeListener<Bytes, Bytes>);

impl Listener {
    pub(super) fn new(endpoint: &str) -> io::Result<Self> {
        let capacity = (super::IO_CHUNK * super::IO_ROUNDS) as u32;
        PipeListenerOptions::new()
            .path(format!(r"\\.\pipe\{endpoint}"))
            .nonblocking(true)
            .accept_remote(false)
            .input_buffer_size_hint(capacity)
            .output_buffer_size_hint(capacity)
            .security_descriptor(Some(descriptor()?))
            .create_duplex()
            .map(Self)
    }

    pub(super) fn accept(&self) -> io::Result<Stream> {
        self.0.accept().map(|pipe| Stream::NamedPipe(pipe.into()))
    }
}

fn sid_text(sid: windows_sys::Win32::Security::PSID) -> io::Result<String> {
    let mut text = std::ptr::null_mut();
    // SAFETY: callers supply a SID from Windows; Windows allocates the returned string.
    unsafe {
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let value = widestring::U16CStr::from_ptr_str(text).to_string_lossy();
        LocalFree(text.cast());
        Ok(value)
    }
}

fn user_sid() -> io::Result<String> {
    let mut token = std::ptr::null_mut();
    // SAFETY: OpenProcessToken initializes the handle, which OwnedHandle closes on every return.
    let token = unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        OwnedHandle::from_raw_handle(token)
    };
    let mut len = 0;
    // SAFETY: the first call queries the size; aligned storage holds all returned token data.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut len,
        );
        if io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
            return Err(io::Error::last_os_error());
        }
        let mut data = vec![0usize; (len as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            data.as_mut_ptr().cast(),
            len,
            &mut len,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        sid_text((*data.as_ptr().cast::<TOKEN_USER>()).User.Sid)
    }
}

pub(super) fn descriptor() -> io::Result<SecurityDescriptor> {
    let sid = user_sid()?;
    let sddl =
        widestring::U16CString::from_str(format!("O:{sid}D:P(D;OICI;GA;;;NU)(A;OICI;GA;;;{sid})"))
            .map_err(io::Error::other)?;
    SecurityDescriptor::deserialize(&sddl)
}

pub(super) fn private_directory(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path.parent().ok_or(io::ErrorKind::InvalidInput)?)?;
    let sd = descriptor()?;
    let wide_path = widestring::U16CString::from_os_str(path).map_err(io::Error::other)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.as_sd().cast_mut(),
        bInheritHandle: 0,
    };
    // SAFETY: the path, security descriptor and attributes remain alive throughout creation.
    if unsafe { CreateDirectoryW(wide_path.as_ptr(), &attributes) } == 0
        && io::Error::last_os_error().raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32)
    {
        return Err(io::Error::last_os_error());
    }
    if !std::fs::symlink_metadata(path)?.is_dir() {
        return Err(io::ErrorKind::PermissionDenied.into());
    }

    let mut owner = std::ptr::null_mut();
    let mut existing = std::ptr::null_mut();
    // SAFETY: Windows initializes owner inside the returned descriptor, which is freed below.
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide_path.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut existing,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let owner_sid = sid_text(owner);
    // SAFETY: GetNamedSecurityInfoW allocated existing with LocalAlloc.
    unsafe {
        LocalFree(existing);
    }
    if owner_sid? != user_sid()? {
        return Err(io::ErrorKind::PermissionDenied.into());
    }

    // SAFETY: the descriptor and NUL-terminated path stay alive for the call.
    if unsafe {
        SetFileSecurityW(
            wide_path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            sd.as_sd().cast_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn connect(info: &shell_control::SessionInfo) -> io::Result<Stream> {
    if info.session.is_empty()
        || info.session.len() > 64
        || !info
            .session
            .bytes()
            .all(|c| c.is_ascii_hexdigit() || c == b'-')
        || info.endpoint != format!("tundra-shell-{}", info.session)
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    // The local-socket adapter in interprocess 2.4.6 ignores its connection timeout on Windows.
    let pipe = interprocess::os::windows::named_pipe::DuplexPipeStream::<
        interprocess::os::windows::named_pipe::pipe_mode::Bytes,
    >::connect_by_path_with_wait_mode(
        format!(r"\\.\pipe\{}", info.endpoint),
        interprocess::ConnectWaitMode::Timeout(std::time::Duration::ZERO),
    )?;
    let stream = Stream::NamedPipe(pipe.into());
    stream.set_nonblocking(true)?;
    Ok(stream)
}

pub(super) fn cancel_flush(stream: &Stream) {
    let Stream::NamedPipe(stream) = stream;
    // Dropped or timed-out peers must not leave a background thread waiting for an unread pipe.
    stream.inner().assume_flushed();
}

pub(super) fn read_nonblocking(stream: &mut Stream, bytes: &mut [u8]) -> io::Result<usize> {
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;
    let Stream::NamedPipe(pipe) = &*stream;
    let mut available = 0;
    // interprocess maps an empty nonblocking pipe to EOF; check availability before reading.
    // SAFETY: the live pipe handle and output pointer remain valid for the query.
    if unsafe {
        PeekNamedPipe(
            pipe.as_handle().as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if available == 0 {
        return Err(io::ErrorKind::WouldBlock.into());
    }
    let count = bytes.len().min(available as usize);
    stream.read(&mut bytes[..count])
}
