use super::*;
use crate::management::{
    ManagementKind,
    selection::{KEY, Target},
};
use std::collections::BTreeMap;

#[test]
fn selected_packages_share_one_native_transaction_and_invalid_targets_abort_it() {
    for backend in [
        PackageBackend::Apt,
        PackageBackend::Dnf4,
        PackageBackend::Dnf5,
        PackageBackend::Pacman,
    ] {
        for action in ["install", "remove", "upgrade"] {
            let action = match (backend, action) {
                (PackageBackend::Pacman, "install") => "pacman_install",
                (PackageBackend::Pacman, "upgrade") => "pacman_upgrade",
                (_, action) => action,
            };
            let mut request = ManagementCommand {
                kind: ManagementKind::Packages,
                action: action.into(),
                target: None,
                identity: BTreeMap::new(),
                values: BTreeMap::new(),
            };
            for (second, valid) in [("omega", true), ("--all", false)] {
                let targets = ["alpha", second].map(|id| Target {
                    id: id.into(),
                    identity: BTreeMap::new(),
                });
                request
                    .values
                    .insert(KEY.into(), serde_json::to_string(&targets).unwrap());
                let result = build_command(&request, backend);
                if !valid {
                    assert!(result.is_err(), "{backend:?} {action}");
                    continue;
                }
                let spec = result.unwrap();
                assert!(
                    spec.args
                        .ends_with(&["--".into(), "alpha".into(), "omega".into()])
                );
                if backend == PackageBackend::Pacman && action != "remove" {
                    assert!(spec.args.iter().any(|arg| arg == "-Syu"));
                }
            }
        }
    }
}
