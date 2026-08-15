//! Dialect-aware command lexing, used only to locate redirection targets.
//!
//! # Why this is not `shlex`
//!
//! The previous guard lexed every command with POSIX rules, including on
//! Windows. POSIX lexing treats `\` as an escape character, so a `cmd.exe`
//! command line was mangled before it was inspected:
//!
//! | command                        | POSIX lexing yields | actually written to |
//! |--------------------------------|---------------------|---------------------|
//! | `echo x > ..\..\evil.txt`      | `....evil.txt`      | `..\..\evil.txt`    |
//! | `type a.txt > sub\out.txt`     | `subout.txt`        | `sub\out.txt`       |
//!
//! The first row is a guard bypass: the mangled token carries no `..` segment,
//! so the containment check passed while the shell wrote two directories above
//! the workspace. POSIX lexing also rejected legitimate commands outright —
//! `echo it's fine` has an unbalanced quote by POSIX rules and failed to lex at
//! all.
//!
//! [`redirect_targets`] lexes with the rules of the dialect that will actually
//! parse the command, and is consulted only when a redirection operator is
//! present, so a command with no `>` is never subject to lexing at all.
//!
//! # What this is not
//!
//! This is a redirection check, not a sandbox. An interpreter can write outside
//! the workspace through any program it starts (`python -c`, `Copy-Item`,
//! `git -C`), and no amount of command-string inspection changes that. Real
//! confinement needs an OS-level boundary — a restricted token or job-object
//! ACL on Windows, Landlock or a bind-mount namespace on Unix. What this buys is
//! that the single most common accident, an unanchored `>` path, is caught
//! before it lands.

use super::resolve::ShellDialect;

/// A redirection target found in a command, or the reason one could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedirectTarget {
    /// A literal path the caller can validate against the workspace root.
    Literal(String),
    /// A target that cannot be known before the shell expands it (a variable, a
    /// subexpression, or a command that failed to lex). Callers must treat this
    /// as unresolvable rather than assume it is safe.
    Unresolvable(String),
}

/// Whether `command` contains a redirection operator at all.
///
/// Cheap pre-check so a command with no `>` never reaches the lexer, which is
/// what keeps a quoting quirk in this module from rejecting an ordinary command.
pub fn has_redirect(command: &str) -> bool {
    command.contains('>')
}

/// Every redirection target in `command`, lexed with `dialect`'s quoting rules.
///
/// Returns an empty vector when the command redirects nothing. A `>&1`-style
/// stream duplication is not a file target and is skipped.
pub fn redirect_targets(command: &str, dialect: ShellDialect) -> Vec<RedirectTarget> {
    if !has_redirect(command) {
        return Vec::new();
    }
    let tokens = lex(command, dialect);
    let mut targets = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        index += 1;
        if token.quoted {
            continue;
        }
        let Some(tail) = split_redirect(&token.text) else {
            continue;
        };
        // `> out` puts the target in the next token; `>out` fuses them.
        let target = if tail.is_empty() {
            match tokens.get(index) {
                Some(next) => {
                    index += 1;
                    next.text.clone()
                }
                // A trailing operator with nothing after it is a syntax error the
                // shell will reject; there is no target to check.
                None => continue,
            }
        } else {
            tail.to_string()
        };
        // `>&1`, `>&2`: duplicating a stream, not opening a file.
        if target.starts_with('&') {
            continue;
        }
        if target.trim().is_empty() {
            continue;
        }
        targets.push(classify_target(&target, dialect));
    }
    targets
}

/// Split a token into its redirection operator and whatever was fused onto it.
///
/// Recognises the operator forms every supported dialect shares — `>`, `>>`,
/// with an optional stream selector (`2>`, `&>`, and PowerShell's `*>`).
fn split_redirect(token: &str) -> Option<&str> {
    let rest = token
        .strip_prefix(|c: char| c.is_ascii_digit() || c == '&' || c == '*')
        .filter(|rest| rest.starts_with('>'))
        .unwrap_or(token);
    let rest = rest.strip_prefix('>')?;
    Some(rest.strip_prefix('>').unwrap_or(rest))
}

/// Decide whether a target is a literal path or something only the shell can
/// resolve. Anything carrying an expansion sigil is reported as unresolvable so
/// the caller fails closed rather than validating a string the shell will
/// replace.
fn classify_target(target: &str, dialect: ShellDialect) -> RedirectTarget {
    let expands = match dialect {
        ShellDialect::PowerShell => target.contains('$') || target.contains('('),
        ShellDialect::Cmd => target.contains('%') || target.contains('!'),
        ShellDialect::Posix => target.contains('$') || target.contains('`') || target.contains('~'),
    };
    if expands {
        RedirectTarget::Unresolvable(target.to_string())
    } else {
        RedirectTarget::Literal(target.to_string())
    }
}

/// One lexed word plus whether it was produced entirely from quoted text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    text: String,
    /// A fully quoted word can never be a redirection operator (`echo "a > b"`
    /// redirects nothing).
    quoted: bool,
}

/// Split `command` into words using `dialect`'s quoting and escaping rules.
///
/// Unlike a POSIX lexer this never fails: an unterminated quote yields the text
/// read so far. The caller only uses the result to locate redirection targets,
/// and a command that does not lex cleanly is one the shell will reject anyway.
fn lex(command: &str, dialect: ShellDialect) -> Vec<Token> {
    // `\` is a path separator on Windows, so only POSIX lexing may treat it as an
    // escape. PowerShell escapes with a backtick and cmd.exe with a caret.
    let escape = match dialect {
        ShellDialect::PowerShell => Some('`'),
        ShellDialect::Cmd => Some('^'),
        ShellDialect::Posix => Some('\\'),
    };

    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut any_unquoted = false;
    let mut chars = command.chars().peekable();

    // Flush the word in progress, if any.
    macro_rules! flush {
        () => {
            if started {
                tokens.push(Token {
                    text: std::mem::take(&mut current),
                    quoted: !any_unquoted,
                });
                started = false;
                any_unquoted = false;
            }
        };
    }

    while let Some(character) = chars.next() {
        match character {
            // Statement separators end the current word and contribute none of
            // their own, so `a > one.txt; b > two.txt` yields a clean `one.txt`.
            // `&` is deliberately absent: it belongs to the `>&1` duplication
            // form, which must stay attached to be recognised as a non-file
            // target.
            ';' | '|' => flush!(),
            c if c.is_whitespace() => flush!(),
            c if Some(c) == escape => {
                started = true;
                any_unquoted = true;
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                }
            }
            '\'' if dialect != ShellDialect::Cmd => {
                // Single quotes are literal in both POSIX and PowerShell; cmd.exe
                // has no single-quote form.
                started = true;
                for quoted in chars.by_ref() {
                    if quoted == '\'' {
                        break;
                    }
                    current.push(quoted);
                }
            }
            '"' => {
                started = true;
                while let Some(quoted) = chars.next() {
                    if quoted == '"' {
                        break;
                    }
                    // Inside double quotes only PowerShell and POSIX honour an
                    // escape; cmd.exe takes the bytes literally.
                    if Some(quoted) == escape && dialect != ShellDialect::Cmd {
                        if let Some(escaped) = chars.next() {
                            current.push(escaped);
                        }
                        continue;
                    }
                    current.push(quoted);
                }
            }
            c => {
                started = true;
                any_unquoted = true;
                current.push(c);
            }
        }
    }
    if started {
        tokens.push(Token {
            text: current,
            quoted: !any_unquoted,
        });
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn literals(command: &str, dialect: ShellDialect) -> Vec<String> {
        redirect_targets(command, dialect)
            .into_iter()
            .map(|target| match target {
                RedirectTarget::Literal(path) | RedirectTarget::Unresolvable(path) => path,
            })
            .collect()
    }

    #[test]
    fn windows_paths_keep_their_separators() {
        // The exact bypass POSIX lexing allowed: the `..` segments survive, so
        // the containment check the caller runs can see them.
        assert_eq!(
            literals(r"echo x > ..\..\evil.txt", ShellDialect::PowerShell),
            vec![r"..\..\evil.txt"]
        );
        assert_eq!(
            literals(r"type a.txt > sub\out.txt", ShellDialect::Cmd),
            vec![r"sub\out.txt"]
        );
    }

    #[test]
    fn posix_backslash_still_escapes() {
        assert_eq!(
            literals(r"echo x > weird\ name.txt", ShellDialect::Posix),
            vec!["weird name.txt"]
        );
    }

    #[test]
    fn a_command_without_redirection_is_never_lexed() {
        assert!(!has_redirect("echo it's fine"));
        assert!(redirect_targets("echo it's fine", ShellDialect::Posix).is_empty());
        assert!(redirect_targets("git commit -m \"it's done\"", ShellDialect::Posix).is_empty());
    }

    #[test]
    fn fused_and_separated_operators_both_resolve() {
        assert_eq!(
            literals("echo a >out.txt", ShellDialect::Posix),
            vec!["out.txt"]
        );
        assert_eq!(
            literals("echo a > out.txt", ShellDialect::Posix),
            vec!["out.txt"]
        );
        assert_eq!(
            literals("echo a 2>err.log", ShellDialect::Posix),
            vec!["err.log"]
        );
        assert_eq!(
            literals("echo a >> append.txt", ShellDialect::Posix),
            vec!["append.txt"]
        );
        assert_eq!(
            literals("cmd *> all.txt", ShellDialect::PowerShell),
            vec!["all.txt"]
        );
    }

    #[test]
    fn quoted_text_is_not_a_redirect() {
        assert!(redirect_targets(r#"echo "a > b""#, ShellDialect::Posix).is_empty());
        assert!(redirect_targets("echo 'a > b'", ShellDialect::PowerShell).is_empty());
    }

    #[test]
    fn stream_duplication_is_not_a_file_target() {
        assert!(redirect_targets("build 2>&1", ShellDialect::Posix).is_empty());
    }

    #[test]
    fn every_redirect_in_a_chain_is_reported() {
        assert_eq!(
            literals("a > one.txt; b > two.txt", ShellDialect::PowerShell),
            vec!["one.txt", "two.txt"]
        );
    }

    #[test]
    fn expansions_are_unresolvable_rather_than_assumed_safe() {
        assert_eq!(
            redirect_targets("echo a > $env:TEMP\\x.txt", ShellDialect::PowerShell),
            vec![RedirectTarget::Unresolvable("$env:TEMP\\x.txt".to_string())]
        );
        assert_eq!(
            redirect_targets("echo a > %TEMP%\\x.txt", ShellDialect::Cmd),
            vec![RedirectTarget::Unresolvable("%TEMP%\\x.txt".to_string())]
        );
        assert_eq!(
            redirect_targets("echo a > $HOME/x.txt", ShellDialect::Posix),
            vec![RedirectTarget::Unresolvable("$HOME/x.txt".to_string())]
        );
    }

    #[test]
    fn an_unterminated_quote_produces_no_bogus_target() {
        // POSIX `shlex` returned `None` here and the caller rejected the whole
        // command. The shell itself will report the syntax error, and the
        // unterminated quote swallows the operator, so no target is invented.
        assert!(redirect_targets("echo it's > out.txt", ShellDialect::Posix).is_empty());
        assert!(redirect_targets("echo \"oops > out.txt", ShellDialect::PowerShell).is_empty());
    }
}
