//! Runs only against a private bus and in-memory accounts, never the host's users.
use super::*;
use std::{
    collections::HashMap,
    io::BufRead,
    process::{Child, Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use zbus::{blocking::connection::Builder, zvariant::OwnedValue};

type SharedUser = Arc<Mutex<Account>>;
#[derive(Clone)]
struct Manager {
    users: Vec<SharedUser>,
    calls: Arc<Mutex<Vec<String>>>,
    created: Arc<AtomicBool>,
}
#[zbus::interface(name = "org.freedesktop.Accounts")]
impl Manager {
    fn find_user_by_id(&self, id: i64) -> zbus::fdo::Result<OwnedObjectPath> {
        self.users
            .iter()
            .find_map(|user| {
                let user = user.lock().unwrap();
                (user.uid == id as u64).then(|| user.path.clone())
            })
            .ok_or_else(|| zbus::fdo::Error::Failed("missing user".into()))
    }
    fn list_cached_users(&self) -> Vec<OwnedObjectPath> {
        self.calls.lock().unwrap().push("list".into());
        self.users
            .iter()
            .filter_map(|user| {
                let user = user.lock().unwrap();
                (user.uid != 1002 || self.created.load(Ordering::Relaxed))
                    .then(|| user.path.clone())
            })
            .collect()
    }
    fn create_user(&self, name: &str, real_name: &str, account_type: i32) -> OwnedObjectPath {
        self.calls
            .lock()
            .unwrap()
            .push(format!("create:{name}:{real_name}:{account_type}"));
        self.created.store(true, Ordering::Relaxed);
        self.users[2].lock().unwrap().path.clone()
    }
    fn delete_user(&self, uid: i64, remove_files: bool) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("delete:{uid}:{remove_files}"));
    }
}
struct User {
    user: SharedUser,
    calls: Arc<Mutex<Vec<String>>>,
    fail_password: Arc<AtomicBool>,
}
#[zbus::interface(name = "org.freedesktop.Accounts.User")]
impl User {
    #[zbus(property)]
    fn uid(&self) -> u64 {
        self.user.lock().unwrap().uid
    }
    #[zbus(property)]
    fn user_name(&self) -> String {
        self.user.lock().unwrap().username.clone()
    }
    #[zbus(property)]
    fn real_name(&self) -> String {
        self.user.lock().unwrap().display_name.clone()
    }
    #[zbus(property)]
    fn account_type(&self) -> i32 {
        i32::from(self.user.lock().unwrap().admin)
    }
    #[zbus(property)]
    fn locked(&self) -> bool {
        self.user.lock().unwrap().locked
    }
    #[zbus(property)]
    fn system_account(&self) -> bool {
        self.user.lock().unwrap().system
    }
    #[zbus(property)]
    fn local_account(&self) -> bool {
        self.user.lock().unwrap().local
    }
    fn set_real_name(&self, name: &str) {
        self.user.lock().unwrap().display_name = name.into();
        self.calls.lock().unwrap().push(format!("rename:{name}"));
    }
    fn set_locked(&self, locked: bool) {
        self.user.lock().unwrap().locked = locked;
        self.calls.lock().unwrap().push(format!("lock:{locked}"));
    }
    fn set_account_type(&self, account_type: i32) {
        self.user.lock().unwrap().admin = account_type == 1;
        self.calls
            .lock()
            .unwrap()
            .push(format!("role:{account_type}"));
    }
    fn set_password(&self, hash: &str, hint: &str) -> zbus::fdo::Result<()> {
        assert!(hash.starts_with("$6$"));
        assert!(hint.is_empty());
        self.calls.lock().unwrap().push("password".into());
        if self.fail_password.load(Ordering::Relaxed) {
            Err(zbus::fdo::Error::AccessDenied("test denial".into()))
        } else {
            Ok(())
        }
    }
}
struct Policy(Arc<AtomicBool>);
#[zbus::interface(name = "org.freedesktop.PolicyKit1.Authority")]
impl Policy {
    fn check_authorization(
        &self,
        _subject: (String, HashMap<String, OwnedValue>),
        _action: String,
        _details: HashMap<String, String>,
        _flags: u32,
        _cancel: String,
    ) -> (bool, bool, HashMap<String, String>) {
        (self.0.load(Ordering::Relaxed), false, HashMap::new())
    }
}
struct Bus(Child);
struct CancelPassword(Arc<AtomicBool>);
impl authorization::Interaction for CancelPassword {
    fn begin(&self) -> Result<(), ServiceError> {
        Ok(())
    }
    fn fallback(&self) -> Result<(), ServiceError> {
        Ok(())
    }
    fn finish(&self) {}
    fn change_own_password(&self) -> Result<(), ServiceError> {
        self.0.store(true, Ordering::Relaxed);
        Err(ServiceError::AuthorizationCancelled)
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires dbus-daemon; uses an isolated session bus, no real account changes"]
fn account_service_reads_and_writes_respect_identity_and_system_authorization() {
    let mut bus = Bus(Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("dbus-daemon"));
    let mut address = String::new();
    std::io::BufReader::new(bus.0.stdout.take().unwrap())
        .read_line(&mut address)
        .unwrap();
    let address = address.trim();
    let users: Vec<_> = [1000, 1001, 1002, 0, 999, 1003]
        .into_iter()
        .map(|uid| {
            Arc::new(Mutex::new(Account {
                uid,
                username: format!("user{uid}"),
                display_name: format!("User {uid}"),
                admin: uid == 1000,
                locked: false,
                system: uid == 999,
                local: uid != 1003,
                path: OwnedObjectPath::try_from(format!("/org/freedesktop/Accounts/User{uid}"))
                    .unwrap(),
            }))
        })
        .collect();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let created = Arc::new(AtomicBool::new(false));
    let fail_password = Arc::new(AtomicBool::new(false));
    let allowed = Arc::new(AtomicBool::new(true));
    let manager = Manager {
        users: users.clone(),
        calls: calls.clone(),
        created: created.clone(),
    };
    let mut builder = Builder::address(address)
        .unwrap()
        .name(SERVICE)
        .unwrap()
        .name("org.freedesktop.PolicyKit1")
        .unwrap()
        .serve_at(PATH, manager)
        .unwrap()
        .serve_at(
            "/org/freedesktop/PolicyKit1/Authority",
            Policy(allowed.clone()),
        )
        .unwrap();
    for user in &users {
        let path = user.lock().unwrap().path.clone();
        builder = builder
            .serve_at(
                path,
                User {
                    user: user.clone(),
                    calls: calls.clone(),
                    fail_password: fail_password.clone(),
                },
            )
            .unwrap();
    }
    let _server = builder.build().unwrap();
    let mut accounts = Accounts {
        connection: Builder::address(address)
            .unwrap()
            .method_timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap(),
        current_uid: 1000,
        interaction: None,
    };

    assert_eq!(
        accounts
            .visible()
            .unwrap()
            .iter()
            .map(|user| user.uid)
            .collect::<Vec<_>>(),
        [1000, 1001]
    );
    users[0].lock().unwrap().admin = false;
    calls.lock().unwrap().clear();
    assert_eq!(accounts.visible().unwrap().len(), 1);
    assert!(
        calls.lock().unwrap().is_empty(),
        "ordinary users must not enumerate"
    );
    assert!(accounts.rename("user1001", "Denied").is_err());
    assert!(
        accounts
            .create("created", "Created", false, "Password123!")
            .is_err()
    );
    assert!(accounts.set_admin("user1000", true).is_err());
    accounts.rename("user1000", "My name").unwrap();
    assert_eq!(users[0].lock().unwrap().display_name, "My name");
    // A password lock does not invalidate an existing session (e.g. SSH keys).
    users[0].lock().unwrap().locked = true;
    accounts.rename("user1000", "Still my account").unwrap();
    let password_prompt = Arc::new(AtomicBool::new(false));
    accounts.set_interaction(Arc::new(CancelPassword(password_prompt.clone())));
    assert_eq!(
        accounts.password("user1000", ""),
        Err(ServiceError::AuthorizationCancelled)
    );
    assert!(password_prompt.load(Ordering::Relaxed));
    assert!(!calls.lock().unwrap().contains(&"password".into()));

    users[0].lock().unwrap().admin = true;
    accounts.rename("user1001", "Other name").unwrap();
    accounts.set_locked("user1001", true).unwrap();
    accounts.set_locked("user1001", false).unwrap();
    accounts.set_admin("user1001", true).unwrap();
    accounts.password("user1001", "Password123!").unwrap();
    accounts.delete("user1001").unwrap();
    assert!(calls.lock().unwrap().contains(&"delete:1001:false".into()));
    assert!(accounts.delete("user1000").is_err());
    assert!(accounts.set_locked("user1000", true).is_err());
    assert!(accounts.set_admin("user1000", false).is_err());

    allowed.store(false, Ordering::Relaxed);
    let count = calls
        .lock()
        .unwrap()
        .iter()
        .filter(|call| call.starts_with("rename:"))
        .count();
    assert_eq!(
        accounts.rename("user1001", "Denied"),
        Err(ServiceError::PermissionDenied)
    );
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.starts_with("rename:"))
            .count(),
        count
    );
    allowed.store(true, Ordering::Relaxed);
    fail_password.store(true, Ordering::Relaxed);
    assert_eq!(
        accounts.create("user1002", "User 1002", false, "Password123!"),
        Err(ServiceError::AccountPasswordSetupFailed)
    );
    assert!(created.load(Ordering::Relaxed));
    assert!(!calls.lock().unwrap().contains(&"delete:1002:false".into()));
    assert_eq!(accounts.visible().unwrap().len(), 3);
}
