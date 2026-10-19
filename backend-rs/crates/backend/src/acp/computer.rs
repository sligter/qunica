//! OpenCode ACP runs entirely on the selected computer. Only bounded protocol
//! messages cross the Supervisor; host CLI state and vendor keys never do.
use super::*;
use crate::{api::computer_proxy, computers::ComputerExecution};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

pub async fn run(
    pool: SqlitePool,
    request: AcpRunRequest,
    computer: ComputerExecution,
    turn: String,
) -> Result<AcpRun, AcpRunError> {
    if request.config.profile != AcpRuntimeProfile::Opencode {
        return Err(AcpRunError::Computer(
            "Only managed OpenCode supports computer ACP execution".into(),
        ));
    }
    if request
        .config
        .config_options
        .as_ref()
        .is_some_and(|options| !options.is_empty())
        || !request.config.env.is_empty()
        || (!request.config.args.is_empty() && request.config.args != ["acp"])
    {
        return Err(AcpRunError::Computer("Computer OpenCode uses managed proxy configuration; custom environment, launch arguments and config options are unsupported".into()));
    }
    let audit = AcpRunAudit::start(
        &pool,
        &AcpRunContext {
            owner_id: request.owner_id.clone(),
            group_id: request.group_id.clone(),
            agent_id: request.agent_id.clone(),
            thread_id: request.thread_id.clone(),
            cwd: computer.workspace_path(),
            argv: vec!["opencode".into(), "acp".into()],
        },
    )
    .await?;
    let run_id = audit.id().to_string();
    let (events_tx, events) = mpsc::unbounded_channel();
    let _ = events_tx.send(run_event(
        &run_id,
        &request.agent_id,
        &computer.workspace_path(),
        "running",
        None,
        None,
    ));
    let control = AcpRunControl {
        cancelled: Arc::new(AtomicBool::new(false)),
        notify: Arc::new(Notify::new()),
    };
    let task_control = control.clone();
    let id = run_id.clone();
    let handle = tokio::spawn(async move {
        let mut grants = None;
        let operation = Uuid::new_v4().to_string();
        let result:Result<(),String>=async {
            let issued=computer_proxy::issue_for_model(&pool,&computer,&request.owner_id,&request.agent_id,&turn,request.config.model.as_deref()).await.map_err(|_|"Computer model proxy provider is unavailable".to_string())?;
            let start=json!({"op":"acp_start","cwd":computer.relative_workspace(),"agent":request.agent_id,"model":issued.model_name,"model_grant":issued.model,"notes_grant":if request.notes.is_some(){Some(&issued.notes)}else{None},"allow_tools":request.config.permission_policy==PermissionPolicy::AutoAllow});
            grants=Some(issued);
            // The process can execute between polls. Keep one durable operation
            // for its entire lifetime, including cancellation and transport loss.
            sqlx::query("INSERT INTO computer_operations(id,turn_id,computer_id,status) VALUES(?,?,?,'running')").bind(&operation).bind(&turn).bind(&computer.id).execute(&pool).await.map_err(|_|"Computer operation storage unavailable")?;
            let session=computer.call(start).await.map_err(|_|"Computer ACP could not start")?;
            let session=session["id"].as_str().ok_or("Computer ACP session missing")?.to_string();
            let (host,relay)=tokio::io::duplex(256*1024);
            let (reader,writer)=tokio::io::split(host);
            let (relay_reader,mut relay_writer)=tokio::io::split(relay);
            let remote=computer.clone();
            let transport=tokio::spawn(async move {
                let mut lines=BufReader::new(relay_reader).lines();
                loop {
                    let mut outgoing=Vec::new();
                    tokio::select! { line=lines.next_line()=>match line {Ok(Some(line))=>outgoing.push(line),_=>break}, _=tokio::time::sleep(Duration::from_millis(100))=>{} }
                    let response=match remote.call(json!({"op":"acp_exchange","id":session,"lines":outgoing})).await {Ok(value)=>value,Err(_)=>break};
                    if let Some(lines)=response["lines"].as_array() { for line in lines { if let Some(line)=line.as_str() { if relay_writer.write_all(format!("{line}\n").as_bytes()).await.is_err() {return;} } } }
                    if response["closed"]==true {break;}
                }
            });
            let conn=AcpConnection::spawn(writer,reader,request.config.permission_policy,events_tx.clone(),None);
            let drive=async {
                let init=conn.request(METHOD_INITIALIZE,json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"qunica","version":env!("CARGO_PKG_VERSION")}})).await.map_err(|_|"Computer ACP initialization failed")?;
                let context=format!("{}:{}:{}",request.context_hash.as_deref().unwrap_or(""),grants.as_ref().unwrap().model_name,computer.workspace_path());
                let old:Option<String>=sqlx::query_scalar("SELECT session_id FROM computer_acp_sessions WHERE computer_id=? AND agent_id=? AND thread_id=? AND context_hash=?").bind(&computer.id).bind(&request.agent_id).bind(&request.thread_id).bind(&context).fetch_optional(&pool).await.unwrap_or(None);
                let reused=init["agentCapabilities"]["loadSession"]==true && old.is_some();
                let session=if init["agentCapabilities"]["loadSession"]==true && old.is_some() {
                    let old=old.unwrap();
                    conn.request(METHOD_SESSION_LOAD,json!({"sessionId":old,"cwd":computer.workspace_path(),"mcpServers":[]})).await.map_err(|_|"Computer ACP saved session could not be loaded")?;
                    old
                } else {
                    conn.request(METHOD_SESSION_NEW,json!({"cwd":computer.workspace_path(),"mcpServers":[]})).await.map_err(|_|"Computer ACP session failed")?["sessionId"].as_str().ok_or("Computer ACP session missing")?.to_string()
                };
                sqlx::query("INSERT INTO computer_acp_sessions(computer_id,agent_id,thread_id,context_hash,session_id) VALUES(?,?,?,?,?) ON CONFLICT(computer_id,agent_id,thread_id) DO UPDATE SET context_hash=excluded.context_hash,session_id=excluded.session_id").bind(&computer.id).bind(&request.agent_id).bind(&request.thread_id).bind(&context).bind(&session).execute(&pool).await.map_err(|_|"Computer ACP session storage unavailable")?;
                let mut settings=request.config.clone();
                settings.model=Some(format!("qunica/{}",grants.as_ref().unwrap().model_name));
                let mode=settings.mode.take();
                apply_session_settings(&conn,&session,&settings).await.map_err(|_|"Computer ACP settings were rejected")?;
                if let Some(mode)=mode {
                    let applied=apply_session_mode(&conn,&session,&mode,settings.profile).await.map_err(|_|"Computer ACP mode was rejected")?;
                    if applied!=SettingOutcome::Applied {return Err("Computer ACP does not support the requested mode".into());}
                }
                let text=if reused {request.incremental_prompt.unwrap_or(request.prompt)}else{request.prompt};
                let supports_images=init["agentCapabilities"]["promptCapabilities"]["image"].as_bool().unwrap_or(false);
                send_prompt(&conn,&session,&text,&request.prompt_images,request.prompt_has_image_attachments,supports_images).await.map_err(|error| { let mut text=error.to_string(); if let Some(g)=grants.as_ref() {text=text.replace(&g.model,"[redacted]").replace(&g.notes,"[redacted]");} format!("Computer ACP prompt failed: {}",text.chars().take(1000).collect::<String>()) })?;
                Ok::<(),String>(())
            };
            let cancelled=async {while !task_control.cancelled.load(Ordering::Acquire) {tokio::time::sleep(Duration::from_millis(100)).await;}};
            let outcome=tokio::select! {
                result=tokio::time::timeout(Duration::from_secs(request.config.timeout_seconds.min(3600)),drive)=>result.unwrap_or_else(|_|Err("Computer ACP timed out".into())),
                _=cancelled=>Err("Computer ACP cancelled".into()),
            };
            transport.abort();
            conn.shutdown(Duration::from_millis(100)).await;
            outcome
        }.await;
        if let Some(grants) = grants {
            grants.revoke(&pool).await;
        }
        let stopped = computer.stop_driver().await.is_ok();
        let _ = sqlx::query("UPDATE computer_operations SET status=? WHERE id=?")
            .bind(if stopped { "completed" } else { "uncertain" })
            .bind(&operation)
            .execute(&pool)
            .await;
        let result = if stopped {
            result
        } else {
            Err("Computer ACP stop is uncertain; inspect and confirm stopped".into())
        };
        let _ = events_tx.send(run_event(
            &id,
            &request.agent_id,
            &computer.workspace_path(),
            if result.is_ok() {
                "completed"
            } else if task_control.cancelled.load(Ordering::Acquire) && stopped {
                "cancelled"
            } else {
                "failed"
            },
            None,
            None,
        ));
        match result {
            Ok(()) => {
                let _ = audit.complete(Some(0), None, None).await;
                Ok(())
            }
            Err(error) if task_control.cancelled.load(Ordering::Acquire) && stopped => {
                let _ = audit.cancel(None, None, &error).await;
                Err(AcpRunJoinError::Cancelled(error))
            }
            Err(error) => {
                let _ = audit.fail(None, None, None, &error).await;
                Err(AcpRunJoinError::Failed(error))
            }
        }
    });
    Ok(AcpRun {
        run_id,
        events,
        control,
        handle,
    })
}
