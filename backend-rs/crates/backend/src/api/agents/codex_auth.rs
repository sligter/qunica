//! Codex-owned account login. Tokens never cross this HTTP surface.

use std::{
    collections::BTreeMap,
    convert::Infallible,
    process::Stdio,
    sync::{Arc, OnceLock},
    time::Duration,
};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_util::stream::{self, BoxStream, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
    time::{timeout, Instant},
};

use crate::{
    acp::{build_child_env, normalize_acp_runtime, AcpRuntimeProfile},
    api::{auth::current_user_id, error::ApiError, AppState},
    process::{spawn_process_tree, ProcessTree},
};

const SETUP_TIMEOUT: Duration = Duration::from_secs(30);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_FRAME_BYTES: u64 = 128 * 1024;

#[derive(Deserialize)]
pub struct AuthRequest {
    #[serde(default)]
    env: BTreeMap<String, String>,
}

fn runtime_env(body: AuthRequest) -> Result<BTreeMap<String, String>, ApiError> {
    normalize_acp_runtime(Some(
        &json!({"profile":"codex", "command":"codex", "env":body.env}),
    ))
    .map(|config| config.env)
    .map_err(|_| ApiError::invalid_input("Invalid Codex runtime environment."))
}

pub async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AuthRequest>,
) -> Result<Json<Value>, ApiError> {
    current_user_id(&headers, &state).await?;
    let env = runtime_env(body)?;
    timeout(SETUP_TIMEOUT, async {
        let mut server = AccountServer::start(&env)?;
        server.initialize().await?;
        let account = server
            .request(2, "account/read", json!({"refreshToken":false}))
            .await?;
        Ok(Json(account_status(&account)))
    })
    .await
    .map_err(|_| login_error("codex_auth_timeout"))?
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AuthRequest>,
) -> Result<Sse<BoxStream<'static, Result<Event, Infallible>>>, ApiError> {
    current_user_id(&headers, &state).await?;
    let env = runtime_env(body)?;
    // Note: Codex profiles share the backend user's credential store. Serialize
    // login attempts without stopping ACP sessions.
    static LOGIN_SLOT: OnceLock<Arc<Semaphore>> = OnceLock::new();
    let permit = LOGIN_SLOT
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ApiError::conflict(
                "A Codex login is already in progress. Cancel it before starting another.",
            )
        })?;
    let login = timeout(SETUP_TIMEOUT, async {
        let mut server = AccountServer::start(&env)?;
        server.initialize().await?;
        let response = server
            .request(
                2,
                "account/login/start",
                json!({"type":"chatgptDeviceCode"}),
            )
            .await?;
        let (login_id, device) = device_code(&response)?;
        Ok::<_, ApiError>(DeviceLogin {
            server,
            login_id,
            device: Some(device),
            deadline: Instant::now() + LOGIN_TIMEOUT,
            _permit: permit,
        })
    })
    .await
    .map_err(|_| login_error("codex_auth_timeout"))??;
    let events = stream::unfold(Some(login), |state| async move {
        let mut login = state?;
        if let Some(device) = login.device.take() {
            return Some((Ok(sse_event(device)), Some(login)));
        }
        let result = timeout(
            login.deadline.saturating_duration_since(Instant::now()),
            login.completed(),
        )
        .await;
        let event = match result {
            Ok(Ok(())) => json!({"type":"completed"}),
            Ok(Err(_)) => json!({"type":"error", "code":"codex_auth_failed"}),
            Err(_) => json!({"type":"error", "code":"codex_auth_timeout"}),
        };
        Some((Ok(sse_event(event)), None))
    })
    .boxed();
    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(10))))
}

fn sse_event(value: Value) -> Event {
    Event::default()
        .event("codex_login")
        .data(value.to_string())
}

fn account_status(response: &Value) -> Value {
    let method = response
        .get("account")
        .and_then(|account| account.get("type"))
        .and_then(Value::as_str);
    let safe_method = method.map(|method| match method {
        "chatgpt" | "apiKey" | "chatgptAuthTokens" | "agentIdentity" | "amazonBedrock" => method,
        _ => "other",
    });
    json!({"authenticated":method.is_some(), "method":safe_method})
}

fn device_code(response: &Value) -> Result<(String, Value), ApiError> {
    let id = response
        .get("loginId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 256);
    let code = response
        .get("userCode")
        .and_then(Value::as_str)
        .filter(|code| {
            !code.is_empty()
                && code.len() <= 64
                && code.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        });
    let url = response
        .get("verificationUrl")
        .and_then(Value::as_str)
        .and_then(|url| reqwest::Url::parse(url).ok())
        .filter(|url| {
            url.scheme() == "https"
                && url.host_str() == Some("auth.openai.com")
                && url.path() == "/codex/device"
                && url.port().is_none()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
        });
    match (id, code, url) {
        (Some(id), Some(code), Some(url)) if response["type"] == "chatgptDeviceCode" => Ok((
            id.to_string(),
            json!({"type":"device_code", "verification_url":url.as_str(), "user_code":code}),
        )),
        _ => Err(login_error("codex_auth_failed")),
    }
}

fn login_error(code: &'static str) -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, code, "Unable to complete Codex sign-in. Check that the Codex CLI is current and device code login is enabled for your account.")
}

struct DeviceLogin {
    server: AccountServer,
    login_id: String,
    device: Option<Value>,
    deadline: Instant,
    _permit: OwnedSemaphorePermit,
}

impl DeviceLogin {
    async fn completed(&mut self) -> Result<(), ApiError> {
        loop {
            let message = self.server.read().await?;
            if message["method"] == "account/login/completed"
                && message["params"]["loginId"] == self.login_id
            {
                return if message["params"]["success"] == true {
                    Ok(())
                } else {
                    Err(login_error("codex_auth_failed"))
                };
            }
            self.server.reject_request(&message).await?;
        }
    }
}

struct AccountServer {
    child: Option<Child>,
    tree: ProcessTree,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr: JoinHandle<()>,
    home: Option<tempfile::TempDir>,
}

impl AccountServer {
    // Note: Device codes and login completion belong to Codex's account protocol,
    // not ACP session updates. See docs/guide/external-cli-agents.md#codex-sign-in.
    fn start(runtime_env: &BTreeMap<String, String>) -> Result<Self, ApiError> {
        let command = super::find_command_on_path("codex").ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "codex_cli_missing",
                "Install @openai/codex on the backend host to use Codex sign-in.",
            )
        })?;
        let command = command.to_string_lossy();
        let (command, args) = super::windows_batch_command(&command, &["app-server"]);
        let mut process = std::process::Command::new(command);
        process.args(args);
        Self::spawn(process, runtime_env)
    }

    fn spawn(
        mut process: std::process::Command,
        runtime_env: &BTreeMap<String, String>,
    ) -> Result<Self, ApiError> {
        let home = tempfile::tempdir().map_err(|_| login_error("codex_auth_failed"))?;
        let env = build_child_env(AcpRuntimeProfile::Codex, home.path(), runtime_env)
            .map_err(|_| login_error("codex_auth_failed"))?;
        process
            .current_dir(home.path())
            .env_clear()
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (mut child, tree) =
            spawn_process_tree(process).map_err(|_| login_error("codex_auth_failed"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| login_error("codex_auth_failed"))?;
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| login_error("codex_auth_failed"))?,
        );
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| login_error("codex_auth_failed"))?;
        // Drain only: diagnostics and credentials from the CLI never enter API responses or logs.
        let stderr = tokio::spawn(async move {
            let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
        });
        Ok(Self {
            child: Some(child),
            tree,
            stdin,
            stdout,
            stderr,
            home: Some(home),
        })
    }

    async fn initialize(&mut self) -> Result<(), ApiError> {
        self.request(1, "initialize", json!({"clientInfo":{"name":"qunica", "title":"Qunica", "version":env!("CARGO_PKG_VERSION")}})).await?;
        self.write(json!({"jsonrpc":"2.0", "method":"initialized", "params":{}}))
            .await
    }

    async fn write(&mut self, value: Value) -> Result<(), ApiError> {
        let mut line = value.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|_| login_error("codex_auth_failed"))?;
        self.stdin
            .flush()
            .await
            .map_err(|_| login_error("codex_auth_failed"))
    }

    async fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value, ApiError> {
        self.write(json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}))
            .await?;
        loop {
            let message = self.read().await?;
            if message.get("id").and_then(Value::as_u64) == Some(id)
                && message.get("method").is_none()
            {
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| login_error("codex_auth_failed"));
            }
            self.reject_request(&message).await?;
        }
    }

    async fn read(&mut self) -> Result<Value, ApiError> {
        loop {
            let mut line = Vec::new();
            let count = (&mut self.stdout)
                .take(MAX_FRAME_BYTES + 1)
                .read_until(b'\n', &mut line)
                .await
                .map_err(|_| login_error("codex_auth_failed"))?;
            if count == 0 || count as u64 > MAX_FRAME_BYTES {
                return Err(login_error("codex_auth_failed"));
            }
            // Test harness banners and CLI startup notices are not protocol frames.
            if let Ok(message) = serde_json::from_slice::<Value>(&line) {
                return Ok(message);
            }
        }
    }

    async fn reject_request(&mut self, message: &Value) -> Result<(), ApiError> {
        if message.get("method").is_some() && message.get("id").is_some() {
            self.write(json!({"jsonrpc":"2.0", "id":message["id"], "error":{"code":-32601,"message":"Account login client does not execute agent requests."}})).await?;
        }
        Ok(())
    }
}

impl Drop for AccountServer {
    fn drop(&mut self) {
        self.tree.terminate();
        self.stderr.abort();
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            let home = self.home.take();
            // Keep the temporary cwd until the process has exited (also on Windows).
            tokio::spawn(async move {
                let _ = timeout(Duration::from_secs(5), child.wait()).await;
                drop(home);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, Write};

    fn fixture(mode: &str) -> AccountServer {
        let mut process = std::process::Command::new(std::env::current_exe().unwrap());
        process.args([
            "--exact",
            "api::agents::codex_auth::tests::fake_app_server",
            "--nocapture",
        ]);
        AccountServer::spawn(
            process,
            &BTreeMap::from([("QUNICA_TEST_CODEX_AUTH".into(), mode.into())]),
        )
        .unwrap()
    }

    #[test]
    fn fake_app_server() {
        let Ok(mode) = std::env::var("QUNICA_TEST_CODEX_AUTH") else {
            return;
        };
        for line in std::io::stdin().lock().lines() {
            let message: Value = serde_json::from_str(&line.unwrap()).unwrap();
            let method = message["method"].as_str().unwrap_or("");
            let reply = match method {
                "initialize" => Some(json!({"id":message["id"], "result":{}})),
                "account/read" => Some(
                    json!({"id":message["id"], "result":{"account":{"type":"chatgpt", "email":"private@example.test", "accessToken":"must-not-leak"}}}),
                ),
                "account/login/start" if mode == "error" => Some(
                    json!({"id":message["id"], "error":{"code":-1,"message":"private-token-must-not-leak"}}),
                ),
                "account/login/start" => Some(
                    json!({"id":message["id"], "result":{"type":"chatgptDeviceCode", "loginId":"login-1", "verificationUrl":"https://auth.openai.com/codex/device", "userCode":"TEST-1234"}}),
                ),
                _ => None,
            };
            if let Some(reply) = reply {
                println!("{reply}");
                std::io::stdout().flush().unwrap();
            }
            if method == "account/login/start" && mode == "success" {
                // Unrelated completions and credential-bearing updates are ignored.
                println!(
                    "{}",
                    json!({"method":"account/login/completed", "params":{"loginId":"another-login", "success":false}})
                );
                println!(
                    "{}",
                    json!({"method":"account/updated", "params":{"token":"must-not-leak"}})
                );
                println!(
                    "{}",
                    json!({"id":97,"method":"item/commandExecution/requestApproval", "params":{"command":"must-not-run"}})
                );
                std::io::stdout().flush().unwrap();
            }
            if message["id"] == 97 && message.get("method").is_none() {
                assert_eq!(message["error"]["code"], -32601);
                println!(
                    "{}",
                    json!({"method":"account/login/completed", "params":{"loginId":"login-1", "success":true}})
                );
                std::io::stdout().flush().unwrap();
            }
        }
    }

    #[tokio::test]
    async fn account_status_only_exposes_authentication_method() {
        let mut server = fixture("success");
        server.initialize().await.unwrap();
        let account = server
            .request(2, "account/read", json!({"refreshToken":false}))
            .await
            .unwrap();
        assert_eq!(
            account_status(&account),
            json!({"authenticated":true,"method":"chatgpt"})
        );
        assert_eq!(
            account_status(&json!({"account":null})),
            json!({"authenticated":false,"method":null})
        );
    }

    #[tokio::test]
    async fn device_login_matches_completion_and_rejects_agent_requests() {
        let mut server = fixture("success");
        server.initialize().await.unwrap();
        let response = server
            .request(
                2,
                "account/login/start",
                json!({"type":"chatgptDeviceCode"}),
            )
            .await
            .unwrap();
        let (login_id, device) = device_code(&response).unwrap();
        assert_eq!(
            device,
            json!({"type":"device_code","verification_url":"https://auth.openai.com/codex/device","user_code":"TEST-1234"})
        );
        let permit = Arc::new(Semaphore::new(1)).acquire_owned().await.unwrap();
        let mut login = DeviceLogin {
            server,
            login_id,
            device: None,
            deadline: Instant::now() + Duration::from_secs(5),
            _permit: permit,
        };
        timeout(Duration::from_secs(5), login.completed())
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn failures_do_not_echo_raw_cli_errors() {
        let mut server = fixture("error");
        server.initialize().await.unwrap();
        let error = server
            .request(
                2,
                "account/login/start",
                json!({"type":"chatgptDeviceCode"}),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), "codex_auth_failed");
        assert!(!format!("{error:?}").contains("private-token"));
    }

    #[tokio::test]
    async fn cancelling_a_pending_login_reaps_the_process_before_removing_its_cwd() {
        let mut server = fixture("pending");
        server.initialize().await.unwrap();
        server
            .request(
                2,
                "account/login/start",
                json!({"type":"chatgptDeviceCode"}),
            )
            .await
            .unwrap();
        let cwd = server.home.as_ref().unwrap().path().to_path_buf();
        drop(server);
        timeout(Duration::from_secs(5), async {
            while cwd.exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }

    #[test]
    fn rejects_untrusted_links_and_environment_locations() {
        for url in [
            "http://auth.openai.com/codex/device",
            "https://evil.test/codex/device",
            "https://auth.openai.com/codex/device?token=secret",
            "https://user@auth.openai.com/codex/device",
        ] {
            assert!(device_code(&json!({"type":"chatgptDeviceCode","loginId":"1","userCode":"TEST-1234","verificationUrl":url})).is_err());
        }
        assert!(runtime_env(AuthRequest {
            env: BTreeMap::from([("CODEX_HOME".into(), "untrusted".into())])
        })
        .is_err());
    }

    #[tokio::test]
    async fn authentication_is_required_before_spawning_a_login() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let app = crate::api::router_for_tests().await;
        for endpoint in ["status", "login"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v2/agents/codex-auth/{endpoint}"))
                        .header("content-type", "application/json")
                        .body(Body::from("{\"env\":{}}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    #[ignore = "Read-only check against an installed Codex CLI; never starts login"]
    async fn installed_codex_status_smoke() {
        let status = timeout(SETUP_TIMEOUT, async {
            let mut server = AccountServer::start(&BTreeMap::new()).unwrap();
            server.initialize().await.unwrap();
            let account = server
                .request(2, "account/read", json!({"refreshToken":false}))
                .await
                .unwrap();
            account_status(&account)
        })
        .await
        .unwrap();
        assert!(status["authenticated"].is_boolean());
        assert_eq!(status.as_object().unwrap().len(), 2);
    }
}
