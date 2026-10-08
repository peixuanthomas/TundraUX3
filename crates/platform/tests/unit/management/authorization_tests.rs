use super::*;

#[test]
fn failed_authorization_returns_promptly_without_waiting_for_the_ready_timeout() {
    use std::sync::mpsc;
    use std::time::Duration;

    // Simulate both sudo's no-password probe and a rejected password. The
    // launcher consumes stdin and exits without sending the broker's ready frame.
    for password in [None, Some("fixture-password")] {
        let authority = PrivilegeSession::default();
        let worker_authority = authority.clone();
        let (send, receive) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "cat >/dev/null; exit 1"]);
            send.send(matches!(
                worker_authority.start_command(command, password),
                Err(ManagementError::PermissionDenied(_))
            ))
            .unwrap();
        });
        let result = receive.recv_timeout(Duration::from_secs(2));
        // Unblock a broken implementation before asserting, so this regression
        // test never leaves a thread waiting for the production 60-second limit.
        authority.revoke();
        worker.join().unwrap();
        assert_eq!(
            result,
            Ok(true),
            "failed authorization must promptly request a password or report rejection"
        );
    }
}

#[test]
fn approved_session_reuses_authority_and_revoke_blocks_cloned_owners() {
    let authority = PrivilegeSession::default();
    let (channel, mut broker) = UnixStream::pair().unwrap();
    let ready = b"\"Done\"";
    broker
        .write_all(&(ready.len() as u32).to_be_bytes())
        .unwrap();
    broker.write_all(ready).unwrap();
    let client = Client::new(channel).unwrap();
    *authority.0.revoker.lock().unwrap() = Some(client.revoke_handle().unwrap());
    *authority.0.session.lock().unwrap() = Some(Authorized {
        client,
        child: Command::new("/bin/true").spawn().unwrap(),
    });
    let clone = authority.clone();
    for _ in 0..2 {
        clone
            .ensure(|| panic!("must not prompt after authorization"))
            .unwrap();
    }
    // A service-level D-Bus outage is not a failure of the authorization socket.
    let response = br#"{"ServiceFailed":"BackendDisconnected"}"#;
    broker
        .write_all(&(response.len() as u32).to_be_bytes())
        .unwrap();
    broker.write_all(response).unwrap();
    assert_eq!(
        clone.execute(Request::Power { reboot: true }),
        Err(ServiceError::BackendDisconnected)
    );
    assert!(!clone.0.revoked.load(Ordering::Acquire));
    clone
        .ensure(|| panic!("service failure must preserve the session"))
        .unwrap();
    // Revocation must not need the request lock held by an in-flight operation.
    let _busy = authority.0.session.lock().unwrap();
    authority.revoke();
    drop(_busy);
    assert!(matches!(
        clone.ensure(|| panic!("closed Shell must not reauthorize")),
        Err(ManagementError::Cancelled)
    ));
    assert_eq!(
        clone.execute(Request::Power { reboot: true }),
        Err(ServiceError::AuthorizationCancelled)
    );
    use std::io::Read;
    // Drain the already-sent request before observing EOF.
    let mut remaining = Vec::new();
    broker.read_to_end(&mut remaining).unwrap();
    assert!(!remaining.is_empty());
}
