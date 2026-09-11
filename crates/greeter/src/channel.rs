//! Bounded newline JSON transport for a private, inherited Unix socket.
use serde::{Serialize, de::DeserializeOwned};
pub use session_protocol::greeter::{ClientMessage, MAX_FRAME_BYTES, PamStyle, ServerMessage};
use std::io::{self, BufRead, Write};
use zeroize::Zeroizing;

pub fn read_frame<T: DeserializeOwned>(reader: &mut impl BufRead) -> io::Result<T> {
    let mut bytes = Zeroizing::new(Vec::new());
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "greeter channel closed",
            ));
        }
        let end = available.iter().position(|&b| b == b'\n');
        let count = end.map_or(available.len(), |n| n + 1);
        if bytes.len() + count > MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "greeter frame exceeds limit",
            ));
        }
        bytes.extend_from_slice(&available[..count]);
        reader.consume(count);
        if end.is_some() {
            break;
        }
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid greeter frame"))
}

pub fn write_frame<T: Serialize>(writer: &mut impl Write, message: &T) -> io::Result<()> {
    let mut bytes = Zeroizing::new(serde_json::to_vec(message).map_err(io::Error::other)?);
    bytes.push(b'\n');
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "greeter frame exceeds limit",
        ));
    }
    writer.write_all(&bytes)?;
    writer.flush()
}

#[cfg(target_os = "linux")]
pub fn inherited_root_channel(
    fd: std::os::fd::RawFd,
) -> io::Result<std::os::unix::net::UnixStream> {
    use std::os::fd::FromRawFd;
    // No terminal descriptors or path-based fallback can be used as an IPC channel.
    if fd < 3 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "invalid greeter descriptor",
        ));
    }
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of_val(&credentials) as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut size,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if credentials.uid != 0 || credentials.pid <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "greeter channel peer is not root",
        ));
    }
    let mut kind: libc::c_int = 0;
    let mut size = std::mem::size_of_val(&kind) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut libc::c_int).cast(),
            &mut size,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if kind != libc::SOCK_STREAM {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "greeter requires stream socket",
        ));
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // Ownership of the launcher-supplied descriptor is transferred exactly once.
    let channel = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
    channel.peer_addr()?; // verifies AF_UNIX, including unnamed socketpairs
    Ok(channel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};
    #[test]
    fn frame_boundary_preserves_next_message_and_embedded_newlines() {
        let mut bytes = Vec::new();
        write_frame(
            &mut bytes,
            &ServerMessage::Login {
                message: Some("hello\nworld".into()),
            },
        )
        .unwrap();
        write_frame(&mut bytes, &ServerMessage::Complete {}).unwrap();
        let mut reader = BufReader::with_capacity(3, Cursor::new(bytes));
        assert_eq!(
            read_frame::<ServerMessage>(&mut reader).unwrap(),
            ServerMessage::Login {
                message: Some("hello\nworld".into())
            }
        );
        assert_eq!(
            read_frame::<ServerMessage>(&mut reader).unwrap(),
            ServerMessage::Complete {}
        );
        assert_eq!(
            read_frame::<ServerMessage>(&mut reader).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
    #[test]
    fn malformed_oversized_and_truncated_frames_fail_closed() {
        for bytes in [
            vec![b'a'; MAX_FRAME_BYTES + 1],
            b"{\"type\":\"Complete\",\"uid\":0}\n".to_vec(),
            b"{\"type\":\"Complete\"}".to_vec(),
        ] {
            assert!(read_frame::<ServerMessage>(&mut Cursor::new(bytes)).is_err());
        }
    }
}
