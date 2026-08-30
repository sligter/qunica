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
    app_control::{read as app_read, write as app_write, AppControlContext},
    controlled, http, media,
    shell::{
        self,
        resolve::{process_shell, shell_for, ResolvedShell, ShellPreference},
    },
    web_search, ApprovalGrants, FileEdit, MediaGenerationConfig, MountedSkill, TavilySearchConfig,
    ToolError, ToolResult, ToolStatus, WorkspaceMount, WorkspaceTools, MAX_GLOB_RESULTS,
    MAX_GREP_RESULTS, MAX_READ_LINES,
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
    /// Shared group-note directory, exposed through note-only tools.
    group_notes: Option<WorkspaceTools>,
    /// Skill metadata/instructions mounted for the non-executing SkillManager.
    mounted_skills: Vec<MountedSkill>,
    /// Tavily settings resolved for the agent owner, if configured.
    web_search: Option<TavilySearchConfig>,
    /// OpenAI-compatible image/video settings resolved for the agent owner.
    media_generation: Option<MediaGenerationConfig>,
    /// The MCP tools this agent may call.
    mcp_mount: McpMount,
    /// Shared connection pool, absent when no MCP tools are mounted.
    mcp_manager: Option<Arc<McpManager>>,
    /// Present only for the built-in Assistant. Its absence is what makes the
    /// app-control tools unavailable to every other agent.
    app_control: Option<AppControlContext>,
    /// Policy rules the user has approved for this thread. Empty by default, so
    /// a caller that never wires approvals through still gets the gate.
    approvals: ApprovalGrants,
    /// The interpreter shell commands run under. Defaults to whatever the host
    /// offers, and follows the account's preference once one is bound.
    shell: &'static ResolvedShell,
}

// `McpManager` holds live connections and so cannot derive `Debug`; the rest of
// the executor is worth printing, and the manager reduces to its presence.
impl std::fmt::Debug for ToolExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolExecutor")
            .field("workspace", &self.workspace)
            .field("group_notes", &self.group_notes)
            .field("mounted_skills", &self.mounted_skills)
            .field("web_search_configured", &self.web_search.is_some())
            .field(
                "media_generation_configured",
                &self.media_generation.is_some(),
            )
            .field("mcp_mount", &self.mcp_mount)
            .field("mcp_connected", &self.mcp_manager.is_some())
            .field("app_control", &self.app_control.is_some())
            .field("approvals", &self.approvals)
            .field("shell", &self.shell.program)
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
            group_notes: None,
            mounted_skills,
            web_search: None,
            media_generation: None,
            mcp_mount: McpMount::default(),
            mcp_manager: None,
            app_control: None,
            approvals: ApprovalGrants::default(),
            shell: process_shell(),
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
            group_notes: None,
            mounted_skills,
            web_search: None,
            media_generation: None,
            mcp_mount: McpMount::default(),
            mcp_manager: None,
            app_control: None,
            approvals: ApprovalGrants::default(),
            shell: process_shell(),
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

    /// Grant read/edit access to the shared notes directory without exposing the group workspace.
    pub fn with_group_notes(mut self, root: Option<PathBuf>) -> Result<Self, ToolError> {
        self.group_notes = root.map(WorkspaceTools::new).transpose()?;
        Ok(self)
    }

    pub fn has_group_notes(&self) -> bool {
        self.group_notes.is_some()
    }

    /// Bind the policy rules the user has approved for this thread.
    ///
    /// Consuming and returning `self` matches the other optional bindings, and
    /// the default of "nothing approved" is what makes the gate fail closed for
    /// callers that never set it.
    pub fn with_approvals(mut self, approvals: ApprovalGrants) -> Self {
        self.approvals = approvals;
        self
    }

    /// The approvals this executor runs with.
    pub fn approvals(&self) -> &ApprovalGrants {
        &self.approvals
    }

    /// Run shell commands under the interpreter this account asked for.
    ///
    /// Binding the resolved shell to the invocation — rather than reading a
    /// process-wide value at the point of each call — is what keeps the tool
    /// name offered to the model, the dialect its guidance teaches, and the
    /// interpreter that finally parses the command from drifting apart within a
    /// turn: all three read the same [`ResolvedShell`].
    pub fn with_shell_preference(mut self, preference: ShellPreference) -> Self {
        self.shell = shell_for(preference);
        self
    }

    /// The interpreter this executor runs shell commands under.
    pub fn shell(&self) -> &'static ResolvedShell {
        self.shell
    }

    /// Grant the app-control tools, scoped to one owner and conversation.
    ///
    /// Only the built-in Assistant gets this. Everything the app-control tools
    /// can reach is decided by the context, so an executor without one cannot
    /// read another owner's configuration by accident.
    pub fn with_app_control(mut self, context: AppControlContext) -> Self {
        self.app_control = Some(context);
        self
    }

    /// Bind the web-search settings resolved for this agent invocation.
    pub(crate) fn with_web_search(mut self, config: Option<TavilySearchConfig>) -> Self {
        self.web_search = config;
        self
    }

    /// Bind the OpenAI-compatible media settings resolved for this invocation.
    pub(crate) fn with_media_generation(mut self, config: Option<MediaGenerationConfig>) -> Self {
        self.media_generation = config;
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
            "ReadGroupNotes" | "EditGroupNote" => {
                let Some(notes) = self.group_notes.clone() else {
                    return Ok(controlled::setup_required(
                        name,
                        "this group has no shared notes",
                    ));
                };
                let raw_path = if name == "ReadGroupNotes" {
                    arg_str_opt(&args, "path").unwrap_or("index.md")
                } else {
                    arg_path(&args)?
                };
                let path = raw_path
                    .strip_prefix("Notes/")
                    .or_else(|| raw_path.strip_prefix("Notes\\"))
                    .unwrap_or(raw_path)
                    .to_string();
                if name == "ReadGroupNotes" {
                    run_blocking(move || notes.read(&path, 1, MAX_READ_LINES)).await
                } else {
                    let edits = arg_file_edits(&args)?;
                    run_blocking(move || notes.edit(&path, &edits)).await
                }
            }
            // The shell tool answers to whichever name the host advertises it
            // under, plus the other dialects' names: a model that has seen
            // `Bash` elsewhere should reach the shell rather than an "unknown
            // tool" error, and its result already states the real dialect.
            "Read" | "Write" | "Edit" | "DeleteFile" | "Glob" | "Grep" | "Bash" | "Pwsh"
            | "Cmd" | "Shell" | "ShellOutput" | "ShellKill" | "ShellJobs" => {
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
                let max_results = arg_u32(&args, "max_results", web_search::DEFAULT_SEARCH_RESULTS);
                web_search::search(self.web_search.as_ref(), query, max_results).await
            }
            "AskUser" => {
                let question = arg_str(&args, "question")?;
                let required = arg_bool(&args, "required", true);
                let choices = arg_string_list(&args, "choices");
                Ok(controlled::ask_user(question, required, &choices))
            }
            "AppList" | "AppGet" | "AppState" | "AppDocs" | "AppPropose" | "AppPrefill" => {
                let Some(context) = self.app_control.as_ref() else {
                    return Ok(controlled::setup_required(
                        name,
                        "app control is not available to this agent",
                    ));
                };
                match name {
                    "AppList" => app_read::list(context, &args).await,
                    "AppGet" => app_read::get(context, &args).await,
                    "AppDocs" => app_read::docs(&args),
                    "AppPropose" => app_write::propose(context, &args).await,
                    "AppPrefill" => app_write::prefill(&args),
                    _ => app_read::state(context).await,
                }
            }
            "GenerateImage" => {
                media::generate_image(
                    self.media_generation.as_ref(),
                    self.workspace_root(),
                    arg_str(&args, "prompt")?,
                    arg_str_opt(&args, "model"),
                )
                .await
            }
            "GenerateVideo" => {
                media::generate_video(
                    self.media_generation.as_ref(),
                    self.workspace_root(),
                    arg_str(&args, "prompt")?,
                    arg_str_opt(&args, "model"),
                )
                .await
            }
            "SkillManager" => {
                let action = arg_str_opt(&args, "action").unwrap_or("list");
                let skill_name = arg_str_opt(&args, "skill_name");
                Ok(controlled::skill_manager(
                    &self.mounted_skills,
                    action,
                    skill_name,
                ))
            }
            "TodoWrite" => Ok(controlled::todo_write(super::todo::parse_todos(&args))),
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
                let path = arg_path(&args)?.to_string();
                let offset = arg_usize_alias(&args, "offset", "start_line", 1)?;
                let limit = arg_usize_alias(&args, "limit", "limit", MAX_READ_LINES)?;
                run_blocking(move || workspace.read(&path, offset, limit)).await
            }
            "Write" => {
                let path = arg_path(&args)?.to_string();
                let content = arg_str(&args, "content")?.to_string();
                run_blocking(move || workspace.write(&path, &content)).await
            }
            "Edit" => {
                let path = arg_path(&args)?.to_string();
                let edits = arg_file_edits(&args)?;
                run_blocking(move || workspace.edit(&path, &edits)).await
            }
            "DeleteFile" => {
                let path = arg_path(&args)?.to_string();
                run_blocking(move || workspace.delete_file(&path)).await
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
            // The shell runs in the primary root only: its command policy is
            // built around a single root, so mounts are not reachable from a
            // shell.
            "Bash" | "Pwsh" | "Cmd" | "Shell" => {
                let command = arg_str(&args, "command")?.to_string();
                let timeout_seconds = arg_u64(
                    &args,
                    "timeout_seconds",
                    shell::DEFAULT_SHELL_TIMEOUT_SECONDS,
                );
                let run_in_background = arg_bool(&args, "run_in_background", false);
                shell::run_shell(
                    self.shell,
                    name,
                    workspace.root(),
                    &command,
                    timeout_seconds,
                    run_in_background,
                    &self.approvals,
                )
                .await
            }
            "ShellOutput" => shell::read_job_output(workspace.root(), arg_str(&args, "job_id")?),
            "ShellKill" => shell::kill_job(workspace.root(), arg_str(&args, "job_id")?),
            "ShellJobs" => Ok(shell::list_jobs(workspace.root())),
            // `dispatch` only routes the names above here.
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

/// Tau-style file path, with the old key retained for interrupted calls.
fn arg_path(args: &Value) -> Result<&str, ToolError> {
    arg_str_opt(args, "path")
        .or_else(|| arg_str_opt(args, "file_path"))
        .ok_or_else(|| ToolError::invalid("missing required string argument 'path'"))
}

/// Parse Tau's `edits[]` shape, while accepting the previous single-edit keys.
fn arg_file_edits(args: &Value) -> Result<Vec<FileEdit>, ToolError> {
    if let Some(value) = args.get("edits") {
        let items = value
            .as_array()
            .ok_or_else(|| ToolError::invalid("'edits' must be an array"))?;
        if items.is_empty() {
            return Err(ToolError::invalid(
                "edits must contain at least one replacement",
            ));
        }
        return items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let old_text = item.get("oldText").and_then(Value::as_str).ok_or_else(|| {
                    ToolError::invalid(format!(
                        "missing required string argument 'edits[{index}].oldText'"
                    ))
                })?;
                let new_text = item.get("newText").and_then(Value::as_str).ok_or_else(|| {
                    ToolError::invalid(format!(
                        "missing required string argument 'edits[{index}].newText'"
                    ))
                })?;
                Ok(FileEdit::new(old_text, new_text))
            })
            .collect();
    }

    let old_text = arg_str_opt(args, "oldText")
        .or_else(|| arg_str_opt(args, "old_string"))
        .ok_or_else(|| ToolError::invalid("missing required array argument 'edits'"))?;
    let new_text = arg_str_opt(args, "newText")
        .or_else(|| arg_str_opt(args, "new_string"))
        .ok_or_else(|| ToolError::invalid("missing required array argument 'edits'"))?;
    Ok(vec![FileEdit::new(old_text, new_text)])
}

/// Optional non-negative integer with a legacy alias and a default.
fn arg_usize_alias(
    args: &Value,
    key: &str,
    legacy_key: &str,
    default: usize,
) -> Result<usize, ToolError> {
    let Some(value) = args.get(key).or_else(|| args.get(legacy_key)) else {
        return Ok(default);
    };
    let value = value
        .as_u64()
        .ok_or_else(|| ToolError::invalid(format!("'{key}' must be a non-negative integer")))?;
    usize::try_from(value).map_err(|_| ToolError::invalid(format!("'{key}' is too large")))
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
