//! Shared Group read projection for API and Notes authorization.
//!
//! Note: keep the original projection and decoder together. Narrowing the
//! Notes query would change which malformed rows fail before workspace checks.
//! See specs/notes-failure-contract.md for this compatibility boundary.
use crate::api::error::ApiError;
use sqlx::SqlitePool;

pub(crate) const GROUP_COLUMNS: &str = "id, owner_id, workspace_id, auto_share_workspace_with_new_agents, name, description, announcement, avatar_url, \
     COALESCE((SELECT json_group_array(json_object( \
       'id', avatar_members.id, 'name', avatar_members.name, 'kind', avatar_members.kind, \
       'avatar_url', avatar_members.avatar_url)) \
       FROM ( \
         SELECT users.id AS id, users.name AS name, 'user' AS kind, users.avatar_url AS avatar_url, \
                group_members.joined_at AS joined_at \
         FROM group_members JOIN users ON users.id = group_members.user_id \
         WHERE group_members.group_id = groups.id AND group_members.status = 'active' \
         UNION ALL \
         SELECT agents.id AS id, COALESCE(group_agents.display_name, agents.name) AS name, \
                'agent' AS kind, agents.avatar_url AS avatar_url, group_agents.joined_at AS joined_at \
         FROM group_agents JOIN agents ON agents.id = group_agents.agent_id \
         WHERE group_agents.group_id = groups.id AND group_agents.status = 'active' \
               AND agents.status = 'active' \
         ORDER BY joined_at ASC, id ASC LIMIT 6 \
       ) AS avatar_members), '[]') AS avatar_members_json, \
     free_speech, proactive_mode, \
     allow_agent_free_mention, agent_free_mention_max_dispatches, communication_mode, \
     default_speaking_order_json, \
     scheduler_mode, agent_mention_policy, max_agent_steps, max_steps_per_agent, \
     max_scheduler_hops, max_moderator_calls, max_consecutive_failures, \
     max_total_failures, max_total_tokens, turn_timeout_seconds, moderator_enabled, \
     moderator_provider_id, moderator_model, \
     decision_enabled, decision_scenarios_json, router_agent_id, max_router_tasks, \
     muted_agent_ids_json, admin_agent_ids_json, muted_member_ids_json, status, \
     created_at, updated_at";

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct GroupRow {
    pub(crate) id: String,
    pub(crate) owner_id: String,
    pub(crate) workspace_id: Option<String>,
    pub(crate) auto_share_workspace_with_new_agents: i64,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) announcement: Option<String>,
    pub(crate) avatar_url: Option<String>,
    pub(crate) avatar_members_json: String,
    // SQLite stores booleans as integers; these are exposed as booleans below.
    pub(crate) free_speech: i64,
    pub(crate) proactive_mode: i64,
    pub(crate) allow_agent_free_mention: i64,
    pub(crate) agent_free_mention_max_dispatches: i64,
    pub(crate) communication_mode: String,
    pub(crate) default_speaking_order_json: Option<String>,
    pub(crate) scheduler_mode: String,
    pub(crate) agent_mention_policy: String,
    pub(crate) max_agent_steps: Option<i64>,
    pub(crate) max_steps_per_agent: i64,
    pub(crate) max_scheduler_hops: i64,
    pub(crate) max_moderator_calls: i64,
    pub(crate) max_consecutive_failures: i64,
    pub(crate) max_total_failures: i64,
    pub(crate) max_total_tokens: i64,
    pub(crate) turn_timeout_seconds: i64,
    pub(crate) moderator_enabled: i64,
    pub(crate) moderator_provider_id: Option<String>,
    pub(crate) moderator_model: Option<String>,
    pub(crate) decision_enabled: i64,
    pub(crate) decision_scenarios_json: String,
    pub(crate) router_agent_id: Option<String>,
    pub(crate) max_router_tasks: i64,
    pub(crate) muted_agent_ids_json: Option<String>,
    pub(crate) admin_agent_ids_json: Option<String>,
    pub(crate) muted_member_ids_json: Option<String>,
    pub(crate) status: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

pub(crate) async fn fetch_row(
    pool: &SqlitePool,
    group_id: &str,
) -> Result<Option<GroupRow>, ApiError> {
    let sql =
        format!("SELECT {GROUP_COLUMNS} FROM groups WHERE id = ? AND conversation_kind = 'group'");
    sqlx::query_as::<_, GroupRow>(&sql)
        .bind(group_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal("database error"))
}
