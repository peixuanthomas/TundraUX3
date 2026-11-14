use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use windows_sys::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_MORE_DATA, HANDLE,
};
use windows_sys::Win32::System::{JobObjects::*, Threading::*};
use workspace_protocol::ProcessResource;

pub(in crate::supervisor) struct WorkerTree {
    job: OwnedHandle,
}

impl WorkerTree {
    pub(in crate::supervisor) fn spawn(command: &mut Command) -> io::Result<(Child, Self)> {
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle == 0 {
            return Err(io::Error::last_os_error());
        }
        let job = unsafe { OwnedHandle::from_raw_handle(handle as _) };
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        checked(unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as _,
            )
        })?;

        // Assign the job before the child can launch descendants.
        let mut child = command.creation_flags(CREATE_SUSPENDED).spawn()?;
        let process = child.as_raw_handle() as HANDLE;
        let result = checked(unsafe { AssignProcessToJobObject(handle, process) })
            .and_then(|()| resume(process));
        if let Err(error) = result {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok((child, Self { job }))
    }

    pub(in crate::supervisor) fn root_exited(&self, child: &mut Child) -> io::Result<bool> {
        Ok(child.try_wait()?.is_some())
    }

    pub(in crate::supervisor) fn request_stop(&self, _: &mut Child) -> io::Result<()> {
        terminate_job(&self.job)
    }

    pub(in crate::supervisor) fn stop_descendants_and_check(&self) -> io::Result<bool> {
        if job_finished(&self.job)? {
            return Ok(true);
        }
        terminate_job(&self.job)?;
        Ok(false)
    }

    pub(in crate::supervisor) fn register(
        &self,
        child: &Child,
        _: u32,
        resource: ProcessResource,
    ) -> io::Result<ResourceTree> {
        let ProcessResource::WindowsJob { handle, .. } = resource else {
            return Err(invalid_resource());
        };
        let mut duplicated = 0;
        checked(unsafe {
            DuplicateHandle(
                child.as_raw_handle() as HANDLE,
                handle as HANDLE,
                GetCurrentProcess(),
                &mut duplicated,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        })?;
        let job = unsafe { OwnedHandle::from_raw_handle(duplicated as _) };
        // A worker may only register a job nested in its own job.
        let members = job_processes(&job)?;
        if members.contains(&(child.id() as usize)) {
            return Err(invalid_resource());
        }
        let own = job_processes(&self.job)?;
        if !members.iter().all(|pid| own.contains(pid)) {
            return Err(invalid_resource());
        }
        Ok(ResourceTree {
            job: (!members.is_empty()).then_some(job),
        })
    }
}

fn resume(process: HANDLE) -> io::Result<()> {
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtResumeProcess(process: HANDLE) -> i32;
    }
    if unsafe { NtResumeProcess(process) } < 0 {
        return Err(io::Error::other("could not resume supervised process"));
    }
    Ok(())
}

pub(in crate::supervisor) struct ResourceTree {
    job: Option<OwnedHandle>,
}

impl ResourceTree {
    pub(in crate::supervisor) fn finished(&mut self) -> io::Result<bool> {
        self.job.as_ref().map_or(Ok(true), job_finished)
    }

    pub(in crate::supervisor) fn stop_and_check(&mut self) -> io::Result<bool> {
        let Some(job) = &self.job else {
            return Ok(false);
        };
        if job_finished(job)? {
            return Ok(false);
        }
        terminate_job(job)?;
        Ok(true)
    }
}

fn terminate_job(job: &OwnedHandle) -> io::Result<()> {
    checked(unsafe { TerminateJobObject(job.as_raw_handle() as HANDLE, 1) })
}

fn job_finished(job: &OwnedHandle) -> io::Result<bool> {
    let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
    checked(unsafe {
        QueryInformationJobObject(
            job.as_raw_handle() as HANDLE,
            JobObjectBasicAccountingInformation,
            (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            std::mem::size_of_val(&accounting) as _,
            std::ptr::null_mut(),
        )
    })?;
    Ok(accounting.ActiveProcesses == 0)
}

fn job_processes(job: &OwnedHandle) -> io::Result<Vec<usize>> {
    let header = 2 * std::mem::size_of::<u32>() / std::mem::size_of::<usize>();
    let mut list = vec![0_usize; header + 16];
    loop {
        let result = unsafe {
            QueryInformationJobObject(
                job.as_raw_handle() as HANDLE,
                JobObjectBasicProcessIdList,
                list.as_mut_ptr().cast(),
                std::mem::size_of_val(list.as_slice()) as _,
                std::ptr::null_mut(),
            )
        };
        match checked(result) {
            Ok(()) => {
                let count = unsafe { *list.as_ptr().cast::<u32>().add(1) } as usize;
                if header + count > list.len() {
                    return Err(invalid_resource());
                }
                list.truncate(header + count);
                return Ok(list.drain(header..).collect());
            }
            Err(error) if error.raw_os_error() == Some(ERROR_MORE_DATA as i32) => {
                list.resize(list.len() * 2, 0);
            }
            Err(error) => return Err(error),
        }
    }
}

fn checked(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn invalid_resource() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "process job is not owned by this worker",
    )
}
