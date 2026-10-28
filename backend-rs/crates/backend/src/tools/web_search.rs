//! Provider adapters for `WebSearch` and DeepSeek's Markdown `Fetch`.

use std::time::Duration;

use serde_json::{json, Value};

use super::{controlled, http, ToolError, ToolResult};

pub const DEFAULT_SEARCH_RESULTS: u32 = 5;
const MAX_SEARCH_RESULTS: u32 = 20;
const MAX_SEARCH_QUERY_CHARS: usize = 500;
const MAX_SEARCH_RESPONSE_BYTES: usize = 5_000_000;

#[derive(Clone)]
pub(crate) enum WebSearchConfig {
    Tavily(TavilySearchConfig),
    DeepSeek(DeepSeekSearchConfig),
}

#[derive(Clone)]
pub(crate) struct DeepSeekSearchConfig {
    pub(crate) api_key: String,
    pub(crate) search_url: String,
    pub(crate) fetch_url: String,
    pub(crate) max_results: u32,
    pub(crate) need_content: bool,
}

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
    config: Option<&WebSearchConfig>,
    query: &str,
    max_results: u32,
) -> Result<ToolResult, ToolError> {
    if query.trim().is_empty() {
        return Err(ToolError::invalid("query must be non-empty"));
    }
    let (query_limit, result_limit) = match config {
        Some(WebSearchConfig::DeepSeek(_)) => (128, 50),
        _ => (MAX_SEARCH_QUERY_CHARS, MAX_SEARCH_RESULTS),
    };
    if query.chars().count() > query_limit {
        return Err(ToolError::invalid(format!(
            "query must be at most {query_limit} characters"
        )));
    }
    if !(1..=result_limit).contains(&max_results) {
        return Err(ToolError::invalid(format!(
            "max_results must be between 1 and {result_limit}"
        )));
    }
    let Some(config) = config else {
        return Ok(controlled::web_search_setup_required());
    };

    // Note: provider limits and payloads differ; normalize only the tool output
    // so switching providers does not change the agent-facing tool contract.
    let (provider, search_url, api_key, effective_max_results, request) = match config {
        WebSearchConfig::Tavily(config) => {
            let count = max_results.min(config.max_results);
            (
                "tavily",
                config.search_url.as_str(),
                config.api_key.as_str(),
                count,
                json!({
                    "api_key": config.api_key,
                    "query": query,
                    "max_results": count,
                    "search_depth": config.search_depth,
                    "include_answer": config.include_answer,
                    "include_raw_content": config.include_raw_content,
                }),
            )
        }
        WebSearchConfig::DeepSeek(config) => {
            let count = max_results.min(config.max_results);
            (
                "deepseek",
                config.search_url.as_str(),
                config.api_key.as_str(),
                count,
                json!({"query": query, "count": count, "need_content": config.need_content}),
            )
        }
    };
    let data = request_json(
        search_url,
        api_key,
        &request,
        http::FETCH_TIMEOUT_SECONDS,
        "web search",
    )
    .await?;
    let results = data
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| ToolError::invalid("web search returned an invalid response"))?
        .iter()
        .take(effective_max_results as usize)
        .filter_map(Value::as_object)
        .map(|item| {
            let content = if provider == "deepseek" {
                ["content", "summary", "snippet"]
                    .into_iter()
                    .filter_map(|key| item.get(key))
                    .find(|value| value.as_str().is_some_and(|text| !text.trim().is_empty()))
            } else {
                item.get("content")
            };
            json!({
                "title": bounded_string(item.get("title"), 300),
                "url": bounded_string(item.get("url"), 1_000),
                "content": bounded_string(content, 2_000),
            })
        })
        .collect::<Vec<_>>();

    Ok(ToolResult::completed(
        json!({
            "tool": "WebSearch",
            "status": "COMPLETED",
            "provider": provider,
            "answer": bounded_string(data.get("answer"), http::MAX_FETCH_CHARS),
            "results": results,
        })
        .to_string(),
    ))
}

pub(crate) async fn fetch(
    config: Option<&WebSearchConfig>,
    url: &str,
    timeout_seconds: u64,
) -> Result<ToolResult, ToolError> {
    let Some(WebSearchConfig::DeepSeek(config)) = config else {
        return http::fetch(url, timeout_seconds).await;
    };
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| ToolError::invalid("url must be an http or https URL"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || url.chars().count() > 8_192
    {
        return Err(ToolError::invalid(
            "url must be an http or https URL without credentials and at most 8192 characters",
        ));
    }
    if !(1..=http::FETCH_TIMEOUT_SECONDS).contains(&timeout_seconds) {
        return Err(ToolError::invalid(format!(
            "timeout_seconds must be between 1 and {}",
            http::FETCH_TIMEOUT_SECONDS
        )));
    }
    let data = request_json(
        &config.fetch_url,
        &config.api_key,
        &json!({"url": url}),
        timeout_seconds,
        "fetch",
    )
    .await?;
    let content = data
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("fetch returned an invalid response"))?;
    Ok(ToolResult::completed(
        json!({
            "tool": "Fetch",
            "status": "COMPLETED",
            "provider": "deepseek",
            "url": url,
            "title": bounded_string(data.get("title"), 300),
            "content": content.chars().take(http::MAX_FETCH_CHARS).collect::<String>(),
            "truncated": content.chars().count() > http::MAX_FETCH_CHARS,
        })
        .to_string(),
    ))
}

async fn request_json(
    url: &str,
    api_key: &str,
    request: &Value,
    timeout_seconds: u64,
    operation: &str,
) -> Result<Value, ToolError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .build()
        .map_err(|_| ToolError::invalid(format!("{operation} client could not be created")))?;
    let mut response = client
        .post(url)
        .bearer_auth(api_key)
        .json(request)
        .send()
        .await
        .map_err(|_| ToolError::invalid(format!("{operation} request failed")))?;
    if !response.status().is_success() {
        return Err(ToolError::invalid(format!(
            "{operation} failed with status {}",
            response.status().as_u16()
        )));
    }

    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ToolError::invalid(format!("{operation} failed while reading the response")))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_SEARCH_RESPONSE_BYTES {
            return Err(ToolError::invalid(format!(
                "{operation} response was too large"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body)
        .map_err(|_| ToolError::invalid(format!("{operation} returned an invalid response")))
}

fn bounded_string(value: Option<&Value>, max_chars: usize) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .take(max_chars)
        .collect()
}

#[cfg(test)]
mod tests {
    use axum::{http::StatusCode, Router};

    use super::*;

    async fn config_for_response(status: StatusCode, body: String) -> WebSearchConfig {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().fallback(move || {
            let body = body.clone();
            async move { (status, body) }
        });
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        WebSearchConfig::DeepSeek(DeepSeekSearchConfig {
            api_key: "secret-that-must-not-leak".into(),
            search_url: url.clone(),
            fetch_url: url,
            max_results: 50,
            need_content: false,
        })
    }

    #[tokio::test]
    async fn deepseek_search_normalizes_content_summary_and_snippet_with_bounds() {
        let config = config_for_response(
            StatusCode::OK,
            json!({"results": [
                {"title": "正文", "content": "正文".repeat(2_001), "summary": "summary"},
                {"content": "", "summary": "long summary", "snippet": "short snippet"},
                {"content": "  ", "summary": "", "snippet": "fallback snippet"},
                {"summary": "not returned"}
            ]})
            .to_string(),
        )
        .await;
        // Count Unicode characters, not UTF-8 bytes; 128 Chinese characters are valid.
        let result = search(Some(&config), &"中".repeat(128), 3).await.unwrap();
        let output: Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["provider"], "deepseek");
        assert_eq!(output["results"].as_array().unwrap().len(), 3);
        assert_eq!(
            output["results"][0]["content"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            2_000
        );
        assert_eq!(output["results"][1]["content"], "long summary");
        assert_eq!(output["results"][2]["content"], "fallback snippet");
        assert!(search(Some(&config), &"中".repeat(129), 1).await.is_err());
        assert!(search(Some(&config), " ", 1).await.is_err());
        assert!(search(Some(&config), "query", 0).await.is_err());
        assert!(search(Some(&config), "query", 51).await.is_err());
    }

    #[tokio::test]
    async fn deepseek_search_accepts_fifty_results_and_empty_results() {
        let config = config_for_response(StatusCode::OK, json!({"results": []}).to_string()).await;
        let result = search(Some(&config), "query", 50).await.unwrap();
        let output: Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["results"], json!([]));
    }

    #[tokio::test]
    async fn deepseek_fetch_preserves_markdown_and_marks_truncation() {
        let markdown = format!("# 标题\n\n- 正文\n{}", "字".repeat(http::MAX_FETCH_CHARS));
        let config = config_for_response(
            StatusCode::OK,
            json!({"title": "标题", "content": markdown}).to_string(),
        )
        .await;
        let result = fetch(Some(&config), "https://example.test/article", 10)
            .await
            .unwrap();
        let output: Value = serde_json::from_str(&result.output).unwrap();
        let content = output["content"].as_str().unwrap();
        assert!(content.starts_with("# 标题\n\n- 正文\n"));
        assert_eq!(content.chars().count(), http::MAX_FETCH_CHARS);
        assert_eq!(output["truncated"], true);
        assert_eq!(output["title"], "标题");
        for url in [
            "file:///secret",
            "https://user:password@example.test",
            "https://example.test@",
        ] {
            assert!(fetch(Some(&config), url, 10).await.is_err());
        }
        assert!(fetch(
            Some(&config),
            &format!("https://example.test/{}", "a".repeat(8_192)),
            10
        )
        .await
        .is_err());
        assert!(fetch(Some(&config), "https://example.test", 0)
            .await
            .is_err());
        assert!(fetch(Some(&config), "https://example.test", 11)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn deepseek_provider_errors_are_bounded_and_do_not_echo_secrets() {
        for (status, body, expected) in [
            (
                StatusCode::UNAUTHORIZED,
                "secret-that-must-not-leak".into(),
                "status 401",
            ),
            (
                StatusCode::PAYMENT_REQUIRED,
                "secret-that-must-not-leak".into(),
                "status 402",
            ),
            (
                StatusCode::TOO_MANY_REQUESTS,
                "secret-that-must-not-leak".into(),
                "status 429",
            ),
            (
                StatusCode::OK,
                "not JSON secret-that-must-not-leak".into(),
                "invalid response",
            ),
            (
                StatusCode::OK,
                json!({"error": "secret-that-must-not-leak"}).to_string(),
                "invalid response",
            ),
            (
                StatusCode::OK,
                "a".repeat(MAX_SEARCH_RESPONSE_BYTES + 1),
                "too large",
            ),
        ] {
            let config = config_for_response(status, body).await;
            for error in [
                search(Some(&config), "query", 1).await.unwrap_err(),
                fetch(Some(&config), "https://example.test", 10)
                    .await
                    .unwrap_err(),
            ] {
                let message = error.to_string();
                assert!(message.contains(expected), "{message}");
                assert!(!message.contains("secret-that-must-not-leak"));
            }
        }
    }
}
