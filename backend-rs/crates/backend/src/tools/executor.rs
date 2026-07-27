//! [`ToolExecutor`]: the facade the group runtime drives to run a named tool.
//!
//! It binds an optional local workspace and dispatches a tool name plus JSON
//! arguments to the right implementation, always returning a [`ToolResult`] with
//! model-safe `output`. Workspace-scoped tools invoked without a workspace report
//! `WORKSPACE_REQUIRED`; unknown tools and bad arguments report `FAILED`; and an
//! internal [`ToolError::Io`] is collapsed to a generic message so no local path
//! leaks back to the model.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde_json::Value;

use crate::mcp::{is_mcp_tool_name, McpManager, McpServerConfig, McpToolBinding};

use super::{
    bash, controlled, http, MountedSkill, ToolError, ToolResult, ToolStatus, WorkspaceMount,
    WorkspaceTools, MAX_GLOB_RESULTS, MAX_GREP_RESULTS, MAX_READ_LINES,
};

/// The MCP tools mounted into an executor, with the servers needed to call them.
///
/// Bindings are resolved once when the agent's invocation context is built, so
/// dispatch is a lookup rather than a re-listing round trip per tool call. The
/// servers that could not be reached are carried alongside, because the system
/// prompt has to say why an expected server contributed nothing.
#[derive(Debug, Clone, Default)]
pub struct McpMount {
    /// Exposed tool name → binding.
    bindings: HashMap<String, McpToolBinding>,
    /// Server id → the config to connect with.
    servers: HashMap<String, McpServerConfig>,
    /// `(server name, reason)` for each server that failed to list its tools.
    failures: Vec<(String, String)>,
}

impl McpMount {
    /// Build a mount from resolved bindings, the configs they address, and the
    /// servers that could not be listed.
    ///
    /// A binding whose server config is absent is dropped: without the config
    /// there is no way to reach the server, and advertising the tool anyway
    /// would give the model something it can only fail to call.
    pub fn new(
        bindings: Vec<McpToolBinding>,
        servers: Vec<McpServerConfig>,
        failures: Vec<(String, String)>,
    ) -> Self {
        let servers: HashMap<String, McpServerConfig> = servers
            .into_iter()
            .map(|config| (config.id.clone(), config))
            .collect();
        let mut failures = failures;
        let mut resolved: HashMap<String, McpToolBinding> = HashMap::new();
        for binding in bindings {
            if !servers.contains_key(&binding.server_id) {
                continue;
            }
            // Two servers whose names slugify identically ("Notion (work)" and
            // "Notion-work" both become `notion_work`) produce the same exposed
            // name. Collecting into a map would let the later one overwrite the
            // earlier, so the model would call one server and silently reach the
            // other. Refuse the collision instead, and say so in the prompt.
            if let Some(existing) = resolved.get(&binding.exposed_name) {
                if existing.server_id != binding.server_id {
                    failures.push((
                        binding.server_name.clone(),
                        format!(
                            "its tool names collide with '{}' — rename one of them so their \
                             tool prefixes differ",
                            existing.server_name
                        ),
                    ));
                }
                continue;
            }
            resolved.insert(binding.exposed_name.clone(), binding);
        }
        // One unreachable server should produce one line, not one per tool.
        failures.dedup();
        Self {
            bindings: resolved,
            servers,
            failures,
        }
    }

    /// Whether this mount contributes nothing at all — no tools and no failure
    /// worth reporting.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty() && self.failures.is_empty()
    }

    /// The mounted bindings, for building provider tool definitions.
    pub fn bindings(&self) -> impl Iterator<Item = &McpToolBinding> {
        self.bindings.values()
    }

    /// The servers that failed to list their tools, as `(name, reason)`.
    pub fn failures(&self) -> &[(String, String)] {
        &self.failures
    }
}

/// Executes workspace, network and MCP tools by name with JSON arguments.
#[derive(Clone)]
pub struct ToolExecutor {
    /// The bound workspace, or `None` when no local workspace is configured.
    workspace: Option<WorkspaceTools>,
    /// Skill metadata/instructions mounted for the non-executing SkillManager.
    mounted_skills: Vec<MountedSkill>,
    /// The MCP tools this agent may call.
    mcp_mount: McpMount,
    /// Shared connection pool, absent when no MCP tools are mounted.
    mcp_manager: Option<Arc<McpManager>>,
}

// `McpManager` holds live connections and so cannot derive `Debug`; the rest of
// the executor is worth printing, and the manager reduces to its presence.
impl std::fmt::Debug for ToolExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolExecutor")
            .field("workspace", &self.workspace)
            .field("mounted_skills", &self.mounted_skills)
            .field("mcp_mount", &self.mcp_mount)
            .field("mcp_connected", &self.mcp_manager.is_some())
            .finish()
    }
}

impl ToolExecutor {
    /// Build an executor bound to `workspace_root`, or to no workspace when
    /// `None`. Returns [`ToolError::Invalid`] if a given root is missing or is
    /// not a directory.
    pub fn new(workspace_root: Option<PathBuf>) -> Result<Self, ToolError> {
        Self::new_with_skills(workspace_root, Vec::new())
    }

    /// Build an executor bound to `workspace_root` and the given mounted skills.
    pub fn new_with_skills(
        workspace_root: Option<PathBuf>,
        mounted_skills: Vec<MountedSkill>,
    ) -> Result<Self, ToolError> {
        Self::new_with_mounts(workspace_root, Vec::new(), mounted_skills)
    }

    /// Build an executor whose primary root is `workspace_root`, with `mounts`
    /// addressable by name and the given mounted skills. Mounts without a
    /// primary root are dropped: there is no address space to hang them off.
    pub fn new_with_mounts(
        workspace_root: Option<PathBuf>,
        mounts: Vec<WorkspaceMount>,
        mounted_skills: Vec<MountedSkill>,
    ) -> Result<Self, ToolError> {
        let workspace = match workspace_root {
            Some(root) => Some(WorkspaceTools::with_mounts(root, mounts)?),
            None => None,
        };
        Ok(Self {
            workspace,
            mounted_skills,
            mcp_mount: McpMount::default(),
            mcp_manager: None,
        })
    }

    /// Build an executor with no local workspace; only non-workspace tools run.
    pub fn without_workspace() -> Self {
        Self::without_workspace_with_skills(Vec::new())
    }

    /// Build an executor with no local workspace and the given mounted skills.
    pub fn without_workspace_with_skills(mounted_skills: Vec<MountedSkill>) -> Self {
        Self {
            workspace: None,
            mounted_skills,
            mcp_mount: McpMount::default(),
            mcp_manager: None,
        }
    }

    /// Mount MCP tools, routed through `manager`'s connection pool.
    ///
    /// Consuming and returning `self` keeps the existing constructors unchanged
    /// for every caller that does not use MCP.
    pub fn with_mcp(mut self, manager: Arc<McpManager>, mount: McpMount) -> Self {
        self.mcp_manager = Some(manager);
        self.mcp_mount = mount;
        self
    }

    /// The MCP tools mounted into this executor.
    pub fn mcp_mount(&self) -> &McpMount {
        &self.mcp_mount
    }

    /// The bound primary workspace root, if any.
    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace.as_ref().map(WorkspaceTools::root)
    }

    /// The named mounts retained alongside the primary root.
    pub fn workspace_mounts(&self) -> &[WorkspaceMount] {
        self.workspace
            .as_ref()
            .map(WorkspaceTools::mounts)
            .unwrap_or_default()
    }

    /// Execute `name` with `args`, returning a model-safe [`ToolResult`]. Never
    /// panics: unknown tools and rejected requests come back as
    /// [`ToolStatus::Failed`].
    pub async fn execute(&self, name: &str, args: Value) -> ToolResult {
        match self.dispatch(name, args).await {
            Ok(result) => result,
            Err(error) => failure(error),
        }
    }

    /// Route a single tool call, surfacing argument/execution errors as
    /// [`ToolError`] for [`execute`](Self::execute) to render.
    async fn dispatch(&self, name: &str, args: Value) -> Result<ToolResult, ToolError> {
        match name {
            "Read" | "Write" | "Edit" | "Glob" | "Grep" | "Bash" => {
                let Some(workspace) = self.workspace.clone() else {
                    return Ok(controlled::workspace_required(name));
                };
                self.run_workspace_tool(name, workspace, args).await
            }
            "Fetch" => {
                let url = arg_str(&args, "url")?.to_string();
                let timeout_seconds =
                    arg_u64(&args, "timeout_seconds", http::FETCH_TIMEOUT_SECONDS);
                http::fetch(&url, timeout_seconds).await
            }
            "WebSearch" => {
                let query = arg_str(&args, "query")?;
                let max_results = arg_u32(&args, "max_results", controlled::DEFAULT_SEARCH_RESULTS);
                controlled::web_search(query, max_results)
            }
            "AskUser" => {
                let question = arg_str(&args, "question")?;
                let required = arg_bool(&args, "required", true);
                let choices = arg_string_list(&args, "choices");
                Ok(controlled::ask_user(question, required, &choices))
            }
            "GenerateImage" => Ok(controlled::generate_image(arg_str(&args, "prompt")?)),
            "GenerateVideo" => Ok(controlled::generate_video(arg_str(&args, "prompt")?)),
            "SkillManager" => {
                let action = arg_str_opt(&args, "action").unwrap_or("list");
                let skill_name = arg_str_opt(&args, "skill_name");
                Ok(controlled::skill_manager(
                    &self.mounted_skills,
                    action,
                    skill_name,
                ))
            }
            "RunSubAgent" => {
                let task = arg_str(&args, "task")?;
                Ok(ToolResult {
                    status: ToolStatus::SetupRequired,
                    output: format!(
                        "RunSubAgent setup required: no sub-agent executor is configured for task '{task}'."
                    ),
                })
            }
            "TodoWrite" => Ok(controlled::todo_write(arg_todos(&args))),
            "ExitPlanMode" => Ok(controlled::exit_plan_mode(arg_str(&args, "plan")?)),
            name if is_mcp_tool_name(name) => Ok(self.run_mcp_tool(name, args).await),
            _ => Ok(unknown_tool(name)),
        }
    }

    /// Call a mounted MCP tool.
    ///
    /// Every outcome is a [`ToolResult`], never an `Err`: a server that is down,
    /// slow, or rejecting arguments is something the model should read and react
    /// to, exactly like a failed file read, rather than an error that aborts the
    /// agent's turn.
    async fn run_mcp_tool(&self, name: &str, args: Value) -> ToolResult {
        let Some(manager) = self.mcp_manager.as_ref() else {
            return ToolResult {
                status: ToolStatus::SetupRequired,
                output: format!("Tool '{name}' needs an MCP server, but none is configured."),
            };
        };
        let Some(binding) = self.mcp_mount.bindings.get(name) else {
            return unknown_tool(name);
        };
        let Some(config) = self.mcp_mount.servers.get(&binding.server_id) else {
            return ToolResult {
                status: ToolStatus::SetupRequired,
                output: format!(
                    "MCP server '{}' is no longer configured, so '{name}' cannot run.",
                    binding.server_name
                ),
            };
        };

        match manager.call_tool(config, &binding.tool_name, &args).await {
            Ok(outcome) => ToolResult {
                status: if outcome.is_error {
                    ToolStatus::Failed
                } else {
                    ToolStatus::Completed
                },
                output: outcome.text,
            },
            Err(error) => ToolResult {
                status: ToolStatus::Failed,
                output: format!("MCP server '{}' failed: {error}", binding.server_name),
            },
        }
    }

    /// Dispatch the workspace-scoped tools. File tools run on a blocking thread
    /// so their synchronous `std::fs` calls do not stall the async executor;
    /// `Bash` is already async.
    async fn run_workspace_tool(
        &self,
        name: &str,
        workspace: WorkspaceTools,
        args: Value,
    ) -> Result<ToolResult, ToolError> {
        match name {
            "Read" => {
                let file_path = arg_str(&args, "file_path")?.to_string();
                let start_line = arg_usize(&args, "start_line", 1);
                let limit = arg_usize(&args, "limit", MAX_READ_LINES);
                run_blocking(move || workspace.read(&file_path, start_line, limit)).await
            }
            "Write" => {
                let file_path = arg_str(&args, "file_path")?.to_string();
                let content = arg_str(&args, "content")?.to_string();
                run_blocking(move || workspace.write(&file_path, &content)).await
            }
            "Edit" => {
                let file_path = arg_str(&args, "file_path")?.to_string();
                let old_string = arg_str(&args, "old_string")?.to_string();
                let new_string = arg_str(&args, "new_string")?.to_string();
                let replace_all = arg_bool(&args, "replace_all", false);
                run_blocking(move || {
                    workspace.edit(&file_path, &old_string, &new_string, replace_all)
                })
                .await
            }
            "Glob" => {
                let pattern = arg_str_opt(&args, "pattern").unwrap_or("**/*").to_string();
                let limit = arg_usize(&args, "limit", MAX_GLOB_RESULTS);
                run_blocking(move || workspace.glob(&pattern, limit)).await
            }
            "Grep" => {
                let pattern = arg_str(&args, "pattern")?.to_string();
                let path = arg_str_opt(&args, "path").unwrap_or("**/*").to_string();
                let limit = arg_usize(&args, "limit", MAX_GREP_RESULTS);
                run_blocking(move || workspace.grep(&pattern, &path, limit)).await
            }
            // `Bash` runs in the primary root only: its command guard is built
            // around a single root, so mounts are not reachable from a shell.
            "Bash" => {
                let command = arg_str(&args, "command")?.to_string();
                let timeout_seconds =
                    arg_u64(&args, "timeout_seconds", bash::DEFAULT_BASH_TIMEOUT_SECONDS);
                bash::run_bash(workspace.root(), &command, timeout_seconds).await
            }
            // `dispatch` only routes the six names above here.
            other => Ok(unknown_tool(other)),
        }
    }
}

/// Run a synchronous file-tool closure on a blocking thread, mapping a join
/// failure to a model-safe error.
async fn run_blocking<F>(f: F) -> Result<ToolResult, ToolError>
where
    F: FnOnce() -> Result<ToolResult, ToolError> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .unwrap_or_else(|_| Err(ToolError::invalid("tool execution failed unexpectedly")))
}

/// Render a [`ToolError`] as a failed, model-safe result.
fn failure(error: ToolError) -> ToolResult {
    ToolResult {
        status: ToolStatus::Failed,
        output: error.model_safe_message(),
    }
}

/// Result for a tool name this runtime does not implement.
fn unknown_tool(name: &str) -> ToolResult {
    ToolResult {
        status: ToolStatus::Failed,
        output: format!("Tool '{name}' is unavailable in this runtime."),
    }
}

/// Required string argument.
fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid(format!("missing required string argument '{key}'")))
}

/// Optional string argument.
fn arg_str_opt<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// Optional `usize` argument with a default.
fn arg_usize(args: &Value, key: &str, default: usize) -> usize {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(default)
}

/// Optional `u64` argument with a default.
fn arg_u64(args: &Value, key: &str, default: u64) -> u64 {
    args.get(key).and_then(Value::as_u64).unwrap_or(default)
}

/// Optional `u32` argument with a default (saturating to `u32::MAX`).
fn arg_u32(args: &Value, key: &str, default: u32) -> u32 {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|value| value.min(u32::MAX as u64) as u32)
        .unwrap_or(default)
}

/// Optional boolean argument with a default.
fn arg_bool(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// Collect a JSON array of strings, ignoring non-string elements.
fn arg_string_list(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// `TodoWrite` accepts either a list of strings or a single string.
fn arg_todos(args: &Value) -> Vec<String> {
    match args.get("todos") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        Some(Value::String(single)) => vec![single.clone()],
        _ => Vec::new(),
    }
}
