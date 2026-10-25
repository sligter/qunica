//! Shared bounded transfer for publication and authenticated artifact downloads.
use std::future::Future;

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::api::error::ApiError;

pub(crate) const MAX_ARTIFACT_BYTES: u64 = 1000 * 1024 * 1024;
const CHUNK_BYTES: usize = 49152;

pub(crate) struct ArtifactMetadata {
    pub size: u64,
    pub sha256: String,
}

#[derive(Default)]
struct Transfer {
    identity: Option<(u64, String)>,
    offset: u64,
    digest: Sha256,
}

impl Transfer {
    fn accept(&mut self, chunk: &Value) -> Result<Vec<u8>, ApiError> {
        let size = chunk["size"]
            .as_u64()
            .filter(|size| *size <= MAX_ARTIFACT_BYTES)
            .ok_or_else(|| ApiError::invalid_input("computer file exceeds the 1000 MiB limit"))?;
        let digest = chunk["sha256"]
            .as_str()
            .filter(|value| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| {
                ApiError::conflict("computer file checksum unavailable; update the execution image")
            })?
            .to_ascii_lowercase();
        let identity = (size, digest);
        if self
            .identity
            .as_ref()
            .is_some_and(|expected| expected != &identity)
        {
            return Err(ApiError::conflict("computer file changed during transfer"));
        }
        let encoded = chunk["data"].as_str().unwrap_or("");
        if encoded.len() > CHUNK_BYTES * 4 / 3 {
            return Err(ApiError::internal("invalid computer file chunk"));
        }
        let data = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| ApiError::internal("invalid computer file chunk"))?;
        if self.offset > size || data.len() as u64 != (size - self.offset).min(CHUNK_BYTES as u64) {
            return Err(ApiError::conflict("computer file changed during transfer"));
        }
        self.digest.update(&data);
        self.offset += data.len() as u64;
        self.identity = Some(identity);
        Ok(data)
    }

    fn complete(&self) -> bool {
        self.identity
            .as_ref()
            .is_some_and(|(size, _)| self.offset == *size)
    }

    fn finish(self) -> Result<ArtifactMetadata, ApiError> {
        let (size, expected) = self
            .identity
            .ok_or_else(|| ApiError::internal("missing computer file metadata"))?;
        let sha256 = format!("{:x}", self.digest.finalize());
        if self.offset != size || sha256 != expected {
            return Err(ApiError::conflict("computer file changed during transfer"));
        }
        Ok(ArtifactMetadata { size, sha256 })
    }
}

/// Note: stage to disk and hash incrementally, so a 1000 MiB deliverable does
/// not require a 1000 MiB allocation or expose unverified bytes to a download.
pub(crate) async fn copy_artifact<F, Fut>(
    file: &mut tokio::fs::File,
    expected_size: Option<u64>,
    mut fetch: F,
) -> Result<ArtifactMetadata, ApiError>
where
    F: FnMut(u64) -> Fut,
    Fut: Future<Output = Result<Value, ApiError>>,
{
    let mut transfer = Transfer::default();
    loop {
        let chunk = fetch(transfer.offset).await?;
        let data = transfer.accept(&chunk)?;
        if expected_size.is_some_and(|expected| Some(expected) != chunk["size"].as_u64()) {
            return Err(ApiError::conflict("Artifact changed; refresh its metadata"));
        }
        file.write_all(&data)
            .await
            .map_err(|_| ApiError::internal("failed to stage computer file"))?;
        if transfer.complete() {
            break;
        }
    }
    let metadata = transfer.finish()?;
    file.flush()
        .await
        .map_err(|_| ApiError::internal("failed to flush computer file"))?;
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_exactly_1000_mib_and_rejects_one_byte_over_without_large_allocations() {
        let data = vec![0; CHUNK_BYTES];
        let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
        let mut transfer = Transfer::default();
        assert!(transfer
            .accept(&json!({"size":MAX_ARTIFACT_BYTES,"sha256":"0".repeat(64),"data":encoded}))
            .is_ok());
        assert_eq!(transfer.offset, CHUNK_BYTES as u64);
        assert!(Transfer::default()
            .accept(&json!({"size":MAX_ARTIFACT_BYTES+1,"sha256":"0".repeat(64),"data":""}))
            .is_err());
    }

    #[tokio::test]
    #[ignore = "writes a 1000 MiB temporary file; opt-in large-file acceptance"]
    async fn stages_exactly_1000_mib_with_bounded_buffers() {
        let full = vec![0; CHUNK_BYTES];
        let tail = vec![0; MAX_ARTIFACT_BYTES as usize % CHUNK_BYTES];
        let mut digest = Sha256::new();
        for _ in 0..MAX_ARTIFACT_BYTES / CHUNK_BYTES as u64 {
            digest.update(&full);
        }
        digest.update(&tail);
        let expected = format!("{:x}", digest.finalize());
        let encoded_full = base64::engine::general_purpose::STANDARD.encode(full);
        let encoded_tail = base64::engine::general_purpose::STANDARD.encode(tail);
        let mut file = tokio::fs::File::from_std(tempfile::tempfile().unwrap());
        let metadata = copy_artifact(&mut file, Some(MAX_ARTIFACT_BYTES), |offset| {
            let data = if MAX_ARTIFACT_BYTES - offset >= CHUNK_BYTES as u64 {
                &encoded_full
            } else {
                &encoded_tail
            };
            std::future::ready(Ok(
                json!({"size":MAX_ARTIFACT_BYTES,"sha256":expected,"data":data}),
            ))
        })
        .await
        .unwrap();
        assert_eq!(metadata.size, MAX_ARTIFACT_BYTES);
        assert_eq!(metadata.sha256, expected);
        assert_eq!(file.metadata().await.unwrap().len(), MAX_ARTIFACT_BYTES);
    }

    #[test]
    fn rejects_changed_metadata_short_chunks_and_wrong_checksums() {
        let encoded = base64::engine::general_purpose::STANDARD.encode(vec![0; CHUNK_BYTES]);
        let mut transfer = Transfer::default();
        transfer
            .accept(&json!({"size":CHUNK_BYTES+1,"sha256":"0".repeat(64),"data":encoded}))
            .unwrap();
        assert!(transfer
            .accept(&json!({"size":CHUNK_BYTES+1,"sha256":"1".repeat(64),"data":"AA=="}))
            .is_err());
        assert!(transfer
            .accept(&json!({"size":CHUNK_BYTES+1,"sha256":"0".repeat(64),"data":""}))
            .is_err());
        transfer
            .accept(&json!({"size":CHUNK_BYTES+1,"sha256":"0".repeat(64),"data":"AA=="}))
            .unwrap();
        assert!(transfer.finish().is_err());
    }
}
