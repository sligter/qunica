use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Binding {
    Team(Uuid),
    Private(Uuid),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerSpec {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub binding: Binding,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Starting,
    Running,
    Executing,
    Stopping,
    Stopped,
    Rebuilding,
    Destroying,
    Destroyed,
    Uncertain,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Computer {
    pub supervisor_id: Uuid,
    pub spec: ComputerSpec,
    pub generation: String,
    pub status: Status,
    pub home_present: bool,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Start,
    Stop,
    Rebuild,
    Destroy,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActionRequest {
    pub expected_generation: String,
    pub action: Action,
    pub confirm: Option<Uuid>,
    #[serde(default)]
    pub purge_home: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecRequest {
    pub expected_generation: String,
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    60
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExecResult {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
}
