use super::*;

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
