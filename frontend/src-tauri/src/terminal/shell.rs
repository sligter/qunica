use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use qunica_backend::tools::{shell_for, ResolvedShell, ShellDialect, ShellPreference};

use super::protocol::TerminalCommandError;

#[derive(Debug, Clone)]
pub struct ShellSpec {
    pub program: PathBuf,
    pub display_name: String,
}

pub fn validate_launch_directory(path: &Path) -> Result<PathBuf, TerminalCommandError> {
    if !path.is_absolute() {
        return Err(TerminalCommandError::new(
            "terminal.cwd_not_absolute",
            format!(
                "Terminal launch directory must be absolute: {}",
                path.display()
            ),
        ));
    }

    let canonical = fs::canonicalize(path).map_err(|_| {
        TerminalCommandError::new(
            "terminal.cwd_missing",
            format!(
                "Terminal launch directory does not exist: {}",
                path.display()
            ),
        )
    })?;
    #[cfg(windows)]
    let canonical = without_windows_verbatim_prefix(canonical);

    if !canonical.is_dir() {
        return Err(TerminalCommandError::new(
            "terminal.cwd_not_directory",
            format!(
                "Terminal launch path is not a directory: {}",
                path.display()
            ),
        ));
    }

    Ok(canonical)
}

#[cfg(windows)]
fn without_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    const VERBATIM_PREFIX: &[u16] = &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    const VERBATIM_UNC_PREFIX: &[u16] = &[
        b'\\' as u16,
        b'\\' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];

    let encoded = path.as_os_str().encode_wide().collect::<Vec<_>>();
    let normalized = if let Some(rest) = encoded.strip_prefix(VERBATIM_UNC_PREFIX) {
        let mut unc = vec![b'\\' as u16, b'\\' as u16];
        unc.extend_from_slice(rest);
        unc
    } else if let Some(rest) = encoded.strip_prefix(VERBATIM_PREFIX) {
        rest.to_vec()
    } else {
        return path;
    };

    PathBuf::from(OsString::from_wide(&normalized))
}

/// The shell a new terminal tab starts, honouring the account's preference.
///
/// The preference is resolved by the same code the agent shell tool uses, so a
/// tab and a tool call on one machine cannot end up in different interpreters.
/// A preference this host cannot satisfy falls back to the terminal's own
/// default, which — unlike the tool's — starts from `$SHELL`.
pub fn resolve_default_shell(
    preference: ShellPreference,
) -> Result<ShellSpec, TerminalCommandError> {
    if let Some(spec) = preferred_shell(preference, cfg!(windows)) {
        return Ok(spec);
    }
    resolve_shell_with(cfg!(windows), std::env::var_os("SHELL"), locate_on_path)
}

/// The account's chosen interpreter, when this host actually has it.
///
/// The shared resolver falls back to the host probe order when the choice is not
/// installed. That fallback is right for the agent tool and wrong here, so a
/// resolution that did not produce the requested dialect is discarded and the
/// caller takes the terminal's own default instead.
fn preferred_shell(preference: ShellPreference, windows: bool) -> Option<ShellSpec> {
    let requested = match preference {
        ShellPreference::Auto => return None,
        ShellPreference::PowerShell => ShellDialect::PowerShell,
        ShellPreference::Bash => ShellDialect::Posix,
        ShellPreference::Cmd => ShellDialect::Cmd,
    };
    let resolved = shell_for(preference);
    (resolved.dialect == requested).then(|| ShellSpec {
        program: resolved.program.clone(),
        display_name: display_name(resolved, windows),
    })
}

fn display_name(shell: &ResolvedShell, windows: bool) -> String {
    match shell.dialect {
        ShellDialect::PowerShell => "PowerShell".to_string(),
        ShellDialect::Cmd => "Command Prompt".to_string(),
        // A POSIX shell on Windows is Git for Windows' bash, and calling it
        // "bash" in the tab strip would hide which one is running.
        ShellDialect::Posix if windows => "Git Bash".to_string(),
        ShellDialect::Posix => shell
            .program
            .file_stem()
            .unwrap_or(shell.program.as_os_str())
            .to_string_lossy()
            .into_owned(),
    }
}

pub(crate) fn resolve_shell_with(
    windows: bool,
    shell_env: Option<OsString>,
    mut locate: impl FnMut(&OsStr) -> Option<PathBuf>,
) -> Result<ShellSpec, TerminalCommandError> {
    if windows {
        for (program, display_name) in [
            ("pwsh", "PowerShell"),
            ("powershell", "PowerShell"),
            ("cmd", "Command Prompt"),
        ] {
            if let Some(program) = locate(OsStr::new(program)) {
                return Ok(ShellSpec {
                    program,
                    display_name: display_name.to_string(),
                });
            }
        }
    } else {
        let candidates = shell_env.into_iter().chain([
            OsString::from("/bin/zsh"),
            OsString::from("/bin/bash"),
            OsString::from("/bin/sh"),
        ]);

        for candidate in candidates {
            if let Some(program) = locate(&candidate) {
                let display_name = program
                    .file_name()
                    .unwrap_or(program.as_os_str())
                    .to_string_lossy()
                    .into_owned();
                return Ok(ShellSpec {
                    program,
                    display_name,
                });
            }
        }
    }

    Err(TerminalCommandError::new(
        "terminal.shell_not_found",
        "No supported terminal shell was found",
    ))
}

fn locate_on_path(program: &OsStr) -> Option<PathBuf> {
    let requested = Path::new(program);
    if requested.components().count() > 1 {
        return is_executable(requested).then(|| requested.to_path_buf());
    }

    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(requested);
        if is_executable(&candidate) {
            return Some(candidate);
        }

        #[cfg(windows)]
        if candidate.extension().is_none() {
            let path_ext = std::env::var_os("PATHEXT")
                .unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
            for extension in path_ext.to_string_lossy().split(';') {
                let extension = extension.trim().trim_start_matches('.');
                if extension.is_empty() {
                    continue;
                }
                let candidate_with_extension = candidate.with_extension(extension);
                if is_executable(&candidate_with_extension) {
                    return Some(candidate_with_extension);
                }
            }
        }
    }

    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::{resolve_shell_with, validate_launch_directory};
    use std::{collections::HashMap, ffi::OsString, fs};

    #[test]
    fn windows_prefers_pwsh_then_powershell_then_cmd() {
        let found = HashMap::from([
            ("pwsh".to_string(), "C:/Tools/pwsh.exe".into()),
            (
                "powershell".to_string(),
                "C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe".into(),
            ),
            ("cmd".to_string(), "C:/Windows/System32/cmd.exe".into()),
        ]);
        let shell = resolve_shell_with(true, None, |name| {
            found.get(&name.to_string_lossy().to_string()).cloned()
        })
        .expect("pwsh should resolve");
        assert_eq!(shell.display_name, "PowerShell");
        assert!(shell.program.ends_with("pwsh.exe"));
    }

    #[test]
    fn unix_prefers_valid_shell_environment() {
        let shell = resolve_shell_with(false, Some(OsString::from("/opt/bin/fish")), |name| {
            (name == "/opt/bin/fish").then(|| "/opt/bin/fish".into())
        })
        .expect("SHELL should resolve");
        assert_eq!(shell.display_name, "fish");
    }

    #[test]
    fn launch_directory_must_be_an_existing_absolute_directory() {
        let root = std::env::temp_dir().join(format!("qunica-terminal-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create temp directory");
        assert_eq!(validate_launch_directory(&root).unwrap(), root);
        assert!(validate_launch_directory(std::path::Path::new("relative")).is_err());
        fs::remove_dir_all(&root).expect("remove temp directory");
    }

    #[test]
    fn launch_directory_errors_have_stable_codes() {
        let relative = validate_launch_directory(std::path::Path::new("relative"))
            .expect_err("relative path should fail");
        assert_eq!(relative.code, "terminal.cwd_not_absolute");

        let root =
            std::env::temp_dir().join(format!("qunica-terminal-errors-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create temp directory");

        let missing =
            validate_launch_directory(&root.join("missing")).expect_err("missing path should fail");
        assert_eq!(missing.code, "terminal.cwd_missing");

        let file = root.join("launch.txt");
        fs::write(&file, b"not a directory").expect("create temp file");
        let not_directory = validate_launch_directory(&file).expect_err("file path should fail");
        assert_eq!(not_directory.code, "terminal.cwd_not_directory");

        fs::remove_dir_all(&root).expect("remove temp directory");
    }
}
