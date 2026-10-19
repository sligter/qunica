use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::sync::{Mutex, OwnedMutexGuard};
use uuid::Uuid;

use crate::{
    protocol::{Computer, ComputerSpec, Status},
    Error,
};

pub(crate) struct Store {
    _process_lock: std::fs::File,
    root: PathBuf,
    computers: Mutex<HashMap<Uuid, Arc<Mutex<Computer>>>>,
    pub supervisor_id: Uuid,
}

pub(crate) fn persist_json(
    root: &Path,
    name: &str,
    value: &impl serde::Serialize,
) -> anyhow::Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    serde_json::to_writer(file.as_file_mut(), value)?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.persist(root.join(name))?;
    // Linux is the deployed Supervisor platform: make the journal rename
    // durable before Docker receives an operation or a new generation is acked.
    #[cfg(unix)]
    std::fs::File::open(root)?.sync_all()?;
    Ok(())
}

impl Store {
    pub fn read_saved(&self, id: Uuid) -> Result<Computer, Error> {
        let path = self.root.join(format!("{id}.json"));
        let bytes = std::fs::read(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::missing()
            } else {
                Error::storage()
            }
        })?;
        serde_json::from_slice(&bytes).map_err(|_| Error::storage())
    }
    pub fn open(root: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&root)?;
        let process_lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("supervisor.lock"))?;
        fs2::FileExt::try_lock_exclusive(&process_lock)?;
        let identity = root.join("supervisor.json");
        let supervisor_id = if identity.exists() {
            serde_json::from_slice(&std::fs::read(identity)?)?
        } else {
            let id = Uuid::new_v4();
            persist_json(&root, "supervisor.json", &id)?;
            id
        };
        let mut computers = HashMap::new();
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json")
                || path == root.join("supervisor.json")
            {
                continue;
            }
            let Some(id) = path
                .file_stem()
                .and_then(|value| value.to_str())
                .and_then(|value| Uuid::parse_str(value).ok())
            else {
                continue;
            };
            let mut computer: Computer = serde_json::from_slice(&std::fs::read(&path)?)?;
            anyhow::ensure!(computer.spec.id == id, "invalid computer state identity");
            anyhow::ensure!(
                computer.supervisor_id == supervisor_id,
                "invalid supervisor identity"
            );
            anyhow::ensure!(
                computer.generation.parse::<u64>().is_ok(),
                "invalid computer generation"
            );
            // Note: an exec can outlive a crashed supervisor. Only a verified
            // explicit stop permits another driver after reopening the journal.
            if !matches!(computer.status, Status::Stopped | Status::Destroyed) {
                computer.status = Status::Uncertain;
                computer.error_code = Some("supervisor_restarted".into());
                persist_json(&root, &format!("{id}.json"), &computer)?;
            }
            computers.insert(id, Arc::new(Mutex::new(computer)));
        }
        Ok(Self {
            root,
            computers: Mutex::new(computers),
            supervisor_id,
            _process_lock: process_lock,
        })
    }

    pub async fn insert(
        &self,
        spec: ComputerSpec,
    ) -> Result<(OwnedMutexGuard<Computer>, bool), Error> {
        let mut computers = self.computers.lock().await;
        if let Some(existing) = computers.get(&spec.id) {
            let guard = existing
                .clone()
                .try_lock_owned()
                .map_err(|_| Error::busy())?;
            if guard.spec != spec {
                return Err(Error::conflict("idempotency_conflict"));
            }
            return Ok((guard, false));
        }
        if computers.len() >= 128 {
            return Err(Error::conflict("computer_capacity"));
        }
        let computer = Computer {
            supervisor_id: self.supervisor_id,
            spec,
            generation: "1".into(),
            status: Status::Starting,
            home_present: true,
            error_code: None,
        };
        self.save(&computer)?;
        let id = computer.spec.id;
        let entry = Arc::new(Mutex::new(computer));
        let guard = entry.clone().lock_owned().await;
        computers.insert(id, entry);
        Ok((guard, true))
    }

    pub async fn lock(&self, id: Uuid) -> Result<OwnedMutexGuard<Computer>, Error> {
        let entry = self
            .computers
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(Error::missing)?;
        entry.try_lock_owned().map_err(|_| Error::busy())
    }

    pub async fn wait_lock(&self, id: Uuid) -> Result<OwnedMutexGuard<Computer>, Error> {
        let entry = self
            .computers
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(Error::missing)?;
        tokio::time::timeout(std::time::Duration::from_secs(310), entry.lock_owned())
            .await
            .map_err(|_| Error::busy())
    }

    pub fn save(&self, computer: &Computer) -> Result<(), Error> {
        persist_json(&self.root, &format!("{}.json", computer.spec.id), computer)
            .map_err(|_| Error::storage())
    }
}
