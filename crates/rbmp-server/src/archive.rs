use std::sync::Arc;
use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use rbmp_core::bmp::types::BmpMessage;

/// Append-only JSONL archive of every BmpMessage received.
/// Disabled (no-op) when `path` is None.
pub struct BmpArchive {
    file: Option<Arc<Mutex<tokio::fs::File>>>,
}

impl BmpArchive {
    pub async fn open(path: Option<&str>) -> Result<Self> {
        match path {
            None => Ok(Self { file: None }),
            Some(p) => {
                if let Some(parent) = std::path::Path::new(p).parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let f = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                    .await?;
                Ok(Self { file: Some(Arc::new(Mutex::new(f))) })
            }
        }
    }

    /// Serialize msg as JSON and append a newline-terminated record.
    pub async fn append(&self, msg: &BmpMessage) -> Result<()> {
        if let Some(file) = &self.file {
            let json = serde_json::to_string(msg)?;
            let mut guard = file.lock().await;
            guard.write_all(json.as_bytes()).await?;
            guard.write_all(b"\n").await?;
            guard.flush().await?;
        }
        Ok(())
    }
}
