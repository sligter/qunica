//! Which interpreter runs a model-issued shell command.
//!
//! Resolution is a pure function of `(configured override, environment,
//! platform)` so the Windows probe order is testable from a POSIX host and vice
//! versa — the host facts arrive through [`ShellHost`] rather than being read
//! from the process.
//!
//! Windows prefers PowerShell over `cmd.exe`. The dialect is part of the
//! model-visible contract, not an implementation detail: a tool advertised as
//! `Bash` that actually runs `cmd.exe` teaches the model to write `ls -la`,
//! `grep -rn`, and `2>/dev/null` on a host that understands none of them.

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// The command dialect an interpreter speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellDialect {
    /// PowerShell 7 (`pwsh`) or Windows PowerShell 5.1 (`powershell`).
    PowerShell,
    /// The Windows command interpreter.
    Cmd,
    /// A POSIX shell (`bash` or `sh`).
    Posix,
}

impl ShellDialect {
    /// The name this dialect's tool is exposed to the model under.
    ///
    /// Agents still enable the tool as `Bash` in their configuration; only the
    /// provider-facing name follows the host, so an existing agent config keeps
    /// working while the model sees a name that matches what will actually parse
    /// its command.
    pub fn tool_name(self) -> &'static str {
        match self {
            ShellDialect::PowerShell => "Pwsh",
            ShellDialect::Cmd => "Cmd",
            ShellDialect::Posix => "Bash",
        }
    }

    /// Short human/model-readable dialect label.
    pub fn label(self) -> &'static str {
        match self {
            ShellDialect::PowerShell => "PowerShell",
            ShellDialect::Cmd => "cmd.exe",
            ShellDialect::Posix => "POSIX shell",
        }
    }

    /// One line of dialect guidance appended to the tool description, so the
    /// model does not have to infer the dialect from the tool name alone.
    pub fn guidance(self) -> &'static str {
        match self {
            ShellDialect::PowerShell => {
                "Commands are parsed by PowerShell, not bash: use `Get-ChildItem`/`gci`, \
                 `Select-String`, `$env:VAR`, `Join-Path`, and `$null` instead of `ls`, `grep`, \
                 `$VAR`, and `/dev/null`. Statements are separated by `;`, and `&&`/`||` need \
                 PowerShell 7. Write file content with the Write and Edit tools rather than `>`: \
                 Windows PowerShell 5.1 redirects as UTF-16."
            }
            ShellDialect::Cmd => {
                "Commands are parsed by cmd.exe, not bash: use `dir`, `findstr`, `%VAR%`, `NUL`, \
                 and `&`/`&&` for chaining. POSIX utilities such as `ls`, `grep`, and `cat` are \
                 not available."
            }
            ShellDialect::Posix => {
                "Commands are parsed by a POSIX shell: ordinary bash syntax, pipes, and \
                 redirection apply."
            }
        }
    }
}

/// The interpreter chosen for this host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedShell {
    /// Absolute path when probing found one, otherwise a bare program name that
    /// the OS resolves through `PATH` at spawn time.
    pub program: PathBuf,
    pub dialect: ShellDialect,
}

/// Host facts consulted while resolving the shell.
///
/// Injected rather than read directly from the process so [`resolve_shell`] is a
/// pure function and each platform's probe order can be exercised from any test
/// host.
pub trait ShellHost {
    fn is_windows(&self) -> bool;
    fn var(&self, key: &str) -> Option<String>;
    fn is_file(&self, path: &Path) -> bool;
    /// Resolve `program` against `PATH`, applying `PATHEXT` on Windows.
    fn lookup_path(&self, program: &str) -> Option<PathBuf>;
}

/// Environment variable that overrides shell resolution with an explicit
/// interpreter — a path, or a name resolvable on `PATH`.
pub const SHELL_OVERRIDE_VAR: &str = "AG_SWARMER_SHELL";

/// Resolve the interpreter for `host`.
///
/// Windows order: the explicit override, PowerShell 7 in its known install
/// locations, `pwsh` on `PATH`, Windows PowerShell 5.1, `powershell` on `PATH`,
/// then `cmd.exe` as the last resort. POSIX order: the explicit override,
/// `bash`, then `sh`.
///
/// The final fallback in each branch is a bare program name rather than a
/// probed path: a host with an unusual layout still gets a spawn attempt (and a
/// real OS error if it fails) instead of a resolution failure here.
pub fn resolve_shell(host: &dyn ShellHost) -> ResolvedShell {
    if let Some(shell) = host
        .var(SHELL_OVERRIDE_VAR)
        .and_then(|configured| resolve_configured(host, &configured))
    {
        return shell;
    }

    if host.is_windows() {
        resolve_windows(host)
    } else {
        resolve_posix(host)
    }
}

/// Resolve an explicit override, which may be an absolute path or a bare name.
fn resolve_configured(host: &dyn ShellHost, configured: &str) -> Option<ResolvedShell> {
    let configured = configured.trim().trim_matches('"');
    if configured.is_empty() {
        return None;
    }
    let path = Path::new(configured);
    if host.is_file(path) {
        return Some(shell_at(path.to_path_buf()));
    }
    host.lookup_path(configured).map(shell_at)
}

fn resolve_windows(host: &dyn ShellHost) -> ResolvedShell {
    // PowerShell 7 ships outside `System32`, so its install roots are probed
    // directly before falling back to `PATH`. `ProgramW6432` is checked too: a
    // 32-bit host process sees the 32-bit view in `ProgramFiles`.
    for (root_var, suffix) in [
        ("ProgramFiles", r"PowerShell\7\pwsh.exe"),
        ("ProgramW6432", r"PowerShell\7\pwsh.exe"),
        ("ProgramFiles(x86)", r"PowerShell\7\pwsh.exe"),
        ("LOCALAPPDATA", r"Microsoft\WindowsApps\pwsh.exe"),
    ] {
        if let Some(path) = probe(host, root_var, suffix) {
            return shell_at(path);
        }
    }
    if let Some(path) = host.lookup_path("pwsh") {
        return shell_at(path);
    }
    if let Some(path) = probe(
        host,
        "SystemRoot",
        r"System32\WindowsPowerShell\v1.0\powershell.exe",
    ) {
        return shell_at(path);
    }
    if let Some(path) = host.lookup_path("powershell") {
        return shell_at(path);
    }
    if let Some(path) = probe(host, "SystemRoot", r"System32\cmd.exe") {
        return shell_at(path);
    }
    ResolvedShell {
        program: PathBuf::from("cmd.exe"),
        dialect: ShellDialect::Cmd,
    }
}

fn resolve_posix(host: &dyn ShellHost) -> ResolvedShell {
    for candidate in ["bash", "sh"] {
        if let Some(path) = host.lookup_path(candidate) {
            return shell_at(path);
        }
        let absolute = PathBuf::from(format!("/bin/{candidate}"));
        if host.is_file(&absolute) {
            return shell_at(absolute);
        }
    }
    ResolvedShell {
        program: PathBuf::from("sh"),
        dialect: ShellDialect::Posix,
    }
}

fn probe(host: &dyn ShellHost, root_var: &str, suffix: &str) -> Option<PathBuf> {
    let root = host.var(root_var)?;
    if root.trim().is_empty() {
        return None;
    }
    let path = Path::new(root.trim()).join(suffix);
    host.is_file(&path).then_some(path)
}

/// Pair a resolved program with the dialect its file name implies.
fn shell_at(program: PathBuf) -> ResolvedShell {
    let dialect = dialect_for(&program);
    ResolvedShell { program, dialect }
}

fn dialect_for(program: &Path) -> ShellDialect {
    let stem = program
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    match stem.as_str() {
        "pwsh" | "powershell" | "pwsh-preview" => ShellDialect::PowerShell,
        "cmd" => ShellDialect::Cmd,
        _ => ShellDialect::Posix,
    }
}

/// [`ShellHost`] backed by the running process.
pub struct ProcessHost;

impl ShellHost for ProcessHost {
    fn is_windows(&self) -> bool {
        cfg!(windows)
    }

    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn lookup_path(&self, program: &str) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        let extensions = executable_extensions(self);
        for entry in std::env::split_paths(&path) {
            // Windows `PATH` entries are frequently quoted; an unstripped quote
            // turns into a literal path component that never exists.
            let entry = PathBuf::from(entry.to_string_lossy().trim().trim_matches('"').to_string());
            if entry.as_os_str().is_empty() {
                continue;
            }
            for extension in &extensions {
                let candidate = entry.join(format!("{program}{extension}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    }
}

/// Suffixes to try when resolving a bare program name: just the name itself on
/// POSIX, plus each `PATHEXT` entry on Windows.
fn executable_extensions(host: &dyn ShellHost) -> Vec<String> {
    let mut extensions = vec![String::new()];
    if !host.is_windows() {
        return extensions;
    }
    let pathext = host
        .var("PATHEXT")
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
    for entry in pathext.split(';') {
        let entry = entry.trim();
        if !entry.is_empty() {
            extensions.push(entry.to_ascii_lowercase());
        }
    }
    extensions
}

/// The interpreter for this process, resolved once.
///
/// Resolution touches the filesystem, so it is cached: the answer cannot change
/// while the process runs, and every shell tool call would otherwise repeat the
/// same probe sequence.
pub fn process_shell() -> &'static ResolvedShell {
    static SHELL: OnceLock<ResolvedShell> = OnceLock::new();
    SHELL.get_or_init(|| {
        let shell = resolve_shell(&ProcessHost);
        tracing::info!(
            program = %shell.program.display(),
            dialect = shell.dialect.label(),
            "resolved workspace shell"
        );
        shell
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    /// A [`ShellHost`] built from literal facts, so either platform's probe
    /// order can be asserted from any test host.
    struct FakeHost {
        windows: bool,
        vars: HashMap<String, String>,
        files: HashSet<PathBuf>,
        path_entries: Vec<PathBuf>,
    }

    impl FakeHost {
        fn windows() -> Self {
            Self {
                windows: true,
                vars: HashMap::from([
                    ("ProgramFiles".to_string(), r"C:\Program Files".to_string()),
                    ("SystemRoot".to_string(), r"C:\Windows".to_string()),
                    (
                        "LOCALAPPDATA".to_string(),
                        r"C:\Users\dev\AppData\Local".to_string(),
                    ),
                ]),
                files: HashSet::new(),
                path_entries: Vec::new(),
            }
        }

        fn posix() -> Self {
            Self {
                windows: false,
                vars: HashMap::new(),
                files: HashSet::new(),
                path_entries: Vec::new(),
            }
        }

        fn with_file(mut self, path: &str) -> Self {
            self.files.insert(PathBuf::from(path));
            self
        }

        fn with_path_entry(mut self, path: &str) -> Self {
            self.path_entries.push(PathBuf::from(path));
            self
        }

        fn with_var(mut self, key: &str, value: &str) -> Self {
            self.vars.insert(key.to_string(), value.to_string());
            self
        }
    }

    impl ShellHost for FakeHost {
        fn is_windows(&self) -> bool {
            self.windows
        }

        fn var(&self, key: &str) -> Option<String> {
            self.vars.get(key).cloned()
        }

        fn is_file(&self, path: &Path) -> bool {
            self.files.contains(path)
        }

        fn lookup_path(&self, program: &str) -> Option<PathBuf> {
            let suffixes: &[&str] = if self.windows { &["", ".exe"] } else { &[""] };
            self.path_entries.iter().find_map(|entry| {
                suffixes.iter().find_map(|suffix| {
                    let candidate = entry.join(format!("{program}{suffix}"));
                    self.files.contains(&candidate).then_some(candidate)
                })
            })
        }
    }

    #[test]
    fn windows_prefers_powershell_seven_over_everything_else() {
        let host = FakeHost::windows()
            .with_file(r"C:\Program Files\PowerShell\7\pwsh.exe")
            .with_file(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
            .with_file(r"C:\Windows\System32\cmd.exe");
        let shell = resolve_shell(&host);
        assert_eq!(
            shell.program,
            PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe")
        );
        assert_eq!(shell.dialect, ShellDialect::PowerShell);
    }

    #[test]
    fn windows_falls_back_through_path_then_five_one_then_cmd() {
        let on_path = FakeHost::windows()
            .with_path_entry(r"C:\tools")
            .with_file(r"C:\tools\pwsh.exe");
        assert_eq!(
            resolve_shell(&on_path).program,
            PathBuf::from(r"C:\tools\pwsh.exe")
        );

        let five_one = FakeHost::windows()
            .with_file(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
            .with_file(r"C:\Windows\System32\cmd.exe");
        let resolved = resolve_shell(&five_one);
        assert_eq!(
            resolved.program,
            PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        );
        assert_eq!(resolved.dialect, ShellDialect::PowerShell);

        let cmd_only = FakeHost::windows().with_file(r"C:\Windows\System32\cmd.exe");
        let resolved = resolve_shell(&cmd_only);
        assert_eq!(
            resolved.program,
            PathBuf::from(r"C:\Windows\System32\cmd.exe")
        );
        assert_eq!(resolved.dialect, ShellDialect::Cmd);
    }

    #[test]
    fn windows_without_any_probe_hit_still_returns_a_spawnable_name() {
        let resolved = resolve_shell(&FakeHost::windows());
        assert_eq!(resolved.program, PathBuf::from("cmd.exe"));
        assert_eq!(resolved.dialect, ShellDialect::Cmd);
    }

    #[test]
    fn posix_prefers_bash_then_sh() {
        let bash = FakeHost::posix().with_file("/bin/bash");
        let resolved = resolve_shell(&bash);
        assert_eq!(resolved.program, PathBuf::from("/bin/bash"));
        assert_eq!(resolved.dialect, ShellDialect::Posix);

        let sh = FakeHost::posix().with_file("/bin/sh");
        assert_eq!(resolve_shell(&sh).program, PathBuf::from("/bin/sh"));
    }

    #[test]
    fn explicit_override_wins_over_probing_on_either_platform() {
        let host = FakeHost::windows()
            .with_file(r"C:\Program Files\PowerShell\7\pwsh.exe")
            .with_file(r"C:\msys64\usr\bin\bash.exe")
            .with_var(SHELL_OVERRIDE_VAR, r"C:\msys64\usr\bin\bash.exe");
        let resolved = resolve_shell(&host);
        assert_eq!(
            resolved.program,
            PathBuf::from(r"C:\msys64\usr\bin\bash.exe")
        );
        assert_eq!(resolved.dialect, ShellDialect::Posix);
    }

    #[test]
    fn quoted_override_is_unquoted_before_probing() {
        let host = FakeHost::windows()
            .with_file(r"C:\Program Files\PowerShell\7\pwsh.exe")
            .with_var(
                SHELL_OVERRIDE_VAR,
                "\"C:\\Program Files\\PowerShell\\7\\pwsh.exe\"",
            );
        assert_eq!(
            resolve_shell(&host).program,
            PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe")
        );
    }

    #[test]
    fn an_unresolvable_override_falls_through_to_normal_probing() {
        let host = FakeHost::windows()
            .with_file(r"C:\Program Files\PowerShell\7\pwsh.exe")
            .with_var(SHELL_OVERRIDE_VAR, r"C:\nope\missing.exe");
        assert_eq!(
            resolve_shell(&host).program,
            PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe")
        );
    }

    #[test]
    fn tool_name_and_dialect_travel_together() {
        assert_eq!(ShellDialect::PowerShell.tool_name(), "Pwsh");
        assert_eq!(ShellDialect::Cmd.tool_name(), "Cmd");
        assert_eq!(ShellDialect::Posix.tool_name(), "Bash");
    }
}
