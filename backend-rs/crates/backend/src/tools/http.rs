//! Bounded HTTP reader backing the `Fetch` tool.
//!
//! [`fetch`] mirrors the Python oracle's `_fetch_url`: only `http`/`https` GET
//! requests, a short timeout, redirects followed, text-like responses only, and
//! both the response body and the returned snippet bounded. Network and decode
//! errors are mapped to model-safe text and never expose internals.

use std::time::Duration;

use reqwest::header::CONTENT_TYPE;

use super::{ToolError, ToolResult};

/// Largest response body (in bytes) read before truncating.
pub const MAX_FETCH_BYTES: usize = 500_000;
/// Largest normalized snippet (in characters) returned to the model.
pub const MAX_FETCH_CHARS: usize = 20_000;
/// Maximum (and default) request timeout in seconds.
pub const FETCH_TIMEOUT_SECONDS: u64 = 10;

/// Fetch a bounded text snippet from an `http`/`https` URL.
///
/// `timeout_seconds` must be in `1..=FETCH_TIMEOUT_SECONDS`. Returns a one-line
/// header (`Fetched <final-url> (<status>, <content-type>).`) followed by a
/// whitespace-normalized snippet of the body, with a `[response truncated]`
/// marker when the body or snippet was shortened.
pub async fn fetch(url: &str, timeout_seconds: u64) -> Result<ToolResult, ToolError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| ToolError::invalid("url must be an http or https URL"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
        return Err(ToolError::invalid("url must be an http or https URL"));
    }
    if !(1..=FETCH_TIMEOUT_SECONDS).contains(&timeout_seconds) {
        return Err(ToolError::invalid(format!(
            "timeout_seconds must be between 1 and {FETCH_TIMEOUT_SECONDS}"
        )));
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .build()
        .map_err(|_| ToolError::invalid("fetch client could not be created"))?;

    let mut response = client
        .get(parsed)
        .send()
        .await
        .map_err(|_| ToolError::invalid("fetch request failed"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(ToolError::invalid(format!(
            "fetch failed with status {}",
            status.as_u16()
        )));
    }

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    if !content_type.is_empty() && !is_text_like(&content_type) {
        return Err(ToolError::invalid(
            "fetch only supports text-like responses",
        ));
    }

    let final_url = response.url().to_string();

    // Stream the body so we stop reading once the byte cap is reached.
    let mut body: Vec<u8> = Vec::new();
    let mut bytes_seen = 0usize;
    let mut truncated_bytes = false;
    loop {
        let chunk = response
            .chunk()
            .await
            .map_err(|_| ToolError::invalid("fetch failed while reading the response"))?;
        let Some(chunk) = chunk else { break };
        if chunk.is_empty() {
            continue;
        }
        bytes_seen += chunk.len();
        let remaining = MAX_FETCH_BYTES.saturating_sub(body.len());
        if remaining > 0 {
            let take = remaining.min(chunk.len());
            body.extend_from_slice(&chunk[..take]);
        }
        if bytes_seen > MAX_FETCH_BYTES {
            truncated_bytes = true;
            break;
        }
    }

    let text = String::from_utf8_lossy(&body);
    let snippet: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_FETCH_CHARS)
        .collect();
    let suffix = if truncated_bytes || text.chars().count() > MAX_FETCH_CHARS {
        "\n[response truncated]"
    } else {
        ""
    };
    let content_type_label = if content_type.is_empty() {
        "unknown content-type"
    } else {
        content_type.as_str()
    };
    let header = format!(
        "Fetched {final_url} ({}, {content_type_label}).",
        status.as_u16()
    );
    Ok(ToolResult::completed(format!(
        "{header}\n{snippet}{suffix}"
    )))
}

/// Whether a `Content-Type` value names a text-like payload, matching the Python
/// oracle's check.
fn is_text_like(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || content_type.contains("json")
        || content_type.contains("xml")
        || content_type.contains("html")
}
