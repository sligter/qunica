#[cfg(windows)]
use std::process::Command;

use super::protocol::TerminalCommandError;

pub(crate) fn taskkill_args(pid: u32) -> [String; 4] {
    [
        "/PID".to_string(),
        pid.to_string(),
        "/T".to_string(),
        "/F".to_string(),
    ]
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use std::io;
    use std::mem;
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    use super::{taskkill_args, Command, TerminalCommandError};

    struct OwnedWinHandle {
        handle: Option<HANDLE>,
    }

    // Windows kernel handles may be transferred between threads. Access is serialized by the
    // NativePtyHandle process-tree mutex.
    unsafe impl Send for OwnedWinHandle {}

    impl OwnedWinHandle {
        fn new(handle: HANDLE) -> Option<Self> {
            (!handle.is_null()).then_some(Self {
                handle: Some(handle),
            })
        }

        fn raw(&self) -> HANDLE {
            self.handle.unwrap_or(ptr::null_mut())
        }

        fn close(mut self) -> io::Result<()> {
            let Some(handle) = self.handle.take() else {
                return Ok(());
            };
            let result = unsafe { CloseHandle(handle) };
            if result == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    impl Drop for OwnedWinHandle {
        fn drop(&mut self) {
            if let Some(handle) = self.handle.take() {
                let _ = unsafe { CloseHandle(handle) };
            }
        }
    }

    pub(crate) struct ProcessTreeGuard {
        pid: u32,
        job: OwnedWinHandle,
    }

    impl ProcessTreeGuard {
        pub(crate) fn attach(pid: u32) -> Result<Self, TerminalCommandError> {
            let job = OwnedWinHandle::new(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) })
                .ok_or_else(|| windows_error("terminal.job_create_failed", "create Job Object"))?;

            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = unsafe {
                SetInformationJobObject(
                    job.raw(),
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const c_void,
                    mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if configured == 0 {
                return Err(windows_error(
                    "terminal.job_configure_failed",
                    "configure Job Object",
                ));
            }

            let process = OwnedWinHandle::new(unsafe {
                OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid)
            })
            .ok_or_else(|| {
                windows_error("terminal.process_open_failed", "open terminal process")
            })?;
            if unsafe { AssignProcessToJobObject(job.raw(), process.raw()) } == 0 {
                return Err(windows_error(
                    "terminal.job_assign_failed",
                    "assign terminal process to Job Object",
                ));
            }

            Ok(Self { pid, job })
        }

        pub(crate) fn terminate(self, root_alive: bool) -> Result<(), TerminalCommandError> {
            let pid = self.pid;
            let close_result = self.job.close().map_err(|error| {
                terminal_os_error(
                    "terminal.job_close_failed",
                    "close terminal Job Object",
                    &error,
                )
            });

            if close_result.is_err() && root_alive {
                let _ = Command::new("taskkill").args(taskkill_args(pid)).status();
            }
            close_result
        }
    }

    fn windows_error(code: &'static str, operation: &'static str) -> TerminalCommandError {
        let error = io::Error::last_os_error();
        terminal_os_error(code, operation, &error)
    }

    fn terminal_os_error(
        code: &'static str,
        operation: &'static str,
        error: &io::Error,
    ) -> TerminalCommandError {
        let detail = error.raw_os_error().map_or_else(
            || "unknown OS error".to_string(),
            |value| format!("OS error {value}"),
        );
        TerminalCommandError::new(code, format!("Failed to {operation}: {detail}"))
    }
}

#[cfg(unix)]
mod platform {
    use std::io;
    use std::thread;
    use std::time::Duration;

    use super::TerminalCommandError;

    pub(crate) struct ProcessTreeGuard {
        process_group: libc::pid_t,
    }

    impl ProcessTreeGuard {
        pub(crate) fn attach(pid: u32) -> Result<Self, TerminalCommandError> {
            let process_group = libc::pid_t::try_from(pid).map_err(|_| {
                TerminalCommandError::new(
                    "terminal.process_group_failed",
                    "Terminal process ID cannot be represented as a Unix process group",
                )
            })?;
            Ok(Self { process_group })
        }

        pub(crate) fn terminate(self, _root_alive: bool) -> Result<(), TerminalCommandError> {
            let term_result = signal_group(self.process_group, libc::SIGTERM);
            if !matches!(term_result, Ok(false)) {
                thread::sleep(Duration::from_millis(500));
            }
            let kill_result = signal_group(self.process_group, libc::SIGKILL);
            term_result.and(kill_result).map(|_| ())
        }
    }

    fn signal_group(
        process_group: libc::pid_t,
        signal: libc::c_int,
    ) -> Result<bool, TerminalCommandError> {
        if unsafe { libc::kill(-process_group, signal) } == 0 {
            return Ok(true);
        }

        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(false);
        }
        let detail = error.raw_os_error().map_or_else(
            || "unknown OS error".to_string(),
            |value| format!("OS error {value}"),
        );
        Err(TerminalCommandError::new(
            "terminal.process_group_failed",
            format!("Failed to signal terminal process group: {detail}"),
        ))
    }
}

pub(crate) use platform::ProcessTreeGuard;

#[cfg(test)]
mod tests {
    use super::taskkill_args;

    #[test]
    fn windows_taskkill_targets_the_process_tree() {
        assert_eq!(
            taskkill_args(4242),
            ["/PID", "4242", "/T", "/F"].map(str::to_string),
        );
    }
}
