use super::*;
#[test]
fn rejects_mismatched_credentials() {
    for (uid, effective_uid, gid, effective_gid) in [
        (1000, 0, 1000, 1000),
        (0, 1000, 1000, 1000),
        (1000, 1001, 1000, 1000),
        (1000, 1000, 1000, 1001),
    ] {
        assert!(
            ProcessIdentity {
                uid,
                effective_uid,
                gid,
                effective_gid
            }
            .validate()
            .is_err()
        );
    }
    // No UID_MIN/UID_MAX or group-name policy.
    assert!(
        ProcessIdentity {
            uid: 42,
            effective_uid: 42,
            gid: 7,
            effective_gid: 7
        }
        .validate()
        .is_ok()
    );
}

#[test]
fn accepts_matching_root_identity_for_confirmed_startup() {
    assert!(
        ProcessIdentity {
            uid: 0,
            effective_uid: 0,
            gid: 0,
            effective_gid: 0,
        }
        .validate()
        .is_ok()
    );
}
