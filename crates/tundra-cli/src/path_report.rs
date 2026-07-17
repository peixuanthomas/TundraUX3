use std::io::Write;

use tundra_platform::{AppPaths, Platform};
use tundra_storage::StorageLayout;

pub(crate) fn run_paths<Stdout: Write, Stderr: Write>(
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
) -> i32 {
    let _ = writeln!(stdout, "Path templates:");
    write_path_templates(stdout);

    match platform.app_paths() {
        Ok(paths) => {
            let _ = writeln!(stdout);
            let _ = writeln!(stdout, "Resolved paths:");
            write_resolved_paths(stdout, &paths);
            let _ = writeln!(stdout);
            let _ = writeln!(stdout, "Storage files:");
            write_storage_files(stdout, &StorageLayout::from_app_paths(&paths));
            0
        }
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            1
        }
    }
}

pub(crate) fn write_path_templates(output: &mut impl Write) {
    let _ = writeln!(output, "Config path: {}", AppPaths::CONFIG_TEMPLATE);
    let _ = writeln!(output, "Data path:   {}", AppPaths::DATA_TEMPLATE);
    let _ = writeln!(output, "Cache path:  {}", AppPaths::CACHE_TEMPLATE);
    let _ = writeln!(output, "Logs path:   {}", AppPaths::LOGS_TEMPLATE);
    let _ = writeln!(output, "Temp path:   {}", AppPaths::TEMP_TEMPLATE);
}

pub(crate) fn write_resolved_paths(output: &mut impl Write, paths: &AppPaths) {
    let _ = writeln!(output, "Config path: {}", paths.config_path().display());
    let _ = writeln!(output, "Data path:   {}", paths.data_path().display());
    let _ = writeln!(output, "Cache path:  {}", paths.cache_path().display());
    let _ = writeln!(output, "Logs path:   {}", paths.logs_path().display());
    let _ = writeln!(output, "Temp path:   {}", paths.temp_path().display());
}

pub(crate) fn write_storage_files(output: &mut impl Write, layout: &StorageLayout) {
    let _ = writeln!(output, "Config file:  {}", layout.config_path.display());
    let _ = writeln!(output, "State file:   {}", layout.state_path.display());
    let _ = writeln!(
        output,
        "Recent files: {}",
        layout.recent_files_path.display()
    );
    let _ = writeln!(output, "Sessions file: {}", layout.sessions_path.display());
    let _ = writeln!(output, "Users file:   {}", layout.users_path.display());
}
