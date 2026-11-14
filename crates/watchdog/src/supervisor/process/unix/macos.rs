use std::io;
use std::sync::OnceLock;

const PROC_PGRP_ONLY: u32 = 2;
const PROC_PIDT_BSDINFOWITHUNIQID: libc::c_int = 18;

#[repr(C)]
struct UniqueInfo {
    uuid: [u8; 16],
    unique_id: u64,
    parent_unique_id: u64,
    version: u32,
    reserved: u32,
    reserved2: u64,
    reserved3: u64,
}

#[repr(C)]
struct ProcessInfo {
    bsd: libc::proc_bsdinfo,
    unique: UniqueInfo,
}

#[derive(Clone, Copy)]
pub(super) struct ProcessIdentity {
    pub(super) pid: libc::pid_t,
    pub(super) parent: libc::pid_t,
    pub(super) group: libc::pid_t,
    pub(super) session: libc::pid_t,
    pub(super) birth: u64,
    pub(super) dead: bool,
    parent_birth: u64,
    version: u32,
}

pub(super) fn read_identity(pid: libc::pid_t) -> io::Result<ProcessIdentity> {
    let mut info: ProcessInfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as libc::c_int;
    if unsafe {
        libc::proc_pidinfo(
            pid,
            PROC_PIDT_BSDINFOWITHUNIQID,
            1,
            (&mut info as *mut ProcessInfo).cast(),
            size,
        )
    } != size
    {
        return Err(io::Error::last_os_error());
    }
    let session = unsafe { libc::getsid(pid) };
    let dead = info.bsd.pbi_status == libc::SZOMB;
    if session < 0 && !dead {
        return Err(io::Error::last_os_error());
    }
    Ok(ProcessIdentity {
        pid,
        parent: info.bsd.pbi_ppid as libc::pid_t,
        group: info.bsd.pbi_pgid as libc::pid_t,
        session,
        birth: info.unique.unique_id,
        dead,
        parent_birth: info.unique.parent_unique_id,
        version: info.unique.version,
    })
}

pub(super) fn list_process_ids(group: Option<libc::pid_t>) -> io::Result<Vec<libc::pid_t>> {
    let mut pids = vec![0; if group.is_some() { 32 } else { 512 }];
    loop {
        unsafe { *libc::__error() = 0 };
        let size = std::mem::size_of_val(pids.as_slice()) as libc::c_int;
        let count = match group {
            Some(group) => unsafe {
                libc::proc_listpids(PROC_PGRP_ONLY, group as u32, pids.as_mut_ptr().cast(), size)
                    / std::mem::size_of::<libc::pid_t>() as libc::c_int
            },
            None => unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), size) },
        };
        if count <= 0 {
            let error = io::Error::last_os_error();
            if count < 0 || error.raw_os_error().is_some_and(|code| code != 0) {
                return Err(error);
            }
        }
        if count as usize >= pids.len() {
            pids.resize(pids.len() * 2, 0);
        } else {
            pids.truncate(count as usize);
            break;
        }
    }
    pids.retain(|&pid| pid > 0);
    Ok(pids)
}

impl ProcessIdentity {
    pub(super) fn has_parent(&self, parent: &Self) -> bool {
        self.parent == parent.pid && self.parent_birth == parent.birth
    }

    pub(super) fn kill(&self) -> io::Result<()> {
        let signal = stable_signal().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "stable process signalling is unavailable on this macOS release",
            )
        })?;
        // Darwin checks pidversion atomically when delivering the signal.
        let mut token = [0; 8];
        token[5] = self.pid as u32;
        token[7] = self.version;
        let result = unsafe { signal(&token, libc::SIGKILL) };
        match result {
            0 | libc::ESRCH => Ok(()),
            error => Err(io::Error::from_raw_os_error(error)),
        }
    }
}

type Signal = unsafe extern "C" fn(*const [u32; 8], libc::c_int) -> libc::c_int;

pub(super) fn stable_signal() -> Option<Signal> {
    static SIGNAL: OnceLock<Option<Signal>> = OnceLock::new();
    *SIGNAL.get_or_init(|| {
        let address =
            unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"proc_signal_with_audittoken".as_ptr()) };
        (!address.is_null())
            .then(|| unsafe { std::mem::transmute::<*mut libc::c_void, Signal>(address) })
    })
}
