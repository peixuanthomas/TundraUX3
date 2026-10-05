use super::*;

#[test]
fn transferred_connection_is_private_and_close_on_exec() {
    let (sender, receiver) = UnixStream::pair().unwrap();
    let (connection, mut peer) = UnixStream::pair().unwrap();
    send_connection(&sender, &connection).unwrap();
    let mut received = receive_connection(&receiver).unwrap();
    assert_ne!(
        unsafe { libc::fcntl(received.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
        0
    );
    received.write_all(b"input").unwrap();
    let mut bytes = [0; 5];
    peer.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"input");
}

#[test]
fn malformed_or_oversized_frames_fail_without_echoing_secrets() {
    for bytes in [b"password-secret".as_slice(), b"{}".as_slice()] {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        sender
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .unwrap();
        sender.write_all(bytes).unwrap();
        let error = read_packet::<Request>(&mut receiver).err().unwrap();
        assert!(!error.to_string().contains("password-secret"));
    }
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    sender
        .write_all(&((MAX_REQUEST + 1) as u32).to_be_bytes())
        .unwrap();
    assert!(read_packet::<Request>(&mut receiver).is_err());
}

#[test]
fn shutdown_revokes_all_clones_of_the_session_channel() {
    let (channel, mut broker) = UnixStream::pair().unwrap();
    let mut retained = channel.try_clone().unwrap();
    channel.shutdown(std::net::Shutdown::Both).unwrap();
    assert!(retained.write_all(b"late request").is_err());
    assert_eq!(broker.read(&mut [0; 1]).unwrap(), 0);
}
