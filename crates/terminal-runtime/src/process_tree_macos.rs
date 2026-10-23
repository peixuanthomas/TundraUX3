//! Interactive shells place background jobs in separate process groups.
use std::collections::BTreeSet;
use std::io;

pub(super) fn terminate_background_groups(session: libc::pid_t) -> io::Result<()> {
    if session <= 0 || session == unsafe { libc::getsid(0) } {
        return Err(io::Error::other(
            "cannot terminate the host terminal session",
        ));
    }
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err(io::Error::last_os_error());
    }
    let mut pids = vec![0 as libc::pid_t; count as usize + 32];
    loop {
        let count = unsafe {
            libc::proc_listallpids(
                pids.as_mut_ptr().cast(),
                std::mem::size_of_val(pids.as_slice()) as libc::c_int,
            )
        };
        if count <= 0 {
            return Err(io::Error::last_os_error());
        }
        if (count as usize) < pids.len() {
            pids.truncate(count as usize);
            break;
        }
        pids.resize(pids.len() * 2, 0);
    }
    let mut groups = BTreeSet::new();
    for pid in pids {
        if pid <= 0 || unsafe { libc::getsid(pid) } != session {
            continue;
        }
        let group = unsafe { libc::getpgid(pid) };
        if group > 0 && group != session {
            groups.insert(group);
        }
    }
    for group in groups {
        if unsafe { libc::kill(-group, libc::SIGKILL) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
    }
    Ok(())
}
