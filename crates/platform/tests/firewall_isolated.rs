#![cfg(target_os = "linux")]
//! Real tools are only run in the private environment made by firewall-isolated.sh.
use platform::management::*;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

#[derive(Default)]
struct Interaction {
    backup: Option<String>,
}
impl OperationInteraction for Interaction {
    fn emit(&mut self, event: OperationEvent) {
        if let OperationEvent::Output { text } = event {
            if let Some(id) = text.strip_prefix("Configuration backup: ") {
                self.backup = Some(id.into());
            }
        }
    }
    fn ask(&mut self, _: &str, _: &str, _: &[String], _: bool) -> Result<String, ManagementError> {
        panic!("Unexpected interactive question");
    }
}
fn guard() {
    assert_eq!(unsafe { libc::geteuid() }, 0);
    assert_eq!(
        fs::read_to_string("/run/tundra-firewall-isolated").unwrap(),
        "private\n"
    );
    for (name, key) in [
        ("net", "TUNDRA_HOST_NET_NS"),
        ("mnt", "TUNDRA_HOST_MNT_NS"),
        ("pid", "TUNDRA_HOST_PID_NS"),
    ] {
        assert_ne!(
            fs::read_link(format!("/proc/self/ns/{name}"))
                .unwrap()
                .to_string_lossy(),
            std::env::var(key).unwrap()
        );
    }
    assert_eq!(
        fs::read_to_string("/etc/tundra-firewall-isolated").unwrap(),
        "private\n"
    );
}
fn tool(name: &str, arguments: &[&str]) -> String {
    guard();
    let path = if name == "ufw" {
        "/usr/sbin/ufw"
    } else {
        "/usr/bin/firewall-cmd"
    };
    let output = Command::new(path)
        .args(arguments)
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{name} {arguments:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn context() -> ExecutionContext {
    ExecutionContext {
        actor_uid: 0,
        caller_pid: std::process::id(),
        caller_start_ticks: 0,
        helper_path: PathBuf::from("/usr/bin/false"),
    }
}
fn snapshot() -> ManagementSnapshot {
    platform::management::query(
        &ManagementQuery::new(ManagementKind::Firewall),
        &AtomicBool::new(false),
    )
    .unwrap()
}
fn apply(
    action: &ManagementAction,
    row: Option<&ManagementRow>,
    values: &[(&str, &str)],
    io: &mut Interaction,
) -> Result<String, ManagementError> {
    let mut fields = action.values.clone();
    for field in &action.fields {
        if !field.value.is_empty() {
            fields.insert(field.id.clone(), field.value.clone());
        }
    }
    fields.extend(values.iter().map(|(k, v)| ((*k).into(), (*v).into())));
    platform::management::execute(
        &ManagementCommand {
            kind: ManagementKind::Firewall,
            action: action.id.clone(),
            target: row.map(|row| row.id.clone()),
            values: fields,
            identity: row.map(|row| row.identity.clone()).unwrap_or_default(),
        },
        &context(),
        io,
        &AtomicBool::new(false),
    )
}
fn add(values: &[(&str, &str)], io: &mut Interaction) -> String {
    let state = snapshot();
    let action = state
        .actions
        .iter()
        .find(|action| action.id == "add_rule")
        .unwrap();
    apply(action, None, values, io).unwrap();
    io.backup.clone().unwrap()
}
fn remove(port: &str, scope: &str, io: &mut Interaction) -> String {
    let state = snapshot();
    let row = state
        .rows
        .iter()
        .find(|row| {
            row.identity
                .get("rule_value")
                .is_some_and(|value| value.contains(port))
                && (scope.is_empty()
                    || row.identity.get("rule_scope").map(String::as_str) == Some(scope))
        })
        .unwrap();
    apply(&row.actions[0], Some(row), &[], io).unwrap();
    io.backup.clone().unwrap()
}
fn restore(id: &str, io: &mut Interaction) -> String {
    io.backup = None;
    let before = fs::read_dir("/var/lib/tundraux3/config-backups/operations")
        .unwrap()
        .count();
    platform::management::execute(
        &ManagementCommand {
            kind: ManagementKind::Backups,
            action: "restore".into(),
            target: Some(id.into()),
            values: BTreeMap::from([("allow_unvalidated".into(), "true".into())]),
            identity: BTreeMap::new(),
        },
        &context(),
        io,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        fs::read_dir("/var/lib/tundraux3/config-backups/operations")
            .unwrap()
            .count(),
        before + 1,
        "One complete recovery must create exactly one reverse record"
    );
    io.backup.clone().unwrap()
}
fn has(port: &str, scope: &str) -> bool {
    snapshot().rows.iter().any(|row| {
        row.identity
            .get("rule_value")
            .is_some_and(|value| value.contains(port))
            && (scope.is_empty()
                || row.identity.get("rule_scope").map(String::as_str) == Some(scope))
    })
}

#[test]
#[ignore = "Requires the private namespaces and unpacked official tools in firewall-isolated.sh"]
fn private_ufw_active_inactive_delete_and_complete_recovery() {
    guard();
    tool("ufw", &["--force", "enable"]);
    assert_eq!(snapshot().backend, "ufw");
    let old = snapshot()
        .actions
        .into_iter()
        .find(|action| action.id == "add_rule")
        .unwrap();
    let mut io = Interaction::default();
    add(&[("port", "38111-38113"), ("source", "127.0.0.1")], &mut io);
    assert!(has("38111:38113", ""));
    assert!(matches!(
        apply(&old, None, &[("port", "38114")], &mut io),
        Err(ManagementError::Conflict(_))
    ));
    let deletion = remove("38111:38113", "", &mut io);
    assert!(!has("38111:38113", ""));
    let reverse = restore(&deletion, &mut io);
    assert!(has("38111:38113", ""));
    restore(&reverse, &mut io);
    assert!(!has("38111:38113", ""));
    tool("ufw", &["--force", "disable"]);
    add(
        &[("port", "38121"), ("protocol", "udp"), ("source", "::1")],
        &mut io,
    );
    assert!(has("38121", ""));
    assert!(tool("ufw", &["status"]).contains("inactive"));
    let deletion = remove("38121", "", &mut io);
    assert!(!has("38121", ""));
    let reverse = restore(&deletion, &mut io);
    assert!(has("38121", ""));
    assert!(tool("ufw", &["status"]).contains("inactive"));
    restore(&reverse, &mut io);
    assert!(!has("38121", ""));
    assert!(Path::new("/var/lib/tundraux3/config-backups/operations").exists());
}

#[test]
#[ignore = "Requires the private namespaces and unpacked official tools in firewall-isolated.sh"]
fn private_firewalld_runtime_permanent_delete_and_complete_recovery() {
    guard();
    assert_eq!(snapshot().backend, "firewalld");
    let mut io = Interaction::default();
    let runtime = add(
        &[
            ("port", "38131"),
            ("source", "::1"),
            ("duration", "runtime"),
        ],
        &mut io,
    );
    assert!(has("38131", "runtime") && !has("38131", "permanent"));
    let zone_path = Path::new("/etc/firewalld/zones/public.xml");
    assert!(!zone_path.exists());
    let permanent = add(
        &[
            ("port", "38132"),
            ("protocol", "udp"),
            ("source", "192.0.2.0/24"),
            ("duration", "permanent"),
        ],
        &mut io,
    );
    assert!(!has("38132", "runtime") && has("38132", "permanent"));
    let inverse = restore(&permanent, &mut io);
    assert!(!has("38132", "permanent"));
    assert!(
        !zone_path.exists(),
        "Recovery must preserve a zone file's original absence"
    );
    restore(&inverse, &mut io);
    assert!(has("38132", "permanent"));
    let inverse = restore(&runtime, &mut io);
    assert!(!has("38131", "runtime") && has("38132", "permanent"));
    restore(&inverse, &mut io);
    assert!(has("38131", "runtime") && has("38132", "permanent"));
    fs::set_permissions(zone_path, fs::Permissions::from_mode(0o640)).unwrap();
    let original_xml = fs::read(zone_path).unwrap();
    let original_old = fs::read("/etc/firewalld/zones/public.xml.old").ok();
    let both = add(&[("port", "38133"), ("duration", "both")], &mut io);
    assert!(has("38133", "runtime") && has("38133", "permanent"));
    let deletion = remove("38133", "runtime", &mut io);
    assert!(!has("38133", "runtime") && has("38133", "permanent"));
    restore(&deletion, &mut io);
    assert!(has("38133", "runtime") && has("38133", "permanent"));
    let inverse = restore(&both, &mut io);
    assert!(!has("38133", "runtime") && !has("38133", "permanent"));
    assert_eq!(fs::read(zone_path).unwrap(), original_xml);
    assert_eq!(
        fs::metadata(zone_path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(
        fs::read("/etc/firewalld/zones/public.xml.old").ok(),
        original_old
    );
    assert!(has("38131", "runtime") && has("38132", "permanent"));
    restore(&inverse, &mut io);
    assert!(has("38133", "runtime") && has("38133", "permanent"));
    tool("firewall-cmd", &["--reload"]);
    assert!(!has("38131", "runtime"));
    assert!(has("38132", "runtime") && has("38133", "runtime"));
}
