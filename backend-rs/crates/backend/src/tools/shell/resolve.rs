//! Which interpreter runs a model-issued shell command.
//!
//! Resolution is a pure function of `(account preference, configured override,
//! environment, platform)` so the Windows probe order is testable from a POSIX
//! host and vice versa — the host facts arrive through [`ShellHost`] rather than
//! being read from the process.
//!
//! Left to itself, Windows prefers PowerShell over `cmd.exe`. The dialect is
//! part of the model-visible contract, not an implementation detail: a tool
//! advertised as `Bash` that actually runs `cmd.exe` teaches the model to write
//! `ls -la`, `grep -rn`, and `2>/dev/null` on a host that understands none of
//! them. An account that would rather run Git Bash — or is scripted against
//! `cmd.exe` — says so with a [`ShellPreference`], and every shell this app
//! starts follows it.

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

/// The interpreter an account asked this app to use.
///
/// Persisted on the account's system settings and honoured by both the agent
/// shell tool and the desktop app's integrated terminal, so a single choice
/// covers every shell the app starts. A preference that is not installed on the
/// host falls back to [`ShellPreference::Auto`] rather than failing the call: a
/// missing Git Bash should cost the agent its preferred dialect, not its ability
/// to run a command at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ShellPreference {
    /// Take whatever the host offers, best first.
    #[default]
    Auto,
    /// PowerShell 7, falling back to Windows PowerShell 5.1.
    PowerShell,
    /// A POSIX shell — Git for Windows' `bash.exe` on Windows.
    Bash,
    /// The Windows command interpreter.
    Cmd,
}

impl ShellPreference {
    /// Every preference, in the order the settings UI offers them.
    pub const ALL: [ShellPreference; 4] = [
        ShellPreference::Auto,
        ShellPreference::PowerShell,
        ShellPreference::Bash,
        ShellPreference::Cmd,
    ];

    /// The value stored in the database and sent over the API.
    pub fn as_str(self) -> &'static str {
        match self {
            ShellPreference::Auto => "auto",
            ShellPreference::PowerShell => "powershell",
            ShellPreference::Bash => "bash",
            ShellPreference::Cmd => "cmd",
        }
    }

    /// Parse a stored or submitted value, accepting the names a person is
    /// likely to type for the same shell. Unknown values are `None` so the API
    /// can reject them instead of silently resolving to `auto`.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "auto" | "default" => Some(ShellPreference::Auto),
            "powershell" | "pwsh" => Some(ShellPreference::PowerShell),
            "bash" | "gitbash" | "git-bash" | "git_bash" | "git bash" | "posix" => {
                Some(ShellPreference::Bash)
            }
            "cmd" | "cmd.exe" => Some(ShellPreference::Cmd),
            _ => None,
        }
    }

    fn index(self) -> usize {
        match self {
            ShellPreference::Auto => 0,
            ShellPreference::PowerShell => 1,
            ShellPreference::Bash => 2,
            ShellPreference::Cmd => 3,
        }
    }
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
pub const SHELL_OVERRIDE_VAR: &str = "QUNICA_SHELL";

/// Resolve the interpreter for `host` under `preference`.
///
/// Precedence: the account's explicit preference when that interpreter is
/// installed, then the [`SHELL_OVERRIDE_VAR`] escape hatch, then the platform
/// probe order. The preference outranks the environment variable on purpose —
/// the variable is set once when the app is launched, while the preference is
/// the choice the user just made in the settings UI.
///
/// Windows probe order: PowerShell 7 in its known install locations, `pwsh` on
/// `PATH`, Windows PowerShell 5.1, `powershell` on `PATH`, then `cmd.exe` as the
/// last resort. POSIX order: `bash`, then `sh`.
///
/// The final fallback in each branch is a bare program name rather than a
/// probed path: a host with an unusual layout still gets a spawn attempt (and a
/// real OS error if it fails) instead of a resolution failure here.
pub fn resolve_shell(host: &dyn ShellHost, preference: ShellPreference) -> ResolvedShell {
    if let Some(shell) = resolve_preferred(host, preference) {
        return shell;
    }

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

/// Locate the interpreter the account asked for.
///
/// `None` means "keep looking": either the account left the choice on `auto`,
/// or the shell it named is not installed on this host.
fn resolve_preferred(host: &dyn ShellHost, preference: ShellPreference) -> Option<ResolvedShell> {
    match preference {
        ShellPreference::Auto => None,
        ShellPreference::PowerShell => find_powershell(host),
        ShellPreference::Bash => find_posix_shell(host),
        ShellPreference::Cmd => find_cmd(host),
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
    find_powershell(host)
        .or_else(|| find_cmd(host))
        .unwrap_or(ResolvedShell {
            program: PathBuf::from("cmd.exe"),
            dialect: ShellDialect::Cmd,
        })
}

fn resolve_posix(host: &dyn ShellHost) -> ResolvedShell {
    find_posix_shell(host).unwrap_or(ResolvedShell {
        program: PathBuf::from("sh"),
        dialect: ShellDialect::Posix,
    })
}

/// PowerShell 7 first, then Windows PowerShell 5.1.
///
/// PowerShell 7 ships outside `System32`, so its install roots are probed
/// directly before falling back to `PATH`. `ProgramW6432` is checked too: a
/// 32-bit host process sees the 32-bit view in `ProgramFiles`.
fn find_powershell(host: &dyn ShellHost) -> Option<ResolvedShell> {
    if !host.is_windows() {
        // `pwsh` is cross-platform; an account that asked for it on Linux or
        // macOS gets it when it is installed.
        return host.lookup_path("pwsh").map(shell_at);
    }
    for (root_var, suffix) in [
        ("ProgramFiles", r"PowerShell\7\pwsh.exe"),
        ("ProgramW6432", r"PowerShell\7\pwsh.exe"),
        ("ProgramFiles(x86)", r"PowerShell\7\pwsh.exe"),
        ("LOCALAPPDATA", r"Microsoft\WindowsApps\pwsh.exe"),
    ] {
        if let Some(path) = probe(host, root_var, suffix) {
            return Some(shell_at(path));
        }
    }
    if let Some(path) = host.lookup_path("pwsh") {
        return Some(shell_at(path));
    }
    if let Some(path) = probe(
        host,
        "SystemRoot",
        r"System32\WindowsPowerShell\v1.0\powershell.exe",
    ) {
        return Some(shell_at(path));
    }
    host.lookup_path("powershell").map(shell_at)
}

fn find_cmd(host: &dyn ShellHost) -> Option<ResolvedShell> {
    if !host.is_windows() {
        return None;
    }
    probe(host, "SystemRoot", r"System32\cmd.exe")
        .or_else(|| host.lookup_path("cmd"))
        .map(shell_at)
}

/// Git for Windows' `bash.exe` on Windows, `bash` then `sh` everywhere else.
///
/// `bash` on `PATH` is not enough on Windows: `%SystemRoot%\System32\bash.exe`
/// is the WSL launcher, which runs the command inside a Linux distribution
/// where the workspace path does not exist and every write lands in a different
/// filesystem. Git's own install roots are probed first, then the tree `git.exe`
/// itself was found in (which covers an install on another drive, or a portable
/// or Scoop one), and a `PATH` hit inside `System32` is rejected outright.
fn find_posix_shell(host: &dyn ShellHost) -> Option<ResolvedShell> {
    if !host.is_windows() {
        for candidate in ["bash", "sh"] {
            if let Some(path) = host.lookup_path(candidate) {
                return Some(shell_at(path));
            }
            let absolute = PathBuf::from(format!("/bin/{candidate}"));
            if host.is_file(&absolute) {
                return Some(shell_at(absolute));
            }
        }
        return None;
    }

    for (root_var, suffix) in [
        ("ProgramFiles", r"Git\bin\bash.exe"),
        ("ProgramW6432", r"Git\bin\bash.exe"),
        ("ProgramFiles(x86)", r"Git\bin\bash.exe"),
        ("LOCALAPPDATA", r"Programs\Git\bin\bash.exe"),
    ] {
        if let Some(path) = probe(host, root_var, suffix) {
            return Some(shell_at(path));
        }
    }
    // An install on another drive — or a portable or Scoop one — is found
    // through `git` itself. Git for Windows puts `git.exe` in `<root>\cmd`,
    // `<root>\bin`, or `<root>\mingw64\bin`, so the search walks up from
    // wherever it was found until the shell that ships with it turns up.
    if let Some(git) = host.lookup_path("git") {
        let mut directory = git.parent();
        while let Some(current) = directory {
            if let Some(bash) = git_bash_under(host, current) {
                return Some(shell_at(bash));
            }
            directory = current.parent();
        }
    }
    host.lookup_path("bash")
        .filter(|path| !is_wsl_launcher(host, path))
        .map(shell_at)
}

/// The `bash.exe` a Git for Windows install keeps under `root`, if any.
///
/// `bin\bash.exe` is the launcher Git for Windows advertises as "Git Bash";
/// `usr\bin\bash.exe` is the MSYS shell it wraps, and is what a stripped-down or
/// portable install may ship on its own.
fn git_bash_under(host: &dyn ShellHost, root: &Path) -> Option<PathBuf> {
    for relative in [&["bin", "bash.exe"][..], &["usr", "bin", "bash.exe"][..]] {
        let candidate = relative
            .iter()
            .fold(root.to_path_buf(), |path, part| path.join(part));
        if host.is_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Is this `bash.exe` the WSL launcher shipped in `System32`?
fn is_wsl_launcher(host: &dyn ShellHost, path: &Path) -> bool {
    let Some(system_root) = host.var("SystemRoot") else {
        return false;
    };
    let system32 = Path::new(system_root.trim())
        .join("System32")
        .to_string_lossy()
        .to_ascii_lowercase();
    path.parent()
        .map(|parent| parent.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|parent| parent == system32)
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

/// The interpreter this process uses for `preference`, resolved once per
/// preference.
///
/// Resolution touches the filesystem, so each answer is cached: it cannot change
/// while the process runs, and every shell tool call would otherwise repeat the
/// same probe sequence. Caching per preference — rather than once for the whole
/// process — is what lets a settings change take effect on the next run without
/// a restart.
pub fn shell_for(preference: ShellPreference) -> &'static ResolvedShell {
    static CACHE: OnceLock<[OnceLock<ResolvedShell>; ShellPreference::ALL.len()]> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        [
            OnceLock::new(),
            OnceLock::new(),
            OnceLock::new(),
            OnceLock::new(),
        ]
    });
    cache[preference.index()].get_or_init(|| {
        let shell = resolve_shell(&ProcessHost, preference);
        tracing::info!(
            preference = preference.as_str(),
            program = %shell.program.display(),
            dialect = shell.dialect.label(),
            "resolved workspace shell"
        );
        shell
    })
}

/// The interpreter for this process when no account preference applies.
pub fn process_shell() -> &'static ResolvedShell {
    shell_for(ShellPreference::Auto)
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
        let shell = resolve_shell(&host, ShellPreference::Auto);
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
            resolve_shell(&on_path, ShellPreference::Auto).program,
            PathBuf::from(r"C:\tools\pwsh.exe")
        );

        let five_one = FakeHost::windows()
            .with_file(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
            .with_file(r"C:\Windows\System32\cmd.exe");
        let resolved = resolve_shell(&five_one, ShellPreference::Auto);
        assert_eq!(
            resolved.program,
            PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        );
        assert_eq!(resolved.dialect, ShellDialect::PowerShell);

        let cmd_only = FakeHost::windows().with_file(r"C:\Windows\System32\cmd.exe");
        let resolved = resolve_shell(&cmd_only, ShellPreference::Auto);
        assert_eq!(
            resolved.program,
            PathBuf::from(r"C:\Windows\System32\cmd.exe")
        );
        assert_eq!(resolved.dialect, ShellDialect::Cmd);
    }

    #[test]
    fn windows_without_any_probe_hit_still_returns_a_spawnable_name() {
        let resolved = resolve_shell(&FakeHost::windows(), ShellPreference::Auto);
        assert_eq!(resolved.program, PathBuf::from("cmd.exe"));
        assert_eq!(resolved.dialect, ShellDialect::Cmd);
    }

    #[test]
    fn posix_prefers_bash_then_sh() {
        let bash = FakeHost::posix().with_file("/bin/bash");
        let resolved = resolve_shell(&bash, ShellPreference::Auto);
        assert_eq!(resolved.program, PathBuf::from("/bin/bash"));
        assert_eq!(resolved.dialect, ShellDialect::Posix);

        let sh = FakeHost::posix().with_file("/bin/sh");
        assert_eq!(
            resolve_shell(&sh, ShellPreference::Auto).program,
            PathBuf::from("/bin/sh")
        );
    }

    #[test]
    fn explicit_override_wins_over_probing_on_either_platform() {
        let host = FakeHost::windows()
            .with_file(r"C:\Program Files\PowerShell\7\pwsh.exe")
            .with_file(r"C:\msys64\usr\bin\bash.exe")
            .with_var(SHELL_OVERRIDE_VAR, r"C:\msys64\usr\bin\bash.exe");
        let resolved = resolve_shell(&host, ShellPreference::Auto);
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
            resolve_shell(&host, ShellPreference::Auto).program,
            PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe")
        );
    }

    #[test]
    fn an_unresolvable_override_falls_through_to_normal_probing() {
        let host = FakeHost::windows()
            .with_file(r"C:\Program Files\PowerShell\7\pwsh.exe")
            .with_var(SHELL_OVERRIDE_VAR, r"C:\nope\missing.exe");
        assert_eq!(
            resolve_shell(&host, ShellPreference::Auto).program,
            PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe")
        );
    }

    #[test]
    fn an_account_preference_outranks_the_probe_order_and_the_override() {
        // PowerShell is present and would win on `auto`, and the escape-hatch
        // variable names it too — the choice made in the settings UI is the
        // more recent instruction, so it wins.
        let host = FakeHost::windows()
            .with_file(r"C:\Program Files\PowerShell\7\pwsh.exe")
            .with_file(r"C:\Program Files\Git\bin\bash.exe")
            .with_file(r"C:\Windows\System32\cmd.exe")
            .with_var(
                SHELL_OVERRIDE_VAR,
                r"C:\Program Files\PowerShell\7\pwsh.exe",
            );

        let bash = resolve_shell(&host, ShellPreference::Bash);
        assert_eq!(
            bash.program,
            PathBuf::from(r"C:\Program Files\Git\bin\bash.exe")
        );
        assert_eq!(bash.dialect, ShellDialect::Posix);

        let cmd = resolve_shell(&host, ShellPreference::Cmd);
        assert_eq!(cmd.program, PathBuf::from(r"C:\Windows\System32\cmd.exe"));
        assert_eq!(cmd.dialect, ShellDialect::Cmd);
    }

    #[test]
    fn a_preference_the_host_cannot_satisfy_falls_back_to_probing() {
        // No Git for Windows anywhere: the agent keeps a working shell rather
        // than losing the tool over an uninstalled preference.
        let host = FakeHost::windows().with_file(r"C:\Program Files\PowerShell\7\pwsh.exe");
        let resolved = resolve_shell(&host, ShellPreference::Bash);
        assert_eq!(
            resolved.program,
            PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe")
        );
        assert_eq!(resolved.dialect, ShellDialect::PowerShell);
    }

    #[test]
    fn git_bash_is_found_through_git_when_it_is_installed_off_the_known_roots() {
        // A Scoop or portable install: `git` is on `PATH` in `<root>\cmd`, and
        // `bash.exe` sits beside it in `<root>\bin`.
        let root = PathBuf::from(r"C:\Users\dev\scoop\apps\git\current");
        let git = root.join("cmd").join("git.exe");
        let bash = root.join("bin").join("bash.exe");
        let host = FakeHost::windows()
            .with_path_entry(&root.join("cmd").to_string_lossy())
            .with_file(&git.to_string_lossy())
            .with_file(&bash.to_string_lossy());

        assert_eq!(resolve_shell(&host, ShellPreference::Bash).program, bash);
    }

    #[test]
    fn git_bash_is_found_when_path_points_at_gits_mingw_directory() {
        // An install on another drive, with `PATH` carrying
        // `<root>\mingw64\bin` rather than `<root>\cmd`: the shell is three
        // levels above the `git.exe` that was found, not one.
        let root = PathBuf::from(r"D:\Install\Git");
        let git = root.join("mingw64").join("bin").join("git.exe");
        let bash = root.join("bin").join("bash.exe");
        let host = FakeHost::windows()
            .with_path_entry(&root.join("mingw64").join("bin").to_string_lossy())
            .with_file(&git.to_string_lossy())
            .with_file(&bash.to_string_lossy())
            .with_file(
                &root
                    .join("usr")
                    .join("bin")
                    .join("bash.exe")
                    .to_string_lossy(),
            );

        // `bin\bash.exe` is the launcher Git advertises, so it wins over the
        // MSYS shell under `usr\bin` that it wraps.
        assert_eq!(resolve_shell(&host, ShellPreference::Bash).program, bash);
    }

    #[test]
    fn the_wsl_launcher_is_never_mistaken_for_git_bash() {
        // `System32\bash.exe` starts a Linux distribution where the workspace
        // path does not exist, so it is rejected and the host falls back.
        let host = FakeHost::windows()
            .with_path_entry(r"C:\Windows\System32")
            .with_file(r"C:\Windows\System32\bash.exe")
            .with_file(r"C:\Windows\System32\cmd.exe");

        let resolved = resolve_shell(&host, ShellPreference::Bash);
        assert_eq!(
            resolved.program,
            PathBuf::from(r"C:\Windows\System32\cmd.exe")
        );
        assert_eq!(resolved.dialect, ShellDialect::Cmd);
    }

    #[test]
    fn a_posix_host_can_still_be_pinned_to_bash_or_pwsh() {
        let host = FakeHost::posix()
            .with_path_entry("/usr/bin")
            .with_file("/usr/bin/pwsh")
            .with_file("/bin/bash");

        assert_eq!(
            resolve_shell(&host, ShellPreference::PowerShell).dialect,
            ShellDialect::PowerShell
        );
        assert_eq!(
            resolve_shell(&host, ShellPreference::Bash).program,
            PathBuf::from("/bin/bash")
        );
        // There is no `cmd.exe` to honour off Windows, so the host decides.
        assert_eq!(
            resolve_shell(&host, ShellPreference::Cmd).program,
            PathBuf::from("/bin/bash")
        );
    }

    #[test]
    fn preferences_round_trip_through_their_stored_values() {
        for preference in ShellPreference::ALL {
            assert_eq!(
                ShellPreference::parse(preference.as_str()),
                Some(preference)
            );
        }
        // The names a person is likely to type for the same shell.
        assert_eq!(
            ShellPreference::parse("Git Bash"),
            Some(ShellPreference::Bash)
        );
        assert_eq!(
            ShellPreference::parse(" PWSH "),
            Some(ShellPreference::PowerShell)
        );
        assert_eq!(ShellPreference::parse(""), Some(ShellPreference::Auto));
        assert_eq!(ShellPreference::parse("fish"), None);
    }

    #[test]
    fn tool_name_and_dialect_travel_together() {
        assert_eq!(ShellDialect::PowerShell.tool_name(), "Pwsh");
        assert_eq!(ShellDialect::Cmd.tool_name(), "Cmd");
        assert_eq!(ShellDialect::Posix.tool_name(), "Bash");
    }
}
