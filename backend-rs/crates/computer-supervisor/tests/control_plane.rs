use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use qunica_computer_supervisor::{protocol::*, ComputerProvider, Error, Supervisor};
use serde_json::{json, Value};
use tokio::sync::Notify;
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "fixture-supervisor-token-at-least-32-bytes";

struct DockerJournal(Option<tempfile::TempDir>);

impl Drop for DockerJournal {
    fn drop(&mut self) {
        if std::thread::panicking() {
            if let Some(directory) = self.0.take() {
                eprintln!(
                    "Docker fixture journal retained for explicit recovery: {:?}",
                    directory.keep()
                );
            }
        }
    }
}

#[derive(Default)]
struct Driver {
    desktop: AtomicBool,
    uncertain: AtomicBool,
    creates: AtomicUsize,
    executions: AtomicUsize,
    stops: AtomicUsize,
    entered: Notify,
    release: Notify,
}

#[async_trait]
impl ComputerProvider for Driver {
    fn capabilities(&self) -> Value {
        json!({"network":false,"browser":self.desktop.load(Ordering::SeqCst),"desktop":self.desktop.load(Ordering::SeqCst)})
    }
    async fn create(&self, _: &Computer) -> Result<(), Error> {
        self.creates.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn start(&self, _: &Computer) -> Result<(), Error> {
        Ok(())
    }
    async fn stop(&self, _: &Computer) -> Result<(), Error> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn rebuild(&self, _: &Computer) -> Result<(), Error> {
        Ok(())
    }
    async fn destroy(&self, _: &Computer, _: bool) -> Result<(), Error> {
        Ok(())
    }
    async fn execute(&self, _: &Computer, request: ExecRequest) -> Result<ExecResult, Error> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        if request.argv[0] == "wait" {
            self.entered.notify_one();
            self.release.notified().await;
        }
        Ok(ExecResult {
            exit_code: if request.argv[0] == "python3" { 0 } else { 7 },
            stdout: if self.uncertain.load(Ordering::SeqCst) {
                "{\"uncertain\":true}".into()
            } else {
                "fixture result".into()
            },
            stderr: String::new(),
            truncated: false,
        })
    }
}

#[tokio::test]
async fn takeover_fences_new_agent_actions_and_old_human_credentials() {
    let (_root, driver, app) = fixture();
    let computer = spec();
    create(&app, &computer).await;
    let path = format!("/v1/computers/{}", computer.id);
    let exec_path = format!("{path}/exec");
    let app2 = app.clone();
    let running = tokio::spawn(async move {
        request(
            &app2,
            "POST",
            &exec_path,
            json!({"expected_generation":"1","argv":["wait"]}),
        )
        .await
    });
    driver.entered.notified().await;
    let app2 = app.clone();
    let ctrl = format!("{path}/control");
    let take = tokio::spawn(async move {
        request(
            &app2,
            "POST",
            &ctrl,
            json!({"expected_generation":"1","action":"request"}),
        )
        .await
    });
    tokio::task::yield_now().await;
    for _ in 0..100 {
        let (_, status) = request(
            &app,
            "POST",
            &format!("{path}/control"),
            json!({"expected_generation":"1","action":"status"}),
        )
        .await;
        if status["state"] == "takeover_requested" {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(!take.is_finished());
    driver.release.notify_one();
    running.await.unwrap();
    let (code, lease) = take.await.unwrap();
    assert_eq!(code, StatusCode::OK, "{lease}");
    assert_eq!(lease["state"], "user_controlled");
    let token = lease["credential"].clone();
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("{path}/exec"),
            json!({"expected_generation":"1","argv":["echo"]})
        )
        .await
        .1["error"]["code"],
        "computer_control_held"
    );
    let stale = request(
        &app,
        "POST",
        &format!("{path}/control"),
        json!({"expected_generation":"0","action":"request"}),
    )
    .await;
    assert_eq!(stale.1["error"]["code"], "generation_mismatch");
    let (_, status) = request(
        &app,
        "POST",
        &format!("{path}/control"),
        json!({"expected_generation":"1","action":"status"}),
    )
    .await;
    assert_eq!(status["state"], "user_controlled");
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("{path}/control"),
            json!({"expected_generation":"1","action":"release","credential":token})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("{path}/exec"),
            json!({"expected_generation":"1","argv":["echo"]})
        )
        .await
        .1["error"]["code"],
        "computer_control_held"
    );
    request(
        &app,
        "POST",
        &format!("{path}/control"),
        json!({"expected_generation":"1","action":"resume"}),
    )
    .await;
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("{path}/exec"),
            json!({"expected_generation":"1","argv":["echo"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("{path}/control"),
            json!({"expected_generation":"1","action":"renew","credential":token})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}

fn spec() -> ComputerSpec {
    ComputerSpec {
        id: Uuid::new_v4(),
        owner_id: Uuid::new_v4(),
        binding: Binding::Team(Uuid::new_v4()),
        name: "Fixture computer".into(),
    }
}

async fn request(app: &Router, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn fixture() -> (tempfile::TempDir, Arc<Driver>, Router) {
    let root = tempfile::tempdir().unwrap();
    let driver = Arc::new(Driver::default());
    let app = Supervisor::open(root.path().to_owned(), TOKEN, driver.clone())
        .unwrap()
        .router();
    (root, driver, app)
}

async fn create(app: &Router, spec: &ComputerSpec) -> Value {
    let (status, computer) = request(
        app,
        "POST",
        "/v1/computers",
        serde_json::to_value(spec).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{computer}");
    computer
}

#[tokio::test]
async fn a_browser_rpc_timeout_fences_the_computer_even_after_the_http_helper_exits() {
    let (_root, driver, app) = fixture();
    driver.desktop.store(true, Ordering::SeqCst);
    driver.uncertain.store(true, Ordering::SeqCst);
    let spec = spec();
    create(&app, &spec).await;
    let path = format!("/v1/computers/{}", spec.id);
    let (status,error)=request(&app,"POST",&format!("{path}/call"),json!({"expected_generation":"1","request":{"op":"browser","tool":"browser_navigate","args":{"url":"https://fixture.test"}}})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "execution_uncertain");
    let (_, state) = request(&app, "GET", &path, Value::Null).await;
    assert_eq!(state["status"], "uncertain");
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("{path}/actions"),
            json!({"expected_generation":"1","action":"start"})
        )
        .await
        .1["error"]["code"],
        "explicit_stop_required"
    );
    driver.uncertain.store(false, Ordering::SeqCst);
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("{path}/actions"),
            json!({"expected_generation":"1","action":"stop"})
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn authentication_precedes_docker_and_browser_origins_are_refused() {
    let (_root, driver, app) = fixture();
    for credential in [None, Some("wrong"), Some(TOKEN)] {
        let mut builder = Request::builder().uri("/v1/health");
        if let Some(token) = credential {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        if credential == Some(TOKEN) {
            builder = builder.header("origin", "https://example.invalid");
        }
        let response = app
            .clone()
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    assert_eq!(driver.creates.load(Ordering::SeqCst), 0);
    let (status, capability) = request(&app, "GET", "/v1/health", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(capability["network"], false);
    assert_eq!(capability["browser"], false);
}

#[tokio::test]
async fn accepted_identity_is_idempotent_and_raw_docker_settings_are_rejected() {
    let (_root, driver, app) = fixture();
    let computer = spec();
    let original = create(&app, &computer).await;
    let duplicate = create(&app, &computer).await;
    assert_eq!(original, duplicate);
    assert_eq!(driver.creates.load(Ordering::SeqCst), 1);
    let mut changed = computer.clone();
    changed.name = "another".into();
    let (status, error) = request(
        &app,
        "POST",
        "/v1/computers",
        serde_json::to_value(changed).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "idempotency_conflict");
    let mut injected = serde_json::to_value(spec()).unwrap();
    injected["mounts"] = json!([{"source":"/","target":"/host"}]);
    assert_eq!(
        request(&app, "POST", "/v1/computers", injected).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(driver.creates.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn generations_destroy_confirmation_and_nonzero_exit_are_authoritative() {
    let (_root, driver, app) = fixture();
    let computer = spec();
    create(&app, &computer).await;
    let path = format!("/v1/computers/{}", computer.id);
    let (status, result) = request(
        &app,
        "POST",
        &format!("{path}/exec"),
        json!({"expected_generation":"1","argv":["false"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["exit_code"], 7);
    let (_, state) = request(&app, "GET", &path, Value::Null).await;
    assert_eq!(state["status"], "running");
    let (_, stopped) = request(
        &app,
        "POST",
        &format!("{path}/actions"),
        json!({"expected_generation":"1","action":"stop"}),
    )
    .await;
    assert_eq!(stopped["generation"], "2");
    assert_eq!(stopped["status"], "stopped");
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("{path}/exec"),
            json!({"expected_generation":"1","argv":["false"]})
        )
        .await
        .1["error"]["code"],
        "generation_mismatch"
    );
    assert_eq!(driver.executions.load(Ordering::SeqCst), 1);
    let (_, started) = request(
        &app,
        "POST",
        &format!("{path}/actions"),
        json!({"expected_generation":"2","action":"start"}),
    )
    .await;
    assert_eq!(started["generation"], "3");
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("{path}/actions"),
            json!({"expected_generation":"3","action":"destroy","purge_home":true})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_,destroyed)=request(&app,"POST",&format!("{path}/actions"),json!({"expected_generation":"3","action":"destroy","confirm":computer.id,"purge_home":true})).await;
    assert_eq!(destroyed["status"], "destroyed");
    assert_eq!(destroyed["home_present"], false);
    assert_eq!(create(&app, &computer).await["status"], "destroyed");
}

#[tokio::test]
async fn cancelled_http_does_not_grant_a_second_driver() {
    let (_root, driver, app) = fixture();
    let computer = spec();
    create(&app, &computer).await;
    let path = format!("/v1/computers/{}", computer.id);
    let target = format!("{path}/exec");
    let client = app.clone();
    let http = tokio::spawn(async move {
        request(
            &client,
            "POST",
            &target,
            json!({"expected_generation":"1","argv":["wait"]}),
        )
        .await
    });
    driver.entered.notified().await;
    http.abort();
    let _ = http.await;
    let (status, error) = request(
        &app,
        "POST",
        &format!("{path}/actions"),
        json!({"expected_generation":"1","action":"stop"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "computer_busy");
    assert_eq!(driver.stops.load(Ordering::SeqCst), 0);
    driver.release.notify_one();
    for _ in 0..100 {
        tokio::task::yield_now().await;
        if request(&app, "GET", &path, Value::Null).await.0 == StatusCode::OK {
            break;
        }
    }
    assert_eq!(
        request(&app, "GET", &path, Value::Null).await.1["status"],
        "running"
    );
    assert_eq!(driver.executions.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn exec_timeout_requires_confirmed_stop_and_never_retries() {
    let (_root, driver, app) = fixture();
    let computer = spec();
    create(&app, &computer).await;
    let path = format!("/v1/computers/{}", computer.id);
    let target = format!("{path}/exec");
    let client = app.clone();
    let http = tokio::spawn(async move {
        request(
            &client,
            "POST",
            &target,
            json!({"expected_generation":"1","argv":["wait"],"timeout_seconds":1}),
        )
        .await
    });
    driver.entered.notified().await;
    tokio::time::advance(std::time::Duration::from_secs(2)).await;
    assert_eq!(http.await.unwrap().0, StatusCode::CONFLICT);
    assert_eq!(
        request(&app, "GET", &path, Value::Null).await.1["status"],
        "uncertain"
    );
    for action in ["start", "rebuild"] {
        assert_eq!(
            request(
                &app,
                "POST",
                &format!("{path}/actions"),
                json!({"expected_generation":"1","action":action})
            )
            .await
            .1["error"]["code"],
            "explicit_stop_required"
        );
    }
    assert_eq!(driver.executions.load(Ordering::SeqCst), 1);
    let (_, state) = request(
        &app,
        "POST",
        &format!("{path}/actions"),
        json!({"expected_generation":"1","action":"stop"}),
    )
    .await;
    assert_eq!(state["status"], "stopped");
    assert_eq!(state["generation"], "2");
}

#[tokio::test]
async fn journal_reopen_fences_existing_processes_and_excludes_second_supervisor() {
    let (root, driver, app) = fixture();
    let computer = spec();
    create(&app, &computer).await;
    assert!(Supervisor::open(root.path().to_owned(), TOKEN, driver.clone()).is_err());
    drop(app);
    let reopened = Supervisor::open(root.path().to_owned(), TOKEN, driver)
        .unwrap()
        .router();
    let path = format!("/v1/computers/{}", computer.id);
    let (_, state) = request(&reopened, "GET", &path, Value::Null).await;
    assert_eq!(state["status"], "uncertain");
    assert_eq!(state["error_code"], "supervisor_restarted");
    assert_eq!(state["generation"], "1");
    assert_eq!(
        request(
            &reopened,
            "POST",
            &format!("{path}/exec"),
            json!({"expected_generation":"1","argv":["false"]})
        )
        .await
        .1["error"]["code"],
        "computer_not_running"
    );
}

#[tokio::test]
async fn invalid_execution_paths_and_overrides_never_reach_provider() {
    let (_root, driver, app) = fixture();
    let computer = spec();
    create(&app, &computer).await;
    let path = format!("/v1/computers/{}/exec", computer.id);
    for cwd in ["../other", "/host", "a/../../etc", "a\\b"] {
        assert_eq!(
            request(
                &app,
                "POST",
                &path,
                json!({"expected_generation":"1","argv":["echo"],"cwd":cwd})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        request(
            &app,
            "POST",
            &path,
            json!({"expected_generation":"1","argv":["echo"],"env":{"KEY":"value"}})
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(driver.executions.load(Ordering::SeqCst), 0);
}

#[tokio::test]
#[ignore = "requires a Linux Docker engine and a local Python computer image"]
async fn docker_preserves_home_isolates_private_data_and_fences_uncertain_execs() {
    use qunica_computer_supervisor::docker::DockerProvider;
    let journal = DockerJournal(Some(tempfile::tempdir().unwrap()));
    let root = journal.0.as_ref().unwrap();
    let image =
        std::env::var("QUNICA_COMPUTER_TEST_IMAGE").unwrap_or_else(|_| "python:3.11-slim".into());
    let driver = Arc::new(DockerProvider::connect(image).await.unwrap());
    let app = Supervisor::open(root.path().to_owned(), TOKEN, driver.clone())
        .unwrap()
        .router();
    let team = spec();
    let team_value = create(&app, &team).await;
    let record: Computer = serde_json::from_value(team_value).unwrap();
    let path = format!("/v1/computers/{}", team.id);
    let docker = bollard::Docker::connect_with_local_defaults()
        .unwrap()
        .negotiate_version()
        .await
        .unwrap();
    let container = docker
        .inspect_container(&DockerProvider::container_name(&record), None)
        .await
        .unwrap();
    let config = container.config.unwrap();
    let host = container.host_config.unwrap();
    assert_eq!(config.user.as_deref(), Some("10001:10001"));
    assert_eq!(host.privileged, Some(false));
    assert_eq!(host.readonly_rootfs, Some(true));
    assert_eq!(host.network_mode.as_deref(), Some("none"));
    assert_eq!(host.cap_drop, Some(vec!["ALL".into()]));
    let mounts = container.mounts.unwrap();
    assert_eq!(mounts.len(), 1);
    assert_eq!(
        mounts[0].name.as_deref(),
        Some(DockerProvider::home_name(&record).as_str())
    );
    assert_eq!(mounts[0].destination.as_deref(), Some("/home/agent"));
    assert!(host
        .port_bindings
        .is_none_or(|bindings| bindings.is_empty()));

    let child="import time,pathlib,os;os.setsid()\nwhile True:\n pathlib.Path('orphan-heartbeat.txt').write_text(str(time.time_ns()));time.sleep(.01)";
    let background=format!("import subprocess,sys,time,pathlib; subprocess.Popen([sys.executable,'-c',{}],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);time.sleep(.2);assert pathlib.Path('orphan-heartbeat.txt').exists();print('parent exited')",serde_json::to_string(child).unwrap());
    let (status, output) = request(
        &app,
        "POST",
        &format!("{path}/exec"),
        json!({"expected_generation":"1","argv":["python3","-c",background]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{output}");
    assert_eq!(
        output["exit_code"], 126,
        "background commands must not report success"
    );
    let check="from pathlib import Path;import time;p=Path('orphan-heartbeat.txt');before=p.read_text() if p.exists() else None;time.sleep(.2);assert (p.read_text() if p.exists() else None)==before;print('descendants stopped')";
    let (_, output) = request(
        &app,
        "POST",
        &format!("{path}/exec"),
        json!({"expected_generation":"1","argv":["python3","-c",check]}),
    )
    .await;
    assert_eq!(output["exit_code"], 0, "{output}");
    let program = "import os,socket,pathlib; assert os.getuid()==10001; assert not pathlib.Path('/var/run/docker.sock').exists(); assert not pathlib.Path('/data/qunica.sqlite3').exists(); pathlib.Path('artifact.txt').write_text('verified persistent artifact'); s=socket.socket(); s.settimeout(.2); assert s.connect_ex(('169.254.169.254',80)) != 0; print('contained')";
    let (status, output) = request(
        &app,
        "POST",
        &format!("{path}/exec"),
        json!({"expected_generation":"1","argv":["python3","-c",program]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{output}");
    assert_eq!(output["exit_code"], 0, "{output}");
    assert_eq!(output["stdout"], "contained\n");
    let (_, rebuilt) = request(
        &app,
        "POST",
        &format!("{path}/actions"),
        json!({"expected_generation":"1","action":"rebuild"}),
    )
    .await;
    assert_eq!(rebuilt["generation"], "2", "{rebuilt}");
    let (status, output) = request(&app,"POST",&format!("{path}/exec"),json!({"expected_generation":"2","argv":["python3","-c","from pathlib import Path; print(Path('artifact.txt').read_text())"]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(output["stdout"], "verified persistent artifact\n");

    let mut private = spec();
    private.owner_id = team.owner_id;
    private.binding = Binding::Private(Uuid::new_v4());
    create(&app, &private).await;
    let private_path = format!("/v1/computers/{}", private.id);
    let (_, output) = request(&app,"POST",&format!("{private_path}/exec"),json!({"expected_generation":"1","argv":["python3","-c","from pathlib import Path; assert not Path('artifact.txt').exists(); print('private')"]})).await;
    assert_eq!(output["stdout"], "private\n");
    let (_, output) = request(
        &app,
        "POST",
        &format!("{private_path}/exec"),
        json!({"expected_generation":"1","argv":["python3","-c","print('x'*200000)"]}),
    )
    .await;
    assert_eq!(output["truncated"], true);
    assert!(output["stdout"].as_str().unwrap().len() <= 128 * 1024);

    let (status, _) = request(&app,"POST",&format!("{path}/exec"),json!({"expected_generation":"2","timeout_seconds":1,"argv":["python3","-c","import time\nfrom pathlib import Path\nwhile True:\n Path('heartbeat.txt').write_text(str(time.time_ns()))\n time.sleep(.05)"]})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        request(&app, "GET", &path, Value::Null).await.1["status"],
        "uncertain"
    );
    let (_, stopped) = request(
        &app,
        "POST",
        &format!("{path}/actions"),
        json!({"expected_generation":"2","action":"stop"}),
    )
    .await;
    assert_eq!(stopped["status"], "stopped", "{stopped}");
    let (_, started) = request(
        &app,
        "POST",
        &format!("{path}/actions"),
        json!({"expected_generation":"3","action":"start"}),
    )
    .await;
    assert_eq!(started["generation"], "4", "{started}");
    let (_, output) = request(&app,"POST",&format!("{path}/exec"),json!({"expected_generation":"4","argv":["python3","-c","import time; from pathlib import Path; marker=Path('heartbeat.txt'); before=marker.read_text(); time.sleep(.3); assert marker.read_text()==before; print(Path('artifact.txt').read_text())"]})).await;
    assert_eq!(output["exit_code"], 0, "{output}");
    assert_eq!(output["stdout"], "verified persistent artifact\n");
    drop(app);

    let app = Supervisor::open(root.path().to_owned(), TOKEN, driver)
        .unwrap()
        .router();
    assert_eq!(
        request(&app, "GET", &path, Value::Null).await.1["status"],
        "uncertain"
    );
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("{path}/exec"),
            json!({"expected_generation":"4","argv":["python3","-V"]})
        )
        .await
        .1["error"]["code"],
        "computer_not_running"
    );
    request(
        &app,
        "POST",
        &format!("{path}/actions"),
        json!({"expected_generation":"4","action":"stop"}),
    )
    .await;
    request(
        &app,
        "POST",
        &format!("{path}/actions"),
        json!({"expected_generation":"5","action":"start"}),
    )
    .await;
    let (_, output) = request(&app,"POST",&format!("{path}/exec"),json!({"expected_generation":"6","argv":["python3","-c","from pathlib import Path; print(Path('artifact.txt').read_text())"]})).await;
    assert_eq!(output["stdout"], "verified persistent artifact\n");
    for (computer, generation) in [(&team, "6"), (&private, "1")] {
        let (status, value) = request(&app,"POST",&format!("/v1/computers/{}/actions",computer.id),json!({"expected_generation":generation,"action":"destroy","confirm":computer.id,"purge_home":true})).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["home_present"], false);
    }
    assert!(docker
        .inspect_volume(&DockerProvider::home_name(&record))
        .await
        .is_err());
}
