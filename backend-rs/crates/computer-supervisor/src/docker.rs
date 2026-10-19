//! Docker adapter with a fixed containment profile. No Docker API objects,
//! host paths, environment overrides or network modes are accepted from callers.

use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use bollard::{
    container::LogOutput,
    exec::{CreateExecOptions, StartExecResults},
    models::{ContainerCreateBody, ContainerInspectResponse, VolumeCreateRequest},
    query_parameters::{
        CreateContainerOptionsBuilder, RemoveContainerOptionsBuilder, StopContainerOptionsBuilder,
    },
    Docker,
};
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;

use crate::{
    protocol::{Computer, ExecRequest, ExecResult},
    ComputerProvider, Error,
};

const OUTPUT_LIMIT: usize = 128 * 1024;

pub struct DockerProvider {
    docker: Docker,
    image: String,
    services: bool,
    attestation_key: Option<String>,
}

impl DockerProvider {
    pub fn new(docker: Docker, image: String) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !image.is_empty() && !image.contains(char::is_whitespace),
            "invalid computer image"
        );
        Ok(Self {
            docker,
            image,
            services: false,
            attestation_key: None,
        })
    }

    pub fn with_services(mut self, enabled: bool) -> Self {
        self.services = enabled;
        self
    }
    pub fn with_attestation(mut self, key: String) -> Self {
        self.attestation_key = Some(key);
        self
    }

    fn network_name(computer: &Computer) -> String {
        format!("{}-network", Self::prefix(computer))
    }
    fn egress_name(computer: &Computer) -> String {
        format!("{}-egress", Self::prefix(computer))
    }

    async fn ensure_network(&self, computer: &Computer) -> Result<(), Error> {
        let key = self
            .attestation_key
            .as_deref()
            .ok_or_else(|| Error::conflict("egress_not_configured"))?;
        let mut proof =
            Hmac::<Sha256>::new_from_slice(key.as_bytes()).map_err(|_| Error::invalid())?;
        proof.update(
            format!(
                "qunica.egress.v1\0{}\0{}",
                computer.spec.id, computer.generation
            )
            .as_bytes(),
        );
        let proof = format!("{:x}", proof.finalize().into_bytes());
        let name = Self::network_name(computer);
        match self.docker.inspect_network(&name, None).await {
            Ok(network) if Self::owns(computer, network.labels.as_ref()) => {}
            Ok(_) => return Err(Error::conflict("resource_conflict")),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {
                self.docker.create_network(serde_json::from_value(json!({"Name":name,"Driver":"bridge","Internal":true,"Labels":Self::labels(computer)})).map_err(|_|Error::invalid())?).await.map_err(|_|Error::uncertain())?;
            }
            Err(_) => return Err(Error::uncertain()),
        }
        let egress = Self::egress_name(computer);
        if self.inspect(computer, &egress).await?.is_some() {
            return Err(Error::conflict("container_exists"));
        }
        // Explicit intranet exceptions must never open Docker/control networks.
        let networks = self
            .docker
            .list_networks(None::<bollard::query_parameters::ListNetworksOptions>)
            .await
            .map_err(|_| Error::uncertain())?;
        let mut denied = vec![std::env::var("QUNICA_EGRESS_DENY_CIDRS").unwrap_or_default()];
        for network in networks {
            if let Some(config) = network.ipam.and_then(|ipam| ipam.config) {
                denied.extend(config.into_iter().filter_map(|item| item.subnet));
            }
        }
        let config: ContainerCreateBody = serde_json::from_value(json!({
            "Image":std::env::var("QUNICA_EGRESS_IMAGE").unwrap_or_else(|_|"qunica-computer-egress:local".into()),
            "User":"10001:10001","Labels":Self::labels(computer),
            "Env":[format!("QUNICA_PROXY_BACKEND={}",std::env::var("QUNICA_PROXY_BACKEND").unwrap_or_else(|_|"http://qunica:8766".into())),format!("QUNICA_EGRESS_ALLOW_CIDRS={}",std::env::var("QUNICA_EGRESS_ALLOW_CIDRS").unwrap_or_default()),format!("QUNICA_EGRESS_DENY_CIDRS={}",denied.join(",")),format!("QUNICA_COMPUTER_ID={}",computer.spec.id),format!("QUNICA_COMPUTER_GENERATION={}",computer.generation),format!("QUNICA_EGRESS_ATTESTATION={proof}")],
            "HostConfig":{"NetworkMode":name,"ReadonlyRootfs":true,"CapDrop":["ALL"],"SecurityOpt":["no-new-privileges:true"],"PidsLimit":64,"Memory":134217728,"NanoCpus":500000000,"LogConfig":{"Type":"none","Config":{}}},
            "NetworkingConfig":{"EndpointsConfig":{name:{"Aliases":["egress"]}}}
        })).map_err(|_|Error::invalid())?;
        let created = self
            .docker
            .create_container(
                Some(
                    CreateContainerOptionsBuilder::default()
                        .name(&egress)
                        .build(),
                ),
                config,
            )
            .await
            .map_err(|_| Error::uncertain())?;
        self.docker
            .connect_network(
                &std::env::var("QUNICA_EGRESS_NETWORK").unwrap_or_else(|_| "bridge".into()),
                serde_json::from_value(json!({"Container":created.id}))
                    .map_err(|_| Error::invalid())?,
            )
            .await
            .map_err(|_| Error::uncertain())?;
        self.docker
            .start_container(&created.id, None)
            .await
            .map_err(|_| Error::uncertain())?;
        Ok(())
    }

    pub async fn connect(image: String) -> anyhow::Result<Self> {
        // Pipe-only transport: no unauthenticated TCP Docker daemon fallback.
        let docker = Docker::connect_with_local_defaults()?
            .negotiate_version()
            .await?;
        Self::new(docker, image)
    }

    fn prefix(computer: &Computer) -> String {
        format!(
            "qunica-{}-{}",
            &computer.supervisor_id.simple().to_string()[..8],
            computer.spec.id
        )
    }

    pub fn container_name(computer: &Computer) -> String {
        format!("{}-computer", Self::prefix(computer))
    }
    pub fn home_name(computer: &Computer) -> String {
        format!("{}-home", Self::prefix(computer))
    }
    fn init_name(computer: &Computer) -> String {
        format!("{}-init", Self::prefix(computer))
    }

    fn labels(computer: &Computer) -> HashMap<String, String> {
        HashMap::from([
            (
                "qunica.supervisor".into(),
                computer.supervisor_id.to_string(),
            ),
            ("qunica.computer".into(), computer.spec.id.to_string()),
            ("qunica.generation".into(), computer.generation.clone()),
        ])
    }

    fn owns(computer: &Computer, labels: Option<&HashMap<String, String>>) -> bool {
        labels.is_some_and(|labels| {
            labels.get("qunica.supervisor") == Some(&computer.supervisor_id.to_string())
                && labels.get("qunica.computer") == Some(&computer.spec.id.to_string())
        })
    }

    async fn inspect(
        &self,
        computer: &Computer,
        name: &str,
    ) -> Result<Option<ContainerInspectResponse>, Error> {
        match self.docker.inspect_container(name, None).await {
            Ok(value) => {
                if !Self::owns(
                    computer,
                    value
                        .config
                        .as_ref()
                        .and_then(|config| config.labels.as_ref()),
                ) {
                    return Err(Error::conflict("resource_conflict"));
                }
                Ok(Some(value))
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(None),
            Err(_) => Err(Error::uncertain()),
        }
    }

    async fn ensure_home(&self, computer: &Computer) -> Result<bool, Error> {
        let name = Self::home_name(computer);
        match self.docker.inspect_volume(&name).await {
            Ok(volume) => {
                if !Self::owns(computer, Some(&volume.labels)) {
                    return Err(Error::conflict("resource_conflict"));
                }
                Ok(false)
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {
                let body: VolumeCreateRequest =
                    serde_json::from_value(json!({"Name":name,"Labels":Self::labels(computer)}))
                        .map_err(|_| Error::invalid())?;
                let volume = self
                    .docker
                    .create_volume(body)
                    .await
                    .map_err(|_| Error::uncertain())?;
                if !Self::owns(computer, Some(&volume.labels)) {
                    return Err(Error::conflict("resource_conflict"));
                }
                Ok(true)
            }
            Err(_) => Err(Error::uncertain()),
        }
    }

    /// The same fixed profile applies to normal and trusted volume-init jobs.
    fn config(&self, computer: &Computer, initialize: bool) -> Result<ContainerCreateBody, Error> {
        let command = if initialize {
            vec!["/bin/sh", "-c", "mkdir -p /home/agent/workspace && chown 10001:10001 /home/agent /home/agent/workspace"]
        } else if self.services {
            vec!["/bin/sh", "/opt/qunica/start.sh"]
        } else {
            vec!["/bin/sh", "-c", "exec sleep infinity"]
        };
        let mut value = json!({
            "Image":self.image,"Entrypoint":command,"Cmd":[],
            "User":if initialize {"0:0"} else {"10001:10001"},
            "Env":["HOME=/home/agent"],
            "WorkingDir":if initialize {"/"} else {"/home/agent/workspace"},
            "Labels":Self::labels(computer),"Healthcheck":{"Test":["NONE"]},
            "HostConfig":{
                "NetworkMode":"none","ReadonlyRootfs":true,"Privileged":false,"Init":true,
                "CapDrop":["ALL"],"SecurityOpt":["no-new-privileges:true"],
                "PidsLimit":128,"Memory":1073741824i64,"MemorySwap":1073741824i64,
                "NanoCpus":1000000000i64,"ShmSize":268435456i64,
                "Tmpfs":{"/tmp":"rw,nosuid,nodev,size=134217728,mode=1777"},
                "Mounts":[{"Type":"volume","Source":Self::home_name(computer),"Target":"/home/agent","ReadOnly":false}],
                "RestartPolicy":{"Name":"no"},"LogConfig":{"Type":"none","Config":{}}
            }
        });
        if initialize {
            // Note: the empty named volume is initialized from the image's
            // agent-owned 0700 home. Root without DAC_OVERRIDE cannot even
            // traverse it to create the workspace, so the short init job adds
            // only the two capabilities its mkdir/chown needs.
            value["HostConfig"]["CapAdd"] = json!(["CHOWN", "DAC_OVERRIDE"]);
        } else if self.services {
            value["HostConfig"]["NetworkMode"] = json!(Self::network_name(computer));
            value["HostConfig"]["PidsLimit"] = json!(512);
            value["HostConfig"]["Memory"] = json!(2147483648i64);
            value["HostConfig"]["MemorySwap"] = json!(2147483648i64);
            value["Env"] = json!([
                "HOME=/home/agent",
                "DISPLAY=:99",
                "HTTP_PROXY=http://egress:3128",
                "HTTPS_PROXY=http://egress:3128",
                "http_proxy=http://egress:3128",
                "https_proxy=http://egress:3128",
                "NO_PROXY=127.0.0.1,localhost,egress"
            ]);
        }
        serde_json::from_value(value).map_err(|_| Error::invalid())
    }

    async fn create_container(
        &self,
        computer: &Computer,
        initialize: bool,
    ) -> Result<String, Error> {
        let name = if initialize {
            Self::init_name(computer)
        } else {
            Self::container_name(computer)
        };
        if self.inspect(computer, &name).await?.is_some() {
            return Err(Error::conflict("container_exists"));
        }
        let created = self
            .docker
            .create_container(
                Some(CreateContainerOptionsBuilder::default().name(&name).build()),
                self.config(computer, initialize)?,
            )
            .await
            .map_err(|_| Error::uncertain())?;
        self.docker
            // Note: a delayed request must never resolve a reused name to a
            // newer generation. Mutations always address immutable Docker ids.
            .start_container(&created.id, None)
            .await
            .map_err(|_| Error::uncertain())?;
        Ok(name)
    }

    async fn remove_stopped(&self, computer: &Computer, name: &str) -> Result<(), Error> {
        if let Some(existing) = self.inspect(computer, name).await? {
            if existing
                .state
                .as_ref()
                .and_then(|state| state.running)
                .unwrap_or(true)
            {
                return Err(Error::uncertain());
            }
            let id = existing.id.ok_or_else(Error::uncertain)?;
            self.docker
                .remove_container(
                    &id,
                    Some(
                        RemoveContainerOptionsBuilder::default()
                            .force(false)
                            .v(false)
                            .build(),
                    ),
                )
                .await
                .map_err(|_| Error::uncertain())?;
        }
        Ok(())
    }

    async fn stop_all(&self, computer: &Computer) -> Result<(), Error> {
        for name in [
            Self::container_name(computer),
            Self::init_name(computer),
            Self::egress_name(computer),
        ] {
            if let Some(existing) = self.inspect(computer, &name).await? {
                let id = existing.id.as_deref().ok_or_else(Error::uncertain)?;
                if existing
                    .state
                    .as_ref()
                    .and_then(|state| state.running)
                    .unwrap_or(true)
                {
                    self.docker
                        .stop_container(
                            id,
                            Some(StopContainerOptionsBuilder::default().t(10).build()),
                        )
                        .await
                        .map_err(|_| Error::uncertain())?;
                }
                self.remove_stopped(computer, id).await?;
            }
        }
        if self.services {
            match self
                .docker
                .inspect_network(&Self::network_name(computer), None)
                .await
            {
                Ok(network) if Self::owns(computer, network.labels.as_ref()) => {
                    self.docker
                        .remove_network(&Self::network_name(computer))
                        .await
                        .map_err(|_| Error::uncertain())?;
                }
                Ok(_) => return Err(Error::conflict("resource_conflict")),
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404, ..
                }) => {}
                Err(_) => return Err(Error::uncertain()),
            }
        }
        Ok(())
    }

    async fn run(&self, computer: &Computer, request: ExecRequest) -> Result<ExecResult, Error> {
        let name = Self::container_name(computer);
        let container = self
            .inspect(computer, &name)
            .await?
            .ok_or_else(Error::missing)?;
        if container.state.as_ref().and_then(|state| state.running) != Some(true)
            || container
                .config
                .as_ref()
                .and_then(|config| config.labels.as_ref())
                .and_then(|labels| labels.get("qunica.generation"))
                != Some(&computer.generation)
        {
            return Err(Error::uncertain());
        }
        let execution = self
            .docker
            .create_exec(
                container.id.as_deref().ok_or_else(Error::uncertain)?,
                CreateExecOptions {
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    tty: Some(false),
                    user: Some("10001:10001".to_string()),
                    working_dir: Some(format!("/home/agent/workspace/{}", request.cwd)),
                    cmd: Some(
                        vec![
                            "python3".into(),
                            "-c".into(),
                            include_str!("exec_guard.py").into(),
                        ]
                        .into_iter()
                        .chain(request.argv)
                        .collect(),
                    ),
                    ..Default::default()
                },
            )
            .await
            .map_err(|_| Error::uncertain())?;
        let StartExecResults::Attached { mut output, .. } = self
            .docker
            .start_exec(&execution.id, None)
            .await
            .map_err(|_| Error::uncertain())?
        else {
            return Err(Error::uncertain());
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut truncated = false;
        while let Some(chunk) = output.next().await {
            let chunk = chunk.map_err(|_| Error::uncertain())?;
            let (target, bytes) = match chunk {
                LogOutput::StdErr { message } => (&mut stderr, message),
                LogOutput::StdOut { message } | LogOutput::Console { message } => {
                    (&mut stdout, message)
                }
                _ => continue,
            };
            let count = bytes.len().min(OUTPUT_LIMIT.saturating_sub(target.len()));
            truncated |= count < bytes.len();
            target.extend_from_slice(&bytes[..count]);
        }
        // EOF alone does not prove the exec ended: inspect its daemon-owned id.
        let state = self
            .docker
            .inspect_exec(&execution.id)
            .await
            .map_err(|_| Error::uncertain())?;
        if state.running != Some(false) {
            return Err(Error::uncertain());
        }
        let exit_code = state.exit_code.ok_or_else(Error::uncertain)?;
        if exit_code >= 128 {
            return Err(Error::uncertain());
        }
        Ok(ExecResult {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            truncated,
        })
    }
}

#[async_trait]
impl ComputerProvider for DockerProvider {
    fn capabilities(&self) -> serde_json::Value {
        json!({"network":self.services,"browser":self.services,"desktop":self.services,"acp_opencode":self.services})
    }
    async fn create(&self, computer: &Computer) -> Result<(), Error> {
        let new_home = self.ensure_home(computer).await?;
        if new_home {
            let name = self.create_container(computer, true).await?;
            loop {
                let state = self
                    .inspect(computer, &name)
                    .await?
                    .and_then(|value| value.state)
                    .ok_or_else(Error::uncertain)?;
                if state.running == Some(false) {
                    if state.exit_code != Some(0) {
                        return Err(Error::uncertain());
                    }
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            self.remove_stopped(computer, &name).await?;
        }
        if self.services {
            self.ensure_network(computer).await?;
        }
        self.create_container(computer, false).await?;
        Ok(())
    }
    async fn start(&self, computer: &Computer) -> Result<(), Error> {
        // A missing persisted home is not replaced with an empty environment.
        let volume = self
            .docker
            .inspect_volume(&Self::home_name(computer))
            .await
            .map_err(|_| Error::conflict("home_missing"))?;
        if !Self::owns(computer, Some(&volume.labels)) {
            return Err(Error::conflict("resource_conflict"));
        }
        if self.services {
            self.ensure_network(computer).await?;
        }
        self.create_container(computer, false).await?;
        Ok(())
    }
    async fn stop(&self, computer: &Computer) -> Result<(), Error> {
        self.stop_all(computer).await
    }
    async fn rebuild(&self, computer: &Computer) -> Result<(), Error> {
        self.stop_all(computer).await?;
        self.start(computer).await
    }
    async fn destroy(&self, computer: &Computer, purge_home: bool) -> Result<(), Error> {
        self.stop_all(computer).await?;
        if purge_home {
            let name = Self::home_name(computer);
            match self.docker.inspect_volume(&name).await {
                Ok(volume) => {
                    if !Self::owns(computer, Some(&volume.labels)) {
                        return Err(Error::conflict("resource_conflict"));
                    }
                    self.docker
                        .remove_volume(
                            &name,
                            None::<bollard::query_parameters::RemoveVolumeOptions>,
                        )
                        .await
                        .map_err(|_| Error::uncertain())?;
                }
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404, ..
                }) => {}
                Err(_) => return Err(Error::uncertain()),
            }
        }
        Ok(())
    }
    async fn execute(
        &self,
        computer: &Computer,
        request: ExecRequest,
    ) -> Result<ExecResult, Error> {
        self.run(computer, request).await
    }
}
