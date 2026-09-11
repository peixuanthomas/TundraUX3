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

fn trusted_connection(name: &str) -> io::Result<(Connection, String)> {
    let connection = zbus::blocking::connection::Builder::system()
        .map_err(io::Error::other)?
        .method_timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(io::Error::other)?;
    let dbus = Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .map_err(io::Error::other)?;
    let owner: String = dbus
        .call("GetNameOwner", &(name,))
        .map_err(io::Error::other)?;
    let uid: u32 = dbus
        .call("GetConnectionUnixUser", &(&owner,))
        .map_err(io::Error::other)?;
    if uid != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "system service must be root-owned",
        ));
    }
    Ok((connection, owner))
}

pub fn managed_snapshot() -> io::Result<Option<crate::SessionSnapshot>> {
    let (connection, owner) = trusted_connection(crate::SESSION_BUS)?;
    let proxy = Proxy::new(
        &connection,
        owner.as_str(),
        crate::SESSION_PATH,
        crate::SESSION_BUS,
    )
    .map_err(io::Error::other)?;
    let json: String = proxy.call("GetSnapshot", &()).map_err(io::Error::other)?;
    let snapshot: Option<crate::SessionSnapshot> =
        serde_json::from_str(&json).map_err(io::Error::other)?;
    if let Some(snapshot) = &snapshot {
        if snapshot.identity != current_session()?.identity {
            return Err(io::Error::other(
                "session service returned a different user session",
            ));
        }
    }
    Ok(snapshot)
}

pub fn session_action(method: &str) -> io::Result<()> {
    if !matches!(method, "Lock" | "Logout" | "SwitchUser") {
        return Err(io::Error::other("unknown session action"));
    }
    managed_snapshot()?.ok_or_else(|| io::Error::other("not a managed Tundra session"))?;
    let (connection, owner) = trusted_connection(crate::SESSION_BUS)?;
    Proxy::new(
        &connection,
        owner.as_str(),
        crate::SESSION_PATH,
        crate::SESSION_BUS,
    )
    .map_err(io::Error::other)?
    .call::<_, _, ()>(method, &())
    .map_err(io::Error::other)
}

pub fn can_request_system_action() -> bool {
    let Ok((connection, owner)) = trusted_connection(crate::PRIVILEGED_BUS) else {
        return false;
    };
    let Ok(proxy) = Proxy::new(
        &connection,
        owner.as_str(),
        crate::PRIVILEGED_PATH,
        crate::PRIVILEGED_BUS,
    ) else {
        return false;
    };
    proxy.call::<_, _, bool>("CanRequest", &()).unwrap_or(false)
}

/// Keeps one unique bus sender alive through request, confirmation and result retrieval.
/// Call on a worker; disconnect/cancellation invalidates any unconsumed grant.
pub fn request_system_action(
    action: &crate::SystemAction,
    cancelled: &std::sync::atomic::AtomicBool,
) -> io::Result<String> {
    use std::sync::atomic::Ordering;
    action.validate().map_err(io::Error::other)?;
    let (connection, owner) = trusted_connection(crate::PRIVILEGED_BUS)?;
    let proxy = Proxy::new(
        &connection,
        owner.as_str(),
        crate::PRIVILEGED_PATH,
        crate::PRIVILEGED_BUS,
    )
    .map_err(io::Error::other)?;
    let version: u32 = proxy
        .call("ProtocolVersion", &())
        .map_err(io::Error::other)?;
    if version != crate::VERSION {
        return Err(io::Error::other("incompatible privileged service"));
    }
    let id: String = proxy
        .call(
            "Request",
            &(serde_json::to_string(action).map_err(io::Error::other)?,),
        )
        .map_err(io::Error::other)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(900);
    loop {
        if cancelled.load(Ordering::Relaxed) || std::time::Instant::now() >= deadline {
            let _ = proxy.call::<_, _, ()>("Cancel", &(&id,));
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "system operation cancelled",
            ));
        }
        let json: String = proxy.call("GetResult", &(&id,)).map_err(io::Error::other)?;
        let (status, result): (crate::OperationStatus, String) =
            serde_json::from_str(&json).map_err(io::Error::other)?;
        match status {
            crate::OperationStatus::Completed => return Ok(result),
            crate::OperationStatus::Cancelled => {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "authorization cancelled",
                ));
            }
            crate::OperationStatus::Failed(error) => return Err(io::Error::other(error)),
            _ => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
}
