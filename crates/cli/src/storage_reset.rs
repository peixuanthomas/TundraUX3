use std::io::Write;

use platform::Platform;
use storage::{StorageLayout, reset_saved_content};

use crate::path_report::write_storage_files;

pub(crate) fn run_new<Stdout: Write, Stderr: Write>(
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
) -> i32 {
    match platform.app_paths() {
        Ok(paths) => match reset_saved_content(&paths) {
            Ok(report) => {
                let _ = writeln!(stdout, "TundraUX3 storage reset");
                let _ = writeln!(stdout, "Removed paths:");
                for path in &report.removed_paths {
                    let _ = writeln!(stdout, "  {}", path.display());
                }
                let _ = writeln!(stdout);
                let _ = writeln!(stdout, "Recreated storage files:");
                let layout = StorageLayout::from_app_paths(&paths);
                write_storage_files(stdout, &layout);
                0
            }
            Err(error) => {
                let _ = writeln!(stderr, "ERROR: could not reset saved content: {error}");
                1
            }
        },
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            1
        }
    }
}
