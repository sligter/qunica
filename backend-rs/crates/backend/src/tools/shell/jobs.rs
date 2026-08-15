//! Background shell jobs.
//!
//! A foreground shell call blocks the agent's turn for as long as the command
//! runs — up to an hour, with the model seeing nothing but a `tool_call_start`
//! until it finishes. Long work (a build, a test suite, a dev server) belongs in
//! the background: the tool returns a job id immediately, and the model polls
//! for whatever output has accumulated since its last read.
//!
//! Reads are incremental and never repeat output. When a job produces more than
//! the retained buffer holds, the *oldest unread* text is dropped and the loss
//! is reported, because the tail of a build log is the part that says what
//! broke. The complete stream is always on disk in the job's spill file.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Child,
};

use crate::process::ProcessTree;

use super::decode::decode_output;

/// Largest amount of unread output a job retains in memory. Beyond this the
/// oldest unread text is dropped; the spill file still has everything.
pub const MAX_JOB_BUFFER_CHARS: usize = 200_000;

/// Largest amount of text one `ShellOutput` read returns.
pub const MAX_JOB_READ_CHARS: usize = 12_000;

/// Jobs are forgotten once this many have finished, newest kept.
const MAX_RETAINED_JOBS: usize = 64;

/// How a job ended, or that it has not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    /// Exited on its own; the string is an exit code or `signal`.
    Exited(String),
    /// Terminated by [`Job::kill`] or by dropping the registry.
    Killed,
}

impl JobStatus {
    pub fn label(&self) -> String {
        match self {
            JobStatus::Running => "running".to_string(),
            JobStatus::Exited(code) => format!("exited (exit_code={code})"),
            JobStatus::Killed => "killed".to_string(),
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self, JobStatus::Running)
    }
}

/// One incremental read: text not previously returned, plus what was lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRead {
    pub text: String,
    /// Characters dropped from the front of the buffer before this read, from
    /// output outrunning the reader.
    pub dropped: usize,
    /// Characters withheld from this read because it hit [`MAX_JOB_READ_CHARS`];
    /// they stay queued for the next read.
    pub withheld: usize,
    pub status: JobStatus,
}

#[derive(Debug, Default)]
struct JobState {
    /// Output produced but not yet returned by a read.
    unread: String,
    dropped: usize,
    total: usize,
    status: Option<JobStatus>,
}

/// A running or finished background command.
pub struct Job {
    pub id: String,
    pub command: String,
    /// Workspace the job was started in. A read from a different workspace is
    /// refused, so a job id leaking into another agent's context is inert.
    root: PathBuf,
    /// Workspace-relative path of the complete output log.
    pub spill_path: String,
    started: Instant,
    state: Mutex<JobState>,
    tree: ProcessTree,
}

impl Job {
    pub fn status(&self) -> JobStatus {
        self.state
            .lock()
            .expect("job state mutex poisoned")
            .status
            .clone()
            .unwrap_or(JobStatus::Running)
    }

    pub fn elapsed_seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Total characters the job has produced, read or not.
    pub fn total_chars(&self) -> usize {
        self.state.lock().expect("job state mutex poisoned").total
    }

    /// Take everything produced since the previous read.
    pub fn read(&self) -> JobRead {
        let mut state = self.state.lock().expect("job state mutex poisoned");
        let dropped = std::mem::take(&mut state.dropped);
        let available = state.unread.chars().count();
        let (text, withheld) = if available <= MAX_JOB_READ_CHARS {
            (std::mem::take(&mut state.unread), 0)
        } else {
            // Return the oldest text first: a read is a stream cursor, so the
            // head is what has not been seen yet.
            let split = state
                .unread
                .char_indices()
                .nth(MAX_JOB_READ_CHARS)
                .map(|(index, _)| index)
                .unwrap_or(state.unread.len());
            let rest = state.unread.split_off(split);
            let head = std::mem::replace(&mut state.unread, rest);
            (head, available - MAX_JOB_READ_CHARS)
        };
        JobRead {
            text,
            dropped,
            withheld,
            status: state.status.clone().unwrap_or(JobStatus::Running),
        }
    }

    /// Terminate the job and everything it started.
    pub fn kill(&self) {
        self.tree.terminate();
        let mut state = self.state.lock().expect("job state mutex poisoned");
        if state.status.is_none() {
            state.status = Some(JobStatus::Killed);
        }
    }

    fn push(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        let mut state = self.state.lock().expect("job state mutex poisoned");
        state.total += text.chars().count();
        state.unread.push_str(text);
        let retained = state.unread.chars().count();
        if retained > MAX_JOB_BUFFER_CHARS {
            let excess = retained - MAX_JOB_BUFFER_CHARS;
            let split = state
                .unread
                .char_indices()
                .nth(excess)
                .map(|(index, _)| index)
                .unwrap_or(state.unread.len());
            state.unread = state.unread.split_off(split);
            state.dropped += excess;
        }
    }

    fn finish(&self, status: JobStatus) {
        let mut state = self.state.lock().expect("job state mutex poisoned");
        if state.status.is_none() {
            state.status = Some(status);
        }
    }
}

/// Every background job started by this process.
pub struct JobRegistry {
    jobs: Mutex<Vec<Arc<Job>>>,
}

impl JobRegistry {
    fn new() -> Self {
        Self {
            jobs: Mutex::new(Vec::new()),
        }
    }

    /// Register `job`, forgetting the oldest finished jobs past the retention
    /// cap so a long-lived process does not accumulate them without bound.
    fn insert(&self, job: Arc<Job>) {
        let mut jobs = self.jobs.lock().expect("job registry mutex poisoned");
        jobs.push(job);
        if jobs.len() > MAX_RETAINED_JOBS {
            let mut kept: Vec<Arc<Job>> = Vec::with_capacity(jobs.len());
            let mut finished_to_drop = jobs.len() - MAX_RETAINED_JOBS;
            for job in jobs.drain(..) {
                if finished_to_drop > 0 && !job.status().is_running() {
                    finished_to_drop -= 1;
                    continue;
                }
                kept.push(job);
            }
            *jobs = kept;
        }
    }

    /// Look up `id`, but only for a caller bound to the same workspace.
    pub fn get(&self, id: &str, root: &Path) -> Option<Arc<Job>> {
        self.jobs
            .lock()
            .expect("job registry mutex poisoned")
            .iter()
            .find(|job| job.id == id && job.root == root)
            .cloned()
    }

    /// Every job started in `root`, oldest first.
    pub fn list(&self, root: &Path) -> Vec<Arc<Job>> {
        self.jobs
            .lock()
            .expect("job registry mutex poisoned")
            .iter()
            .filter(|job| job.root == root)
            .cloned()
            .collect()
    }
}

/// The process-wide job registry.
pub fn registry() -> &'static JobRegistry {
    static REGISTRY: OnceLock<JobRegistry> = OnceLock::new();
    REGISTRY.get_or_init(JobRegistry::new)
}

/// Start pumping `child`'s output into a new registered job.
///
/// Returns immediately; a detached task owns the child from here. The job's
/// [`ProcessTree`] lives as long as the job does, so a killed job takes its
/// descendants with it.
pub(crate) fn start(
    id: String,
    command: String,
    root: PathBuf,
    spill_path: String,
    mut child: Child,
    tree: ProcessTree,
) -> Arc<Job> {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let job = Arc::new(Job {
        id,
        command,
        root: root.clone(),
        spill_path: spill_path.clone(),
        started: Instant::now(),
        state: Mutex::new(JobState::default()),
        tree,
    });
    registry().insert(job.clone());

    let pumped = job.clone();
    let spill = root.join(&spill_path);
    tokio::spawn(async move {
        // One file, two streams: the sink is shared so interleaved stdout and
        // stderr writes cannot tear each other's text.
        let sink = Arc::new(tokio::sync::Mutex::new(SpillFile::create(&spill).await));
        let collector = async {
            tokio::join!(pump(stdout, &pumped, &sink), pump(stderr, &pumped, &sink),);
            child.wait().await
        };
        let status = match collector.await {
            Ok(status) => JobStatus::Exited(
                status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
            ),
            Err(_) => JobStatus::Killed,
        };
        sink.lock().await.flush().await;
        pumped.finish(status);
    });

    job
}

/// Read one stream to EOF, appending decoded text to the job and the spill file.
async fn pump<R>(stream: Option<R>, job: &Job, sink: &tokio::sync::Mutex<SpillFile>)
where
    R: AsyncRead + Unpin,
{
    let Some(stream) = stream else {
        return;
    };
    let mut reader = stream;
    let mut chunk = [0u8; 8 * 1024];
    // Bytes after the last newline in the previous chunk. Decoding whole lines
    // only keeps a multi-byte character from being split across two chunks and
    // decoded as two replacement characters.
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let read = match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        pending.extend_from_slice(&chunk[..read]);
        let Some(boundary) = pending.iter().rposition(|byte| *byte == b'\n') else {
            continue;
        };
        let rest = pending.split_off(boundary + 1);
        let text = decode_output(&pending);
        pending = rest;
        job.push(&text);
        sink.lock().await.write(&text).await;
    }
    if !pending.is_empty() {
        let text = decode_output(&pending);
        job.push(&text);
        sink.lock().await.write(&text).await;
    }
}

/// The complete output log for a job, best effort.
///
/// A spill failure must not take the job down: losing the durable copy is worth
/// far less than losing the build.
struct SpillFile {
    file: Option<tokio::fs::File>,
}

impl SpillFile {
    async fn create(path: &Path) -> Self {
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        Self {
            file: tokio::fs::File::create(path).await.ok(),
        }
    }

    async fn write(&mut self, text: &str) {
        use tokio::io::AsyncWriteExt;
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if file.write_all(text.as_bytes()).await.is_err() {
            self.file = None;
        }
    }

    async fn flush(&mut self) {
        use tokio::io::AsyncWriteExt;
        if let Some(file) = self.file.as_mut() {
            let _ = file.flush().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str) -> Job {
        Job {
            id: id.to_string(),
            command: "probe".to_string(),
            root: PathBuf::from("/root"),
            spill_path: ".ag-swarmer/shell/probe.log".to_string(),
            started: Instant::now(),
            state: Mutex::new(JobState::default()),
            tree: ProcessTree::detached(),
        }
    }

    #[test]
    fn reads_are_incremental_and_never_repeat() {
        let job = job("a");
        job.push("first\n");
        assert_eq!(job.read().text, "first\n");
        assert_eq!(job.read().text, "");
        job.push("second\n");
        assert_eq!(job.read().text, "second\n");
    }

    #[test]
    fn an_oversized_read_withholds_the_remainder_for_the_next_one() {
        let job = job("b");
        job.push(&"x".repeat(MAX_JOB_READ_CHARS + 500));
        let first = job.read();
        assert_eq!(first.text.chars().count(), MAX_JOB_READ_CHARS);
        assert_eq!(first.withheld, 500);
        let second = job.read();
        assert_eq!(second.text.chars().count(), 500);
        assert_eq!(second.withheld, 0);
    }

    #[test]
    fn overflow_drops_the_oldest_unread_text_and_reports_it() {
        let job = job("c");
        job.push("OLDEST");
        job.push(&"y".repeat(MAX_JOB_BUFFER_CHARS));
        let read = job.read();
        assert_eq!(read.dropped, 6, "the oldest six characters should be gone");
        assert!(!read.text.starts_with("OLDEST"));
        assert_eq!(job.total_chars(), MAX_JOB_BUFFER_CHARS + 6);
    }

    #[test]
    fn status_is_recorded_once() {
        let job = job("d");
        assert_eq!(job.status(), JobStatus::Running);
        job.finish(JobStatus::Exited("0".to_string()));
        job.finish(JobStatus::Killed);
        assert_eq!(job.status(), JobStatus::Exited("0".to_string()));
    }

    #[test]
    fn a_job_is_only_visible_to_its_own_workspace() {
        let registry = JobRegistry::new();
        registry.insert(Arc::new(job("e")));
        assert!(registry.get("e", Path::new("/root")).is_some());
        assert!(registry.get("e", Path::new("/elsewhere")).is_none());
        assert_eq!(registry.list(Path::new("/root")).len(), 1);
        assert!(registry.list(Path::new("/elsewhere")).is_empty());
    }
}
