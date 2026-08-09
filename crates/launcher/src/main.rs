#![cfg_attr(windows, windows_subsystem = "windows")]

use launcher::{BundleLayout, production_supervisor, show_critical_fallback};

fn main() {
    let result = BundleLayout::from_current_exe()
        .map_err(launcher::LauncherError::from)
        .and_then(production_supervisor)
        .and_then(|mut supervisor| supervisor.run());
    if let Err(error) = result {
        show_critical_fallback(&error);
        std::process::exit(1);
    }
}
