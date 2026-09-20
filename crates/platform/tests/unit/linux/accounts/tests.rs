use super::*;
fn account(uid: u64, admin: bool) -> Account {
    Account {
        uid,
        username: format!("user{uid}"),
        display_name: String::new(),
        admin,
        locked: false,
        system: false,
        local: true,
        path: OwnedObjectPath::try_from(format!("/user/{uid}")).unwrap(),
    }
}
#[test]
fn normal_users_can_only_edit_themselves() {
    let actor = account(1000, false);
    assert!(authorize(&actor, &actor, false, false).is_ok());
    assert!(authorize(&actor, &account(1001, false), false, false).is_err());
    assert!(authorize(&actor, &actor, true, false).is_err());
    assert!(authorize(&actor, &actor, true, true).is_err());
}
#[test]
fn admins_manage_others_but_cannot_remove_their_own_access() {
    let actor = account(1000, true);
    assert!(authorize(&actor, &account(1001, false), true, true).is_ok());
    assert!(authorize(&actor, &actor, true, true).is_err());
    assert!(authorize(&actor, &account(0, true), true, true).is_err());
    let mut system = account(10, false);
    system.system = true;
    assert!(authorize(&actor, &system, true, false).is_err());
    let mut remote = account(1002, false);
    remote.local = false;
    assert!(authorize(&actor, &remote, true, false).is_err());
}
