//! OpenAI-compatible image and video generation.

use std::{path::Path, time::Duration};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::{header::CONTENT_TYPE, multipart::Form, Response, StatusCode, Url};
use serde_json::{json, Value};
use tokio::{fs, io::AsyncWriteExt};
use uuid::Uuid;

use super::{controlled, resolve_workspace_path, ToolError, ToolResult};

const MAX_PROMPT_CHARS: usize = 20_000;
const MAX_JSON_BYTES: usize = 64 * 1024 * 1024;
const MAX_MEDIA_BYTES: usize = 512 * 1024 * 1024;
const VIDEO_POLL_ATTEMPTS: usize = 60;
const VIDEO_POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub(crate) struct MediaGenerationConfig {
    pub(crate) api_key: String,
    pub(crate) base_url: String,
    pub(crate) image_model: Option<String>,
    pub(crate) image_endpoint: String,
    pub(crate) video_model: Option<String>,
    pub(crate) video_endpoint: String,
    pub(crate) video_status_endpoint: String,
    pub(crate) video_content_endpoint: String,
}

#[derive(Clone, Copy)]
enum MediaKind {
    Image,
    Video,
}

impl MediaKind {
    fn tool(self) -> &'static str {
        match self {
            Self::Image => "GenerateImage",
            Self::Video => "GenerateVideo",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
        }
    }

    fn default_extension(self) -> &'static str {
        match self {
            Self::Image => "png",
            Self::Video => "mp4",
        }
    }

    fn configured_model(self, config: &MediaGenerationConfig) -> Option<&str> {
        match self {
            Self::Image => config.image_model.as_deref(),
            Self::Video => config.video_model.as_deref(),
        }
    }

    fn endpoint(self, config: &MediaGenerationConfig) -> &str {
        match self {
            Self::Image => &config.image_endpoint,
            Self::Video => &config.video_endpoint,
        }
    }
}

#[derive(Debug)]
enum MediaSource {
    Base64(String),
    Url(String),
}

#[derive(Debug)]
struct MediaItem {
    source: MediaSource,
    mime: Option<String>,
}

pub(crate) async fn generate_image(
    config: Option<&MediaGenerationConfig>,
    workspace_root: Option<&Path>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<ToolResult, ToolError> {
    generate(
        MediaKind::Image,
        config,
        workspace_root,
        prompt,
        model_override,
    )
    .await
}

pub(crate) async fn generate_video(
    config: Option<&MediaGenerationConfig>,
    workspace_root: Option<&Path>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<ToolResult, ToolError> {
    generate(
        MediaKind::Video,
        config,
        workspace_root,
        prompt,
        model_override,
    )
    .await
}

async fn generate(
    kind: MediaKind,
    config: Option<&MediaGenerationConfig>,
    workspace_root: Option<&Path>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<ToolResult, ToolError> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(ToolError::invalid("prompt must be non-empty"));
    }
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(ToolError::invalid(format!(
            "prompt must be at most {MAX_PROMPT_CHARS} characters"
        )));
    }
    let Some(config) = config else {
        return Ok(controlled::setup_required(
            kind.tool(),
            "No OpenAI-compatible media API key is configured in Settings > Media.",
        ));
    };
    let model = model_override
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .or_else(|| kind.configured_model(config));
    let Some(model) = model else {
        return Ok(controlled::setup_required(
            kind.tool(),
            &format!(
                "No default {} generation model is configured in Settings > Media.",
                kind.label()
            ),
        ));
    };
    if model.chars().count() > 200 {
        return Err(ToolError::invalid("model must be at most 200 characters"));
    }
    let Some(workspace_root) = workspace_root else {
        return Ok(controlled::workspace_required(kind.tool()));
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|_| ToolError::invalid("media client could not be created"))?;
    let url = build_endpoint_url(&config.base_url, kind.endpoint(config), None)?;
    let response = send_generation_request(&client, config, kind, url, model, prompt).await?;
    let (mut paths, mut payload) =
        consume_generation_response(&client, config, workspace_root, kind, response).await?;

    if matches!(kind, MediaKind::Video) && paths.is_empty() {
        let data = payload
            .take()
            .ok_or_else(|| ToolError::invalid("video generation returned no job metadata"))?;
        let job_id = job_id(&data)
            .ok_or_else(|| ToolError::invalid("video generation returned no media or job id"))?;
        let completed = if video_completed(&data) {
            data
        } else {
            poll_video(&client, config, &job_id).await?
        };
        paths = save_items(
            &client,
            config,
            workspace_root,
            kind,
            extract_media_items(&completed, kind),
        )
        .await?;
        if paths.is_empty() {
            let content_url = build_endpoint_url(
                &config.base_url,
                &config.video_content_endpoint,
                Some(&job_id),
            )?;
            let response = client
                .get(content_url)
                .bearer_auth(&config.api_key)
                .send()
                .await
                .map_err(|_| ToolError::invalid("video content request failed"))?;
            let (content_paths, _) =
                consume_generation_response(&client, config, workspace_root, kind, response)
                    .await?;
            paths = content_paths;
        }
    }

    if paths.is_empty() {
        return Err(ToolError::invalid(format!(
            "{} generation returned no downloadable media",
            kind.label()
        )));
    }
    Ok(ToolResult::completed(
        json!({
            "tool": kind.tool(),
            "status": "COMPLETED",
            "provider": "openai-compatible",
            "model": model,
            "paths": paths,
        })
        .to_string(),
    ))
}

async fn send_generation_request(
    client: &reqwest::Client,
    config: &MediaGenerationConfig,
    kind: MediaKind,
    url: Url,
    model: &str,
    prompt: &str,
) -> Result<Response, ToolError> {
    let response = client
        .post(url.clone())
        .bearer_auth(&config.api_key)
        .json(&json!({ "model": model, "prompt": prompt }))
        .send()
        .await
        .map_err(|_| ToolError::invalid(format!("{} generation request failed", kind.label())))?;

    // OpenAI's video endpoint is multipart; many compatible gateways use JSON.
    // Retry only format-rejection statuses so a successful job is never duplicated.
    if matches!(kind, MediaKind::Video)
        && matches!(
            response.status(),
            StatusCode::BAD_REQUEST
                | StatusCode::UNSUPPORTED_MEDIA_TYPE
                | StatusCode::UNPROCESSABLE_ENTITY
        )
    {
        return client
            .post(url)
            .bearer_auth(&config.api_key)
            .multipart(
                Form::new()
                    .text("model", model.to_string())
                    .text("prompt", prompt.to_string()),
            )
            .send()
            .await
            .map_err(|_| ToolError::invalid("video generation request failed"));
    }
    Ok(response)
}

async fn consume_generation_response(
    client: &reqwest::Client,
    config: &MediaGenerationConfig,
    workspace_root: &Path,
    kind: MediaKind,
    response: Response,
) -> Result<(Vec<String>, Option<Value>), ToolError> {
    if !response.status().is_success() {
        return Err(ToolError::invalid(format!(
            "{} generation failed with status {}",
            kind.label(),
            response.status().as_u16()
        )));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.starts_with("image/") || content_type.starts_with("video/") {
        let path = save_response_stream(response, workspace_root, kind, &content_type).await?;
        return Ok((vec![path], None));
    }

    let payload = read_json(response).await?;
    let paths = save_items(
        client,
        config,
        workspace_root,
        kind,
        extract_media_items(&payload, kind),
    )
    .await?;
    Ok((paths, Some(payload)))
}

async fn poll_video(
    client: &reqwest::Client,
    config: &MediaGenerationConfig,
    job_id: &str,
) -> Result<Value, ToolError> {
    let url = build_endpoint_url(
        &config.base_url,
        &config.video_status_endpoint,
        Some(job_id),
    )?;
    for attempt in 0..VIDEO_POLL_ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(VIDEO_POLL_INTERVAL).await;
        }
        let response = client
            .get(url.clone())
            .bearer_auth(&config.api_key)
            .send()
            .await
            .map_err(|_| ToolError::invalid("video status request failed"))?;
        if !response.status().is_success() {
            return Err(ToolError::invalid(format!(
                "video status request failed with status {}",
                response.status().as_u16()
            )));
        }
        let data = read_json(response).await?;
        if video_completed(&data) {
            return Ok(data);
        }
        if video_failed(&data) {
            return Err(ToolError::invalid("video generation failed"));
        }
    }
    Err(ToolError::invalid("video generation timed out"))
}

fn video_status(data: &Value) -> &str {
    data.get("status")
        .or_else(|| data.get("state"))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn video_completed(data: &Value) -> bool {
    matches!(
        video_status(data).to_ascii_lowercase().as_str(),
        "succeeded" | "completed" | "complete" | "done" | "success"
    )
}

fn video_failed(data: &Value) -> bool {
    matches!(
        video_status(data).to_ascii_lowercase().as_str(),
        "failed" | "error" | "cancelled" | "canceled"
    )
}

fn job_id(data: &Value) -> Option<String> {
    ["id", "job_id", "task_id"]
        .into_iter()
        .find_map(|key| data.get(key).and_then(Value::as_str))
        .map(str::to_string)
}

fn extract_media_items(data: &Value, kind: MediaKind) -> Vec<MediaItem> {
    let mut items = Vec::new();
    collect_media_items(data, kind, &mut items);
    items
}

fn collect_media_items(value: &Value, kind: MediaKind, items: &mut Vec<MediaItem>) {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_media_items(value, kind, items);
            }
        }
        Value::Object(object) => {
            let mime = object
                .get("mime_type")
                .or_else(|| object.get("mime"))
                .and_then(Value::as_str)
                .map(str::to_string);
            for key in ["b64_json", "base64"] {
                if let Some(encoded) = object.get(key).and_then(Value::as_str) {
                    items.push(MediaItem {
                        source: MediaSource::Base64(encoded.to_string()),
                        mime: mime.clone(),
                    });
                }
            }
            if let Some(data_url) = object
                .get("data")
                .and_then(Value::as_str)
                .filter(|value| value.starts_with("data:"))
            {
                items.push(MediaItem {
                    source: MediaSource::Url(data_url.to_string()),
                    mime: mime.clone(),
                });
            }
            let url_keys: &[&str] = if matches!(kind, MediaKind::Video) {
                &["url", "video_url", "output_url"]
            } else {
                &["url"]
            };
            for key in url_keys {
                if let Some(url) = object.get(*key).and_then(Value::as_str) {
                    items.push(MediaItem {
                        source: MediaSource::Url(url.to_string()),
                        mime: mime.clone(),
                    });
                }
            }
            for key in [
                "data", "output", "outputs", "result", "results", "images", "videos", "content",
            ] {
                if let Some(child @ (Value::Array(_) | Value::Object(_))) = object.get(key) {
                    collect_media_items(child, kind, items);
                }
            }
        }
        Value::String(url)
            if url.starts_with("data:")
                || url.starts_with("http://")
                || url.starts_with("https://") =>
        {
            items.push(MediaItem {
                source: MediaSource::Url(url.to_string()),
                mime: None,
            });
        }
        _ => {}
    }
}

async fn save_items(
    client: &reqwest::Client,
    config: &MediaGenerationConfig,
    workspace_root: &Path,
    kind: MediaKind,
    items: Vec<MediaItem>,
) -> Result<Vec<String>, ToolError> {
    let mut paths = Vec::with_capacity(items.len());
    for item in items {
        let path = match item.source {
            MediaSource::Base64(encoded) => {
                let bytes = STANDARD
                    .decode(encoded)
                    .map_err(|_| ToolError::invalid("media API returned invalid base64"))?;
                save_bytes(workspace_root, kind, item.mime.as_deref(), &bytes).await?
            }
            MediaSource::Url(url) if url.starts_with("data:") => {
                let (mime, bytes) = decode_data_url(&url)?;
                save_bytes(
                    workspace_root,
                    kind,
                    mime.as_deref().or(item.mime.as_deref()),
                    &bytes,
                )
                .await?
            }
            MediaSource::Url(url) => {
                save_remote_url(
                    client,
                    config,
                    workspace_root,
                    kind,
                    item.mime.as_deref(),
                    &url,
                )
                .await?
            }
        };
        paths.push(path);
    }
    Ok(paths)
}

async fn save_remote_url(
    client: &reqwest::Client,
    config: &MediaGenerationConfig,
    workspace_root: &Path,
    kind: MediaKind,
    hinted_mime: Option<&str>,
    raw_url: &str,
) -> Result<String, ToolError> {
    let url = Url::parse(raw_url)
        .map_err(|_| ToolError::invalid("media API returned an invalid media URL"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ToolError::invalid(
            "media API returned a non-http media URL",
        ));
    }
    let mut response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|_| ToolError::invalid("generated media download failed"))?;
    let provider_url = Url::parse(&config.base_url).ok();
    if matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) && provider_url
        .as_ref()
        .is_some_and(|provider_url| same_origin(provider_url, &url))
    {
        response = client
            .get(url)
            .bearer_auth(&config.api_key)
            .send()
            .await
            .map_err(|_| ToolError::invalid("generated media download failed"))?;
    }
    if !response.status().is_success() {
        return Err(ToolError::invalid(format!(
            "generated media download failed with status {}",
            response.status().as_u16()
        )));
    }
    let mime = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or(hinted_mime.unwrap_or_default())
        .to_string();
    save_response_stream(response, workspace_root, kind, &mime).await
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

async fn save_response_stream(
    mut response: Response,
    workspace_root: &Path,
    kind: MediaKind,
    mime: &str,
) -> Result<String, ToolError> {
    let (relative, target) = generation_path(workspace_root, kind, mime)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).await?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .await?;
    let mut written = 0usize;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ToolError::invalid("generated media download was interrupted"))?
    {
        written = written.saturating_add(chunk.len());
        if written > MAX_MEDIA_BYTES {
            drop(file);
            let _ = fs::remove_file(&target).await;
            return Err(ToolError::invalid("generated media was too large"));
        }
        if let Err(error) = file.write_all(&chunk).await {
            drop(file);
            let _ = fs::remove_file(&target).await;
            return Err(error.into());
        }
    }
    file.flush().await?;
    Ok(relative)
}

async fn save_bytes(
    workspace_root: &Path,
    kind: MediaKind,
    mime: Option<&str>,
    bytes: &[u8],
) -> Result<String, ToolError> {
    if bytes.len() > MAX_MEDIA_BYTES {
        return Err(ToolError::invalid("generated media was too large"));
    }
    let (relative, target) = generation_path(workspace_root, kind, mime.unwrap_or_default())?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).await?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .await?;
    file.write_all(bytes).await?;
    file.flush().await?;
    Ok(relative)
}

fn generation_path(
    workspace_root: &Path,
    kind: MediaKind,
    mime: &str,
) -> Result<(String, std::path::PathBuf), ToolError> {
    let extension = extension_for_mime(mime).unwrap_or_else(|| kind.default_extension());
    let relative = format!(
        "generations/{}-{}.{}",
        kind.label(),
        Uuid::now_v7(),
        extension
    );
    let target = resolve_workspace_path(workspace_root, &relative)?;
    Ok((relative, target))
}

fn extension_for_mime(mime: &str) -> Option<&'static str> {
    let mime = mime.to_ascii_lowercase();
    if mime.contains("jpeg") {
        Some("jpg")
    } else if mime.contains("png") {
        Some("png")
    } else if mime.contains("webp") {
        Some("webp")
    } else if mime.contains("gif") {
        Some("gif")
    } else if mime.contains("webm") {
        Some("webm")
    } else if mime.contains("quicktime") {
        Some("mov")
    } else if mime.contains("mp4") {
        Some("mp4")
    } else {
        None
    }
}

fn decode_data_url(raw: &str) -> Result<(Option<String>, Vec<u8>), ToolError> {
    let (header, payload) = raw
        .split_once(',')
        .ok_or_else(|| ToolError::invalid("media API returned an invalid data URL"))?;
    if !header.ends_with(";base64") {
        return Err(ToolError::invalid(
            "media API returned an unsupported non-base64 data URL",
        ));
    }
    let mime = header
        .strip_prefix("data:")
        .and_then(|value| value.strip_suffix(";base64"))
        .map(str::to_string)
        .filter(|value| !value.is_empty());
    let bytes = STANDARD
        .decode(payload)
        .map_err(|_| ToolError::invalid("media API returned invalid base64"))?;
    Ok((mime, bytes))
}

async fn read_json(mut response: Response) -> Result<Value, ToolError> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ToolError::invalid("media API response was interrupted"))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_JSON_BYTES {
            return Err(ToolError::invalid("media API response was too large"));
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body)
        .map_err(|_| ToolError::invalid("media API returned an invalid response"))
}

fn build_endpoint_url(base_url: &str, endpoint: &str, id: Option<&str>) -> Result<Url, ToolError> {
    let endpoint = match id {
        Some(id) => endpoint.replace("{id}", &encode_path_segment(id)),
        None => endpoint.to_string(),
    };
    if let Ok(url) = Url::parse(&endpoint) {
        if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() {
            return Ok(url);
        }
    }
    let mut base = base_url.trim_end_matches('/');
    let path = endpoint.trim_start_matches('/');
    let endpoint_version = path.split('/').next().unwrap_or_default();
    let base_version = base.rsplit('/').next().unwrap_or_default();
    if endpoint_version == base_version && is_api_version(endpoint_version) {
        base = base
            .strip_suffix(base_version)
            .unwrap_or(base)
            .trim_end_matches('/');
    }
    Url::parse(&format!("{base}/{path}"))
        .map_err(|_| ToolError::invalid("media endpoint is not a valid http or https URL"))
}

fn is_api_version(segment: &str) -> bool {
    segment
        .strip_prefix('v')
        .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit()))
}

fn encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use axum::{routing::post, Json, Router};
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use reqwest::Url;
    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::{generate_image, generate_video, same_origin, MediaGenerationConfig};
    use crate::tools::ToolStatus;

    #[test]
    fn authentication_is_limited_to_the_provider_origin() {
        let provider = Url::parse("https://media.example.test/v1").unwrap();
        assert!(same_origin(
            &provider,
            &Url::parse("https://media.example.test/file.mp4").unwrap()
        ));
        assert!(!same_origin(
            &provider,
            &Url::parse("https://downloads.example.test/file.mp4").unwrap()
        ));
    }

    #[tokio::test]
    async fn compatible_image_and_video_responses_are_saved_to_generations() {
        let image = STANDARD.encode(b"image-bytes");
        let video = STANDARD.encode(b"video-bytes");
        let app = Router::new()
            .route(
                "/v1/images/generations",
                post(move || {
                    let image = image.clone();
                    async move { Json(json!({ "data": [{ "b64_json": image, "mime_type": "image/png" }] })) }
                }),
            )
            .route(
                "/v1/videos",
                post(move || {
                    let video = video.clone();
                    async move { Json(json!({ "id": "video-1", "status": "completed", "data": [{ "base64": video, "mime_type": "video/mp4" }] })) }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let config = MediaGenerationConfig {
            api_key: "secret".to_string(),
            base_url: format!("http://{address}/v1"),
            image_model: Some("image-model".to_string()),
            image_endpoint: "/v1/images/generations".to_string(),
            video_model: Some("video-model".to_string()),
            video_endpoint: "/v1/videos".to_string(),
            video_status_endpoint: "/v1/videos/{id}".to_string(),
            video_content_endpoint: "/v1/videos/{id}/content".to_string(),
        };
        let root = tempdir().unwrap();

        let image_result = generate_image(Some(&config), Some(root.path()), "draw", None)
            .await
            .unwrap();
        let video_result = generate_video(Some(&config), Some(root.path()), "animate", None)
            .await
            .unwrap();

        assert_eq!(image_result.status, ToolStatus::Completed);
        assert_eq!(video_result.status, ToolStatus::Completed);
        let image_path = serde_json::from_str::<Value>(&image_result.output).unwrap()["paths"][0]
            .as_str()
            .unwrap()
            .to_string();
        let video_path = serde_json::from_str::<Value>(&video_result.output).unwrap()["paths"][0]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            std::fs::read(root.path().join(image_path)).unwrap(),
            b"image-bytes"
        );
        assert_eq!(
            std::fs::read(root.path().join(video_path)).unwrap(),
            b"video-bytes"
        );
    }
}
