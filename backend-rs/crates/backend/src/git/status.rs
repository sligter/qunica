use serde::Serialize;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct WorkspaceGitFileStatus {
    path: String,
    status: String,
    staged: bool,
    unstaged: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct WorkspaceGitStatus {
    available: bool,
    branch: Option<String>,
    clean: bool,
    files: Vec<WorkspaceGitFileStatus>,
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ahead: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    behind: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<String>,
}

pub(super) fn unavailable_status(message: impl Into<String>) -> WorkspaceGitStatus {
    WorkspaceGitStatus {
        available: false,
        branch: None,
        clean: true,
        files: Vec::new(),
        message: Some(message.into()),
        ahead: None,
        behind: None,
        state: None,
    }
}

pub(super) fn parse_status(stdout: &str) -> WorkspaceGitStatus {
    let mut branch = None;
    let mut ahead = None;
    let mut behind = None;
    let mut branch_state = None;
    let mut has_conflict = false;
    let mut files = Vec::new();

    let mut records = stdout.split('\0');
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if let Some(header) = record.strip_prefix("# ") {
            parse_header(
                header,
                &mut branch,
                &mut ahead,
                &mut behind,
                &mut branch_state,
            );
            continue;
        }

        match record.as_bytes().first().copied() {
            Some(b'1') => {
                if let Some(file) = parse_v2_file_record(record, 9) {
                    files.push(file);
                }
            }
            Some(b'2') => {
                if let Some(file) = parse_v2_file_record(record, 10) {
                    files.push(file);
                }
                let _old_path = records.next();
            }
            Some(b'u') => {
                if let Some(file) = parse_v2_file_record(record, 11) {
                    has_conflict = true;
                    files.push(file);
                }
            }
            Some(b'?') => {
                if let Some(path) = record.strip_prefix("? ").filter(|path| !path.is_empty()) {
                    files.push(file_status(path, "??"));
                }
            }
            _ => {}
        }
    }

    WorkspaceGitStatus {
        available: true,
        branch,
        clean: files.is_empty(),
        files,
        message: None,
        ahead,
        behind,
        state: if has_conflict {
            Some("conflict".to_string())
        } else {
            branch_state
        },
    }
}

fn parse_header(
    header: &str,
    branch: &mut Option<String>,
    ahead: &mut Option<i64>,
    behind: &mut Option<i64>,
    branch_state: &mut Option<String>,
) {
    if let Some(value) = header.strip_prefix("branch.head ") {
        let value = value.trim();
        if value == "(detached)" {
            *branch_state = Some("detached".to_string());
        } else if !value.is_empty() {
            *branch = Some(value.to_string());
        }
        return;
    }
    if let Some(value) = header.strip_prefix("branch.oid ") {
        if value.trim() == "(initial)" {
            *branch_state = Some("initial".to_string());
        }
        return;
    }
    if let Some(value) = header.strip_prefix("branch.ab ") {
        for part in value.split_whitespace() {
            if let Some(raw) = part.strip_prefix('+') {
                *ahead = raw.parse::<i64>().ok();
            } else if let Some(raw) = part.strip_prefix('-') {
                *behind = raw.parse::<i64>().ok();
            }
        }
    }
}

fn parse_v2_file_record(record: &str, expected_fields: usize) -> Option<WorkspaceGitFileStatus> {
    let fields: Vec<&str> = record.splitn(expected_fields, ' ').collect();
    if fields.len() != expected_fields {
        return None;
    }
    let xy = fields.get(1).copied()?;
    let path = fields.last().copied()?;
    if path.is_empty() {
        return None;
    }
    Some(file_status(path, &normalize_xy(xy)?))
}

fn normalize_xy(xy: &str) -> Option<String> {
    let mut chars = xy.chars();
    let first = normalize_status_char(chars.next()?)?;
    let second = normalize_status_char(chars.next()?)?;
    Some(format!("{first}{second}"))
}

fn normalize_status_char(ch: char) -> Option<char> {
    match ch {
        '.' => Some(' '),
        value if value.is_ascii() => Some(value),
        _ => None,
    }
}

fn file_status(path: &str, status: &str) -> WorkspaceGitFileStatus {
    let bytes = status.as_bytes();
    let staged = bytes
        .first()
        .is_some_and(|value| *value != b' ' && *value != b'?');
    let unstaged = bytes.get(1).is_some_and(|value| *value != b' ');
    WorkspaceGitFileStatus {
        path: path.to_string(),
        status: status.to_string(),
        staged,
        unstaged,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_status;

    #[test]
    fn parses_branch_counts_and_paths_with_spaces() {
        let status = parse_status(
            "# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -3\01 .M N... 100644 100644 100644 abc def tracked file.txt\0? new file.txt\0",
        );

        assert!(status.available);
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.ahead, Some(2));
        assert_eq!(status.behind, Some(3));
        assert_eq!(status.files.len(), 2);
        assert_eq!(status.files[0].path, "tracked file.txt");
        assert_eq!(status.files[0].status, " M");
        assert!(!status.files[0].staged);
        assert!(status.files[0].unstaged);
        assert_eq!(status.files[1].path, "new file.txt");
        assert_eq!(status.files[1].status, "??");
    }

    #[test]
    fn parses_rename_and_copy_destinations_and_skips_source_paths() {
        let status = parse_status(
            "# branch.oid abc\0# branch.head main\02 R. N... 100644 100644 100644 abc def R100 new name.txt\0old name.txt\02 C. N... 100644 100644 100644 abc def C100 copied name.txt\0source name.txt\01 M. N... 100644 100644 100644 abc def staged.txt\0",
        );

        assert_eq!(status.files.len(), 3);
        assert_eq!(status.files[0].path, "new name.txt");
        assert_eq!(status.files[0].status, "R ");
        assert!(status.files[0].staged);
        assert!(!status.files[0].unstaged);
        assert_eq!(status.files[1].path, "copied name.txt");
        assert_eq!(status.files[1].status, "C ");
        assert_eq!(status.files[2].path, "staged.txt");
        assert_eq!(status.files[2].status, "M ");
    }

    #[test]
    fn parses_unmerged_files_as_conflict_state() {
        let status = parse_status(
            "# branch.oid abc\0# branch.head main\0u UU N... 100644 100644 100644 100644 a b c conflict.txt\0",
        );

        assert_eq!(status.state.as_deref(), Some("conflict"));
        assert_eq!(status.files[0].path, "conflict.txt");
        assert_eq!(status.files[0].status, "UU");
        assert!(status.files[0].staged);
        assert!(status.files[0].unstaged);
    }

    #[test]
    fn preserves_path_boundary_whitespace_from_z_output() {
        let status = parse_status(
            "# branch.oid abc\0# branch.head main\01 .M N... 100644 100644 100644 abc def  leading and trailing \0?  untracked trailing \0",
        );

        assert_eq!(status.files[0].path, " leading and trailing ");
        assert_eq!(status.files[1].path, " untracked trailing ");
    }

    #[test]
    fn parses_detached_and_initial_states() {
        let detached = parse_status("# branch.oid abc\0# branch.head (detached)\0");
        assert_eq!(detached.branch, None);
        assert_eq!(detached.state.as_deref(), Some("detached"));

        let initial = parse_status("# branch.oid (initial)\0# branch.head main\0");
        assert_eq!(initial.branch.as_deref(), Some("main"));
        assert_eq!(initial.state.as_deref(), Some("initial"));
    }
}
