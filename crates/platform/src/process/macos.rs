use std::io;

pub(crate) fn group_is_dead(group: u32) -> io::Result<bool> {
    const PROC_PGRP_ONLY: u32 = 2;
    let mut pids = vec![0_i32; 32];
    loop {
        let bytes = unsafe {
            libc::proc_listpids(
                PROC_PGRP_ONLY,
                group,
                pids.as_mut_ptr().cast(),
                std::mem::size_of_val(pids.as_slice()) as i32,
            )
        };
        if bytes < 0 {
            return Err(io::Error::last_os_error());
        }
        let count = bytes as usize / std::mem::size_of::<i32>();
        if count < pids.len() {
            pids.truncate(count);
            break;
        }
        pids.resize(pids.len() * 2, 0);
    }
    for pid in pids.into_iter().filter(|pid| *pid > 0) {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let bytes = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                &mut info as *mut _ as *mut _,
                std::mem::size_of_val(&info) as i32,
            )
        };
        if bytes != std::mem::size_of_val(&info) as i32 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                continue;
            }
            return Err(error);
        }
        // Darwin returns EPERM when a group contains only zombies. The owner
        // keeps its child unreaped, so this group ID cannot be recycled.
        if info.pbi_pgid == group && info.pbi_status != libc::SZOMB {
            return Ok(false);
        }
    }
    Ok(true)
}
