use std::fmt;
use std::io::Write;

use tundra_platform::Platform;
use tundra_storage::{StorageLayout, StorageManager};
use tundra_weathr::{LaunchLocation, LaunchOptions};

pub(crate) fn run_weathr<Stderr, Launcher, LaunchError>(
    platform: &dyn Platform,
    stderr: &mut Stderr,
    weathr_launcher: Launcher,
) -> i32
where
    Stderr: Write,
    Launcher: FnOnce(LaunchOptions) -> Result<(), LaunchError>,
    LaunchError: fmt::Display,
{
    match weathr_launcher(weathr_launch_options(platform)) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: could not launch weathr: {error}");
            1
        }
    }
}

fn weathr_launch_options(platform: &dyn Platform) -> LaunchOptions {
    let Some(config) = platform
        .app_paths()
        .ok()
        .map(|paths| StorageLayout::from_app_paths(&paths))
        .map(StorageManager::from_layout)
        .and_then(|storage| storage.load_config().ok())
    else {
        return LaunchOptions::default();
    };

    let mut options = LaunchOptions {
        timezone_id: Some(config.timezone.clone()),
        ..LaunchOptions::default()
    };

    if let Some(timezone) = tundra_ui::setup_timezone_options()
        .into_iter()
        .find(|timezone| timezone.id == config.timezone)
    {
        options.location_override = Some(LaunchLocation {
            latitude: timezone.latitude,
            longitude: timezone.longitude,
            city: Some(timezone.label),
        });
    }

    options
}
