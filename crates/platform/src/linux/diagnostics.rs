//! Read-only service and identity diagnostics; missing desktop services are optional.
use super::{
    dbus,
    identity::{LinuxUserContext, ProcessIdentity},
};
use crate::{CheckStatus, EnvironmentCheck};

pub fn checks() -> Vec<EnvironmentCheck> {
    let mut checks = Vec::new();
    let mut add = |id, label: &str, status, message: String| {
        checks.push(EnvironmentCheck {
            id,
            label: label.into(),
            status,
            message,
        })
    };
    let process = ProcessIdentity::current();
    add(
        "linux-process-identity",
        "Process UID/GID",
        if process.validate().is_ok() {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        format!(
            "UID={} EUID={} GID={} EGID={}",
            process.uid, process.effective_uid, process.gid, process.effective_gid
        ),
    );
    match LinuxUserContext::current() {
        Ok(user) => {
            add(
                "linux-nss-user",
                "NSS user",
                CheckStatus::Pass,
                format!(
                    "{}; HOME={}; shell={}; supplementary groups={:?}",
                    user.username,
                    user.home.display(),
                    user.shell.display(),
                    user.supplementary_groups
                ),
            );
            add(
                "linux-xdg",
                "XDG paths",
                CheckStatus::Pass,
                format!(
                    "config={}; data={}; cache={}; state={}",
                    user.config_home.display(),
                    user.data_home.display(),
                    user.cache_home.display(),
                    user.state_home.display()
                ),
            );
            add(
                "linux-runtime-dir",
                "XDG_RUNTIME_DIR",
                if user.runtime_dir.is_some() {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Warning
                },
                user.runtime_dir
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "Unavailable; local applications remain available".into()),
            );
        }
        Err(error) => add(
            "linux-nss-user",
            "NSS user",
            CheckStatus::Fail,
            error.to_string(),
        ),
    }
    add(
        "linux-user-bus",
        "User D-Bus",
        if dbus::session().is_ok() {
            CheckStatus::Pass
        } else {
            CheckStatus::Warning
        },
        "Optional for local applications".into(),
    );
    let system = dbus::system();
    for (id, label, name) in [
        ("linux-logind", "logind", "org.freedesktop.login1"),
        (
            "linux-packagekit",
            "PackageKit",
            "org.freedesktop.PackageKit",
        ),
        ("linux-polkit", "polkit", "org.freedesktop.PolicyKit1"),
    ] {
        let available = system
            .as_ref()
            .ok()
            .is_some_and(|bus| dbus::name_available(bus, name).unwrap_or(false));
        add(
            id,
            label,
            if available {
                CheckStatus::Pass
            } else {
                CheckStatus::Warning
            },
            if available {
                "Service is active or activatable".into()
            } else {
                "Unavailable".into()
            },
        );
    }
    let agent = std::fs::metadata("/usr/bin/pkttyagent").is_ok_and(|meta| {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    });
    add(
        "linux-pkttyagent",
        "pkttyagent",
        if agent {
            CheckStatus::Pass
        } else {
            CheckStatus::Warning
        },
        if agent {
            "Fedora terminal authentication fallback available".into()
        } else {
            "Unavailable; an existing system authentication agent may still be used".into()
        },
    );
    let installation = crate::installation::current_installation();
    add(
        "linux-update-backend",
        "Installation/update backend",
        if installation.backend == crate::installation::UpdateBackend::Unavailable {
            CheckStatus::Warning
        } else {
            CheckStatus::Pass
        },
        format!(
            "{:?}{}",
            installation.backend,
            installation
                .reason
                .map(|v| format!(": {v}"))
                .unwrap_or_default()
        ),
    );
    if let Some(rpm) = installation.rpm {
        add(
            "linux-installed-rpm",
            "Installed RPM",
            CheckStatus::Pass,
            format!("{} {} {}", rpm.name, rpm.version, rpm.architecture),
        );
    }
    checks
}
