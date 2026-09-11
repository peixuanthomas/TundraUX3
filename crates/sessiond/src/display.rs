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
    ) -> io::Result<Child>;
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
    ) -> io::Result<Child> {
        let kmscon = Path::new("/usr/bin/kmscon");
        process::trusted_file(kmscon)?;
        let exe = if trusted.is_some() {
            "/usr/libexec/tundra/tundra-greeter"
        } else {
            "/usr/bin/tundra-shell"
        };
        process::trusted_file(Path::new(exe))?;
        let vt = if trusted.is_some() {
            TRUSTED_VT
        } else {
            USER_VT
        };
        let mut cmd = Command::new(kmscon);
        cmd.env_clear()
            .envs(env)
            .current_dir(&user.home)
            .args([
                "--libseat",
                "--no-switchvt",
                "--no-session-control",
                "--session-max=1",
                "--mouse",
                "--no-reset-env",
                "--oneshot",
                "--font-engine=pango",
                "--font-name=Noto Sans Mono CJK SC",
            ])
            .arg(format!("--vt={vt}"))
            .args(["--login", "--", exe]);
        if let Some(socket) = trusted {
            cmd.arg("--channel-fd").arg(socket.as_raw_fd().to_string());
        }
        process::demote(&mut cmd, user, trusted.map(AsRawFd::as_raw_fd))?;
        cmd.spawn()
    }
}
/// Kernel-enforced gate: only CAP_SYS_TTY_CONFIG can release a trusted VT.
/// Deliberately has no unlocking Drop: a crashed authentication daemon must not
/// uncover the user framebuffer. Recovery is an explicit root operation via SSH.
pub struct VtGate {
    console: std::fs::File,
    held: bool,
}
impl VtGate {
    pub fn new() -> io::Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let console = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_CLOEXEC)
            .open("/dev/tty0")?;
        Ok(Self {
            console,
            held: false,
        })
    }
    pub fn secure(&mut self) -> io::Result<()> {
        const VT_ACTIVATE: libc::c_ulong = 0x5606;
        const VT_LOCKSWITCH: libc::c_ulong = 0x560b;
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
    pub fn restore(&mut self) -> io::Result<()> {
        if !self.held || self.active()? != TRUSTED_VT {
            return Err(io::Error::other("trusted gate not held"));
        }
        self.unlock()?;
        Kmscon.activate(USER_VT)
    }
}
