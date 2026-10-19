//! Human control is a separate, persisted lease, never a login credential.
use crate::Error;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
struct Control {
    state: String,
    generation: String,
    hash: String,
    expires: u64,
}
pub(crate) struct Controls {
    root: PathBuf,
    values: Mutex<HashMap<Uuid, Control>>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
impl Controls {
    pub fn open(root: PathBuf) -> anyhow::Result<Self> {
        let mut values = HashMap::new();
        for entry in std::fs::read_dir(&root)? {
            let path = entry?.path();
            if let Some(id) = path
                .file_stem()
                .and_then(|v| v.to_str())
                .and_then(|v| v.strip_prefix("control-"))
                .and_then(|v| Uuid::parse_str(v).ok())
            {
                let mut value: Control = serde_json::from_slice(&std::fs::read(&path)?)?;
                // A process restart cannot preserve an unattended input lease.
                value.state = "released".into();
                value.hash.clear();
                value.expires = 0;
                crate::store::persist_json(&root, &format!("control-{id}.json"), &value)?;
                values.insert(id, value);
            }
        }
        Ok(Self {
            root,
            values: Mutex::new(values),
        })
    }
    pub async fn check(
        &self,
        id: Uuid,
        generation: &str,
        credential: Option<&str>,
        observe: bool,
    ) -> Result<(), Error> {
        let mut values = self.values.lock().await;
        let Some(value) = values.get_mut(&id) else {
            return if credential.is_none() {
                Ok(())
            } else {
                Err(Error::conflict("control_lease_expired"))
            };
        };
        if value.generation != generation {
            return if credential.is_none() {
                Ok(())
            } else {
                Err(Error::conflict("control_lease_expired"))
            };
        }
        let expired = value.state == "user_controlled" && value.expires <= now();
        if expired {
            value.state = "released".into();
            value.hash.clear();
            crate::store::persist_json(&self.root, &format!("control-{id}.json"), value)
                .map_err(|_| Error::storage())?;
        }
        if observe {
            return Ok(());
        }
        match credential {
            Some(token)
                if value.state == "user_controlled"
                    && value.hash == hash(token)
                    && value.expires > now() =>
            {
                Ok(())
            }
            None if value.state == "agent_controlled" => Ok(()),
            _ => Err(Error::conflict("computer_control_held")),
        }
    }
    pub async fn transition(
        &self,
        id: Uuid,
        generation: &str,
        action: &str,
        credential: Option<&str>,
    ) -> Result<serde_json::Value, Error> {
        let mut values = self.values.lock().await;
        let value = values.entry(id).or_insert(Control {
            state: "agent_controlled".into(),
            generation: generation.into(),
            hash: String::new(),
            expires: 0,
        });
        if value.generation != generation {
            *value = Control {
                state: "agent_controlled".into(),
                generation: generation.into(),
                hash: String::new(),
                expires: 0,
            };
        }
        let expired = value.state == "user_controlled" && value.expires <= now();
        if expired {
            value.state = "released".into();
            value.hash.clear();
        }
        let mut token = None;
        match action {
            "status" => {}
            "request" if value.state == "agent_controlled" || value.state == "released" => {
                value.state = "takeover_requested".into();
            }
            "grant" if value.state == "takeover_requested" => {
                let secret = Uuid::new_v4().to_string() + &Uuid::new_v4().to_string();
                value.hash = hash(&secret);
                value.state = "user_controlled".into();
                value.expires = now() + 60;
                token = Some(secret);
            }
            "release" | "renew"
                if value.state == "user_controlled"
                    && credential.is_some_and(|t| value.hash == hash(t)) =>
            {
                if action == "release" {
                    value.state = "released".into();
                    value.hash.clear();
                    value.expires = 0;
                } else {
                    value.expires = now() + 60;
                }
            }
            "resume" if value.state == "released" => {
                value.state = "agent_controlled".into();
            }
            _ => return Err(Error::conflict("computer_control_held")),
        }
        if action != "status" || expired {
            crate::store::persist_json(&self.root, &format!("control-{id}.json"), value)
                .map_err(|_| Error::storage())?;
        }
        Ok(
            json!({"state":value.state,"generation":generation,"expires_at":value.expires,"credential":token}),
        )
    }
}
