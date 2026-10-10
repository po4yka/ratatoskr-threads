//! Content-addressed, immutable raw-evidence directory for public observations.

use super::{PublicResolutionError, raw_object_delete};
use crate::PersistenceError;
use sha2::{Digest as _, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWriteExt as _};
use uuid::Uuid;

/// A bounded service-owned SHA-256 raw-evidence directory with immutable reused bodies.
#[derive(Debug, Clone)]
pub struct RawObjectStore {
    pub(super) root: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct StoredRaw {
    pub(crate) blob_ref: String,
    pub(crate) content_hash: Vec<u8>,
    pub(crate) byte_size: i64,
    pub(crate) media_type: &'static str,
}

impl RawObjectStore {
    /// Creates a service-owned raw evidence store rooted at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Stores one raw response before normalization, rejecting write or digest conflicts.
    pub(crate) async fn store(&self, bytes: &[u8]) -> Result<StoredRaw, PublicResolutionError> {
        let content_hash = Sha256::digest(bytes).to_vec();
        let digest = raw_object_delete::hex(&content_hash);
        let path = self.root.join("sha256").join(&digest);
        let byte_size = i64::try_from(bytes.len()).map_err(|_| {
            PublicResolutionError::Persistence(PersistenceError::Query(sqlx::Error::Configuration(
                "raw response size exceeds bigint".into(),
            )))
        })?;
        if fs::try_exists(&path)
            .await
            .map_err(PublicResolutionError::RawStorage)?
        {
            raw_object_delete::verify_raw_object(&path, &content_hash).await?;
        } else {
            self.create_once(&path, bytes, &content_hash).await?;
        }
        Ok(StoredRaw {
            blob_ref: format!("threads-archive/raw/sha256/{digest}"),
            content_hash,
            byte_size,
            media_type: "application/json",
        })
    }

    /// Streams raw bytes before parsing, rejecting read, budget, or immutable-storage failures.
    pub(crate) async fn store_stream<R>(
        &self,
        reader: &mut R,
        max_bytes: u64,
        media_type: &'static str,
    ) -> Result<StoredRaw, PublicResolutionError>
    where
        R: AsyncRead + Unpin,
    {
        let temporary = self.temporary_path();
        let mut output = self.create_temporary(&temporary).await?;
        let (content_hash, byte_size) =
            match Box::pin(copy_and_hash(reader, &mut output, max_bytes)).await {
                Ok(copied) => copied,
                Err(error) => {
                    drop(output);
                    fs::remove_file(&temporary)
                        .await
                        .map_err(PublicResolutionError::RawStorage)?;
                    return Err(error);
                }
            };
        output
            .sync_all()
            .await
            .map_err(PublicResolutionError::RawStorage)?;
        drop(output);
        let digest = raw_object_delete::hex(&content_hash);
        let path = self.root.join("sha256").join(&digest);
        self.promote_temporary(&temporary, &path, &content_hash)
            .await?;
        Ok(StoredRaw {
            blob_ref: format!("threads-archive/raw/sha256/{digest}"),
            content_hash,
            byte_size,
            media_type,
        })
    }

    /// Reads one content-addressed object only when its reference and bytes
    /// still agree with the durable digest evidence.
    pub(crate) async fn read_verified(
        &self,
        blob_ref: &str,
        content_hash: &[u8],
    ) -> Result<Vec<u8>, PublicResolutionError> {
        let digest = raw_object_delete::hex(content_hash);
        if blob_ref != format!("threads-archive/raw/sha256/{digest}") {
            return Err(PublicResolutionError::RawDigestMismatch);
        }
        let path = self.root.join("sha256").join(digest);
        let bytes = fs::read(&path)
            .await
            .map_err(PublicResolutionError::RawStorage)?;
        if Sha256::digest(&bytes).as_slice() == content_hash {
            Ok(bytes)
        } else {
            Err(PublicResolutionError::RawDigestMismatch)
        }
    }

    fn temporary_path(&self) -> PathBuf {
        self.root.join("tmp").join(Uuid::now_v7().to_string())
    }

    async fn create_temporary(
        &self,
        path: &Path,
    ) -> Result<tokio::fs::File, PublicResolutionError> {
        let parent = path
            .parent()
            .ok_or(PublicResolutionError::RawDigestMismatch)?;
        fs::create_dir_all(parent)
            .await
            .map_err(PublicResolutionError::RawStorage)?;
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .await
            .map_err(PublicResolutionError::RawStorage)
    }

    async fn promote_temporary(
        &self,
        temporary: &Path,
        path: &Path,
        content_hash: &[u8],
    ) -> Result<(), PublicResolutionError> {
        let parent = path
            .parent()
            .ok_or(PublicResolutionError::RawDigestMismatch)?;
        fs::create_dir_all(parent)
            .await
            .map_err(PublicResolutionError::RawStorage)?;
        match fs::hard_link(temporary, path).await {
            Ok(()) => fs::remove_file(temporary)
                .await
                .map_err(PublicResolutionError::RawStorage),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                fs::remove_file(temporary)
                    .await
                    .map_err(PublicResolutionError::RawStorage)?;
                raw_object_delete::verify_raw_object(path, content_hash).await
            }
            Err(error) => Err(PublicResolutionError::RawStorage(error)),
        }
    }

    async fn create_once(
        &self,
        path: &Path,
        bytes: &[u8],
        content_hash: &[u8],
    ) -> Result<(), PublicResolutionError> {
        let parent = path
            .parent()
            .ok_or(PublicResolutionError::RawDigestMismatch)?;
        fs::create_dir_all(parent)
            .await
            .map_err(PublicResolutionError::RawStorage)?;
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .await
        {
            Ok(mut file) => {
                file.write_all(bytes)
                    .await
                    .map_err(PublicResolutionError::RawStorage)?;
                file.sync_all()
                    .await
                    .map_err(PublicResolutionError::RawStorage)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                raw_object_delete::verify_raw_object(path, content_hash).await
            }
            Err(error) => Err(PublicResolutionError::RawStorage(error)),
        }
    }
}

async fn copy_and_hash<R>(
    reader: &mut R,
    output: &mut tokio::fs::File,
    max_bytes: u64,
) -> Result<(Vec<u8>, i64), PublicResolutionError>
where
    R: AsyncRead + Unpin,
{
    let mut hasher = Sha256::new();
    let mut byte_size = 0_u64;
    let mut buffer = [0_u8; 16_384];
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .map_err(PublicResolutionError::RawStorage)?;
        if read == 0 {
            break;
        }
        byte_size = byte_size.checked_add(read as u64).ok_or_else(|| {
            PublicResolutionError::Persistence(PersistenceError::Query(sqlx::Error::Configuration(
                "raw response size exceeds bigint".into(),
            )))
        })?;
        if byte_size > max_bytes || byte_size > i64::MAX as u64 {
            return Err(PublicResolutionError::Persistence(PersistenceError::Query(
                sqlx::Error::Configuration("raw response exceeds configured limit".into()),
            )));
        }
        hasher.update(buffer.get(..read).ok_or_else(|| {
            PublicResolutionError::Persistence(PersistenceError::Query(sqlx::Error::Configuration(
                "stream read exceeds buffer".into(),
            )))
        })?);
        output
            .write_all(buffer.get(..read).ok_or_else(|| {
                PublicResolutionError::Persistence(PersistenceError::Query(
                    sqlx::Error::Configuration("stream read exceeds buffer".into()),
                ))
            })?)
            .await
            .map_err(PublicResolutionError::RawStorage)?;
    }
    let byte_size = i64::try_from(byte_size).map_err(|_| {
        PublicResolutionError::Persistence(PersistenceError::Query(sqlx::Error::Configuration(
            "raw response size exceeds bigint".into(),
        )))
    })?;
    Ok((hasher.finalize().to_vec(), byte_size))
}
