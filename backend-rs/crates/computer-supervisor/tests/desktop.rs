use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use qunica_computer_supervisor::{
    docker::DockerProvider,
    protocol::{Binding, ComputerSpec},
    Supervisor,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
const TOKEN: &str = "desktop-fixture-secret-at-least-32-characters";
async fn post(app: &Router, path: &str, body: Value) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let code = response.status();
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(code, StatusCode::OK, "{value}");
    value
}
async fn call(app: &Router, path: &str, request: Value) -> Value {
    call_at(app, path, "1", request).await
}
async fn call_at(app: &Router, path: &str, generation: &str, request: Value) -> Value {
    let value = post(
        app,
        &format!("{path}/call"),
        json!({"expected_generation":generation,"request":request}),
    )
    .await;
    assert_eq!(value["exit_code"], 0, "{value}");
    assert_ne!(value["truncated"], true, "{value}");
    serde_json::from_str(value["stdout"].as_str().unwrap()).unwrap()
}

// The fixture website is infrastructure, independent of the foreground Agent
// exec under test; it is restarted explicitly after a lifecycle operation.
async fn fixture_site(spec: &qunica_computer_supervisor::protocol::Computer) {
    let docker = bollard::Docker::connect_with_local_defaults()
        .unwrap()
        .negotiate_version()
        .await
        .unwrap();
    let exec = docker
        .create_exec(
            &DockerProvider::container_name(spec),
            bollard::exec::CreateExecOptions {
                cmd: Some(vec![
                    "python3",
                    "-m",
                    "http.server",
                    "8080",
                    "--bind",
                    "127.0.0.1",
                    "--directory",
                    "fixture",
                ]),
                user: Some("10001:10001"),
                working_dir: Some("/home/agent/workspace"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    docker
        .start_exec(
            &exec.id,
            Some(bollard::exec::StartExecOptions {
                detach: true,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
}
#[tokio::test]
#[ignore = "requires built qunica-computer and qunica-computer-egress images"]
async fn docker_chromium_download_takeover_and_stale_references() {
    let root = tempfile::tempdir().unwrap();
    let provider = Arc::new(
        DockerProvider::connect("qunica-computer:local".into())
            .await
            .unwrap()
            .with_services(true)
            .with_attestation(TOKEN.into()),
    );
    let app = Supervisor::open(root.path().into(), TOKEN, provider)
        .unwrap()
        .router();
    let spec = ComputerSpec {
        id: Uuid::new_v4(),
        owner_id: Uuid::new_v4(),
        binding: Binding::Team(Uuid::new_v4()),
        name: "Desktop fixture".into(),
    };
    let path = format!("/v1/computers/{}", spec.id);
    let computer: qunica_computer_supervisor::protocol::Computer = serde_json::from_value(
        post(&app, "/v1/computers", serde_json::to_value(&spec).unwrap()).await,
    )
    .unwrap();
    use futures_util::FutureExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let served = app.clone();
    let server = tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });
    let outcome=std::panic::AssertUnwindSafe(async {
    // Keep a bounded readiness loop; it never resubmits a browser action.
    post(&app,&format!("{path}/exec"),json!({"expected_generation":"1","argv":["python3","-c","import socket,time\nfor _ in range(100):\n s=socket.socket();s.settimeout(.1)\n if s.connect_ex(('127.0.0.1',8765))==0:break\n time.sleep(.1)\nelse:raise Exception('not ready')"]})).await;

    let csrf="import urllib.request,urllib.error\nr=urllib.request.Request('http://127.0.0.1:8765',data=b'{\"op\":\"quiesce\"}',headers={'Content-Type':'application/json','Origin':'https://untrusted.example'})\ntry:\n urllib.request.urlopen(r);raise Exception('browser origin accepted')\nexcept urllib.error.HTTPError as e:\n assert e.code==403";
    let denied=post(&app,&format!("{path}/exec"),json!({"expected_generation":"1","argv":["python3","-c",csrf]})).await;assert_eq!(denied["exit_code"],0,"{denied}");
    let setup="mkdir -p fixture; printf '%s' '<html><body><h1>Fixture</h1><a href=\"/artifact.txt\" download>Download artifact</a><input aria-label=\"Name\"></body></html>' > fixture/index.html; printf 'verified browser artifact' > fixture/artifact.txt";
    post(&app,&format!("{path}/exec"),json!({"expected_generation":"1","argv":["/bin/sh","-c",setup]})).await;
    fixture_site(&computer).await;
    let page=call(&app,&path,json!({"op":"browser","tool":"browser_navigate","args":{"url":"http://127.0.0.1:8080/index.html"}})).await;
    assert_ne!(page["result"]["isError"],true,"{page}");
    let snap=call(&app,&path,json!({"op":"browser","tool":"browser_snapshot","args":{}})).await;
    let text=snap["result"]["content"].as_array().unwrap().iter().filter_map(|v|v["text"].as_str()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("Download artifact"),"{text}");
    let line=text.lines().find(|v|v.contains("link") && v.contains("Download artifact")).unwrap();
    let reference=line.split("ref=").nth(1).unwrap().split(']').next().unwrap();
    let downloaded=call(&app,&path,json!({"op":"browser","tool":"browser_click","expected_page":snap["page"],"args":{"element":"Download artifact","ref":reference}})).await;
    assert_ne!(downloaded["result"]["isError"],true,"{downloaded}");
    let files=downloaded["artifacts"].as_array().unwrap();assert!(!files.is_empty(),"{downloaded}");
    let artifact=files.iter().find(|f|f["path"].as_str().unwrap_or("").ends_with("artifact.txt")).unwrap();
    let bytes=call(&app,&path,json!({"op":"artifact","path":artifact["path"]})).await;
    use base64::Engine;
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(bytes["data"].as_str().unwrap()).unwrap(),b"verified browser artifact");
    let stale=post(&app,&format!("{path}/call"),json!({"expected_generation":"1","request":{"op":"browser","tool":"browser_click","expected_page":snap["page"],"args":{"ref":reference}}})).await;
    assert_eq!(stale["exit_code"],1,"{stale}");

    let input=text.lines().find(|v|v.contains("textbox") && v.contains("Name")).unwrap().split("ref=").nth(1).unwrap().split(']').next().unwrap();
    let mut current=downloaded["page"].clone();let mut limited=false;
    for _ in 0..6 {
        let response=post(&app,&format!("{path}/call"),json!({"expected_generation":"1","request":{"op":"browser","tool":"browser_click","expected_page":current,"args":{"target":input}}})).await;
        let result:Value=serde_json::from_str(response["stdout"].as_str().unwrap()).unwrap();
        if result["code"]=="action_repeat_limit" {limited=true;break;}
        current=result["page"].clone();
    }
    assert!(limited,"unchanged browser actions must stop after bounded repetition");
    let screen=call(&app,&path,json!({"op":"screen"})).await;assert!(screen["data"].as_str().unwrap().len()>100);
    let lease=post(&app,&format!("{path}/control"),json!({"expected_generation":"1","action":"request"})).await;
    assert_eq!(lease["state"],"user_controlled");
    let user=post(&app,&format!("{path}/call"),json!({"expected_generation":"1","credential":lease["credential"],"request":{"op":"rfb_open"}})).await;
    assert_eq!(user["exit_code"],0,"{user}");
    post(&app,&format!("{path}/control"),json!({"expected_generation":"1","action":"release","credential":lease["credential"]})).await;
    post(&app,&format!("{path}/control"),json!({"expected_generation":"1","action":"resume"})).await;
    let result=post(&app,&format!("{path}/exec"),json!({"expected_generation":"1","argv":["/bin/sh","-c","test ! -e /var/run/docker.sock && test ! -e /data/qunica.sqlite3 && opencode --version"]})).await;
    assert_eq!(result["exit_code"],0,"{result}");

    let identity="from pathlib import Path;Path('fixture/login.html').write_text('<script>document.cookie=\"identity=fixture;max-age=3600\"</script>Logged in');Path('fixture/identity.html').write_text('<body><script>document.write(document.cookie)</script></body>')";
    post(&app,&format!("{path}/exec"),json!({"expected_generation":"1","argv":["python3","-c",identity]})).await;
    call(&app,&path,json!({"op":"browser","tool":"browser_navigate","args":{"url":"http://127.0.0.1:8080/login.html"}})).await;
    let repo=std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf();
    let local=tempfile::Builder::new().prefix(".computer-home-test-").tempdir_in(&repo).unwrap();
    let relative=local.path().strip_prefix(&repo).unwrap();
    let token=relative.join("token");let backup=relative.join("backup.zip");std::fs::write(repo.join(&token),TOKEN).unwrap();
    for action in ["backup","restore"] {
        let output=tokio::process::Command::new("python").current_dir(&repo).arg("scripts/computer-home.py").arg(action).arg(spec.id.to_string()).arg(&backup).arg("--confirm").arg(spec.id.to_string()).arg("--supervisor").arg(format!("http://{address}")).arg("--token-file").arg(&token).output().await.unwrap();
        assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    }
    post(&app,&format!("{path}/actions"),json!({"expected_generation":"2","action":"start"})).await;
    fixture_site(&computer).await;
    // Wait only for the local broker after this explicit lifecycle operation.
    post(&app,&format!("{path}/exec"),json!({"expected_generation":"3","argv":["python3","-c","import socket,time\nfor _ in range(100):\n s=socket.socket();s.settimeout(.1)\n if s.connect_ex(('127.0.0.1',8765))==0:break\n time.sleep(.1)"]})).await;
    let restored=call_at(&app,&path,"3",json!({"op":"browser","tool":"browser_navigate","args":{"url":"http://127.0.0.1:8080/identity.html"}})).await;
    let snapshot=call_at(&app,&path,"3",json!({"op":"browser","tool":"browser_snapshot","args":{}})).await;
    let mut text=snapshot.to_string();
    if !text.contains("identity=fixture") {
        let page=snapshot["artifacts"].as_array().unwrap().iter().rfind(|f|f["path"].as_str().unwrap_or("").contains("page-")).unwrap();
        let file=call_at(&app,&path,"3",json!({"op":"artifact","path":page["path"]})).await;
        text=String::from_utf8(base64::engine::general_purpose::STANDARD.decode(file["data"].as_str().unwrap()).unwrap()).unwrap();
    }
    assert!(text.contains("identity=fixture"),"{text}: {restored}");
    let bytes=call_at(&app,&path,"3",json!({"op":"artifact","path":artifact["path"]})).await;
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(bytes["data"].as_str().unwrap()).unwrap(),b"verified browser artifact");
    }).catch_unwind().await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&path)
                .header("authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let current: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    post(&app,&format!("{path}/actions"),json!({"expected_generation":current["generation"],"action":"destroy","confirm":spec.id,"purge_home":true})).await;
    server.abort();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
