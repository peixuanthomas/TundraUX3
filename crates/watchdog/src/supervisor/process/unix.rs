use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use workspace_protocol::ProcessResource;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
use linux as native;
#[cfg(target_os = "macos")]
use macos as native;
use native::{ProcessIdentity, list_process_ids, read_identity};

pub(in crate::supervisor) struct WorkerTree {
    group: libc::pid_t,
}

impl WorkerTree {
    pub(in crate::supervisor) fn spawn(command: &mut Command) -> io::Result<(Child, Self)> {
        let child = command.process_group(0).spawn()?;
        let group = child.id() as libc::pid_t;
        Ok((child, Self { group }))
    }

    pub(in crate::supervisor) fn root_exited(&self, _: &mut Child) -> io::Result<bool> {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // Keep the leader unreaped while its group can still be signaled.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.group as _,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 {
            Ok(unsafe { info.si_pid() } != 0
                && matches!(
                    info.si_code,
                    libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED
                ))
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(in crate::supervisor) fn request_stop(&self, child: &mut Child) -> io::Result<()> {
        if unsafe { libc::kill(-self.group, libc::SIGKILL) } == 0 {
            return Ok(());
        }
        match child.kill() {
            Err(error) if vanished(&error) => Ok(()),
            result => result,
        }
    }

    pub(in crate::supervisor) fn stop_descendants_and_check(&self) -> io::Result<bool> {
        let result = unsafe { libc::kill(-self.group, libc::SIGKILL) };
        let error = (result != 0).then(io::Error::last_os_error);
        let finished = group_finished(self.group)?;
        if let Some(error) = error
            && !vanished(&error)
            && !finished
        {
            return Err(error);
        }
        Ok(finished)
    }

    pub(in crate::supervisor) fn register(
        &self,
        _: &Child,
        owner: u32,
        resource: ProcessResource,
    ) -> io::Result<ResourceTree> {
        #[cfg(target_os = "linux")]
        let delegated = matches!(resource, ProcessResource::LinuxDelegatedSession { .. });
        let (pid, group, birth, session) = match resource {
            ProcessResource::UnixSession { leader_pid, birth } => {
                (leader_pid, leader_pid, birth, true)
            }
            ProcessResource::UnixGroup {
                leader_pid,
                process_group,
                birth,
            } => (leader_pid, process_group, birth, false),
            #[cfg(target_os = "linux")]
            ProcessResource::LinuxDelegatedSession { leader_pid, birth } => {
                (leader_pid, leader_pid, birth, true)
            }
            _ => return Err(invalid_resource()),
        };
        if pid != group || pid <= 1 || pid > libc::pid_t::MAX as u32 || pid == self.group as u32 {
            return Err(invalid_resource());
        }
        let leader = read_identity(pid as libc::pid_t)?;
        if leader.birth != birth
            || leader.group as u32 != group
            || (session && leader.session as u32 != pid)
            || leader.dead
            || !self.owns(leader, owner as libc::pid_t)?
        {
            return Err(invalid_resource());
        }
        Ok(ResourceTree {
            leader,
            whole_session: session,
            known: BTreeMap::new(),
            #[cfg(target_os = "macos")]
            local_child: false,
            #[cfg(target_os = "linux")]
            delegated: if delegated {
                Some(native::observe(&leader)?)
            } else {
                None
            },
        })
    }

    pub(in crate::supervisor) fn owns_pid(&self, pid: u32) -> io::Result<bool> {
        if pid <= 1 || pid > libc::pid_t::MAX as u32 || pid == self.group as u32 {
            return Ok(false);
        }
        self.owns(read_identity(pid as libc::pid_t)?, self.group)
    }

    fn owns(&self, mut process: ProcessIdentity, owner: libc::pid_t) -> io::Result<bool> {
        if process.dead {
            return Ok(false);
        }
        let mut belongs_to_owner = false;
        while process.parent > 1 {
            let parent = read_identity(process.parent)?;
            if !process.has_parent(&parent) {
                break;
            }
            if parent.pid == owner && !parent.dead {
                belongs_to_owner = true;
            }
            if parent.pid == self.group && parent.group == self.group && !parent.dead {
                return Ok(belongs_to_owner);
            }
            process = parent;
        }
        Ok(false)
    }
}

pub(in crate::supervisor) struct ResourceTree {
    leader: ProcessIdentity,
    whole_session: bool,
    known: BTreeMap<libc::pid_t, u64>,
    #[cfg(target_os = "macos")]
    local_child: bool,
    #[cfg(target_os = "linux")]
    delegated: Option<std::os::fd::OwnedFd>,
}

#[cfg(target_os = "macos")]
pub struct OwnedProcessSession(std::sync::Mutex<ResourceTree>);

#[cfg(target_os = "macos")]
impl OwnedProcessSession {
    /// The caller owns the unreaped child created as a dedicated session leader.
    pub fn attach(pid: u32) -> io::Result<Self> {
        if pid <= 1 || pid > libc::pid_t::MAX as u32 {
            return Err(invalid_resource());
        }
        let leader = read_identity(pid as libc::pid_t)?;
        if leader.group != leader.pid
            || (!leader.dead && leader.session != leader.pid)
            || leader.parent != std::process::id() as libc::pid_t
        {
            return Err(invalid_resource());
        }
        Ok(Self(std::sync::Mutex::new(ResourceTree {
            leader,
            whole_session: true,
            known: BTreeMap::new(),
            local_child: true,
        })))
    }

    pub fn terminate(&self) -> io::Result<()> {
        use std::time::{Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(1);
        while self
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .stop_and_check()?
        {
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    pub fn request_stop(&self) -> io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .stop_and_check()
            .map(|_| ())
    }
}

impl ResourceTree {
    pub(in crate::supervisor) fn finished(&mut self) -> io::Result<bool> {
        #[cfg(target_os = "linux")]
        if let Some(owner) = &self.delegated {
            return native::exited(owner);
        }
        let (members, failure) = self.members()?;
        failure.map_or(Ok(members.is_empty()), Err)
    }

    pub(in crate::supervisor) fn stop_and_check(&mut self) -> io::Result<bool> {
        #[cfg(target_os = "linux")]
        if let Some(owner) = &self.delegated {
            // The elevated helper stops its own descendants when the worker dies.
            return native::exited(owner).map(|exited| !exited);
        }
        let (members, mut failure) = self.members()?;
        let alive = !members.is_empty();
        #[cfg(target_os = "macos")]
        let use_local_group = self.local_child
            && native::stable_signal().is_none()
            && read_identity(self.leader.pid).is_ok_and(|leader| {
                leader.birth == self.leader.birth
                    && leader.parent == std::process::id() as libc::pid_t
            });
        #[cfg(target_os = "macos")]
        if use_local_group
            && members
                .iter()
                .any(|process| process.group == self.leader.group)
            && unsafe { libc::kill(-self.leader.group, libc::SIGKILL) } != 0
        {
            let error = io::Error::last_os_error();
            if !vanished(&error) {
                failure.get_or_insert(error);
            }
        }
        for process in members.into_iter().rev() {
            #[cfg(target_os = "macos")]
            if use_local_group && process.group == self.leader.group {
                continue;
            }
            if let Err(error) = process.kill() {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(alive), Err)
    }

    fn members(&mut self) -> io::Result<(Vec<ProcessIdentity>, Option<io::Error>)> {
        let mut topology = BTreeMap::new();
        let mut permission_error = None;
        for pid in list_process_ids(None)? {
            match read_identity(pid) {
                Ok(process) if !process.dead && process.birth >= self.leader.birth => {
                    topology.insert(pid, process);
                }
                Ok(_) => {}
                Err(error) if vanished(&error) => {}
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                    let in_boundary = unsafe {
                        if self.whole_session {
                            libc::getsid(pid) == self.leader.pid
                        } else {
                            libc::getpgid(pid) == self.leader.group
                        }
                    };
                    if in_boundary || self.known.contains_key(&pid) {
                        permission_error.get_or_insert(error);
                    }
                }
                Err(error) => return Err(error),
            }
        }
        let leader_current = self.same_incarnation()?
            && topology
                .get(&self.leader.pid)
                .is_none_or(|leader| leader.birth == self.leader.birth);
        let mut members = BTreeSet::new();
        for process in topology.values() {
            let in_boundary = if self.whole_session {
                process.session == self.leader.pid
            } else {
                process.group == self.leader.group && process.session == self.leader.session
            };
            if (leader_current && in_boundary)
                || self.known.get(&process.pid) == Some(&process.birth)
            {
                members.insert(process.pid);
            }
        }
        loop {
            let size = members.len();
            for process in topology.values() {
                if members.contains(&process.parent)
                    && process.has_parent(&topology[&process.parent])
                {
                    members.insert(process.pid);
                }
            }
            if size == members.len() {
                break;
            }
        }
        let mut result = Vec::new();
        if permission_error.is_none() {
            self.known.clear();
        }
        for pid in members {
            let process = topology[&pid];
            self.known.insert(process.pid, process.birth);
            result.push(process);
        }
        result.sort_unstable_by_key(|process| process.birth);
        Ok((result, permission_error))
    }

    fn same_incarnation(&self) -> io::Result<bool> {
        // An extant process group or session reserves its numeric leader ID.
        match read_identity(self.leader.pid) {
            Ok(leader) => Ok(leader.birth == self.leader.birth),
            Err(error) if vanished(&error) => Ok(true),
            Err(error) => Err(error),
        }
    }
}

fn group_finished(group: libc::pid_t) -> io::Result<bool> {
    for pid in list_process_ids(Some(group))? {
        match read_identity(pid) {
            Ok(process) if !process.dead && process.group == group => return Ok(false),
            Ok(_) => {}
            Err(error) if vanished(&error) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

pub(in crate::supervisor) fn unix_session(pid: u32) -> io::Result<ProcessResource> {
    let process = resource_leader(pid)?;
    if process.session != process.pid {
        return Err(invalid_resource());
    }
    Ok(ProcessResource::UnixSession {
        leader_pid: pid,
        birth: process.birth,
    })
}

pub(in crate::supervisor) fn unix_group(pid: u32) -> io::Result<ProcessResource> {
    let process = resource_leader(pid)?;
    Ok(ProcessResource::UnixGroup {
        leader_pid: pid,
        process_group: pid,
        birth: process.birth,
    })
}

#[cfg(target_os = "linux")]
pub(in crate::supervisor) fn linux_delegated_session(pid: u32) -> io::Result<ProcessResource> {
    let process = resource_leader(pid)?;
    if process.session != process.pid {
        return Err(invalid_resource());
    }
    Ok(ProcessResource::LinuxDelegatedSession {
        leader_pid: pid,
        birth: process.birth,
    })
}

fn resource_leader(pid: u32) -> io::Result<ProcessIdentity> {
    if pid <= 1 || pid > libc::pid_t::MAX as u32 {
        return Err(invalid_resource());
    }
    let process = read_identity(pid as libc::pid_t)?;
    if process.group != process.pid || process.dead {
        return Err(invalid_resource());
    }
    Ok(process)
}

fn invalid_resource() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "process resource is not owned by this worker",
    )
}

fn vanished(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(libc::ESRCH | libc::ENOENT))
}

#[cfg(test)]
pub(in crate::supervisor) fn test_process_exited(pid: libc::pid_t) -> bool {
    match read_identity(pid) {
        Ok(process) => process.dead,
        Err(error) => vanished(&error),
    }
}
