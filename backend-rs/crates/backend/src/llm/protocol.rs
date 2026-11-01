//! Provider wire conventions shared by the native and Rig transports.

pub(crate) const RESPONSES_DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Clone, Copy)]
pub(crate) enum Protocol {
    OpenAi,
    Responses,
    Anthropic,
    Gemini,
}

impl Protocol {
    // Note: saved provider aliases must route identically in either engine;
    // keeping this policy outside the clients also permits their retirement.
    pub(crate) fn for_kind(kind: &str) -> Option<Self> {
        match kind {
            "openai-responses" => Some(Self::Responses),
            "openai-compatible" | "openai_compatible" | "openai" | "deepseek" | "vllm"
            | "openrouter" => Some(Self::OpenAi),
            "anthropic" | "anthropic-compatible" | "anthropic_compatible" => Some(Self::Anthropic),
            "gemini" | "google" => Some(Self::Gemini),
            _ => None,
        }
    }
}

/// Resolve an API root or full Responses endpoint without losing tenant queries.
pub(crate) fn responses_endpoint(base_url: &str) -> anyhow::Result<reqwest::Url> {
    let base = base_url.trim();
    let mut url = reqwest::Url::parse(if base.is_empty() {
        RESPONSES_DEFAULT_BASE_URL
    } else {
        base
    })?;
    let path = url.path().trim_end_matches('/');
    let path = if path.ends_with("/responses") {
        path.to_string()
    } else {
        format!("{path}/responses")
    };
    url.set_path(&path);
    url.set_fragment(None);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_accept_roots_and_full_paths_without_losing_queries() {
        for (base, expected) in [
            ("", "https://api.openai.com/v1/responses"),
            (
                " https://gateway.test/v1/ ",
                "https://gateway.test/v1/responses",
            ),
            (
                "https://gateway.test/v1/responses/?tenant=a#fragment",
                "https://gateway.test/v1/responses?tenant=a",
            ),
        ] {
            assert_eq!(responses_endpoint(base).unwrap().as_str(), expected);
        }
    }
}
