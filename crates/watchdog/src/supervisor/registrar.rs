use std::{io, sync::OnceLock};
use workspace_protocol::ProcessResource;

struct Registrar {
    register: Box<dyn Fn(ProcessResource) -> io::Result<()> + Send + Sync>,
    unregister: Box<dyn Fn(ProcessResource) + Send + Sync>,
}

static REGISTRAR: OnceLock<Registrar> = OnceLock::new();

pub fn set_process_resource_registrar(
    register: impl Fn(ProcessResource) -> io::Result<()> + Send + Sync + 'static,
    unregister: impl Fn(ProcessResource) + Send + Sync + 'static,
) -> io::Result<()> {
    REGISTRAR
        .set(Registrar {
            register: Box::new(register),
            unregister: Box::new(unregister),
        })
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "process resource registrar is installed",
            )
        })
}

pub fn process_session_registration_enabled() -> bool {
    REGISTRAR.get().is_some()
}

/// Keep the registration while the native process tree may still contain live children.
pub struct ProcessResourceGuard(ProcessResource);

impl Drop for ProcessResourceGuard {
    fn drop(&mut self) {
        if let Some(registrar) = REGISTRAR.get() {
            (registrar.unregister)(self.0);
        }
    }
}

#[cfg(unix)]
pub fn register_process_session(leader_pid: u32) -> io::Result<Option<ProcessResourceGuard>> {
    register(|| super::process::unix_session(leader_pid))
}

#[cfg(unix)]
pub fn register_process_group(leader_pid: u32) -> io::Result<Option<ProcessResourceGuard>> {
    register(|| super::process::unix_group(leader_pid))
}

#[cfg(target_os = "linux")]
pub fn register_delegated_session(leader_pid: u32) -> io::Result<Option<ProcessResourceGuard>> {
    register(|| super::process::linux_delegated_session(leader_pid))
}

#[cfg(windows)]
pub fn register_process_job(handle: u64) -> io::Result<Option<ProcessResourceGuard>> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    register(|| {
        Ok(ProcessResource::WindowsJob {
            handle,
            generation: NEXT.fetch_add(1, Ordering::Relaxed),
        })
    })
}

fn register(
    resource: impl FnOnce() -> io::Result<ProcessResource>,
) -> io::Result<Option<ProcessResourceGuard>> {
    let Some(registrar) = REGISTRAR.get() else {
        return Ok(None);
    };
    let resource = resource()?;
    (registrar.register)(resource)?;
    Ok(Some(ProcessResourceGuard(resource)))
}
