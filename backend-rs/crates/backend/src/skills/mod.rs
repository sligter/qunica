use std::{
    collections::HashSet,
    fs,
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use zip::ZipArchive;

use crate::api::error::ApiError;

const CLASSIFIED_DIRS: &[&str] = &["references", "scripts", "assets", "tools"];
const TEXT_EXTENSIONS: &[&str] = &[
    "md", "markdown", "txt", "json", "yaml", "yml", "toml", "xml", "html", "css", "js", "jsx",
    "ts", "tsx", "py", "sh", "bash", "zsh", "fish", "ps1", "sql", "csv", "ini", "cfg",
];
const MAX_TEXT_FILE_BYTES: u64 = 1_000_000;

#[derive(Debug, Clone)]
pub struct ParsedSkillMarkdown {
    pub name: String,
    pub description: Option<String>,
    pub body_markdown: String,
    pub metadata: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SkillFileInfo {
    pub path: String,
    pub size: u64,
    #[serde(default = "default_category")]
    pub category: String,
}

#[derive(Debug, Clone)]
pub struct PackageFile {
    pub info: SkillFileInfo,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ParsedSkillPackage {
    pub parsed: ParsedSkillMarkdown,
    pub files: Vec<SkillFileInfo>,
    pub payloads: Vec<PackageFile>,
}

pub fn parse_skill_markdown(raw: &str) -> Result<ParsedSkillMarkdown, ApiError> {
    if raw.trim().is_empty() {
        return Err(ApiError::invalid_input("SKILL.md must not be empty"));
    }
    let without_bom = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let rest = without_bom
        .strip_prefix("---")
        .ok_or_else(|| ApiError::invalid_input("SKILL.md must start with YAML frontmatter"))?;
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))
        .ok_or_else(|| ApiError::invalid_input("SKILL.md frontmatter is not delimited"))?;
    let (frontmatter, body_start) = split_frontmatter(rest)?;

    let metadata = match serde_yaml::from_str::<serde_yaml::Value>(frontmatter) {
        Ok(yaml) => {
            yaml_mapping_to_json(yaml).or_else(|_| parse_simple_frontmatter(frontmatter))?
        }
        Err(_) => parse_simple_frontmatter(frontmatter)?,
    };
    let name = metadata
        .get("name")
        .and_then(Value::as_str)
        .map(validate_skill_name)
        .transpose()?
        .ok_or_else(|| ApiError::invalid_input("SKILL.md frontmatter must include name"))?;
    let description = metadata
        .get("description")
        .and_then(Value::as_str)
        .map(normalize_description)
        .unwrap_or(None);

    let body_markdown = body_start
        .trim_start_matches(['\r', '\n'])
        .trim()
        .to_string();
    if body_markdown.is_empty() {
        return Err(ApiError::invalid_input(
            "SKILL.md body_markdown must not be empty",
        ));
    }

    Ok(ParsedSkillMarkdown {
        name,
        description,
        body_markdown,
        metadata,
    })
}

pub fn parse_skill_package(bytes: &[u8]) -> Result<ParsedSkillPackage, ApiError> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|_| ApiError::invalid_input("uploaded file is not a valid zip archive"))?;

    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|_| ApiError::invalid_input("zip skill package cannot be read"))?;
        if file.is_dir() {
            let dir = file.name().trim_end_matches(['/', '\\']);
            if !dir.is_empty() {
                normalize_relative_path(dir)?;
            }
            continue;
        }
        let normalized = normalize_archive_entry(file.name())?;
        entries.push((index, normalized, file.size()));
    }

    let prefix = detect_skill_root(&entries)?;
    let mut seen = HashSet::new();
    let mut normalized_entries = Vec::new();
    for (index, path, size) in entries {
        let rel = strip_prefix(&path, prefix.as_deref())?;
        if !seen.insert(rel.clone()) {
            return Err(ApiError::invalid_input(
                "zip skill package contains duplicate resource paths",
            ));
        }
        normalized_entries.push((index, rel, size));
    }

    let skill_md_index = normalized_entries
        .iter()
        .find_map(|(index, path, _)| (path == "SKILL.md").then_some(*index))
        .ok_or_else(|| ApiError::invalid_input("zip skill package must contain SKILL.md"))?;
    let skill_md = read_zip_file_to_string(&mut archive, skill_md_index)?;
    let parsed = parse_skill_markdown(&skill_md)?;

    let mut files = Vec::new();
    let mut payloads = Vec::new();
    for (index, path, size) in normalized_entries {
        let category = classify_resource(&path).to_string();
        let info = SkillFileInfo {
            path: path.clone(),
            size,
            category,
        };
        let content = read_zip_file_to_bytes(&mut archive, index)?;
        files.push(info.clone());
        payloads.push(PackageFile { info, content });
    }

    Ok(ParsedSkillPackage {
        parsed,
        files,
        payloads,
    })
}

pub fn parse_files_json(raw: Option<&str>) -> Vec<SkillFileInfo> {
    raw.and_then(|value| serde_json::from_str::<Vec<SkillFileInfo>>(value).ok())
        .unwrap_or_default()
}

pub fn files_to_json(files: &[SkillFileInfo]) -> Result<String, ApiError> {
    serde_json::to_string(files).map_err(|_| ApiError::internal("failed to serialize files"))
}

pub fn validate_resource_path(raw: &str) -> Result<String, ApiError> {
    normalize_relative_path(raw)
}

pub fn write_package_files(storage_dir: &Path, payloads: &[PackageFile]) -> Result<(), ApiError> {
    fs::create_dir_all(storage_dir)
        .map_err(|_| ApiError::internal("failed to create skill storage"))?;
    let storage_root = storage_dir
        .canonicalize()
        .map_err(|_| ApiError::internal("failed to resolve skill storage"))?;

    for payload in payloads {
        let target = safe_storage_path(&storage_root, &payload.info.path)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|_| ApiError::internal("failed to create skill resource directory"))?;
        }
        fs::write(&target, &payload.content)
            .map_err(|_| ApiError::internal("failed to write skill resource"))?;
    }

    Ok(())
}

pub fn resource_storage_path(
    storage_root: &Path,
    skill_storage: &Path,
    rel_path: &str,
) -> Result<PathBuf, ApiError> {
    let canonical_root = storage_root
        .canonicalize()
        .map_err(|_| ApiError::internal("failed to resolve skill storage root"))?;
    let canonical_skill = skill_storage
        .canonicalize()
        .map_err(|_| ApiError::not_found("skill resources not found"))?;
    if !canonical_skill.starts_with(&canonical_root) {
        return Err(ApiError::invalid_input(
            "skill storage is outside storage root",
        ));
    }

    let target = safe_storage_path(&canonical_skill, rel_path)?;
    if !target.is_file() {
        return Err(ApiError::not_found("skill resource not found"));
    }
    Ok(target)
}

pub fn is_text_resource(path: &Path, size: u64) -> bool {
    if size > MAX_TEXT_FILE_BYTES {
        return false;
    }
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    if !TEXT_EXTENSIONS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(ext))
    {
        return false;
    }
    fs::read_to_string(path).is_ok()
}

pub fn classify_resource(rel_path: &str) -> &str {
    if rel_path == "SKILL.md" {
        return "SKILL.md";
    }
    let first = rel_path.split('/').next().unwrap_or_default();
    if CLASSIFIED_DIRS.contains(&first) {
        first
    } else {
        "other"
    }
}

pub fn find_file<'a>(
    files: &'a [SkillFileInfo],
    rel_path: &str,
) -> Result<&'a SkillFileInfo, ApiError> {
    files
        .iter()
        .find(|file| file.path == rel_path)
        .ok_or_else(|| ApiError::not_found("skill resource not found"))
}

fn split_frontmatter(raw_after_open: &str) -> Result<(&str, &str), ApiError> {
    for delimiter in ["\r\n---\r\n", "\n---\n", "\r\n---\n", "\n---\r\n"] {
        if let Some(index) = raw_after_open.find(delimiter) {
            let frontmatter = &raw_after_open[..index];
            let body = &raw_after_open[index + delimiter.len()..];
            return Ok((frontmatter, body));
        }
    }

    for delimiter in ["\r\n---", "\n---"] {
        if let Some(index) = raw_after_open.find(delimiter) {
            let body_start = index + delimiter.len();
            if raw_after_open[body_start..].trim().is_empty() {
                return Ok((&raw_after_open[..index], ""));
            }
        }
    }

    Err(ApiError::invalid_input(
        "SKILL.md frontmatter closing delimiter is missing",
    ))
}

fn yaml_mapping_to_json(value: serde_yaml::Value) -> Result<Value, ApiError> {
    let serde_yaml::Value::Mapping(mapping) = value else {
        return Err(ApiError::invalid_input(
            "SKILL.md frontmatter must be a YAML mapping",
        ));
    };

    let mut out = Map::new();
    for (key, value) in mapping {
        if let serde_yaml::Value::String(key) = key {
            out.insert(key, yaml_value_to_json(value));
        }
    }
    Ok(Value::Object(out))
}

fn parse_simple_frontmatter(raw: &str) -> Result<Value, ApiError> {
    let mut out = Map::new();
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        if line != line.trim_start() {
            return Err(ApiError::invalid_input(
                "SKILL.md frontmatter is not valid YAML",
            ));
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| ApiError::invalid_input("SKILL.md frontmatter is not valid YAML"))?;
        let key = key.trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
            || out.contains_key(key)
        {
            return Err(ApiError::invalid_input(
                "SKILL.md frontmatter is not valid YAML",
            ));
        }
        out.insert(key.to_string(), Value::String(value.trim().to_string()));
    }
    if out.is_empty() {
        return Err(ApiError::invalid_input(
            "SKILL.md frontmatter is not valid YAML",
        ));
    }
    Ok(Value::Object(out))
}

fn yaml_value_to_json(value: serde_yaml::Value) -> Value {
    match value {
        serde_yaml::Value::Null => Value::Null,
        serde_yaml::Value::Bool(value) => Value::Bool(value),
        serde_yaml::Value::Number(value) => serde_json::to_value(value).unwrap_or(Value::Null),
        serde_yaml::Value::String(value) => Value::String(value),
        serde_yaml::Value::Sequence(values) => {
            Value::Array(values.into_iter().map(yaml_value_to_json).collect())
        }
        serde_yaml::Value::Mapping(mapping) => {
            let mut out = Map::new();
            for (key, value) in mapping {
                if let serde_yaml::Value::String(key) = key {
                    out.insert(key, yaml_value_to_json(value));
                }
            }
            Value::Object(out)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_value_to_json(tagged.value),
    }
}

fn validate_skill_name(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim().to_string();
    let len = name.chars().count();
    if !(1..=100).contains(&len) {
        return Err(ApiError::invalid_input(
            "name must be between 1 and 100 characters",
        ));
    }
    Ok(name)
}

fn normalize_description(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn normalize_archive_entry(raw: &str) -> Result<String, ApiError> {
    normalize_relative_path(raw)
}

fn normalize_relative_path(raw: &str) -> Result<String, ApiError> {
    if raw.trim().is_empty() {
        return Err(ApiError::invalid_input("resource path is not allowed"));
    }
    if has_windows_drive_prefix(raw) {
        return Err(ApiError::invalid_input("resource path is not allowed"));
    }
    if raw.starts_with("\\\\") {
        return Err(ApiError::invalid_input("resource path is not allowed"));
    }

    let normalized = raw.replace('\\', "/");
    if normalized.starts_with('/') || normalized.starts_with("//") || normalized.ends_with('/') {
        return Err(ApiError::invalid_input("resource path is not allowed"));
    }

    let mut parts = Vec::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(ApiError::invalid_input("resource path is not allowed"));
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(ApiError::invalid_input("resource path is not allowed"));
    }

    Ok(parts.join("/"))
}

fn has_windows_drive_prefix(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

fn detect_skill_root(entries: &[(usize, String, u64)]) -> Result<Option<String>, ApiError> {
    let mut root_skill = false;
    let mut candidates = HashSet::new();

    for (_, path, _) in entries {
        if path == "SKILL.md" {
            root_skill = true;
        } else if let Some(prefix) = path.strip_suffix("/SKILL.md") {
            candidates.insert(prefix.to_string());
        }
    }

    if root_skill {
        return Ok(None);
    }

    if candidates.len() != 1 {
        return Err(ApiError::invalid_input(
            "zip skill package must contain a single SKILL.md",
        ));
    }

    let prefix = candidates.into_iter().next().unwrap();
    let required_prefix = format!("{prefix}/");
    if entries
        .iter()
        .any(|(_, path, _)| !path.starts_with(&required_prefix))
    {
        return Err(ApiError::invalid_input(
            "zip skill package must contain a single skill root directory",
        ));
    }

    Ok(Some(prefix))
}

fn strip_prefix(path: &str, prefix: Option<&str>) -> Result<String, ApiError> {
    let Some(prefix) = prefix else {
        return Ok(path.to_string());
    };
    let required_prefix = format!("{prefix}/");
    let stripped = path
        .strip_prefix(&required_prefix)
        .ok_or_else(|| ApiError::invalid_input("resource path is not allowed"))?;
    normalize_relative_path(stripped)
}

fn read_zip_file_to_string(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    index: usize,
) -> Result<String, ApiError> {
    let bytes = read_zip_file_to_bytes(archive, index)?;
    String::from_utf8(bytes).map_err(|_| ApiError::invalid_input("SKILL.md must be UTF-8"))
}

fn read_zip_file_to_bytes(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    index: usize,
) -> Result<Vec<u8>, ApiError> {
    let mut file = archive
        .by_index(index)
        .map_err(|_| ApiError::invalid_input("zip skill package cannot be read"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|_| ApiError::invalid_input("zip skill package cannot be read"))?;
    Ok(bytes)
}

fn safe_storage_path(root: &Path, rel_path: &str) -> Result<PathBuf, ApiError> {
    let rel_path = validate_resource_path(rel_path)?;
    let rel = Path::new(&rel_path);
    if rel.components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        )
    }) {
        return Err(ApiError::invalid_input("resource path is not allowed"));
    }
    let target = root.join(rel);
    if !target.starts_with(root) {
        return Err(ApiError::invalid_input("resource path is not allowed"));
    }
    Ok(target)
}

fn default_category() -> String {
    "other".to_string()
}
