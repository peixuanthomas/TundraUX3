//! Real system tools run only inside the private systemd fixture.
use super::*;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use zbus::blocking::connection::Builder;

struct Timesync;
#[zbus::interface(name = "org.freedesktop.timesync1.Manager")]
impl Timesync {
    #[zbus(property, name = "LinkNTPServers")]
    fn link_ntp_servers(&self) -> Vec<String> {
        vec!["dhcp.example".into()]
    }
    #[zbus(property)]
    fn server_name(&self) -> String {
        "dhcp.example".into()
    }
    #[zbus(property, name = "FallbackNTPServers")]
    fn fallback_ntp_servers(&self) -> Vec<String> {
        vec!["fallback.example".into()]
    }
}
#[derive(Default)]
struct Review {
    decline: bool,
    events: Vec<OperationEvent>,
}
impl OperationInteraction for Review {
    fn emit(&mut self, event: OperationEvent) {
        self.events.push(event);
    }
    fn ask(
        &mut self,
        _: &str,
        _: &str,
        choices: &[String],
        _: bool,
    ) -> Result<String, ManagementError> {
        Ok(choices[usize::from(!self.decline)].clone())
    }
}
fn command(action: &str, provider: Option<&str>) -> ManagementCommand {
    let mut request = ManagementQuery::new(ManagementKind::SystemSettings);
    if let Some(provider) = provider {
        request.options.insert("provider".into(), provider.into());
    }
    let snapshot = query(&request, &AtomicBool::new(false)).unwrap();
    let (row, action) = snapshot
        .rows
        .iter()
        .find_map(|row| {
            row.actions
                .iter()
                .find(|candidate| candidate.id == action)
                .map(|action| (row, action))
        })
        .unwrap_or_else(|| panic!("missing action {action}: {snapshot:?}"));
    let mut values = action.values.clone();
    values.extend(
        action
            .fields
            .iter()
            .map(|field| (field.id.clone(), field.value.clone())),
    );
    ManagementCommand {
        kind: ManagementKind::SystemSettings,
        action: action.id.clone(),
        target: Some(row.id.clone()),
        identity: row.identity.clone(),
        values,
    }
}
fn run(command: &ManagementCommand, review: &mut Review) -> Result<String, ManagementError> {
    execute(
        command,
        &ExecutionContext {
            actor_uid: 0,
            helper_path: PathBuf::new(),
            caller_pid: 0,
            caller_start_ticks: 0,
        },
        review,
        &AtomicBool::new(false),
    )
}
fn systemctl(args: &[&str]) {
    native("/usr/bin/systemctl", args, &AtomicBool::new(false)).unwrap();
}
fn active(unit: &str) -> bool {
    native(
        "/usr/bin/systemctl",
        &["show", "--property=ActiveState", "--value", unit],
        &AtomicBool::new(false),
    )
    .unwrap()
    .trim()
        == "active"
}

fn verify_file_ownership(review: &mut Review) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("owned-file");
    fs::write(&path, "private test").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o6750)).unwrap();
    let mut request = ManagementQuery::new(ManagementKind::FilePermissions);
    request.options.insert(
        "paths".into(),
        super::super::file_permissions::encode_paths(std::slice::from_ref(&path)),
    );
    let action = super::super::file_permissions::query(&request, &AtomicBool::new(false))
        .unwrap()
        .actions
        .remove(0);
    let owner = super::super::users::accounts(None)
        .unwrap()
        .into_iter()
        .find(|account| account.uid == 65534)
        .unwrap();
    let mut values = action.values;
    values.extend(
        action
            .fields
            .into_iter()
            .map(|field| (field.id, field.value)),
    );
    values.insert(
        "owner".into(),
        format!("{} ({})", owner.username, owner.uid),
    );
    super::super::file_permissions::execute(
        &ManagementCommand {
            kind: ManagementKind::FilePermissions,
            action: action.id,
            target: None,
            identity: BTreeMap::new(),
            values,
        },
        &ExecutionContext {
            actor_uid: 0,
            helper_path: PathBuf::new(),
            caller_pid: 0,
            caller_start_ticks: 0,
        },
        review,
        &AtomicBool::new(false),
    )
    .unwrap();
    let after = fs::metadata(path).unwrap();
    assert_eq!(after.uid(), owner.uid);
    assert_eq!(after.gid(), 0);
    assert_eq!(after.mode() & 0o7777, 0o750);
    assert!(review.events.iter().any(|event| matches!(event, OperationEvent::Output { text } if text.contains(&i18n::tr!("permissions-special-were-cleared")))));
}

#[test]
#[ignore = "requires isolated-systemd.sh settings mode and an unpacked official chrony package"]
fn private_system_settings_and_time_configuration() {
    assert_eq!(unsafe { libc::geteuid() }, 0);
    assert!(Path::new("/etc/tundra-system-settings-isolated").is_file());
    assert!(Path::new("/run/tundra-system-settings-isolated").is_file());
    for (kind, variable) in [
        ("mnt", "TUNDRA_HOST_MNT_NS"),
        ("net", "TUNDRA_HOST_NET_NS"),
        ("pid", "TUNDRA_HOST_PID_NS"),
        ("uts", "TUNDRA_HOST_UTS_NS"),
    ] {
        assert_ne!(
            fs::read_link(format!("/proc/self/ns/{kind}")).unwrap(),
            PathBuf::from(std::env::var_os(variable).unwrap())
        );
    }
    let _timesync = Builder::system()
        .unwrap()
        .name("org.freedesktop.timesync1")
        .unwrap()
        .serve_at("/org/freedesktop/timesync1", Timesync)
        .unwrap()
        .build()
        .unwrap();
    let connection = crate::linux::dbus::system().unwrap();
    systemctl(&["enable", "--now", "systemd-timesyncd.service"]);
    let mut review = Review::default();
    verify_file_ownership(&mut review);
    let mut hostname = command("set_system_hostname", None);
    hostname
        .values
        .insert("hostname".into(), "tundra-isolated".into());
    run(&hostname, &mut review).unwrap();
    assert_eq!(
        state(&connection).unwrap().static_hostname,
        "tundra-isolated"
    );
    let mut timezone = command("set_system_timezone", None);
    timezone
        .values
        .insert("timezone".into(), "Asia/Shanghai".into());
    run(&timezone, &mut review).unwrap();
    assert_eq!(state(&connection).unwrap().timezone, "Asia/Shanghai");
    let mut declined = command("set_system_hostname", None);
    declined
        .values
        .insert("hostname".into(), "should-not-save".into());
    review.decline = true;
    assert!(matches!(
        run(&declined, &mut review),
        Err(ManagementError::Cancelled)
    ));
    assert_eq!(
        state(&connection).unwrap().static_hostname,
        "tundra-isolated"
    );
    review.decline = false;
    let mut stale = command("set_system_hostname", None);
    stale.values.insert("hostname".into(), "stale".into());
    stale.identity.insert("hostname".into(), "old".into());
    assert!(matches!(
        run(&stale, &mut review),
        Err(ManagementError::Conflict(_))
    ));
    let mut off = command("set_system_ntp", Some("systemd-timesyncd.service"));
    off.values.insert("enabled".into(), "false".into());
    run(&off, &mut review).unwrap();
    assert!(!active("systemd-timesyncd.service"));
    let mut servers = command("set_system_ntp_servers", Some("systemd-timesyncd.service"));
    servers
        .values
        .insert("source_0".into(), "new.example".into());
    run(&servers, &mut review).unwrap();
    assert!(!active("systemd-timesyncd.service"));
    let own = Path::new("/etc/systemd/timesyncd.conf.d/90-tundraux3.conf");
    assert!(fs::read_to_string(own).unwrap().contains("NTP=new.example"));
    let mut on = command("set_system_ntp", Some("systemd-timesyncd.service"));
    on.values.insert("enabled".into(), "true".into());
    run(&on, &mut review).unwrap();
    assert!(active("systemd-timesyncd.service"));
    let mut configuration = command("set_system_ntp_servers", Some("systemd-timesyncd.service"));
    assert!(
        !configuration
            .values
            .values()
            .any(|value| value == "dhcp.example")
    );
    configuration
        .values
        .insert("source_0".into(), "another.example".into());
    run(&configuration, &mut review).unwrap();
    let before = fs::read_to_string(own).unwrap();
    fs::write(
        "/etc/systemd/timesyncd.conf.d/99-foreign.conf",
        "[Time]\nNTP=foreign.example\n",
    )
    .unwrap();
    let mut conflict = command("set_system_ntp_servers", Some("systemd-timesyncd.service"));
    conflict
        .values
        .insert("source_0".into(), "refused.example".into());
    assert!(matches!(
        run(&conflict, &mut review),
        Err(ManagementError::Conflict(_))
    ));
    assert_eq!(fs::read_to_string(own).unwrap(), before);
    fs::remove_file("/etc/systemd/timesyncd.conf.d/99-foreign.conf").unwrap();
    systemctl(&["disable", "--now", "systemd-timesyncd.service"]);
    let main = "pool old.example iburst\nconfdir /etc/chrony/conf.d\nsourcedir /run/chrony-dhcp\n";
    fs::write("/etc/chrony/chrony.conf", main).unwrap();
    fs::write(
        "/etc/chrony/conf.d/servers.conf",
        "server included.example iburst\n",
    )
    .unwrap();
    let dynamic = "server dhcp.example iburst\n";
    fs::write("/run/chrony-dhcp/network.sources", dynamic).unwrap();
    systemctl(&["enable", "--now", "chrony.service"]);
    let included = "/etc/chrony/conf.d/servers.conf";
    let invalid_sources = "server included.example iburst\nnot-a-chrony-directive\n";
    fs::write(included, invalid_sources).unwrap();
    let mut invalid_config = command("set_system_ntp_servers", Some("chrony.service"));
    invalid_config
        .values
        .insert("source_0".into(), "must-not-save.example".into());
    assert!(run(&invalid_config, &mut review).is_err());
    assert_eq!(fs::read_to_string("/etc/chrony/chrony.conf").unwrap(), main);
    assert_eq!(fs::read_to_string(included).unwrap(), invalid_sources);
    fs::write(included, "server included.example iburst\n").unwrap();
    let mut stale_config = command("set_system_ntp_servers", Some("chrony.service"));
    stale_config
        .values
        .insert("source_0".into(), "stale.example".into());
    fs::write(included, "server external.example iburst\n").unwrap();
    assert!(matches!(
        run(&stale_config, &mut review),
        Err(ManagementError::Conflict(_))
    ));
    assert_eq!(fs::read_to_string("/etc/chrony/chrony.conf").unwrap(), main);
    assert_eq!(
        fs::read_to_string(included).unwrap(),
        "server external.example iburst\n"
    );
    fs::write(included, "server included.example iburst\n").unwrap();
    let mut chrony = command("set_system_ntp_servers", Some("chrony.service"));
    chrony
        .values
        .insert("source_0".into(), "updated.example".into());
    chrony.values.insert("source_1".into(), String::new());
    chrony
        .values
        .insert("add_servers".into(), "extra.example".into());
    run(&chrony, &mut review).unwrap();
    assert!(
        fs::read_to_string("/etc/chrony/chrony.conf")
            .unwrap()
            .starts_with("pool updated.example iburst\n")
    );
    assert!(
        fs::read_to_string("/etc/chrony/conf.d/servers.conf")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fs::read_to_string("/run/chrony-dhcp/network.sources").unwrap(),
        dynamic
    );
    let record = fs::read_dir("/var/lib/tundraux3/config-backups/operations")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter_map(|path| {
            serde_json::from_str::<serde_json::Value>(&fs::read_to_string(path).unwrap()).ok()
        })
        .find(|record| {
            record["before"]
                .as_array()
                .is_some_and(|files| files.len() == 2)
        })
        .unwrap();
    super::super::config_backups::restore(record["id"].as_str().unwrap()).unwrap();
    assert_eq!(fs::read_to_string("/etc/chrony/chrony.conf").unwrap(), main);
    assert_eq!(
        fs::read_to_string("/etc/chrony/conf.d/servers.conf").unwrap(),
        "server included.example iburst\n"
    );
    let unit = "/etc/systemd/system/chrony.service";
    let unit_text = fs::read_to_string(unit).unwrap();
    fs::write(
        unit,
        unit_text.replace(
            "/usr/sbin/chronyd -f /etc/chrony/chrony.conf",
            "/usr/bin/false\nType=oneshot",
        ),
    )
    .unwrap();
    systemctl(&["daemon-reload"]);
    let mut failed = command("set_system_ntp_servers", Some("chrony.service"));
    failed
        .values
        .insert("source_0".into(), "saved-before-failure.example".into());
    assert!(run(&failed, &mut review).is_err());
    assert!(
        fs::read_to_string("/etc/chrony/chrony.conf")
            .unwrap()
            .contains("saved-before-failure.example")
    );
    assert!(!active("chrony.service"));
}
