//! P1 compile/link checks only: no provider requests or runtime selection.

use rig_agent::run::{AgentRun, AgentRunStep};
use rig_core::{http_client::DynHttpClient, providers::openai::OpenAIConfig};

#[test]
fn rig_run_can_be_host_driven_without_executing_io() -> anyhow::Result<()> {
    let mut run = AgentRun::new("P1 dependency smoke test").max_turns(1);
    assert!(matches!(
        run.next_step()?,
        AgentRunStep::CallModel { turn: 1, .. }
    ));
    Ok(())
}

#[tokio::test]
async fn rig_accepts_our_reqwest_013_transport_alongside_native_012() -> anyhow::Result<()> {
    let native_client = reqwest::Client::builder().no_proxy().build()?;
    let rig_client = reqwest_013::Client::builder()
        .no_proxy()
        .user_agent("qunica-rig-poc")
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let transport = rig_reqwest::ReqwestClient::from(rig_client);
    assert!(transport.inner().is_some());
    let provider = OpenAIConfig::new("poc-unused-key").connect(DynHttpClient::new(transport));
    let _model = provider.chat("poc-unused-model");
    drop(native_client);
    Ok(())
}
