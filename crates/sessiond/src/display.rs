//! Fixed seat/VT backend; all terminal escape parsing occurs after dropping root.
use crate::process;
use session_protocol::SystemUser;
use std::{
    io,
    os::unix::io::AsRawFd,
    path::Path,
    process::{Child, Command},
};
use zbus::blocking::{Connection, Proxy};
pub const TRUSTED_VT: u32 = 8;
pub const USER_VT: u32 = 9;
pub trait SessionDisplayBackend {
    fn activate(&self, vt: u32) -> io::Result<()>;
    fn spawn(
        &self,
        user: &SystemUser,
        env: std::collections::BTreeMap<String, String>,
        trusted: Option<&std::os::unix::net::UnixStream>,
    ) -> io::Result<Terminal>;
}
pub struct Terminal {
    child: Option<Child>,
    tty: std::fs::File,
}
impl Terminal {
    pub fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.child.as_mut().expect("spawned terminal").try_wait()
    }
    pub fn kill(&mut self) -> io::Result<()> {
        self.child.as_mut().expect("spawned terminal").kill()
    }
    pub fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
        self.child.as_mut().expect("spawned terminal").wait()
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        unsafe {
            libc::fchown(self.tty.as_raw_fd(), 0, 0);
            libc::fchmod(self.tty.as_raw_fd(), 0o600);
        }
    }
}
pub struct Kmscon;
impl SessionDisplayBackend for Kmscon {
    fn activate(&self, vt: u32) -> io::Result<()> {
        let c = Connection::system().map_err(io::Error::other)?;
        let p = Proxy::new(
            &c,
            "org.freedesktop.login1",
            "/org/freedesktop/login1/seat/seat0",
            "org.freedesktop.login1.Seat",
        )
        .map_err(io::Error::other)?;
        p.call::<_, _, ()>("SwitchTo", &(vt,))
            .map_err(io::Error::other)
    }
    fn spawn(
        &self,
        user: &SystemUser,
        env: std::collections::BTreeMap<String, String>,
        trusted: Option<&std::os::unix::net::UnixStream>,
    ) -> io::Result<Terminal> {
        let kmscon = Path::new("/usr/libexec/tundra/kmscon");
        let kmscon = process::trusted_executable(kmscon)?;
        process::verify_kmscon(&kmscon)?;
        let exe = if trusted.is_some() {
            "/usr/libexec/tundra/tundra-greeter"
        } else {
            "/usr/bin/tundra-shell"
        };
        let exe = process::trusted_executable(Path::new(exe))?;
        let vt = if trusted.is_some() {
            TRUSTED_VT
        } else {
            USER_VT
        };
        use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_CLOEXEC)
            .open(format!("/dev/tty{vt}"))?;
        if !tty.metadata()?.file_type().is_char_device() {
            return Err(io::Error::other("session VT is not a character device"));
        }
        let mut cmd = Command::new(kmscon);
        cmd.env_clear()
            .envs(env)
            .current_dir(if trusted.is_some() {
                Path::new("/")
            } else {
                &user.home
            })
            .args([
                "--libseat",
                "--term=xterm-256color",
                "--no-switchvt",
                "--no-issue",
                "--no-session-control",
                "--session-max=1",
                "--mouse",
                "--no-reset-env",
                "--oneshot",
                "--font-engine=pango",
                "--font-name=Noto Sans Mono CJK SC",
            ])
            .arg(format!("--vt={vt}"))
            .args(["--login", "--"])
            .arg(exe);
        if let Some(socket) = trusted {
            cmd.arg("--channel-fd").arg(socket.as_raw_fd().to_string());
        }
        process::demote(&mut cmd, user, trusted.map(AsRawFd::as_raw_fd))?;
        let mut terminal = Terminal { child: None, tty };
        if unsafe { libc::fchown(terminal.tty.as_raw_fd(), user.uid, user.gid) } < 0
            || unsafe { libc::fchmod(terminal.tty.as_raw_fd(), 0o600) } < 0
        {
            return Err(io::Error::last_os_error());
        }
        terminal.child = Some(cmd.spawn()?);
        Ok(terminal)
    }
}
/// Kernel-enforced gate: only CAP_SYS_TTY_CONFIG can release a trusted VT.
/// Deliberately has no unlocking Drop: a crashed authentication daemon must not
/// uncover the user framebuffer. Recovery is an explicit root operation via SSH.
pub struct VtGate {
    console: std::fs::File,
    held: bool,
    original_vt: u32,
}
impl VtGate {
    pub fn new() -> io::Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let console = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_CLOEXEC)
            .open("/dev/tty0")?;
        let mut gate = Self {
            console,
            held: false,
            original_vt: 0,
        };
        gate.original_vt = gate.active()?;
        Ok(gate)
    }
    pub fn secure(&mut self) -> io::Result<()> {
        const VT_ACTIVATE: libc::c_ulong = 0x5606;
        const VT_LOCKSWITCH: libc::c_ulong = 0x560b;
        if self.active()? == TRUSTED_VT {
            if unsafe { libc::ioctl(self.console.as_raw_fd(), VT_LOCKSWITCH, 0) } < 0 {
                return Err(io::Error::last_os_error());
            }
            self.held = true;
            if self.active()? != TRUSTED_VT {
                return Err(io::Error::other(
                    "VT raced trusted handover; gate remains locked",
                ));
            }
            return Ok(());
        }
        self.unlock()?;
        if unsafe { libc::ioctl(self.console.as_raw_fd(), VT_ACTIVATE, TRUSTED_VT) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while self.active()? != TRUSTED_VT {
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::other("trusted VT activation timed out"));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if unsafe { libc::ioctl(self.console.as_raw_fd(), VT_LOCKSWITCH, 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
        self.held = true;
        if self.active()? != TRUSTED_VT {
            return Err(io::Error::other(
                "VT changed during trusted handover; gate remains locked",
            ));
        }
        Ok(())
    }
    pub fn active(&self) -> io::Result<u32> {
        #[repr(C)]
        struct State {
            active: u16,
            signal: u16,
            state: u16,
        }
        let mut state = State {
            active: 0,
            signal: 0,
            state: 0,
        };
        if unsafe {
            libc::ioctl(
                self.console.as_raw_fd(),
                0x5603 as libc::c_ulong,
                &mut state,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(u32::from(state.active))
    }
    fn unlock(&mut self) -> io::Result<()> {
        if unsafe { libc::ioctl(self.console.as_raw_fd(), 0x560c as libc::c_ulong, 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
        self.held = false;
        Ok(())
    }
    /// Only orderly, verified PAM teardown may release the secure gate.
    pub fn restore_original(&mut self) -> io::Result<()> {
        if self.original_vt == 0 || [TRUSTED_VT, USER_VT].contains(&self.original_vt) {
            return Err(io::Error::other("no safe original VT recorded"));
        }
        self.unlock()?;
        if unsafe {
            libc::ioctl(
                self.console.as_raw_fd(),
                0x5606 as libc::c_ulong,
                self.original_vt,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while self.active()? != self.original_vt {
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::other("original VT restoration timed out"));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        Ok(())
    }
    pub fn restore(&mut self) -> io::Result<()> {
        if !self.held || self.active()? != TRUSTED_VT {
            return Err(io::Error::other("trusted gate not held"));
        }
        self.unlock()?;
        Kmscon.activate(USER_VT)
    }
}

/// Refuse to take VTs that belong to an existing login or a waiting getty.
pub fn ensure_reserved_vts_available() -> io::Result<()> {
    let c = Connection::system().map_err(io::Error::other)?;
    let login = Proxy::new(
        &c,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .map_err(io::Error::other)?;
    let sessions: Vec<(String, u32, String, String, zbus::zvariant::OwnedObjectPath)> =
        login.call("ListSessions", &()).map_err(io::Error::other)?;
    for (_, _, _, _, path) in sessions {
        let session = Proxy::new(
            &c,
            "org.freedesktop.login1",
            path,
            "org.freedesktop.login1.Session",
        )
        .map_err(io::Error::other)?;
        let vt: u32 = session.get_property("VTNr").map_err(io::Error::other)?;
        if [TRUSTED_VT, USER_VT].contains(&vt) {
            return Err(io::Error::other(format!(
                "reserved tty{vt} already belongs to a logind session"
            )));
        }
    }
    let manager = Proxy::new(
        &c,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    )
    .map_err(io::Error::other)?;
    for vt in [TRUSTED_VT, USER_VT] {
        let path: zbus::zvariant::OwnedObjectPath = manager
            .call("LoadUnit", &(format!("getty@tty{vt}.service"),))
            .map_err(io::Error::other)?;
        let unit = Proxy::new(
            &c,
            "org.freedesktop.systemd1",
            path,
            "org.freedesktop.systemd1.Unit",
        )
        .map_err(io::Error::other)?;
        let state: String = unit.get_property("ActiveState").map_err(io::Error::other)?;
        if state != "inactive" && state != "failed" {
            return Err(io::Error::other(format!(
                "reserved tty{vt} has an active getty"
            )));
        }
    }
    if [TRUSTED_VT, USER_VT].contains(&VtGate::new()?.active()?) {
        return Err(io::Error::other(
            "reserved VT is already foreground; explicit root recovery is required",
        ));
    }
    Ok(())
}
