//! Restricted Docker control plane. Agents never receive this service's token
//! or socket. Deployment and staged capability boundaries: guide/computers.md.

mod control;
pub mod docker;
pub mod protocol;
mod store;

use std::{path::PathBuf, sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{
    extract::{DefaultBodyLimit, Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use protocol::{Action, ActionRequest, Computer, ComputerSpec, ExecRequest, ExecResult, Status};
use serde_json::json;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

#[derive(Debug)]
pub struct Error {
    status: StatusCode,
    code: &'static str,
}

impl Error {
    fn conflict(code: &'static str) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code,
        }
    }
    fn busy() -> Self {
        Self::conflict("computer_busy")
    }
    fn missing() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "computer_not_found",
        }
    }
    fn storage() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "state_unavailable",
        }
    }
    fn invalid() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_input",
        }
    }
    pub fn uncertain() -> Self {
        Self::conflict("execution_uncertain")
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error":{"code":self.code,"message":self.code}})),
        )
            .into_response()
    }
}

#[async_trait]
pub trait ComputerProvider: Send + Sync + 'static {
    fn capabilities(&self) -> serde_json::Value {
        json!({"network":false,"browser":false,"desktop":false,"acp_opencode":false})
    }
    async fn create(&self, computer: &Computer) -> Result<(), Error>;
    async fn start(&self, computer: &Computer) -> Result<(), Error>;
    async fn stop(&self, computer: &Computer) -> Result<(), Error>;
    async fn rebuild(&self, computer: &Computer) -> Result<(), Error>;
    async fn destroy(&self, computer: &Computer, purge_home: bool) -> Result<(), Error>;
    async fn execute(&self, computer: &Computer, request: ExecRequest)
        -> Result<ExecResult, Error>;
}

#[derive(Clone)]
pub struct Supervisor(Arc<Inner>);

struct Inner {
    store: store::Store,
    controls: control::Controls,
    token_hash: [u8; 32],
    provider: Arc<dyn ComputerProvider>,
}

impl Supervisor {
    pub fn open(
        root: PathBuf,
        token: &str,
        provider: Arc<dyn ComputerProvider>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            token.len() >= 32 && !token.contains(char::is_whitespace),
            "supervisor token must contain at least 32 non-whitespace bytes"
        );
        Ok(Self(Arc::new(Inner {
            store: store::Store::open(root.clone())?,
            controls: control::Controls::open(root)?,
            token_hash: Sha256::digest(token).into(),
            provider,
        })))
    }

    pub fn supervisor_id(&self) -> Uuid {
        self.0.store.supervisor_id
    }

    pub fn router(self) -> Router {
        Router::new()
            .route("/v1/health", get(health))
            .route("/v1/computers", post(create))
            .route("/v1/computers/:id", get(read))
            .route("/v1/computers/:id/actions", post(action))
            .route("/v1/computers/:id/exec", post(execute))
            .route("/v1/computers/:id/control", post(control_action))
            .route("/v1/computers/:id/call", post(computer_call))
            .layer(DefaultBodyLimit::max(256 * 1024))
            .layer(middleware::from_fn_with_state(self.clone(), authenticate))
            .with_state(self)
    }

    async fn create(&self, spec: ComputerSpec) -> Result<Computer, Error> {
        if spec.name.trim().is_empty() || spec.name.len() > 120 {
            return Err(Error::invalid());
        }
        let (mut computer, fresh) = self.0.store.insert(spec).await?;
        if !fresh {
            return Ok(computer.clone());
        }
        let result =
            tokio::time::timeout(Duration::from_secs(60), self.0.provider.create(&computer))
                .await
                .unwrap_or_else(|_| Err(Error::uncertain()));
        self.finish(&mut computer, result, Status::Running)?;
        Ok(computer.clone())
    }

    fn finish(
        &self,
        computer: &mut Computer,
        result: Result<(), Error>,
        success: Status,
    ) -> Result<(), Error> {
        computer.status = if result.is_ok() {
            success
        } else {
            Status::Uncertain
        };
        computer.error_code = result.as_ref().err().map(|error| error.code.to_string());
        if self.0.store.save(computer).is_err() {
            computer.status = Status::Uncertain;
            computer.error_code = Some("state_unavailable".into());
            return Err(Error::storage());
        }
        result
    }

    async fn action(&self, id: Uuid, request: ActionRequest) -> Result<Computer, Error> {
        let mut computer = self.0.store.lock(id).await?;
        if computer.generation != request.expected_generation {
            return Err(Error::conflict("generation_mismatch"));
        }
        if (request.purge_home && !matches!(request.action, Action::Destroy))
            || (matches!(request.action, Action::Destroy) && request.confirm != Some(id))
        {
            return Err(Error::invalid());
        }
        if matches!(request.action, Action::Start | Action::Rebuild)
            && (!computer.home_present
                || matches!(computer.status, Status::Uncertain | Status::Destroyed))
        {
            return Err(Error::conflict("explicit_stop_required"));
        }
        if computer.status == Status::Destroyed {
            return Err(Error::conflict("computer_destroyed"));
        }
        if matches!(request.action, Action::Start) && computer.status != Status::Stopped {
            return Err(Error::conflict("computer_not_stopped"));
        }
        let generation = computer
            .generation
            .parse::<u64>()
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(Error::storage)?;
        computer.generation = generation.to_string();
        computer.status = match request.action {
            Action::Start => Status::Starting,
            Action::Stop => Status::Stopping,
            Action::Rebuild => Status::Rebuilding,
            Action::Destroy => Status::Destroying,
        };
        self.0.store.save(&computer)?;
        let result = tokio::time::timeout(Duration::from_secs(60), async {
            match request.action {
                Action::Start => self.0.provider.start(&computer).await,
                Action::Stop => self.0.provider.stop(&computer).await,
                Action::Rebuild => self.0.provider.rebuild(&computer).await,
                Action::Destroy => self.0.provider.destroy(&computer, request.purge_home).await,
            }
        })
        .await
        .unwrap_or_else(|_| Err(Error::uncertain()));
        if result.is_ok() && matches!(request.action, Action::Destroy) && request.purge_home {
            computer.home_present = false;
        }
        self.finish(
            &mut computer,
            result,
            match request.action {
                Action::Stop => Status::Stopped,
                Action::Destroy => Status::Destroyed,
                _ => Status::Running,
            },
        )?;
        Ok(computer.clone())
    }

    async fn execute(&self, id: Uuid, request: ExecRequest) -> Result<ExecResult, Error> {
        if request.argv.is_empty()
            || request.argv.len() > 128
            || request.argv.iter().any(|arg| arg.contains('\0'))
            || request.argv.iter().map(String::len).sum::<usize>() > 32 * 1024
            || request.argv[0].is_empty()
            || request.cwd.starts_with('/')
            || request.cwd.contains('\\')
            || request.cwd.split('/').any(|part| part == "..")
            || request.cwd.contains('\0')
            || !(1..=300).contains(&request.timeout_seconds)
        {
            return Err(Error::invalid());
        }
        let mut computer = self.0.store.lock(id).await?;
        if computer.generation != request.expected_generation {
            return Err(Error::conflict("generation_mismatch"));
        }
        if computer.status != Status::Running {
            return Err(Error::conflict("computer_not_running"));
        }
        self.0
            .controls
            .check(id, &computer.generation, None, false)
            .await?;
        computer.status = Status::Executing;
        self.0.store.save(&computer)?;
        let duration = Duration::from_secs(request.timeout_seconds);
        let result = tokio::time::timeout(duration, self.0.provider.execute(&computer, request))
            .await
            .unwrap_or_else(|_| Err(Error::uncertain()));
        self.finish(
            &mut computer,
            result.as_ref().map(|_| ()).map_err(|_| Error::uncertain()),
            Status::Running,
        )?;
        result
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlRequest {
    expected_generation: String,
    action: String,
    credential: Option<String>,
}
async fn control_action(
    State(state): State<Supervisor>,
    Path(id): Path<Uuid>,
    Json(body): Json<ControlRequest>,
) -> Result<Json<serde_json::Value>, Error> {
    detached(async move {
        let saved = state.0.store.read_saved(id)?;
        if saved.generation != body.expected_generation {
            return Err(Error::conflict("generation_mismatch"));
        }
        if body.action == "status" {
            return state
                .0
                .controls
                .transition(id, &saved.generation, "status", None)
                .await;
        }
        if body.action == "request" {
            // Fence new actions before waiting for an existing driver.
            state
                .0
                .controls
                .transition(id, &body.expected_generation, "request", None)
                .await?;
        }
        let computer = state.0.store.wait_lock(id).await?;
        if computer.generation != body.expected_generation {
            return Err(Error::conflict("generation_mismatch"));
        }
        if body.action == "request" {
            if computer.status != Status::Running {
                return Err(Error::conflict("explicit_stop_required"));
            }
            let result = tokio::time::timeout(
                Duration::from_secs(30),
                state.0.provider.execute(
                    &computer,
                    local_request(&body.expected_generation, json!({"op":"quiesce"})),
                ),
            )
            .await
            .map_err(|_| Error::uncertain())??;
            if result.exit_code != 0 {
                return Err(Error::uncertain());
            }
            return state
                .0
                .controls
                .transition(id, &computer.generation, "grant", None)
                .await;
        }
        state
            .0
            .controls
            .transition(
                id,
                &computer.generation,
                &body.action,
                body.credential.as_deref(),
            )
            .await
    })
    .await
    .map(Json)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CallRequest {
    expected_generation: String,
    credential: Option<String>,
    request: serde_json::Value,
}
fn local_request(generation: &str, body: serde_json::Value) -> ExecRequest {
    let code="import sys,json,urllib.request; r=urllib.request.Request('http://127.0.0.1:8765',data=sys.argv[1].encode(),headers={'Content-Type':'application/json'});\ntry:\n print(urllib.request.urlopen(r,timeout=55).read().decode())\nexcept urllib.error.HTTPError as e:\n print(e.read().decode());sys.exit(1)";
    ExecRequest {
        expected_generation: generation.into(),
        argv: vec!["python3".into(), "-c".into(), code.into(), body.to_string()],
        cwd: String::new(),
        timeout_seconds: 60,
    }
}
async fn computer_call(
    State(state): State<Supervisor>,
    Path(id): Path<Uuid>,
    Json(mut body): Json<CallRequest>,
) -> Result<Json<ExecResult>, Error> {
    detached(async move {
        let rfb = matches!(
            body.request["op"].as_str(),
            Some("rfb_open" | "rfb_exchange" | "rfb_close")
        );
        if rfb {
            // Pixels and read-only RFB traffic remain observable during a long
            // agent action. The writable endpoint still requires human control.
            // Release waits for accepted writable RFB requests; observation
            // does not monopolize the execution lock during an agent action.
            let _input_guard = if body.credential.is_some() {
                Some(state.0.store.lock(id).await?)
            } else {
                None
            };
            let computer = match _input_guard.as_deref() {
                Some(value) => value.clone(),
                None => state.0.store.read_saved(id)?,
            };
            if computer.generation != body.expected_generation {
                return Err(Error::conflict("generation_mismatch"));
            }
            if !matches!(computer.status, Status::Running | Status::Executing) {
                return Err(Error::conflict("computer_not_running"));
            }
            body.request["read_only"] = json!(body.credential.is_none());
            state
                .0
                .controls
                .check(
                    id,
                    &computer.generation,
                    body.credential.as_deref(),
                    body.credential.is_none(),
                )
                .await?;
            if state.0.provider.capabilities()["desktop"] != true {
                return Err(Error::conflict("capability_unavailable"));
            }
            return tokio::time::timeout(
                Duration::from_secs(10),
                state.0.provider.execute(
                    &computer,
                    local_request(&body.expected_generation, body.request),
                ),
            )
            .await
            .unwrap_or_else(|_| Err(Error::uncertain()));
        }
        let mut computer = state.0.store.lock(id).await?;
        if computer.generation != body.expected_generation {
            return Err(Error::conflict("generation_mismatch"));
        }
        if computer.status != Status::Running {
            return Err(Error::conflict("computer_not_running"));
        }
        let observe = matches!(
            body.request["op"].as_str(),
            Some("screen" | "artifact" | "quiesce")
        );
        state
            .0
            .controls
            .check(
                id,
                &computer.generation,
                body.credential.as_deref(),
                observe,
            )
            .await?;
        if !state.0.provider.capabilities()["desktop"]
            .as_bool()
            .unwrap_or(false)
        {
            return Err(Error::conflict("capability_unavailable"));
        }
        computer.status = Status::Executing;
        state.0.store.save(&computer)?;
        let result = tokio::time::timeout(
            Duration::from_secs(60),
            state.0.provider.execute(
                &computer,
                local_request(&body.expected_generation, body.request),
            ),
        )
        .await
        .unwrap_or_else(|_| Err(Error::uncertain()));
        let browser_uncertain = result.as_ref().ok().is_some_and(|output| {
            serde_json::from_str::<serde_json::Value>(&output.stdout)
                .ok()
                .is_some_and(|value| value["uncertain"] == true)
        });
        let result = if browser_uncertain {
            Err(Error::uncertain())
        } else {
            result
        };
        state.finish(
            &mut computer,
            result.as_ref().map(|_| ()).map_err(|_| Error::uncertain()),
            Status::Running,
        )?;
        result
    })
    .await
    .map(Json)
}

async fn health(State(state): State<Supervisor>) -> Json<serde_json::Value> {
    let mut value = state.0.provider.capabilities();
    value["status"] = json!("ok");
    value["protocol"] = json!("qunica.computer.v1");
    Json(value)
}

async fn authenticate(State(state): State<Supervisor>, request: Request, next: Next) -> Response {
    let credential = request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    let hash: [u8; 32] = Sha256::digest(credential).into();
    // Note: this is a server-only control plane, never a browser CORS surface.
    if request.headers().contains_key("origin") || !bool::from(hash.ct_eq(&state.0.token_hash)) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":{"code":"unauthorized"}})),
        )
            .into_response();
    }
    next.run(request).await
}

async fn detached<T: Send + 'static>(
    future: impl std::future::Future<Output = Result<T, Error>> + Send + 'static,
) -> Result<T, Error> {
    // Dropping the HTTP future detaches this task rather than relinquishing its
    // computer lock while Docker may still be applying the operation.
    tokio::spawn(future).await.map_err(|_| Error::uncertain())?
}

async fn create(
    State(state): State<Supervisor>,
    Json(spec): Json<ComputerSpec>,
) -> Result<Json<Computer>, Error> {
    detached(async move { state.create(spec).await })
        .await
        .map(Json)
}
async fn read(
    State(state): State<Supervisor>,
    Path(id): Path<Uuid>,
) -> Result<Json<Computer>, Error> {
    Ok(Json(state.0.store.read_saved(id)?))
}
async fn action(
    State(state): State<Supervisor>,
    Path(id): Path<Uuid>,
    Json(request): Json<ActionRequest>,
) -> Result<Json<Computer>, Error> {
    detached(async move { state.action(id, request).await })
        .await
        .map(Json)
}
async fn execute(
    State(state): State<Supervisor>,
    Path(id): Path<Uuid>,
    Json(request): Json<ExecRequest>,
) -> Result<Json<ExecResult>, Error> {
    detached(async move { state.execute(id, request).await })
        .await
        .map(Json)
}
