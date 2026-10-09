//! HTTP client for a System One decision endpoint.
//!
//! Two wire dialects are spoken, chosen from the endpoint URL:
//!
//! * **System One** — TypeSafe's `POST /v1/systemone`, which OpenRouter
//!   re-exposes unchanged at `POST /api/alpha/decisions`. The body carries
//!   `model`, `state` and `questions`; answers are `noul`, `choice` or
//!   `score`, the last two with a `confidence`.
//! * **AI SDK gateway** — the Vercel AI SDK's evaluation-model protocol,
//!   served by Vercel AI Gateway at `POST /v4/ai/evaluation-model`. The model
//!   travels in the `ai-model-id` header, a yes/no question is `boolean`
//!   rather than `noul`, and answers omit `confidence`: TypeSafe's own value
//!   arrives under `providerMetadata.typesafe.confidence` when the gateway
//!   forwards it, and is otherwise derived from the probabilities. Any URL
//!   whose path ends in `/evaluation-model` is treated as this dialect.
//!
//! Only the three question types exist: a yes/no question answered with the
//! probability of "yes"; one option out of a set, with per-option
//! probabilities and a confidence derived from their spread; and a
//! probability-weighted position across ordered levels, also with a
//! confidence. The model never generates text, so this client has no
//! streaming and no tool calls; a request is one round trip with a small JSON
//! body.

use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Longest error body kept from a non-2xx response. Providers echo the request
/// back in some errors, and the request holds conversation excerpts.
const MAX_ERROR_BODY_CHARS: usize = 512;

/// The AI SDK gateway protocol the `@ai-sdk/gateway` client speaks.
const GATEWAY_PROTOCOL_VERSION: &str = "0.0.1";
/// The evaluation-model specification version the gateway route expects.
const GATEWAY_EVALUATION_SPEC_VERSION: &str = "4";

/// How an endpoint expects to be spoken to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// TypeSafe's `/v1/systemone` and OpenRouter's `/api/alpha/decisions`.
    SystemOne,
    /// The AI SDK evaluation-model protocol, as served by Vercel AI Gateway.
    AiSdkGateway,
}

impl Dialect {
    /// The dialect an endpoint speaks, read off its path. The AI SDK route is
    /// always `/evaluation-model`, whoever hosts it; everything else is the
    /// System One contract.
    pub fn detect(endpoint: &reqwest::Url) -> Self {
        if endpoint
            .path()
            .trim_end_matches('/')
            .ends_with("/evaluation-model")
        {
            Self::AiSdkGateway
        } else {
            Self::SystemOne
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SystemOne => "system_one",
            Self::AiSdkGateway => "ai_sdk_gateway",
        }
    }
}

/// One typed question, keyed by the caller under `questions`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Question {
    Noul {
        instructions: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    Choice {
        instructions: String,
        /// Option → rubric. `None` sends JSON `null`, which the API accepts for
        /// an option that needs no description.
        criteria: BTreeMap<String, Option<String>>,
    },
    Score {
        instructions: String,
        /// Ordered level descriptions, lowest first. At least two.
        criteria: Vec<String>,
    },
}

impl Question {
    pub fn noul(
        instructions: impl Into<String>,
        yes: impl Into<String>,
        no: impl Into<String>,
    ) -> Self {
        Self::Noul {
            instructions: instructions.into(),
            criteria: Some(NoulCriteria {
                yes: yes.into(),
                no: no.into(),
            }),
        }
    }

    pub fn choice(
        instructions: impl Into<String>,
        options: impl IntoIterator<Item = (String, Option<String>)>,
    ) -> Self {
        Self::Choice {
            instructions: instructions.into(),
            criteria: options.into_iter().collect(),
        }
    }

    pub fn score(
        instructions: impl Into<String>,
        levels: impl IntoIterator<Item = String>,
    ) -> Self {
        Self::Score {
            instructions: instructions.into(),
            criteria: levels.into_iter().collect(),
        }
    }
}

/// What a yes and a no mean for a [`Question::Noul`].
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    pub yes: String,
    #[serde(rename = "false")]
    pub no: String,
}

/// The questions as the AI SDK gateway spells them: identical, except that a
/// yes/no question is `boolean`. Its `criteria` keep the same `true`/`false`
/// keys.
fn gateway_questions(questions: &BTreeMap<String, Question>) -> Value {
    let mut value = serde_json::to_value(questions).unwrap_or_else(|_| json!({}));
    if let Value::Object(map) = &mut value {
        for question in map.values_mut().filter_map(Value::as_object_mut) {
            if question.get("type").and_then(Value::as_str) == Some("noul") {
                question.insert("type".to_string(), Value::String("boolean".to_string()));
            }
        }
    }
    value
}

/// One typed answer, under the same id as its question.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    Noul {
        noul: f64,
    },
    /// The AI SDK spelling of a noul answer.
    Boolean {
        probability: f64,
    },
    Choice {
        choice: String,
        #[serde(default)]
        probabilities: BTreeMap<String, f64>,
        /// Absent on the gateway dialect; see [`Answer::confidence`].
        #[serde(default)]
        confidence: Option<f64>,
    },
    Score {
        score: f64,
        #[serde(default)]
        probabilities: BTreeMap<String, f64>,
        #[serde(default)]
        legend: BTreeMap<String, String>,
        #[serde(default)]
        confidence: Option<f64>,
    },
}

impl Answer {
    /// The probability of "yes", for a noul (or boolean) answer.
    pub fn noul(&self) -> Option<f64> {
        match self {
            Self::Noul { noul } => Some(*noul),
            Self::Boolean { probability } => Some(*probability),
            _ => None,
        }
    }

    /// How sure a choice or score answer is.
    ///
    /// The endpoint's own figure when it sent one. Otherwise the largest
    /// probability in the distribution — the plainest reading of "how sure is
    /// the model of its pick". On the illustrative values in TypeSafe's docs
    /// that runs a little above their spread-based statistic, so a gateway
    /// user who finds the model acting too readily should raise the confidence
    /// floor rather than expect parity. With no distribution at all the answer
    /// has no confidence, and the callers treat it as no opinion.
    pub fn confidence(&self) -> Option<f64> {
        match self {
            Self::Choice {
                probabilities,
                confidence,
                ..
            }
            | Self::Score {
                probabilities,
                confidence,
                ..
            } => confidence.or_else(|| {
                probabilities
                    .values()
                    .copied()
                    .filter(|probability| probability.is_finite())
                    .reduce(f64::max)
            }),
            _ => None,
        }
    }

    /// The chosen option and the answer's confidence, for a choice answer.
    pub fn choice(&self) -> Option<(&str, f64)> {
        match self {
            Self::Choice { choice, .. } => {
                Some((choice.as_str(), self.confidence().unwrap_or(0.0)))
            }
            _ => None,
        }
    }

    /// The probability the answer gave one option, for a choice answer.
    pub fn probability_of(&self, option: &str) -> Option<f64> {
        match self {
            Self::Choice { probabilities, .. } => probabilities.get(option).copied(),
            _ => None,
        }
    }

    /// The weighted level and the answer's confidence, for a score answer.
    pub fn score(&self) -> Option<(f64, f64)> {
        match self {
            Self::Score { score, .. } => Some((*score, self.confidence().unwrap_or(0.0))),
            _ => None,
        }
    }

    /// Attach a confidence the endpoint reported outside the answer itself.
    fn set_confidence(&mut self, value: f64) {
        match self {
            Self::Choice { confidence, .. } | Self::Score { confidence, .. } => {
                *confidence = Some(value);
            }
            _ => {}
        }
    }
}

/// Token accounting reported with every response. OpenRouter adds `cost`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: Option<i64>,
    #[serde(default)]
    pub output_tokens: Option<i64>,
    #[serde(default)]
    pub cost: Option<f64>,
}

/// A successful evaluation.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct DecisionResponse {
    /// The versioned model that answered, when the endpoint reports it. The
    /// gateway dialect names none.
    #[serde(default)]
    pub model: Option<String>,
    pub answers: BTreeMap<String, Answer>,
    #[serde(default)]
    pub usage: Option<Usage>,
    /// Provider warnings, on the dialects that send them.
    #[serde(skip)]
    pub warnings: Vec<String>,
}

impl DecisionResponse {
    pub fn answer(&self, id: &str) -> Option<&Answer> {
        self.answers.get(id)
    }

    pub fn noul(&self, id: &str) -> Option<f64> {
        self.answer(id).and_then(Answer::noul)
    }

    pub fn choice(&self, id: &str) -> Option<(&str, f64)> {
        self.answer(id).and_then(Answer::choice)
    }

    pub fn score(&self, id: &str) -> Option<(f64, f64)> {
        self.answer(id).and_then(Answer::score)
    }
}

/// The AI SDK gateway's response body.
#[derive(Debug, Deserialize)]
struct GatewayResponse {
    answers: BTreeMap<String, Answer>,
    #[serde(default)]
    usage: Option<GatewayUsage>,
    #[serde(default)]
    warnings: Vec<Value>,
    #[serde(default, rename = "providerMetadata")]
    provider_metadata: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
struct GatewayUsage {
    #[serde(default, rename = "inputTokens")]
    input_tokens: Option<f64>,
    #[serde(default, rename = "outputTokens")]
    output_tokens: Option<f64>,
}

impl GatewayResponse {
    fn into_decision(self) -> DecisionResponse {
        let mut answers = self.answers;
        // TypeSafe's confidence survives the gateway under provider metadata,
        // keyed by question id, when the gateway forwards it.
        if let Some(confidence) = self
            .provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("typesafe"))
            .and_then(|typesafe| typesafe.get("confidence"))
            .and_then(Value::as_object)
        {
            for (id, value) in confidence {
                if let (Some(answer), Some(value)) = (answers.get_mut(id), value.as_f64()) {
                    answer.set_confidence(value);
                }
            }
        }
        let warnings = self
            .warnings
            .iter()
            .map(|warning| {
                ["message", "details", "feature"]
                    .into_iter()
                    .find_map(|key| warning.get(key).and_then(Value::as_str))
                    .map(str::to_string)
                    .unwrap_or_else(|| warning.to_string())
            })
            .collect();
        DecisionResponse {
            model: None,
            answers,
            usage: self.usage.map(|usage| Usage {
                input_tokens: usage.input_tokens.map(|tokens| tokens.round() as i64),
                output_tokens: usage.output_tokens.map(|tokens| tokens.round() as i64),
                cost: None,
            }),
            warnings,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecisionError {
    #[error("decision endpoint is not a valid URL")]
    InvalidEndpoint,
    #[error("decision request timed out")]
    Timeout,
    #[error("decision request failed: {0}")]
    Transport(String),
    /// A non-2xx response. `body` is trimmed and is for the operator's log
    /// only: it can contain the provider's own text.
    #[error("decision endpoint returned HTTP {status}")]
    Http { status: u16, body: String },
    #[error("decision response could not be parsed")]
    InvalidResponse,
}

/// The provider's own explanation from an error body shaped like
/// `{"error":{"message":...}}` or `{"message":...}`. Providers put billing
/// and verification reasons here, which the status code alone hides. Only
/// a short plain string is taken, never the echoed request.
fn provider_error_message(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let message = value
        .pointer("/error/message")
        .or_else(|| value.pointer("/detail/message"))
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| fastapi_detail(value.get("detail")?))?;
    let message = message.trim();
    if message.is_empty() {
        return None;
    }
    Some(message.chars().take(240).collect())
}

/// TypeSafe's validation errors follow FastAPI: `detail` is either a string
/// or a list of `{loc, msg}` entries naming the offending field.
fn fastapi_detail(detail: &Value) -> Option<String> {
    if let Some(text) = detail.as_str() {
        return Some(text.to_string());
    }
    let entries = detail.as_array()?;
    let parts: Vec<String> = entries
        .iter()
        .filter_map(|entry| {
            let msg = entry.get("msg")?.as_str()?;
            let loc = entry
                .get("loc")
                .and_then(Value::as_array)
                .map(|loc| {
                    loc.iter()
                        .filter_map(|part| match part {
                            Value::String(s) => Some(s.clone()),
                            Value::Number(n) => Some(n.to_string()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(".")
                })
                .filter(|loc| !loc.is_empty());
            Some(match loc {
                Some(loc) => format!("{loc}: {msg}"),
                None => msg.to_string(),
            })
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("; "))
}

impl DecisionError {
    /// A message safe to show a user: no body, no credential.
    pub fn safe_message(&self) -> String {
        match self {
            Self::Http { status, body } => match provider_error_message(body) {
                Some(reason) => format!("The decision endpoint returned HTTP {status}: {reason}"),
                None => format!("The decision endpoint returned HTTP {status}."),
            },
            Self::Transport(_) => "The decision endpoint could not be reached.".to_string(),
            other => other.to_string(),
        }
    }
}

/// A client bound to one endpoint, credential, and model.
#[derive(Clone)]
pub struct DecisionClient {
    http: reqwest::Client,
    endpoint: String,
    dialect: Dialect,
    api_key: String,
    model: String,
}

impl std::fmt::Debug for DecisionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecisionClient")
            .field("endpoint", &self.endpoint)
            .field("dialect", &self.dialect)
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

impl DecisionClient {
    pub fn new(
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, DecisionError> {
        let endpoint = endpoint.into();
        let parsed = reqwest::Url::parse(&endpoint).map_err(|_| DecisionError::InvalidEndpoint)?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(DecisionError::InvalidEndpoint);
        }
        let dialect = Dialect::detect(&parsed);
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|error| DecisionError::Transport(error.to_string()))?;
        Ok(Self {
            http,
            endpoint,
            dialect,
            api_key: api_key.into(),
            model: model.into(),
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Evaluate `state` against `questions` in one request.
    pub async fn evaluate(
        &self,
        state: Value,
        questions: BTreeMap<String, Question>,
    ) -> Result<DecisionResponse, DecisionError> {
        let request = match self.dialect {
            Dialect::SystemOne => self.http.post(&self.endpoint).json(&json!({
                "model": self.model,
                "state": state,
                "questions": questions,
            })),
            Dialect::AiSdkGateway => self
                .http
                .post(&self.endpoint)
                .header("ai-gateway-protocol-version", GATEWAY_PROTOCOL_VERSION)
                .header("ai-gateway-auth-method", "api-key")
                .header(
                    "ai-evaluation-model-specification-version",
                    GATEWAY_EVALUATION_SPEC_VERSION,
                )
                .header("ai-model-id", self.model.as_str())
                .json(&json!({
                    "state": state,
                    "questions": gateway_questions(&questions),
                })),
        };
        let response = request
            // `.json()` already set Content-Type; adding it again sends a
            // duplicate header, which FastAPI-based servers (TypeSafe) treat
            // as non-JSON and reject with 422.
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(map_reqwest_error)?;
        if !status.is_success() {
            return Err(DecisionError::Http {
                status: status.as_u16(),
                body: String::from_utf8_lossy(&bytes)
                    .chars()
                    .take(MAX_ERROR_BODY_CHARS)
                    .collect(),
            });
        }
        match self.dialect {
            Dialect::SystemOne => serde_json::from_slice::<DecisionResponse>(&bytes)
                .map_err(|_| DecisionError::InvalidResponse),
            Dialect::AiSdkGateway => serde_json::from_slice::<GatewayResponse>(&bytes)
                .map(GatewayResponse::into_decision)
                .map_err(|_| DecisionError::InvalidResponse),
        }
    }
}

fn map_reqwest_error(error: reqwest::Error) -> DecisionError {
    if error.is_timeout() {
        DecisionError::Timeout
    } else {
        // `without_url` keeps a query string that could carry a key out of the
        // log; the endpoint itself is configuration, not a secret.
        DecisionError::Transport(error.without_url().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn questions_serialize_to_the_wire_shape() {
        let mut questions = BTreeMap::new();
        questions.insert(
            "is_urgent".to_string(),
            Question::noul("Does this convey urgency?", "Time-sensitive", "No urgency"),
        );
        questions.insert(
            "department".to_string(),
            Question::choice(
                "Which team?",
                [
                    ("billing".to_string(), Some("Payments".to_string())),
                    ("technical".to_string(), None),
                ],
            ),
        );
        questions.insert(
            "frustration".to_string(),
            Question::score("How frustrated?", ["Calm".to_string(), "Angry".to_string()]),
        );
        let value = serde_json::to_value(&questions).unwrap();
        assert_eq!(
            value,
            json!({
                "is_urgent": {
                    "type": "noul",
                    "instructions": "Does this convey urgency?",
                    "criteria": { "true": "Time-sensitive", "false": "No urgency" }
                },
                "department": {
                    "type": "choice",
                    "instructions": "Which team?",
                    "criteria": { "billing": "Payments", "technical": null }
                },
                "frustration": {
                    "type": "score",
                    "instructions": "How frustrated?",
                    "criteria": ["Calm", "Angry"]
                }
            })
        );
    }

    #[test]
    fn answers_parse_from_typesafe_and_openrouter_bodies() {
        let body = json!({
            "id": "gen-dec-1",
            "provider": "TypeSafe",
            "model": "typesafe/jev-1.13-20260917",
            "answers": {
                "is_bug": { "type": "noul", "noul": 0.96 },
                "team": {
                    "type": "choice",
                    "choice": "payments",
                    "probabilities": { "account": 0, "frontend": 0.16, "payments": 0.84 },
                    "confidence": 0.75
                },
                "urgency": {
                    "type": "score",
                    "score": 1.99,
                    "legend": { "0": "Can wait", "1": "This week", "2": "Blocking" },
                    "probabilities": { "0": 0, "1": 0.01, "2": 0.99 },
                    "confidence": 0.99
                }
            },
            "usage": { "cost": 0.000019992, "input_tokens": 476, "output_tokens": 70 }
        });
        let parsed: DecisionResponse = serde_json::from_value(body).unwrap();
        assert_eq!(parsed.model.as_deref(), Some("typesafe/jev-1.13-20260917"));
        assert_eq!(parsed.noul("is_bug"), Some(0.96));
        assert_eq!(parsed.choice("team"), Some(("payments", 0.75)));
        assert_eq!(
            parsed.answer("team").unwrap().probability_of("frontend"),
            Some(0.16)
        );
        assert_eq!(parsed.score("urgency"), Some((1.99, 0.99)));
        assert_eq!(parsed.noul("team"), None);
        assert_eq!(parsed.usage.unwrap().input_tokens, Some(476));

        let minimal: DecisionResponse = serde_json::from_value(json!({
            "model": "jev-latest",
            "answers": { "q": { "type": "noul", "noul": 0.5 } },
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        }))
        .unwrap();
        assert_eq!(minimal.noul("q"), Some(0.5));
    }

    #[test]
    fn dialect_is_detected_from_the_path() {
        let gateway =
            reqwest::Url::parse("https://ai-gateway.vercel.sh/v4/ai/evaluation-model").unwrap();
        assert_eq!(Dialect::detect(&gateway), Dialect::AiSdkGateway);
        let trailing = reqwest::Url::parse("http://127.0.0.1:9/v4/ai/evaluation-model/").unwrap();
        assert_eq!(Dialect::detect(&trailing), Dialect::AiSdkGateway);
        for url in [
            "https://openrouter.ai/api/alpha/decisions",
            "https://api.typesafe.ai/v1/systemone",
            "https://example.test/evaluation-models",
        ] {
            assert_eq!(
                Dialect::detect(&reqwest::Url::parse(url).unwrap()),
                Dialect::SystemOne,
                "{url}"
            );
        }
        let client = DecisionClient::new(
            "https://ai-gateway.vercel.sh/v4/ai/evaluation-model",
            "k",
            "typesafe-ai/jev",
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(client.dialect(), Dialect::AiSdkGateway);
        assert_eq!(Dialect::AiSdkGateway.as_str(), "ai_sdk_gateway");
    }

    #[test]
    fn gateway_questions_spell_noul_as_boolean() {
        let mut questions = BTreeMap::new();
        questions.insert("q".to_string(), Question::noul("Yes?", "Y", "N"));
        questions.insert(
            "c".to_string(),
            Question::choice("Which?", [("a".to_string(), None)]),
        );
        let value = gateway_questions(&questions);
        assert_eq!(value["q"]["type"], "boolean");
        assert_eq!(value["q"]["instructions"], "Yes?");
        assert_eq!(value["q"]["criteria"]["true"], "Y");
        assert_eq!(value["q"]["criteria"]["false"], "N");
        assert_eq!(value["c"]["type"], "choice");
    }

    #[test]
    fn gateway_answers_parse_with_forwarded_or_derived_confidence() {
        let body = json!({
            "answers": {
                "yes": { "type": "boolean", "probability": 0.93 },
                "forwarded": {
                    "type": "choice", "choice": "a",
                    "probabilities": { "a": 0.6, "b": 0.4 }
                },
                "derived": {
                    "type": "choice", "choice": "x",
                    "probabilities": { "x": 0.8, "y": 0.2 }
                },
                "bare": { "type": "choice", "choice": "only" },
                "level": {
                    "type": "score", "score": 1.5,
                    "probabilities": { "0": 0.0, "1": 0.5, "2": 0.5 }
                }
            },
            "providerMetadata": { "typesafe": { "confidence": { "forwarded": 0.42 } } },
            "usage": { "inputTokens": 10, "outputTokens": 2 },
            "warnings": [{ "type": "other", "message": "be careful" }]
        });
        let parsed: GatewayResponse = serde_json::from_value(body).unwrap();
        let response = parsed.into_decision();
        assert_eq!(response.model, None);
        assert_eq!(response.noul("yes"), Some(0.93));
        assert_eq!(response.choice("forwarded"), Some(("a", 0.42)));
        assert_eq!(response.choice("derived"), Some(("x", 0.8)));
        assert_eq!(response.choice("bare"), Some(("only", 0.0)));
        assert_eq!(response.score("level"), Some((1.5, 0.5)));
        assert_eq!(response.usage.as_ref().unwrap().input_tokens, Some(10));
        assert_eq!(response.usage.as_ref().unwrap().output_tokens, Some(2));
        assert_eq!(response.warnings, vec!["be careful".to_string()]);
    }

    #[test]
    fn client_rejects_non_http_endpoints() {
        assert!(matches!(
            DecisionClient::new("ftp://example.test/x", "k", "m", Duration::from_secs(1)),
            Err(DecisionError::InvalidEndpoint)
        ));
        assert!(matches!(
            DecisionClient::new("not a url", "k", "m", Duration::from_secs(1)),
            Err(DecisionError::InvalidEndpoint)
        ));
        assert!(DecisionClient::new(
            "https://openrouter.ai/api/alpha/decisions",
            "k",
            "m",
            Duration::from_secs(1)
        )
        .is_ok());
    }

    #[test]
    fn http_errors_are_summarised_without_their_body() {
        let error = DecisionError::Http {
            status: 402,
            body: "Insufficient credits sk-live-secret".to_string(),
        };
        assert_eq!(
            error.safe_message(),
            "The decision endpoint returned HTTP 402."
        );
        assert!(!error.safe_message().contains("secret"));
    }
}

#[cfg(test)]
mod error_message_tests {
    use super::*;

    #[test]
    fn http_error_surfaces_provider_message() {
        let error = DecisionError::Http {
            status: 403,
            body: r#"{"error":{"message":"AI Gateway requires a valid credit card on file.","type":"customer_verification_required"}}"#.to_string(),
        };
        assert_eq!(
            error.safe_message(),
            "The decision endpoint returned HTTP 403: AI Gateway requires a valid credit card on file."
        );
        let typesafe = DecisionError::Http {
            status: 422,
            body: r#"{"detail":[{"loc":["body","questions","is_urgent","criteria"],"msg":"extra fields not permitted","type":"value_error"}]}"#.to_string(),
        };
        assert_eq!(
            typesafe.safe_message(),
            "The decision endpoint returned HTTP 422: body.questions.is_urgent.criteria: extra fields not permitted"
        );
        let plain = DecisionError::Http {
            status: 502,
            body: "<html>bad gateway</html>".to_string(),
        };
        assert_eq!(
            plain.safe_message(),
            "The decision endpoint returned HTTP 502."
        );
    }
}
