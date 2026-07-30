//! Tavily-backed implementation of the `WebSearch` tool.

use std::time::Duration;

use serde_json::{json, Value};

use super::{controlled, http, ToolError, ToolResult};

pub const DEFAULT_SEARCH_RESULTS: u32 = 5;
const MAX_SEARCH_RESULTS: u32 = 20;
const MAX_SEARCH_QUERY_CHARS: usize = 500;
const MAX_SEARCH_RESPONSE_BYTES: usize = 5_000_000;

#[derive(Clone)]
pub(crate) struct TavilySearchConfig {
    pub(crate) api_key: String,
    pub(crate) search_url: String,
    pub(crate) max_results: u32,
    pub(crate) search_depth: String,
    pub(crate) include_answer: bool,
    pub(crate) include_raw_content: bool,
}

pub(crate) async fn search(
    config: Option<&TavilySearchConfig>,
    query: &str,
    max_results: u32,
) -> Result<ToolResult, ToolError> {
    if query.trim().is_empty() {
        return Err(ToolError::invalid("query must be non-empty"));
    }
    if query.chars().count() > MAX_SEARCH_QUERY_CHARS {
        return Err(ToolError::invalid(format!(
            "query must be at most {MAX_SEARCH_QUERY_CHARS} characters"
        )));
    }
    if !(1..=MAX_SEARCH_RESULTS).contains(&max_results) {
        return Err(ToolError::invalid(format!(
            "max_results must be between 1 and {MAX_SEARCH_RESULTS}"
        )));
    }
    let Some(config) = config else {
        return Ok(controlled::web_search_setup_required());
    };

    let effective_max_results = max_results.min(config.max_results);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(http::FETCH_TIMEOUT_SECONDS))
        .build()
        .map_err(|_| ToolError::invalid("web search client could not be created"))?;
    let mut response = client
        .post(&config.search_url)
        .bearer_auth(&config.api_key)
        .json(&json!({
            "api_key": config.api_key,
            "query": query,
            "max_results": effective_max_results,
            "search_depth": config.search_depth,
            "include_answer": config.include_answer,
            "include_raw_content": config.include_raw_content,
        }))
        .send()
        .await
        .map_err(|_| ToolError::invalid("web search request failed"))?;
    if !response.status().is_success() {
        return Err(ToolError::invalid(format!(
            "web search failed with status {}",
            response.status().as_u16()
        )));
    }

    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ToolError::invalid("web search failed while reading the response"))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_SEARCH_RESPONSE_BYTES {
            return Err(ToolError::invalid("web search response was too large"));
        }
        body.extend_from_slice(&chunk);
    }
    let data: Value = serde_json::from_slice(&body)
        .map_err(|_| ToolError::invalid("web search returned an invalid response"))?;
    let results = data
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(effective_max_results as usize)
        .filter_map(Value::as_object)
        .map(|item| {
            json!({
                "title": bounded_string(item.get("title"), 300),
                "url": bounded_string(item.get("url"), 1_000),
                "content": bounded_string(item.get("content"), 2_000),
            })
        })
        .collect::<Vec<_>>();

    Ok(ToolResult::completed(
        json!({
            "tool": "WebSearch",
            "status": "COMPLETED",
            "provider": "tavily",
            "answer": bounded_string(data.get("answer"), http::MAX_FETCH_CHARS),
            "results": results,
        })
        .to_string(),
    ))
}

fn bounded_string(value: Option<&Value>, max_chars: usize) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .take(max_chars)
        .collect()
}
