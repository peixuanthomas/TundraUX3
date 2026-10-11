//! Shared by app and Shell tests: fail the real atomic manifest write while
//! keeping the original, valid Prepared manifest readable and unchanged.
use std::path::Path;

pub(super) fn with_blocked_write<T>(root: &Path, manifest: &Path, action: impl FnOnce() -> T) -> T {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let _ = root;
        // Permit reads, but deny replacement/deletion of the journal.
        let _lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(manifest)
            .unwrap();
        action()
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let directory = manifest.parent().unwrap();
        let permissions = std::fs::metadata(directory).unwrap().permissions();
        struct RestoreDirectory<'a>(&'a Path, std::fs::Permissions);
        impl Drop for RestoreDirectory<'_> {
            fn drop(&mut self) {
                std::fs::set_permissions(self.0, self.1.clone()).unwrap();
            }
        }
        // Root test runs must exercise permission errors too. setfsuid affects
        // only this thread's filesystem access, not other tests or watchdogs.
        unsafe extern "C" {
            fn setfsuid(uid: u32) -> i32;
        }
        struct RestoreFsuid(u32);
        impl Drop for RestoreFsuid {
            fn drop(&mut self) {
                unsafe { setfsuid(self.0) };
            }
        }
        for parent in directory
            .ancestors()
            .take_while(|path| path.starts_with(root))
        {
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::set_permissions(manifest, std::fs::Permissions::from_mode(0o644)).unwrap();
        let restore_directory = RestoreDirectory(directory, permissions);
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o555)).unwrap();
        let previous_fsuid = unsafe { setfsuid(u32::MAX) } as u32;
        let restore_fsuid = RestoreFsuid(previous_fsuid);
        if previous_fsuid == 0 {
            unsafe { setfsuid(65534) };
            assert_eq!(unsafe { setfsuid(u32::MAX) }, 65534);
        }
        let result = action();
        drop(restore_fsuid);
        drop(restore_directory);
        result
    }
}
