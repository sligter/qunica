use std::collections::{BTreeMap, HashSet};

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use time::{Date, Duration, Month};

use crate::api::{auth::current_user_id, error::ApiError, AppState};

const EXTERNAL_PROVIDER_ID: &str = "__external__";
const MODERATOR_AGENT_ID: &str = "__moderator__";
/// The widest real UTC offset, in minutes. Anything past it is a typo or an
/// attempt to page in the whole ledger under the guise of a timezone.
const MAX_TZ_OFFSET_MINUTES: i64 = 14 * 60;

#[derive(Debug, Default, Deserialize)]
pub struct TokenUsageQuery {
    from: Option<String>,
    to: Option<String>,
    group_id: Option<String>,
    provider_id: Option<String>,
    model: Option<String>,
    agent_id: Option<String>,
    /// Minutes east of UTC the caller's `from`/`to` dates are expressed in.
    ///
    /// Records are stored with a UTC timestamp, so bucketing them by the first
    /// ten characters put a UTC+8 user's whole morning on the previous day —
    /// "today" opened missing every request made before 08:00 local. Absent, or
    /// zero, keeps the old UTC behaviour.
    tz_offset_minutes: Option<i64>,
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
    timeline: Vec<UsageTimelinePoint>,
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
    let tz_offset_minutes = normalize_tz_offset(query.tz_offset_minutes)?;

    let query = TokenUsageQuery {
        from: from.clone(),
        to: to.clone(),
        group_id: normalize_filter(query.group_id, "group_id")?,
        provider_id: normalize_filter(query.provider_id, "provider_id")?,
        model: normalize_filter(query.model, "model")?,
        agent_id: normalize_filter(query.agent_id, "agent_id")?,
        tz_offset_minutes: Some(tz_offset_minutes),
    };

    // A local day straddles two UTC days, so the scan is widened by one on each
    // side and the exact boundary is enforced on the local day computed below.
    let scan_from = from.as_deref().and_then(|day| shift_day(day, -1));
    let scan_to = to.as_deref().and_then(|day| shift_day(day, 1));

    // ponytail: one owner/time scan keeps four groupings simple; move the
    // groupings into SQL if a user's usage history becomes too large for RAM.
    let rows = sqlx::query_as::<_, UsageRow>(
        "SELECT created_at AS day, group_id, group_name, agent_id, agent_name, \
                provider_id, provider_name, model, input_tokens, output_tokens, total_tokens \
         FROM token_usage_records \
         WHERE owner_id = ? \
           AND (? IS NULL OR substr(created_at, 1, 10) >= ?) \
           AND (? IS NULL OR substr(created_at, 1, 10) <= ?) \
         ORDER BY created_at ASC",
    )
    .bind(owner_id)
    .bind(scan_from.as_deref())
    .bind(scan_from.as_deref())
    .bind(scan_to.as_deref())
    .bind(scan_to.as_deref())
    .fetch_all(state.db.pool())
    .await
    .map_err(|_| ApiError::internal("failed to load token usage"))?;

    let rows = rows
        .into_iter()
        .filter_map(|mut row| {
            row.day = local_day(&row.day, tz_offset_minutes)?;
            let within = from.as_deref().is_none_or(|from| row.day.as_str() >= from)
                && to.as_deref().is_none_or(|to| row.day.as_str() <= to);
            within.then_some(row)
        })
        .collect::<Vec<_>>();

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
    let mut grouped =
        BTreeMap::<String, (String, UsageTotals, BTreeMap<String, UsageTotals>)>::new();
    for row in rows {
        let (id, name) = key(row);
        let item = grouped
            .entry(id)
            .or_insert_with(|| (name.clone(), UsageTotals::default(), BTreeMap::new()));
        item.0 = name;
        item.1.add(row);
        item.2.entry(row.day.clone()).or_default().add(row);
    }
    let mut items = grouped
        .into_iter()
        .map(|(id, (name, totals, timeline))| UsageBreakdown {
            id,
            name,
            totals,
            timeline: timeline
                .into_iter()
                .map(|(date, totals)| UsageTimelinePoint { date, totals })
                .collect(),
        })
        .collect::<Vec<_>>();
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

fn normalize_tz_offset(value: Option<i64>) -> Result<i64, ApiError> {
    let offset = value.unwrap_or(0);
    if offset.abs() > MAX_TZ_OFFSET_MINUTES {
        return Err(ApiError::invalid_input(
            "tz_offset_minutes must be within ±840",
        ));
    }
    Ok(offset)
}

/// Parse a `YYYY-MM-DD` string this module already validated.
fn parse_day(value: &str) -> Option<Date> {
    let year = value.get(0..4)?.parse::<i32>().ok()?;
    let month = value.get(5..7)?.parse::<u8>().ok()?;
    let day = value.get(8..10)?.parse::<u8>().ok()?;
    Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()
}

fn format_day(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

fn shift_day(value: &str, days: i64) -> Option<String> {
    parse_day(value)?
        .checked_add(Duration::days(days))
        .map(format_day)
}

/// The calendar day a UTC `created_at` falls on for a reader `offset_minutes`
/// east of UTC.
///
/// Hand-rolled rather than parsed: the stored form is always the RFC 3339 this
/// crate writes, and `time`'s parser is not compiled in.
fn local_day(created_at: &str, offset_minutes: i64) -> Option<String> {
    let date = parse_day(created_at)?;
    let hour = created_at.get(11..13)?.parse::<i64>().ok()?;
    let minute = created_at.get(14..16)?.parse::<i64>().ok()?;
    let shift = (hour * 60 + minute + offset_minutes).div_euclid(24 * 60);
    date.checked_add(Duration::days(shift)).map(format_day)
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
    if value.len() != 10 || value.split('-').count() != 3 || parse_day(value).is_none() {
        return Err(ApiError::invalid_input(format!(
            "{field} must be a YYYY-MM-DD date"
        )));
    }
    Ok(Some(value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{filter_options, local_day, matches_query, response, shift_day, TokenUsageQuery, UsageRow};

    fn row(day: &str, agent: &str, provider: &str, model: &str, total: i64) -> UsageRow {
        UsageRow {
            day: day.to_string(),
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
            row("2026-08-12", "agent-1", "provider-1", "model-a", 100),
            row("2026-08-13", "agent-1", "provider-1", "model-a", 25),
            row("2026-08-12", "agent-2", "provider-2", "model-b", 50),
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

        assert_eq!(result.summary.totals.total_tokens, 125);
        assert_eq!(result.summary.totals.calls, 2);
        assert_eq!(result.summary.active_agents, 1);
        assert_eq!(result.by_provider[0].id, "provider-1");
        assert_eq!(result.by_provider[0].timeline.len(), 2);
        assert_eq!(result.by_provider[0].timeline[1].totals.total_tokens, 25);
        assert_eq!(result.filters.providers.len(), 2);
    }

    #[test]
    fn buckets_records_by_the_readers_calendar_day() {
        // 16:10 UTC is already tomorrow morning at UTC+8, and 03:20 UTC is
        // still the previous evening at UTC-5.
        assert_eq!(
            local_day("2026-08-22T16:10:15.5680336Z", 480).as_deref(),
            Some("2026-08-23"),
        );
        assert_eq!(
            local_day("2026-08-22T03:20:47.0347287Z", -300).as_deref(),
            Some("2026-08-21"),
        );
        assert_eq!(
            local_day("2026-08-22T03:20:47Z", 0).as_deref(),
            Some("2026-08-22"),
        );
    }

    #[test]
    fn widens_the_scan_across_month_and_year_ends() {
        assert_eq!(shift_day("2026-09-01", -1).as_deref(), Some("2026-08-31"));
        assert_eq!(shift_day("2026-12-31", 1).as_deref(), Some("2027-01-01"));
    }
}
