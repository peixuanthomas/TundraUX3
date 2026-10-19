use super::ManagementError;

pub(super) fn system_manager() -> Result<(), ManagementError> {
    let name = std::fs::read_to_string("/proc/1/comm").map_err(super::network::io_error)?;
    check_system_manager(&name)
}

fn check_system_manager(name: &str) -> Result<(), ManagementError> {
    if name.trim() == "systemd" {
        Ok(())
    } else {
        Err(ManagementError::UnsupportedEnvironment(i18n::tr!(
            "management-systemd-not-pid1",
            process = name.trim()
        )))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/management/capability_tests.rs"]
mod tests;
