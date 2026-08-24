//! Launch support for the deepseek-harness (`dsh`) ACP runtime.
//!
//! `dsh` is a Cordis plugin tree: the ACP server itself is one plugin, and what
//! the agent can actually do is decided by a `cordis.yml` composition file read
//! from the launch directory. Two consequences drive this module:
//!
//! * The default config path is `./cordis.yml` and the default session store is
//!   `./.sessions`, both relative to the launch cwd — which for us is the
//!   user's workspace. Left alone, `dsh` would read whatever `cordis.yml`
//!   happens to sit in the user's repository and write session state into it.
//!   We therefore always launch with an explicit `--config` pointing at a
//!   managed file, with an absolute `persistenceRoot` under the session's
//!   isolated home.
//! * Settings that other ACP agents expose over the wire (`session/set_model`
//!   and friends, which `dsh` does not implement at all) are expressed here
//!   instead.
//!
//! The generated composition has two shapes, selected by the config's `mode`:
//!
//! * **text-only** — no bash, no filesystem tools, no workspace context. The
//!   LLM adapter is the only leaf the bundled agent spine requires.
//! * **read-only / workspace-write / danger-full-access** — the shell and
//!   filesystem tools, mounted over `dsh`'s sandbox seam. Confinement is real
//!   on every platform we ship: Linux uses bwrap or Landlock, macOS Seatbelt,
//!   and Windows a restricted-token ACL runner. Two boundaries are narrower
//!   than the mode names suggest, and both are upstream's design rather than
//!   ours: `workspace-write` grants the workspace root *plus the platform temp
//!   directories*, and the Windows and older-Landlock rungs self-report
//!   `partial` enforcement because objects granting `Everyone` write, and NTFS
//!   hard links, stay reachable. The mode descriptions in the runtime preset
//!   say both out loud rather than promising containment we cannot deliver.
//!   An unusable runner fails closed with `SANDBOX_UNAVAILABLE`; it never
//!   silently degrades to running unconfined.
//!
//! The shell tool is platform-split the same way `dsh`'s own presets split it:
//! `pwsh` on Windows, `bash` elsewhere. Mounting the bash tool on Windows would
//! hand the model a dialect the host cannot run.

use std::io;
use std::path::{Path, PathBuf};

use crate::acp::config::{AcpRuntimeConfig, AcpRuntimeProfile};

/// The npm package providing the `dsh-acp-demo` ACP server binary.
pub const DSH_ACP_PACKAGE: &str = "@deepseek-ai/dsh-acp-demo";

/// The npm package providing the DeepSeek LLM adapter.
///
/// This is *not* a peer dependency of [`DSH_ACP_PACKAGE`]: installing the ACP
/// server alone leaves the composition with no adapter to route
/// `provider`/`model` to, and the plugin tree fails to settle at boot.
pub const DSH_LLM_PACKAGE: &str = "@deepseek-ai/dsh-llm-deepseek";

/// The npm dist-tag carrying the compatible preview releases.
///
/// The `latest` tag on both packages still points at `0.0.1-rc.1`, an older
/// release with a different peer-dependency graph. Anything that resolves a
/// version for these packages — install commands, update checks — must use this
/// tag rather than the `latest` default.
pub const DSH_DIST_TAG: &str = "next";

/// The model used when an agent does not pick one.
pub const DSH_DEFAULT_MODEL: &str = "deepseek-v4-pro";

/// The executable name the packages install onto `PATH`.
pub const DSH_ACP_COMMAND: &str = "dsh-acp-demo";

/// [`DSH_ACP_PACKAGE`] on the preview channel tracked by the version panel.
pub const DSH_ACP_INSTALL_SPEC: &str = "@deepseek-ai/dsh-acp-demo@next";

/// The specs a working `dsh` runtime needs, primary first.
///
/// Everything past the LLM adapter serves the tool tiers. They are installed
/// unconditionally, including the shell backend for the other platform: a
/// couple of unused packages cost far less than an agent that boots into a
/// mode whose plugins are missing, which surfaces only as a failed plugin
/// tree at launch.
pub const DSH_INSTALL_SPECS: &[&str] = &[
    DSH_ACP_INSTALL_SPEC,
    "@deepseek-ai/dsh-llm-deepseek@next",
    "@deepseek-ai/dsh-sandbox-local@next",
    "@deepseek-ai/dsh-subprocess-local@next",
    "@deepseek-ai/dsh-bash-sandbox@next",
    "@deepseek-ai/dsh-pwsh-sandbox@next",
    "@deepseek-ai/dsh-tool-pwsh@next",
    "@deepseek-ai/dsh-fs-sandbox@next",
    "@deepseek-ai/dsh-fs-observation-policy@next",
    "@deepseek-ai/dsh-tool-fs@next",
];

/// How much of the workspace a `dsh` agent may touch, and whether it gets tools
/// at all.
///
/// The three named tiers are `dsh`'s own sandbox modes; [`DshMode::TextOnly`]
/// is ours, for an agent that only talks. A config with no mode takes
/// `TextOnly`, which is what agents saved before the tool tiers existed have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DshMode {
    /// No shell, no filesystem, no workspace context.
    TextOnly,
    /// Tools mounted; the sandbox denies every write.
    ReadOnly,
    /// Tools mounted; writes confined to the workspace and a private temp dir.
    WorkspaceWrite,
    /// Tools mounted with no confinement, and approvals answered `never`.
    DangerFullAccess,
}

impl DshMode {
    /// The wire value used in saved configs and the runtime preset.
    pub fn as_str(self) -> &'static str {
        match self {
            DshMode::TextOnly => "text-only",
            DshMode::ReadOnly => "read-only",
            DshMode::WorkspaceWrite => "workspace-write",
            DshMode::DangerFullAccess => "danger-full-access",
        }
    }

    fn parse(mode: Option<&str>) -> Result<Self, String> {
        match mode {
            None | Some("text-only") => Ok(DshMode::TextOnly),
            Some("read-only") => Ok(DshMode::ReadOnly),
            Some("workspace-write") => Ok(DshMode::WorkspaceWrite),
            Some("danger-full-access") => Ok(DshMode::DangerFullAccess),
            Some(other) => Err(format!(
                "dsh mode must be one of text-only, read-only, workspace-write, \
                 danger-full-access, got {other:?}"
            )),
        }
    }

    /// Whether this tier mounts the shell and filesystem tools.
    fn has_tools(self) -> bool {
        self != DshMode::TextOnly
    }

    /// The `dsh-sandbox-policy` mode, for tiers that have one.
    fn sandbox_mode(self) -> Option<&'static str> {
        match self {
            DshMode::TextOnly => None,
            other => Some(other.as_str()),
        }
    }
}

/// Every mode the runtime preset offers, in escalating order.
pub const DSH_MODES: [DshMode; 4] = [
    DshMode::TextOnly,
    DshMode::ReadOnly,
    DshMode::WorkspaceWrite,
    DshMode::DangerFullAccess,
];

/// The args to launch `config` with, given the session's isolated home and the
/// workspace the agent runs in.
///
/// For every profile except [`AcpRuntimeProfile::Dsh`] this is the config's own
/// args unchanged. For `dsh` it appends `--config <path>` to a freshly written
/// managed composition, unless the config already names one — the escape hatch
/// for anyone who wants to drive the plugin tree themselves.
pub fn launch_args(
    config: &AcpRuntimeConfig,
    isolated_home: &Path,
    workspace: &Path,
) -> io::Result<Vec<String>> {
    if config.profile != AcpRuntimeProfile::Dsh || has_explicit_config_arg(&config.args) {
        return Ok(config.args.clone());
    }
    let path = write_managed_composition(isolated_home, workspace, config)?;
    let mut args = config.args.clone();
    args.push("--config".to_string());
    args.push(path.to_string_lossy().into_owned());
    Ok(args)
}

fn has_explicit_config_arg(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--config" || arg == "-c" || arg.starts_with("--config="))
}

/// The reasoning-effort values `dsh-llm-deepseek` accepts for `reasoningEffort`.
const DSH_REASONING_EFFORTS: [&str; 3] = ["off", "high", "max"];

/// Map the config's `thinking_effort` to the `reasoningEffort` value written
/// into the managed composition.
///
/// Rejecting an invalid value here — rather than letting `dsh` fail to settle
/// its plugin tree — turns an opaque "plugin tree failed to load" into a clear
/// launch error. The text-only composition has no other place to express a
/// thinking preference: `dsh` reads it from the adapter's plugin config, not
/// from an ACP session method.
fn normalize_reasoning_effort(thinking_effort: Option<&str>) -> Result<Option<&str>, String> {
    match thinking_effort {
        None => Ok(None),
        Some(effort) if DSH_REASONING_EFFORTS.contains(&effort) => Ok(Some(effort)),
        Some(other) => Err(format!(
            "dsh thinking effort must be one of {}, got {other:?}",
            DSH_REASONING_EFFORTS.join(", ")
        )),
    }
}

/// The reason `config` cannot produce a valid composition, if any.
///
/// `dsh` expresses model, mode, and reasoning effort through its plugin config
/// rather than through ACP session methods, so a value we would otherwise send
/// over the wire has to be validated here. Reporting it up front beats letting
/// the plugin tree fail to settle, which reaches the user as a dead child
/// process and nothing else.
fn config_rejection(config: &AcpRuntimeConfig) -> Option<String> {
    DshMode::parse(config.mode.as_deref())
        .err()
        .or_else(|| normalize_reasoning_effort(config.thinking_effort.as_deref()).err())
}

/// Write the managed `cordis.yml` under `isolated_home` and return its path.
fn write_managed_composition(
    isolated_home: &Path,
    workspace: &Path,
    config: &AcpRuntimeConfig,
) -> io::Result<PathBuf> {
    let invalid = |message: String| io::Error::new(io::ErrorKind::InvalidInput, message);
    let mode = DshMode::parse(config.mode.as_deref()).map_err(invalid)?;
    let reasoning_effort =
        normalize_reasoning_effort(config.thinking_effort.as_deref()).map_err(invalid)?;

    let root = isolated_home.join("dsh");
    let sessions = root.join("sessions");
    std::fs::create_dir_all(&sessions)?;
    let path = root.join("cordis.yml");
    std::fs::write(
        &path,
        managed_composition(
            config.model.as_deref().unwrap_or(DSH_DEFAULT_MODEL),
            reasoning_effort,
            mode,
            &sessions,
            workspace,
        ),
    )?;
    Ok(path)
}

/// The shell executor and model-facing shell tool for the host platform.
///
/// `dsh` ships a bash pair and a pwsh pair and picks between them by platform;
/// mounting the wrong one gives the model a tool whose dialect the host cannot
/// run. The tuple is `(executor plugin, tool plugin or None)`: on Unix the
/// bundled `acp-agent` row already owns the bash tool through its `toolBash`
/// config, so only the executor needs mounting.
fn platform_shell_plugins() -> (&'static str, Option<&'static str>) {
    if cfg!(windows) {
        (
            "@deepseek-ai/dsh-pwsh-sandbox",
            Some("@deepseek-ai/dsh-tool-pwsh"),
        )
    } else {
        ("@deepseek-ai/dsh-bash-sandbox", None)
    }
}

/// Render the Cordis composition for one session.
fn managed_composition(
    model: &str,
    reasoning_effort: Option<&str>,
    mode: DshMode,
    sessions: &Path,
    workspace: &Path,
) -> String {
    let quoted_model = yaml_quote(model);
    let mut yaml = String::new();
    yaml.push_str(
        "# Generated by ag-swarmer for one dsh session. Edits here are discarded.\n\
         #\n\
         # The LLM adapter is the only leaf the bundled agent spine requires;\n\
         # everything else below is this session's permission tier.\n",
    );

    yaml.push_str("- id: llm-deepseek\n  name: '");
    yaml.push_str(DSH_LLM_PACKAGE);
    yaml.push_str("'\n  config:\n");
    if let Some(effort) = reasoning_effort {
        yaml.push_str("    reasoningEffort: ");
        yaml.push_str(effort);
        yaml.push('\n');
    }
    yaml.push_str("    models:\n      - id: ");
    yaml.push_str(&quoted_model);
    yaml.push('\n');

    if let Some(sandbox_mode) = mode.sandbox_mode() {
        let (executor, shell_tool) = platform_shell_plugins();
        // `workspaceRoot` is only the fallback for calls that arrive without a
        // session cwd; ACP tool calls carry the session's own. It is written
        // as an absolute literal rather than upstream's `!!js process.cwd()`
        // so the file stays inert data.
        yaml.push_str("- id: subprocess\n  name: '@deepseek-ai/dsh-subprocess-local'\n");
        yaml.push_str("- id: sandbox\n  name: '@deepseek-ai/dsh-sandbox-local'\n");
        yaml.push_str("- id: sandbox-policy\n  name: '@deepseek-ai/dsh-sandbox-policy'\n  config:\n    mode: ");
        yaml.push_str(sandbox_mode);
        yaml.push_str("\n    workspaceRoot: ");
        yaml.push_str(&yaml_quote(&workspace.to_string_lossy()));
        yaml.push('\n');
        // `never` short-circuits the escalation request the shell tool would
        // otherwise raise for a denied write; the other tiers ask, and our ACP
        // client answers with the agent's own permission policy.
        yaml.push_str(
            "- id: approval\n  name: '@deepseek-ai/dsh-user-approval'\n  config:\n    policy: ",
        );
        yaml.push_str(if mode == DshMode::DangerFullAccess {
            "never"
        } else {
            "ask"
        });
        yaml.push('\n');
        yaml.push_str("- id: shell-env\n  name: '@deepseek-ai/dsh-shell-env'\n");
        yaml.push_str("- id: shell\n  name: '");
        yaml.push_str(executor);
        yaml.push_str("'\n");
        if let Some(tool) = shell_tool {
            yaml.push_str("- id: tool-shell\n  name: '");
            yaml.push_str(tool);
            yaml.push_str("'\n");
        }
        yaml.push_str("- id: fs-sandbox\n  name: '@deepseek-ai/dsh-fs-sandbox'\n");
        yaml.push_str(
            "- id: fs-observation-policy\n  name: '@deepseek-ai/dsh-fs-observation-policy'\n",
        );
        yaml.push_str("- id: tool-fs\n  name: '@deepseek-ai/dsh-tool-fs'\n");
    }

    yaml.push_str("- id: acp-agent\n  name: '");
    yaml.push_str(DSH_ACP_PACKAGE);
    yaml.push_str("'\n  config:\n    provider: deepseek-official\n    model: ");
    yaml.push_str(&quoted_model);
    yaml.push_str("\n    persistenceRoot: ");
    yaml.push_str(&yaml_quote(&sessions.to_string_lossy()));
    yaml.push('\n');
    yaml.push_str("    workspaceContext: false\n");
    yaml.push_str("    skills:\n      enabled: false\n");
    // On Windows the bundle's bash tool is replaced by the pwsh row above, so
    // it stays off in every tier; on Unix the tool tiers let the bundle mount
    // its own bash tool with owner defaults.
    if !mode.has_tools() || platform_shell_plugins().1.is_some() {
        yaml.push_str("    toolBash: false\n");
    }
    yaml.push_str("    toolJobs: false\n    goals: false\n");
    yaml
}

/// Render `value` as a single-quoted YAML scalar.
///
/// Single quotes are used rather than double so that Windows path separators
/// are not read as escape sequences.
fn yaml_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// The lowest Node version `dsh` runs on, as `(major, minor)` pairs. It uses
/// `node:sqlite`, so an older runtime does not merely warn — it fails to boot.
const DSH_NODE_REQUIREMENT: [(u64, u64); 2] = [(22, 19), (24, 0)];

/// Check a `dsh` config for reasons it cannot run, in the order they would
/// bite: an unusable value first, then the host Node version.
///
/// Any other profile, or a Node that cannot be interrogated, is left alone: a
/// probe that guesses wrong is worse than one that stays quiet. A bad config
/// value is reported here as well as at launch, because the capability probe
/// turns a launch failure into a generic "unable to prepare the probe" and the
/// actual reason would never reach the user.
pub async fn preflight(config: &AcpRuntimeConfig) -> Option<String> {
    if config.profile != AcpRuntimeProfile::Dsh {
        return None;
    }
    if let Some(rejection) = config_rejection(config) {
        return Some(rejection);
    }
    let version = host_node_version().await?;
    if node_version_is_supported(version) {
        return None;
    }
    let (major, minor) = version;
    Some(format!(
        "The dsh runtime needs Node 22.19+ or 24+, but Node {major}.{minor} is on PATH."
    ))
}

fn node_version_is_supported((major, minor): (u64, u64)) -> bool {
    let newest = DSH_NODE_REQUIREMENT[DSH_NODE_REQUIREMENT.len() - 1];
    major > newest.0
        || DSH_NODE_REQUIREMENT
            .iter()
            .any(|&(req_major, req_minor)| major == req_major && minor >= req_minor)
}

async fn host_node_version() -> Option<(u64, u64)> {
    let mut command = std::process::Command::new("node");
    command.arg("--version");
    let output = tokio::time::timeout(
        NODE_VERSION_TIMEOUT,
        crate::process::tokio_command_no_window(command).output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_node_version(&String::from_utf8_lossy(&output.stdout))
}

/// How long to wait for `node --version` before giving up on the preflight.
const NODE_VERSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Parse the `vMAJOR.MINOR.PATCH` that `node --version` prints.
fn parse_node_version(text: &str) -> Option<(u64, u64)> {
    let text = text.trim().trim_start_matches('v');
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::acp::config::normalize_acp_runtime;

    fn dsh_config(extra: serde_json::Value) -> AcpRuntimeConfig {
        let mut obj = serde_json::Map::new();
        obj.insert("command".into(), json!(DSH_ACP_COMMAND));
        obj.insert("profile".into(), json!("dsh"));
        if let serde_json::Value::Object(extra) = extra {
            for (key, value) in extra {
                obj.insert(key, value);
            }
        }
        normalize_acp_runtime(Some(&serde_json::Value::Object(obj))).expect("dsh config normalizes")
    }

    /// Materialize the composition for `extra`, keeping both temp roots alive
    /// so the caller can assert on the paths written into it.
    fn composition(extra: serde_json::Value) -> (String, tempfile::TempDir, tempfile::TempDir) {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let args = launch_args(&dsh_config(extra), home.path(), workspace.path()).unwrap();
        assert_eq!(args[args.len() - 2], "--config");
        let path = Path::new(&args[args.len() - 1]);
        assert!(path.is_file(), "composition was not written: {path:?}");
        let text = std::fs::read_to_string(path).unwrap();
        (text, home, workspace)
    }

    fn parse(text: &str) -> Vec<serde_yaml::Value> {
        serde_yaml::from_str::<serde_yaml::Value>(text)
            .unwrap_or_else(|error| panic!("managed cordis.yml is not valid YAML: {error}\n{text}"))
            .as_sequence()
            .expect("top-level sequence")
            .clone()
    }

    fn row_ids(text: &str) -> Vec<String> {
        parse(text)
            .iter()
            .map(|row| {
                row.get("id")
                    .and_then(serde_yaml::Value::as_str)
                    .expect("row id")
                    .to_string()
            })
            .collect()
    }

    fn config_of<'a>(rows: &'a [serde_yaml::Value], id: &str) -> &'a serde_yaml::Value {
        rows.iter()
            .find(|row| row.get("id").and_then(serde_yaml::Value::as_str) == Some(id))
            .unwrap_or_else(|| panic!("no row with id {id}"))
            .get("config")
            .unwrap_or_else(|| panic!("row {id} has no config"))
    }

    fn str_at(value: &serde_yaml::Value, key: &str) -> String {
        value
            .get(key)
            .and_then(serde_yaml::Value::as_str)
            .unwrap_or_else(|| panic!("no string at {key}"))
            .to_string()
    }

    #[test]
    fn text_only_is_the_default_and_mounts_no_tools() {
        let (text, home, _workspace) = composition(json!({}));
        assert_eq!(row_ids(&text), vec!["llm-deepseek", "acp-agent"]);
        // The session store must never be the relative `./.sessions` default,
        // which would land in the user's workspace.
        assert!(!text.contains("./.sessions"), "{text}");
        assert_eq!(
            str_at(config_of(&parse(&text), "acp-agent"), "persistenceRoot"),
            home.path().join("dsh").join("sessions").to_str().unwrap()
        );
        assert!(text.contains("toolBash: false"), "{text}");
        assert!(text.contains("workspaceContext: false"), "{text}");
        assert!(!text.contains("dsh-sandbox"), "{text}");
    }

    #[test]
    fn tool_tiers_mount_the_sandbox_stack_and_the_platform_shell() {
        let (text, _home, workspace) = composition(json!({ "mode": "workspace-write" }));
        let ids = row_ids(&text);
        for required in [
            "subprocess",
            "sandbox",
            "sandbox-policy",
            "approval",
            "shell-env",
            "shell",
            "fs-sandbox",
            "fs-observation-policy",
            "tool-fs",
            "acp-agent",
        ] {
            assert!(ids.contains(&required.to_string()), "ids: {ids:?}");
        }

        let rows = parse(&text);
        let policy = config_of(&rows, "sandbox-policy");
        assert_eq!(str_at(policy, "mode"), "workspace-write");
        // The fallback workspace root is the session cwd, spelled absolutely
        // rather than as upstream's `!!js process.cwd()`.
        assert_eq!(
            str_at(policy, "workspaceRoot"),
            workspace.path().to_str().unwrap()
        );
        assert_eq!(str_at(config_of(&rows, "approval"), "policy"), "ask");

        // The host supplies this agent's brief, so dsh must not also inject
        // the workspace's own instruction files — no other agent in a group
        // gets a second, unmanaged prompt source.
        assert!(text.contains("workspaceContext: false"), "{text}");

        // The shell pair must match the host: a bash tool on Windows would
        // hand the model a dialect the machine cannot run.
        let shell = rows
            .iter()
            .find(|row| row.get("id").and_then(serde_yaml::Value::as_str) == Some("shell"))
            .and_then(|row| row.get("name"))
            .and_then(serde_yaml::Value::as_str)
            .expect("shell row name");
        if cfg!(windows) {
            assert_eq!(shell, "@deepseek-ai/dsh-pwsh-sandbox");
            assert!(ids.contains(&"tool-shell".to_string()), "ids: {ids:?}");
            assert!(text.contains("toolBash: false"), "{text}");
        } else {
            assert_eq!(shell, "@deepseek-ai/dsh-bash-sandbox");
            assert!(!ids.contains(&"tool-shell".to_string()), "ids: {ids:?}");
            // The bundled acp-agent row owns the bash tool on Unix.
            assert!(!text.contains("toolBash: false"), "{text}");
        }
    }

    #[test]
    fn read_only_keeps_the_tools_and_narrows_the_sandbox() {
        let (text, _home, _workspace) = composition(json!({ "mode": "read-only" }));
        let rows = parse(&text);
        assert_eq!(
            str_at(config_of(&rows, "sandbox-policy"), "mode"),
            "read-only"
        );
        assert_eq!(str_at(config_of(&rows, "approval"), "policy"), "ask");
    }

    #[test]
    fn full_access_stops_asking_before_it_writes() {
        let (text, _home, _workspace) = composition(json!({ "mode": "danger-full-access" }));
        let rows = parse(&text);
        assert_eq!(
            str_at(config_of(&rows, "sandbox-policy"), "mode"),
            "danger-full-access"
        );
        assert_eq!(str_at(config_of(&rows, "approval"), "policy"), "never");
    }

    #[test]
    fn configured_model_reaches_the_composition() {
        let (text, _home, _workspace) = composition(json!({ "model": "deepseek-v4-flash" }));
        assert!(text.contains("deepseek-v4-flash"), "{text}");
        assert!(!text.contains(DSH_DEFAULT_MODEL), "{text}");
    }

    #[test]
    fn thinking_effort_reaches_the_composition() {
        let (text, _home, _workspace) = composition(json!({ "thinking_effort": "max" }));
        assert!(text.contains("reasoningEffort: max"), "{text}");
    }

    #[test]
    fn invalid_values_are_rejected_before_the_plugin_tree_sees_them() {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();

        let effort = dsh_config(json!({ "thinking_effort": "medium" }));
        let error = launch_args(&effort, home.path(), workspace.path()).unwrap_err();
        assert!(
            error.to_string().contains("off, high, max"),
            "unexpected error: {error}"
        );
        // The same reason must be available before a spawn, because the
        // capability probe reports a launch failure only in the generic.
        assert!(config_rejection(&effort)
            .unwrap()
            .contains("off, high, max"));

        let mode = dsh_config(json!({ "mode": "sandbox-please" }));
        let error = launch_args(&mode, home.path(), workspace.path()).unwrap_err();
        assert!(
            error.to_string().contains("text-only"),
            "unexpected error: {error}"
        );
        assert!(config_rejection(&mode).unwrap().contains("text-only"));

        // Every mode the preset offers must survive the round trip.
        for offered in DSH_MODES {
            let config = dsh_config(json!({ "mode": offered.as_str() }));
            assert!(
                config_rejection(&config).is_none(),
                "preset mode {} rejected",
                offered.as_str()
            );
        }
    }

    #[test]
    fn an_explicit_config_arg_is_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let config = dsh_config(json!({ "args": ["--config", "mine.yml"] }));
        let args = launch_args(&config, home.path(), workspace.path()).unwrap();
        assert_eq!(args, vec!["--config", "mine.yml"]);
    }

    #[test]
    fn other_profiles_keep_their_args() {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let config = normalize_acp_runtime(Some(&json!({
            "command": "opencode",
            "args": ["acp"],
            "profile": "opencode",
        })))
        .unwrap();
        assert_eq!(
            launch_args(&config, home.path(), workspace.path()).unwrap(),
            vec!["acp"]
        );
    }

    #[test]
    fn node_versions_are_read_against_the_engine_requirement() {
        assert_eq!(parse_node_version("v22.23.2\n"), Some((22, 23)));
        assert_eq!(parse_node_version("24.0.0"), Some((24, 0)));
        assert_eq!(parse_node_version("not a version"), None);

        assert!(node_version_is_supported((22, 19)));
        assert!(node_version_is_supported((22, 23)));
        assert!(node_version_is_supported((24, 0)));
        assert!(node_version_is_supported((25, 0)));
        // 22.18 and the whole of 23 are below or outside the supported range.
        assert!(!node_version_is_supported((22, 18)));
        assert!(!node_version_is_supported((23, 9)));
        assert!(!node_version_is_supported((20, 11)));
    }

    #[test]
    fn dsh_specs_track_the_version_panel_channel() {
        assert_eq!(
            DSH_INSTALL_SPECS[0],
            format!("{DSH_ACP_PACKAGE}@{DSH_DIST_TAG}")
        );
        assert_eq!(
            DSH_INSTALL_SPECS[1],
            format!("{DSH_LLM_PACKAGE}@{DSH_DIST_TAG}")
        );
        for spec in DSH_INSTALL_SPECS {
            assert!(
                spec.ends_with(&format!("@{DSH_DIST_TAG}")),
                "spec does not track {DSH_DIST_TAG}: {spec}"
            );
        }
        // Every plugin the tool tiers mount has to be installed too, or
        // choosing that mode boots a plugin tree that cannot settle.
        let (executor, tool) = platform_shell_plugins();
        for mounted in [Some(executor), tool].into_iter().flatten() {
            assert!(
                DSH_INSTALL_SPECS
                    .iter()
                    .any(|spec| spec.starts_with(&format!("{mounted}@"))),
                "{mounted} is mounted but never installed"
            );
        }
    }
}
