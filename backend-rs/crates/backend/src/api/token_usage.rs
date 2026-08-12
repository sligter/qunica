use std::collections::{BTreeMap, HashSet};

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use time::{Date, Month};

use crate::api::{auth::current_user_id, error::ApiError, AppState};

const EXTERNAL_PROVIDER_ID: &str = "__external__";
const MODERATOR_AGENT_ID: &str = "__moderator__";

#[derive(Debug, Default, Deserialize)]
pub struct TokenUsageQuery {
    from: Option<String>,
    to: Option<String>,
    group_id: Option<String>,
    provider_id: Option<String>,
    model: Option<String>,
    agent_id: Option<String>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct UsageRow {
    day: String,
    group_id: Option<String>,
    group_name: String,
    agent_id: Option<String>,
    agent_name: String,
    provider_id: Option<String>,
    provider_name: String,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct UsageTotals {
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    calls: i64,
}

impl UsageTotals {
    fn add(&mut self, row: &UsageRow) {
        self.input_tokens = self.input_tokens.saturating_add(row.input_tokens.max(0));
        self.output_tokens = self.output_tokens.saturating_add(row.output_tokens.max(0));
        self.total_tokens = self.total_tokens.saturating_add(row.total_tokens.max(0));
        self.calls = self.calls.saturating_add(1);
    }
}

#[derive(Debug, Serialize)]
pub struct UsageSummary {
    #[serde(flatten)]
    totals: UsageTotals,
    active_agents: usize,
}

#[derive(Debug, Serialize)]
pub struct UsageTimelinePoint {
    date: String,
    #[serde(flatten)]
    totals: UsageTotals,
}

#[derive(Debug, Serialize)]
pub struct UsageBreakdown {
    id: String,
    name: String,
    #[serde(flatten)]
    totals: UsageTotals,
}

#[derive(Debug, Serialize)]
pub struct UsageFilterOption {
    id: String,
    name: String,
}

#[derive(Debug, Serialize)]
pub struct UsageFilterOptions {
    groups: Vec<UsageFilterOption>,
    providers: Vec<UsageFilterOption>,
    models: Vec<UsageFilterOption>,
    agents: Vec<UsageFilterOption>,
}

#[derive(Debug, Serialize)]
pub struct TokenUsageResponse {
    summary: UsageSummary,
    timeline: Vec<UsageTimelinePoint>,
    by_group: Vec<UsageBreakdown>,
    by_provider: Vec<UsageBreakdown>,
    by_model: Vec<UsageBreakdown>,
    by_agent: Vec<UsageBreakdown>,
    filters: UsageFilterOptions,
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<TokenUsageQuery>,
) -> Result<Json<TokenUsageResponse>, ApiError> {
    let owner_id = current_user_id(&headers, &state.auth.secret_key)?;
    let from = normalize_date(query.from.as_deref(), "from")?;
    let to = normalize_date(query.to.as_deref(), "to")?;
    if matches!((&from, &to), (Some(from), Some(to)) if from > to) {
        return Err(ApiError::invalid_input("from must not be after to"));
    }

    let query = TokenUsageQuery {
        from: from.clone(),
        to: to.clone(),
        group_id: normalize_filter(query.group_id, "group_id")?,
        provider_id: normalize_filter(query.provider_id, "provider_id")?,
        model: normalize_filter(query.model, "model")?,
        agent_id: normalize_filter(query.agent_id, "agent_id")?,
    };

    // ponytail: one owner/time scan keeps four groupings simple; move the
    // groupings into SQL if a user's usage history becomes too large for RAM.
    let rows = sqlx::query_as::<_, UsageRow>(
        "SELECT substr(created_at, 1, 10) AS day, group_id, group_name, agent_id, agent_name, \
                provider_id, provider_name, model, input_tokens, output_tokens, total_tokens \
         FROM token_usage_records \
         WHERE owner_id = ? \
           AND (? IS NULL OR substr(created_at, 1, 10) >= ?) \
           AND (? IS NULL OR substr(created_at, 1, 10) <= ?) \
         ORDER BY created_at ASC",
    )
    .bind(owner_id)
    .bind(from.as_deref())
    .bind(from.as_deref())
    .bind(to.as_deref())
    .bind(to.as_deref())
    .fetch_all(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to load token usage"))?;

    let filters = filter_options(&rows);
    let filtered = rows
        .iter()
        .filter(|row| matches_query(row, &query))
        .collect::<Vec<_>>();

    Ok(Json(response(&filtered, filters)))
}

fn response(rows: &[&UsageRow], filters: UsageFilterOptions) -> TokenUsageResponse {
    let mut totals = UsageTotals::default();
    let mut active_agents = HashSet::new();
    let mut timeline = BTreeMap::<String, UsageTotals>::new();
    for row in rows {
        totals.add(row);
        if let Some(agent_id) = row.agent_id.as_deref() {
            active_agents.insert(agent_id);
        }
        timeline.entry(row.day.clone()).or_default().add(row);
    }

    TokenUsageResponse {
        summary: UsageSummary {
            totals,
            active_agents: active_agents.len(),
        },
        timeline: timeline
            .into_iter()
            .map(|(date, totals)| UsageTimelinePoint { date, totals })
            .collect(),
        by_group: breakdown(rows, |row| {
            (
                row.group_id
                    .clone()
                    .unwrap_or_else(|| "__unknown_group__".to_string()),
                row.group_name.clone(),
            )
        }),
        by_provider: breakdown(rows, |row| {
            (
                row.provider_id
                    .clone()
                    .unwrap_or_else(|| EXTERNAL_PROVIDER_ID.to_string()),
                row.provider_name.clone(),
            )
        }),
        by_model: breakdown(rows, |row| (row.model.clone(), row.model.clone())),
        by_agent: breakdown(rows, |row| {
            (
                row.agent_id
                    .clone()
                    .unwrap_or_else(|| MODERATOR_AGENT_ID.to_string()),
                row.agent_name.clone(),
            )
        }),
        filters,
    }
}

fn breakdown(
    rows: &[&UsageRow],
    key: impl Fn(&UsageRow) -> (String, String),
) -> Vec<UsageBreakdown> {
    let mut grouped = BTreeMap::<String, UsageBreakdown>::new();
    for row in rows {
        let (id, name) = key(row);
        let item = grouped.entry(id.clone()).or_insert_with(|| UsageBreakdown {
            id,
            name: name.clone(),
            totals: UsageTotals::default(),
        });
        item.name = name;
        item.totals.add(row);
    }
    let mut items = grouped.into_values().collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .totals
            .total_tokens
            .cmp(&left.totals.total_tokens)
            .then_with(|| left.name.cmp(&right.name))
    });
    items
}

fn filter_options(rows: &[UsageRow]) -> UsageFilterOptions {
    let mut groups = BTreeMap::new();
    let mut providers = BTreeMap::new();
    let mut models = BTreeMap::new();
    let mut agents = BTreeMap::new();
    for row in rows {
        groups.insert(
            row.group_id
                .clone()
                .unwrap_or_else(|| "__unknown_group__".to_string()),
            row.group_name.clone(),
        );
        providers.insert(
            row.provider_id
                .clone()
                .unwrap_or_else(|| EXTERNAL_PROVIDER_ID.to_string()),
            row.provider_name.clone(),
        );
        models.insert(row.model.clone(), row.model.clone());
        agents.insert(
            row.agent_id
                .clone()
                .unwrap_or_else(|| MODERATOR_AGENT_ID.to_string()),
            row.agent_name.clone(),
        );
    }
    UsageFilterOptions {
        groups: options(groups),
        providers: options(providers),
        models: options(models),
        agents: options(agents),
    }
}

fn options(values: BTreeMap<String, String>) -> Vec<UsageFilterOption> {
    let mut values = values
        .into_iter()
        .map(|(id, name)| UsageFilterOption { id, name })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| left.name.cmp(&right.name));
    values
}

fn matches_query(row: &UsageRow, query: &TokenUsageQuery) -> bool {
    query
        .group_id
        .as_deref()
        .is_none_or(|id| row.group_id.as_deref() == Some(id))
        && query
            .provider_id
            .as_deref()
            .is_none_or(|id| row.provider_id.as_deref().unwrap_or(EXTERNAL_PROVIDER_ID) == id)
        && query
            .model
            .as_deref()
            .is_none_or(|model| row.model == model)
        && query
            .agent_id
            .as_deref()
            .is_none_or(|id| row.agent_id.as_deref().unwrap_or(MODERATOR_AGENT_ID) == id)
}

fn normalize_filter(value: Option<String>, field: &str) -> Result<Option<String>, ApiError> {
    let Some(value) = value else { return Ok(None) };
    let value = value.trim();
    if value.is_empty() || value.len() > 200 || value.chars().any(char::is_control) {
        return Err(ApiError::invalid_input(format!("invalid {field}")));
    }
    Ok(Some(value.to_string()))
}

fn normalize_date(value: Option<&str>, field: &str) -> Result<Option<String>, ApiError> {
    let Some(value) = value else { return Ok(None) };
    let parts = value.split('-').collect::<Vec<_>>();
    let valid = match parts.as_slice() {
        [year, month, day] => year
            .parse::<i32>()
            .ok()
            .zip(month.parse::<u8>().ok())
            .zip(day.parse::<u8>().ok())
            .and_then(|((year, month), day)| {
                Month::try_from(month)
                    .ok()
                    .and_then(|month| Date::from_calendar_date(year, month, day).ok())
            })
            .is_some(),
        _ => false,
    };
    if !valid || value.len() != 10 {
        return Err(ApiError::invalid_input(format!(
            "{field} must be a YYYY-MM-DD date"
        )));
    }
    Ok(Some(value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{filter_options, matches_query, response, TokenUsageQuery, UsageRow};

    fn row(agent: &str, provider: &str, model: &str, total: i64) -> UsageRow {
        UsageRow {
            day: "2026-08-12".to_string(),
            group_id: Some("group-1".to_string()),
            group_name: "Research".to_string(),
            agent_id: Some(agent.to_string()),
            agent_name: agent.to_string(),
            provider_id: Some(provider.to_string()),
            provider_name: provider.to_string(),
            model: model.to_string(),
            input_tokens: total - 10,
            output_tokens: 10,
            total_tokens: total,
        }
    }

    #[test]
    fn filters_and_groups_usage_once() {
        let rows = vec![
            row("agent-1", "provider-1", "model-a", 100),
            row("agent-2", "provider-2", "model-b", 50),
        ];
        let query = TokenUsageQuery {
            provider_id: Some("provider-1".to_string()),
            ..Default::default()
        };
        let filtered = rows
            .iter()
            .filter(|row| matches_query(row, &query))
            .collect::<Vec<_>>();
        let result = response(&filtered, filter_options(&rows));

        assert_eq!(result.summary.totals.total_tokens, 100);
        assert_eq!(result.summary.totals.calls, 1);
        assert_eq!(result.summary.active_agents, 1);
        assert_eq!(result.by_provider[0].id, "provider-1");
        assert_eq!(result.filters.providers.len(), 2);
    }
}
