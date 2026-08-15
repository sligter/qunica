//! What the shell tool refuses to run, what it asks about, and why.
//!
//! Three verdicts, not two. A pure denylist could only ever say no, which is the
//! wrong answer for most of what it matched: deleting a build directory and
//! discarding a botched rebase are ordinary work, and an agent that cannot do
//! them just asks the human to do it by hand. So the destructive rules are split
//! by whether a human could sensibly authorise them:
//!
//! * [`CommandVerdict::Ask`] — legitimate but destructive. The turn pauses and
//!   the user approves or declines; see [`crate::runtime::approval`].
//! * [`CommandVerdict::Deny`] — never part of a development workflow, and a
//!   click cannot make it safe (formatting a volume, powering off the host,
//!   writing raw bytes over a device).
//!
//! Two independent checks run, in order:
//!
//! 1. The destructive-verb rules, matched against the lowercased command.
//! 2. Redirection containment, using the dialect-aware lexer in [`super::lex`].
//!
//! The denylist is a **union across dialects**, not a per-dialect set. That is
//! deliberate: any interpreter can start another one (`cmd` running
//! `powershell -c`, `pwsh` running `bash -lc`), so gating `Remove-Item` on
//! "the resolved shell is PowerShell" would only teach a caller which wrapper to
//! reach for. Dialect awareness belongs where parsing happens — the lexer — not
//! where verbs are matched.
//!
//! # Honest limits
//!
//! A denylist over a command string is a speed bump, not a boundary. Anything
//! this module blocks is reachable through a program the shell starts:
//! `python -c "shutil.rmtree(...)"`, `node -e`, a `package.json` script, a
//! `Makefile` target. Treating it as a security control would be a mistake. Its
//! job is to stop an agent from destroying work by accident and to hand back a
//! reason the model can act on. Confinement that actually holds needs an
//! OS-level boundary around the child process.

use std::{path::Path, sync::OnceLock};

use regex::Regex;

use super::{
    lex::{redirect_targets, RedirectTarget},
    resolve::ShellDialect,
};
use crate::tools::resolve_workspace_path;

/// The outcome of reviewing a command before it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandVerdict {
    Allow,
    /// Needs a human decision before it runs.
    Ask {
        /// Stable id of the rule that asked. A remembered approval is keyed on
        /// this rather than on the command text, so approving `rm build` also
        /// covers `rm dist` later in the same thread — which is what a user who
        /// chose "allow for this thread" meant.
        rule: &'static str,
        /// Short name of the capability being granted, for the approval card.
        capability: &'static str,
        /// Why this command needs a decision, in model- and user-facing terms.
        detail: String,
    },
    /// Rejected outright. `reason` is model-facing text that always contains the
    /// word "blocked" and says what to do instead.
    Deny {
        reason: String,
    },
}

impl CommandVerdict {
    fn deny(detail: impl std::fmt::Display) -> Self {
        CommandVerdict::Deny {
            reason: format!("command is blocked by workspace safety policy: {detail}"),
        }
    }
}

/// What a rule does when it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    /// Pause for a human decision, keyed on `rule` and named by `capability`.
    Ask {
        rule: &'static str,
        capability: &'static str,
    },
    /// Refuse; no approval is offered.
    Deny,
}

/// Rule id for a redirection that leaves the workspace. Not a pattern rule — it
/// comes out of the lexer — but it is granted and remembered the same way.
pub const RULE_WRITE_OUTSIDE_WORKSPACE: &str = "write-outside-workspace";

/// One policy entry: what it matches, what it does, and what the reader is told.
struct Rule {
    pattern: Regex,
    disposition: Disposition,
    detail: &'static str,
}

fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        // Matched at command position: the start of the command, or just after a
        // separator, allowing a privilege/prefix wrapper and a leading path.
        const AT_COMMAND: &str =
            r"(?:^|[;&|\n])\s*(?:sudo\s+|command\s+|builtin\s+|env\s+)*(?:[\w./-]*[/\\])?";
        const DELETE_FILES: Disposition = Disposition::Ask {
            rule: "delete-files",
            capability: "delete files in this workspace",
        };
        const GIT_DISCARD: Disposition = Disposition::Ask {
            rule: "git-discard",
            capability: "discard uncommitted or untracked work",
        };
        [
            (
                format!(r"{AT_COMMAND}(?:rmdir|unlink|erase|rm|del|rd)\b"),
                DELETE_FILES,
                "it deletes files, which cannot be undone from here.",
            ),
            (
                r"\b(?:remove-item|remove-itemproperty|clear-content|clear-item)\b".to_string(),
                DELETE_FILES,
                "it deletes or truncates files, which cannot be undone from here.",
            ),
            (
                r"\bgit\s+reset\s+--hard\b".to_string(),
                GIT_DISCARD,
                "it discards uncommitted work. `git stash` would make the same change \
                 recoverable.",
            ),
            (
                r"\bgit\s+clean\b".to_string(),
                GIT_DISCARD,
                "it deletes untracked files, which are not recoverable from git.",
            ),
            (
                r"\bgit\s+push\b[^\n]*\s--force(?:\b|-with-lease\b)".to_string(),
                Disposition::Ask {
                    rule: "git-force-push",
                    capability: "rewrite published history on a remote",
                },
                "it rewrites history other people may have pulled.",
            ),
            // Below: nothing a click can make safe, so no approval is offered.
            (
                format!(r"{AT_COMMAND}(?:format|mkfs(?:\.\w+)?|diskpart)\b"),
                Disposition::Deny,
                "it formats a volume, which is not part of any workspace task.",
            ),
            (
                r"\b(?:shutdown|reboot|halt|poweroff|stop-computer|restart-computer)\b".to_string(),
                Disposition::Deny,
                "it powers off or restarts the host.",
            ),
            (
                r"\bdd\b[^\n]*\bof=".to_string(),
                Disposition::Deny,
                "it writes raw bytes over a device or file.",
            ),
        ]
        .into_iter()
        .map(|(pattern, disposition, detail)| Rule {
            pattern: Regex::new(&pattern).expect("static command policy pattern must compile"),
            disposition,
            detail,
        })
        .collect()
    })
}

/// Review `command` before it is handed to `dialect`'s interpreter in `root`.
///
/// `granted` holds the rule ids the user has already approved for this thread; a
/// rule listed there does not ask again.
pub fn review(
    command: &str,
    dialect: ShellDialect,
    root: &Path,
    granted: &dyn Fn(&str) -> bool,
) -> CommandVerdict {
    if command.trim().is_empty() {
        return CommandVerdict::Deny {
            reason: "command must be non-empty".to_string(),
        };
    }

    let lowered = command.to_lowercase();
    // A hard denial wins over an approvable one no matter what order the rules
    // matched in: `rm -rf build; shutdown /s` must not become runnable because
    // file deletion was approved earlier in the thread.
    let mut pending_ask: Option<CommandVerdict> = None;
    for rule in rules() {
        if !rule.pattern.is_match(&lowered) {
            continue;
        }
        match rule.disposition {
            Disposition::Deny => return CommandVerdict::deny(rule.detail),
            Disposition::Ask {
                rule: id,
                capability,
            } => {
                if granted(id) || pending_ask.is_some() {
                    continue;
                }
                pending_ask = Some(CommandVerdict::Ask {
                    rule: id,
                    capability,
                    detail: rule.detail.to_string(),
                });
            }
        }
    }

    for target in redirect_targets(command, dialect) {
        match target {
            RedirectTarget::Literal(path) => {
                if resolve_workspace_path(root, &path).is_err() {
                    if granted(RULE_WRITE_OUTSIDE_WORKSPACE) {
                        continue;
                    }
                    return CommandVerdict::Ask {
                        rule: RULE_WRITE_OUTSIDE_WORKSPACE,
                        capability: "write outside the workspace",
                        detail: format!(
                            "it redirects output to `{path}`, which is outside the workspace root."
                        ),
                    };
                }
            }
            RedirectTarget::Unresolvable(raw) => {
                if granted(RULE_WRITE_OUTSIDE_WORKSPACE) {
                    continue;
                }
                return CommandVerdict::Ask {
                    rule: RULE_WRITE_OUTSIDE_WORKSPACE,
                    capability: "write outside the workspace",
                    detail: format!(
                        "its redirection target `{raw}` is only known after the shell expands \
                         it, so it cannot be checked against the workspace root."
                    ),
                };
            }
        }
    }

    pending_ask.unwrap_or(CommandVerdict::Allow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// No rule has been approved yet — the state a fresh thread is in.
    fn nothing_granted(_rule: &str) -> bool {
        false
    }

    fn verdict(command: &str, dialect: ShellDialect) -> CommandVerdict {
        let root = tempdir().unwrap();
        review(command, dialect, root.path(), &nothing_granted)
    }

    fn denied(command: &str, dialect: ShellDialect) -> String {
        match verdict(command, dialect) {
            CommandVerdict::Deny { reason } => {
                assert!(
                    reason.contains("blocked") || reason.contains("non-empty"),
                    "{reason}"
                );
                reason
            }
            other => panic!("expected `{command}` to be denied, got {other:?}"),
        }
    }

    fn asked(command: &str, dialect: ShellDialect) -> (&'static str, String) {
        match verdict(command, dialect) {
            CommandVerdict::Ask { rule, detail, .. } => (rule, detail),
            other => panic!("expected `{command}` to need approval, got {other:?}"),
        }
    }

    fn allowed(command: &str, dialect: ShellDialect) {
        assert_eq!(
            verdict(command, dialect),
            CommandVerdict::Allow,
            "expected `{command}` to be allowed"
        );
    }

    #[test]
    fn destructive_but_legitimate_work_asks_instead_of_refusing() {
        // Every one of these is ordinary development work. Refusing outright
        // just moved the job to the human; asking keeps the agent useful.
        for (command, expected_rule) in [
            ("rm -rf build", "delete-files"),
            ("del file.txt", "delete-files"),
            ("rmdir target", "delete-files"),
            ("powershell Remove-Item secret.txt", "delete-files"),
            ("Remove-Item -Recurse -Force src", "delete-files"),
            ("ls; rm -rf /", "delete-files"),
            ("git reset --hard HEAD", "git-discard"),
            ("git clean -fd", "git-discard"),
            ("git push origin main --force", "git-force-push"),
        ] {
            let (rule, detail) = asked(command, ShellDialect::PowerShell);
            assert_eq!(rule, expected_rule, "for `{command}`");
            assert!(detail.len() > 30, "reason should explain itself: {detail}");
        }
    }

    #[test]
    fn a_granted_rule_stops_asking() {
        let root = tempdir().unwrap();
        let granted = |rule: &str| rule == "delete-files";
        assert_eq!(
            review("rm -rf build", ShellDialect::Posix, root.path(), &granted),
            CommandVerdict::Allow
        );
        // A different capability still asks: one grant is not a blanket one.
        assert!(matches!(
            review(
                "git push origin main --force",
                ShellDialect::Posix,
                root.path(),
                &granted
            ),
            CommandVerdict::Ask {
                rule: "git-force-push",
                ..
            }
        ));
    }

    #[test]
    fn host_level_operations_are_refused_with_no_approval_offered() {
        for command in [
            "shutdown /s /t 0",
            "mkfs.ext4 /dev/sda1",
            "diskpart",
            "dd if=/dev/zero of=/dev/sda",
        ] {
            let reason = denied(command, ShellDialect::Posix);
            assert!(reason.len() > 40, "reason should explain itself: {reason}");
        }
    }

    #[test]
    fn a_hard_denial_survives_an_approved_rule_in_the_same_command() {
        // Approving file deletion must not make a chained `shutdown` runnable.
        let root = tempdir().unwrap();
        let granted = |rule: &str| rule == "delete-files";
        assert!(matches!(
            review(
                "rm -rf build; shutdown /s /t 0",
                ShellDialect::Posix,
                root.path(),
                &granted
            ),
            CommandVerdict::Deny { .. }
        ));
        // And without the grant, the denial still wins over the question.
        assert!(matches!(
            verdict("rm -rf build; shutdown /s /t 0", ShellDialect::Posix),
            CommandVerdict::Deny { .. }
        ));
    }

    #[test]
    fn ordinary_commands_are_allowed() {
        for command in [
            "cargo build --release",
            "npm run format",
            "git status --short",
            "Get-ChildItem -Recurse src",
            "echo workspace_probe > probe.txt",
            "type big.txt",
            "echo it's fine",
            "cargo test 2>&1",
        ] {
            allowed(command, ShellDialect::PowerShell);
        }
    }

    #[test]
    fn an_empty_command_is_rejected() {
        denied("   ", ShellDialect::Posix);
    }

    #[test]
    fn redirection_outside_the_workspace_asks_in_every_dialect() {
        for dialect in [
            ShellDialect::PowerShell,
            ShellDialect::Cmd,
            ShellDialect::Posix,
        ] {
            let (rule, _) = asked("echo hi > ../escape.txt", dialect);
            assert_eq!(rule, RULE_WRITE_OUTSIDE_WORKSPACE);
        }
    }

    #[test]
    fn the_posix_lexing_bypass_is_closed() {
        // POSIX lexing collapsed this target to `....evil.txt`, which carried no
        // `..` segment and passed containment while the shell wrote two levels
        // above the workspace. It is still caught — now as a question rather
        // than a refusal, since the user can legitimately allow it.
        let (rule, detail) = asked(r"echo x > ..\..\evil.txt", ShellDialect::PowerShell);
        assert_eq!(rule, RULE_WRITE_OUTSIDE_WORKSPACE);
        assert!(detail.contains("outside the workspace"), "{detail}");
        asked(r"echo x > ..\..\evil.txt", ShellDialect::Cmd);
    }

    #[test]
    fn redirecting_inside_the_workspace_is_allowed() {
        let root = tempdir().unwrap();
        std::fs::create_dir(root.path().join("sub")).unwrap();
        assert_eq!(
            review(
                r"type a.txt > sub\out.txt",
                ShellDialect::Cmd,
                root.path(),
                &nothing_granted
            ),
            CommandVerdict::Allow
        );
        assert_eq!(
            review(
                "cat a.txt > sub/out.txt",
                ShellDialect::Posix,
                root.path(),
                &nothing_granted
            ),
            CommandVerdict::Allow
        );
    }

    #[test]
    fn an_expanded_redirection_target_fails_closed() {
        let (rule, detail) = asked(r"echo a > $env:TEMP\x.txt", ShellDialect::PowerShell);
        assert_eq!(rule, RULE_WRITE_OUTSIDE_WORKSPACE);
        assert!(detail.contains("expands"), "{detail}");
        asked(r"echo a > %TEMP%\x.txt", ShellDialect::Cmd);
    }
}
