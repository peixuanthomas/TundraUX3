//! Exercise the Linux folder resolver on every Unix test host, including macOS.
#![cfg(all(unix, not(target_os = "linux")))]

use platform::{PlatformError, UserDirs};

#[path = "../src/linux/user_dirs.rs"]
mod user_dirs;
