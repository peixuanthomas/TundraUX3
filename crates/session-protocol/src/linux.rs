//! Linux identity discovery; never accepts UID, HOME or session IDs from the environment.
use crate::{SessionIdentity, SystemUser};
use std::ffi::{CStr, CString};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

pub fn account(uid: u32) -> io::Result<SystemUser> {
    let mut size = 16_384;
    loop {
        let mut buffer = vec![0u8; size];
        let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        // SAFETY: buffers are writable and remain alive while the record is copied.
        let status = unsafe {
            libc::getpwuid_r(
                uid,
                entry.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if status == libc::ERANGE && size < 1_048_576 {
            size *= 2;
            continue;
        }
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status));
        }
        if result.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "NSS account not found",
            ));
        }
        let entry = unsafe { entry.assume_init() };
        if entry.pw_name.is_null() || entry.pw_dir.is_null() || entry.pw_shell.is_null() {
            return Err(io::Error::other("incomplete NSS account"));
        }
        let username = unsafe { CStr::from_ptr(entry.pw_name) }
            .to_str()
            .map_err(io::Error::other)?
            .to_owned();
        let home = PathBuf::from(std::ffi::OsStr::from_bytes(
            unsafe { CStr::from_ptr(entry.pw_dir) }.to_bytes(),
        ));
        let shell = PathBuf::from(std::ffi::OsStr::from_bytes(
            unsafe { CStr::from_ptr(entry.pw_shell) }.to_bytes(),
        ));
        if !home.is_absolute() || !shell.is_absolute() {
            return Err(io::Error::other("NSS HOME and shell must be absolute"));
        }
        return Ok(SystemUser {
            uid: entry.pw_uid,
            gid: entry.pw_gid,
            username,
            home,
            shell,
        });
    }
}

pub fn current_user() -> io::Result<SystemUser> {
    let uid = unsafe { libc::getuid() };
    if uid == 0 || uid != unsafe { libc::geteuid() } || unsafe { libc::getgid() != libc::getegid() }
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Tundra UX requires an unprivileged, non-setuid user process",
        ));
    }
    account(uid)
}

pub fn account_in_group(user: &SystemUser, name: &str) -> io::Result<bool> {
    let name = CString::new(name)?;
    let mut buffer = vec![0u8; 1_048_576];
    let mut group = std::mem::MaybeUninit::<libc::group>::uninit();
    let mut result = std::ptr::null_mut();
    let status = unsafe {
        libc::getgrnam_r(
            name.as_ptr(),
            group.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    if result.is_null() {
        return Ok(false);
    }
    let gid = unsafe { group.assume_init().gr_gid };
    let username = CString::new(user.username.as_str())?;
    let mut count = 0;
    unsafe {
        libc::getgrouplist(
            username.as_ptr(),
            user.gid,
            std::ptr::null_mut(),
            &mut count,
        );
    }
    if !(1..=65536).contains(&count) {
        return Err(io::Error::other("invalid NSS group count"));
    }
    let mut groups = vec![0; count as usize];
    if unsafe { libc::getgrouplist(username.as_ptr(), user.gid, groups.as_mut_ptr(), &mut count) }
        < 0
    {
        return Err(io::Error::other("NSS group membership changed"));
    }
    Ok(groups[..count as usize].contains(&gid))
}

#[derive(Debug, Clone)]
pub struct LogindSession {
    pub identity: SessionIdentity,
    pub path: OwnedObjectPath,
    pub seat: String,
    pub active: bool,
    pub remote: bool,
    pub locked: bool,
}

pub fn session_for_pid(connection: &Connection, pid: u32) -> zbus::Result<LogindSession> {
    let manager = Proxy::new(
        connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;
    let path: OwnedObjectPath = manager.call("GetSessionByPID", &(pid,))?;
    let proxy = Proxy::new(
        connection,
        "org.freedesktop.login1",
        path.clone(),
        "org.freedesktop.login1.Session",
    )?;
    let (uid, _): (u32, OwnedObjectPath) = proxy.get_property("User")?;
    let (seat, _): (String, OwnedObjectPath) = proxy.get_property("Seat")?;
    Ok(LogindSession {
        identity: SessionIdentity {
            uid,
            logind_session_id: proxy.get_property("Id")?,
        },
        path,
        seat,
        active: proxy.get_property("Active")?,
        remote: proxy.get_property("Remote")?,
        locked: proxy.get_property("LockedHint")?,
    })
}

pub fn current_session() -> io::Result<LogindSession> {
    let user = current_user()?;
    let connection = Connection::system().map_err(io::Error::other)?;
    let session = session_for_pid(&connection, std::process::id()).map_err(io::Error::other)?;
    if session.identity.uid != user.uid {
        return Err(io::Error::other("process and logind UID disagree"));
    }
    Ok(session)
}
