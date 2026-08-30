//! Child-process spawning shared by the shell tool and the external agent
//! runtimes.
//!
//! Beyond suppressing the Windows console window, this module owns the one fact
//! every timeout and cancellation path depends on: **killing a shell is not the
//! same as killing the work it started**. `pwsh -Command "npm run build"` spawns
//! `node`, and `sh -c "make -j8"` spawns a fan of compilers. Terminating only the
//! direct child leaves those descendants running, still holding the pipes the
//! caller is waiting on. [`spawn_process_tree`] attaches an OS-level owner — a
//! job object on Windows, a process group on Unix — so a single
//! [`ProcessTree::terminate`] reaches the whole tree.

use std::{io, process::Command as StdCommand};

use tokio::process::{Child, Command as TokioCommand};

/// `CreateNoWindow` process-creation flag, so a Windows GUI session does not
/// flash a console window when spawning a CLI child process.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Convert an app-managed std command into a Tokio command, suppressing the
/// Windows console window that CLI children would otherwise create.
pub(crate) fn tokio_command_no_window(
    #[allow(unused_mut)] mut command: StdCommand,
) -> TokioCommand {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    TokioCommand::from(command)
}

/// Spawn `command` with its descendants owned by a [`ProcessTree`].
///
/// The child is spawned with `kill_on_drop`, so dropping the [`Child`] still
/// reaps the direct process; the returned tree is what reaches everything it
/// started. Callers that can time out or be cancelled must call
/// [`ProcessTree::terminate`] rather than only killing the child.
pub(crate) fn spawn_process_tree(
    #[allow(unused_mut)] mut command: StdCommand,
) -> io::Result<(Child, ProcessTree)> {
    // A new process group must be requested before the fork, so it is set on the
    // std command rather than after the spawn.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // `0` means "a new group whose id is the child's pid", which is what
        // `killpg` below addresses.
        command.process_group(0);
    }

    let mut command = tokio_command_no_window(command);
    command.kill_on_drop(true);
    let child = command.spawn()?;
    let tree = ProcessTree::attach(&child);
    Ok((child, tree))
}

#[cfg(windows)]
pub(crate) use windows_tree::ProcessTree;

#[cfg(unix)]
pub(crate) use unix_tree::ProcessTree;

#[cfg(not(any(windows, unix)))]
pub(crate) use fallback_tree::ProcessTree;

#[cfg(windows)]
mod windows_tree {
    use std::{mem, ptr};

    use tokio::process::Child;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
    };

    /// A Windows job object owning a spawned child and every process it starts.
    ///
    /// The job carries `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so closing the last
    /// handle in [`Drop`] terminates any descendant still running. That is the
    /// tree-wide equivalent of Tokio's `kill_on_drop`, which reaches only the
    /// process it spawned.
    pub(crate) struct ProcessTree {
        /// `None` when the job could not be created or assigned; every method is
        /// then a no-op and the caller falls back to killing the direct child.
        job: Option<HANDLE>,
    }

    // `HANDLE` is a raw pointer, which is what blocks the auto impls. It is only
    // ever passed to the job-object calls below, all of which are thread-safe.
    unsafe impl Send for ProcessTree {}
    unsafe impl Sync for ProcessTree {}

    impl ProcessTree {
        /// Put `child` and its future descendants into a fresh kill-on-close job.
        ///
        /// Assignment happens just after the spawn rather than under
        /// `CREATE_SUSPENDED`, because Tokio does not expose the child's main
        /// thread handle. The window in which the child could spawn a
        /// grandchild that escapes the job is the few microseconds before a
        /// shell has even parsed its command line.
        pub(crate) fn attach(child: &Child) -> Self {
            let Some(process) = child.raw_handle() else {
                return Self { job: None };
            };
            // SAFETY: every pointer below addresses a local, correctly sized
            // value, and `process` is a live handle owned by `child` for the
            // duration of this call.
            unsafe {
                let job = CreateJobObjectW(ptr::null(), ptr::null());
                if job.is_null() {
                    return Self { job: None };
                }
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = mem::zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let configured = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    ptr::addr_of!(limits).cast(),
                    mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if configured == 0 || AssignProcessToJobObject(job, process as HANDLE) == 0 {
                    CloseHandle(job);
                    return Self { job: None };
                }
                Self { job: Some(job) }
            }
        }

        /// Terminate every process in the job. Safe to call more than once.
        pub(crate) fn terminate(&self) {
            let Some(job) = self.job else {
                return;
            };
            // SAFETY: `job` is a live job handle owned by `self` until `Drop`.
            unsafe {
                TerminateJobObject(job, 1);
            }
        }

        /// A tree owning nothing, for tests that exercise bookkeeping rather
        /// than process control.
        #[cfg(test)]
        pub(crate) fn detached() -> Self {
            Self { job: None }
        }
    }

    impl Drop for ProcessTree {
        fn drop(&mut self) {
            let Some(job) = self.job.take() else {
                return;
            };
            // SAFETY: `job` is owned by `self` and closed exactly once. Closing
            // the last handle kills whatever the job still contains.
            unsafe {
                CloseHandle(job);
            }
        }
    }
}

#[cfg(unix)]
mod unix_tree {
    use tokio::process::Child;

    /// The process group a spawned child leads.
    ///
    /// [`super::spawn_process_tree`] asks for a new group before the fork, so the
    /// child's pid is also its process-group id and `killpg` reaches every
    /// descendant that did not deliberately leave the group.
    pub(crate) struct ProcessTree {
        /// `None` once the child has been reaped, or if it exited before its pid
        /// could be read.
        group: Option<i32>,
    }

    impl ProcessTree {
        pub(crate) fn attach(child: &Child) -> Self {
            Self {
                group: child.id().map(|pid| pid as i32),
            }
        }

        /// Send `SIGKILL` to the whole group. Safe to call more than once: a
        /// group that is already gone reports `ESRCH`, which is ignored.
        pub(crate) fn terminate(&self) {
            let Some(group) = self.group else {
                return;
            };
            // SAFETY: `killpg` is safe to call with any pid; an unknown group
            // fails with `ESRCH` rather than affecting an unrelated process.
            unsafe {
                libc::killpg(group, libc::SIGKILL);
            }
        }

        /// A tree owning nothing, for tests that exercise bookkeeping rather
        /// than process control.
        #[cfg(test)]
        pub(crate) fn detached() -> Self {
            Self { group: None }
        }
    }
}

#[cfg(not(any(windows, unix)))]
mod fallback_tree {
    use tokio::process::Child;

    /// Platforms without a process-tree primitive fall back to Tokio's
    /// `kill_on_drop`, which reaches the direct child only.
    pub(crate) struct ProcessTree;

    impl ProcessTree {
        pub(crate) fn attach(_child: &Child) -> Self {
            Self
        }

        pub(crate) fn terminate(&self) {}

        #[cfg(test)]
        pub(crate) fn detached() -> Self {
            Self
        }
    }
}
