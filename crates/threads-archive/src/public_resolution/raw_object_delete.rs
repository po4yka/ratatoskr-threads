use super::{PublicResolutionError, RawObjectStore};
use sha2::{Digest as _, Sha256};
use std::path::Path;
use tokio::fs;

pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(result, "{byte:02x}");
    }
    result
}

pub(crate) async fn verify_raw_object(
    path: &Path,
    expected_hash: &[u8],
) -> Result<(), PublicResolutionError> {
    let existing = fs::read(path)
        .await
        .map_err(PublicResolutionError::RawStorage)?;
    if Sha256::digest(existing).as_slice() == expected_hash {
        Ok(())
    } else {
        Err(PublicResolutionError::RawDigestMismatch)
    }
}

impl RawObjectStore {
    /// Deletes one service-owned object only when its reference and current bytes match.
    pub(crate) async fn delete_if_matches(
        &self,
        blob_ref: &str,
        content_hash: &[u8],
    ) -> Result<(), PublicResolutionError> {
        let digest = hex(content_hash);
        if blob_ref != format!("threads-archive/raw/sha256/{digest}") {
            return Err(PublicResolutionError::RawDigestMismatch);
        }
        let path = self.root.join("sha256").join(digest);
        if !fs::try_exists(&path)
            .await
            .map_err(PublicResolutionError::RawStorage)?
        {
            return Ok(());
        }
        verify_raw_object(&path, content_hash).await?;
        fs::remove_file(&path)
            .await
            .map_err(PublicResolutionError::RawStorage)?;
        if fs::try_exists(&path)
            .await
            .map_err(PublicResolutionError::RawStorage)?
        {
            return Err(PublicResolutionError::RawStorage(std::io::Error::other(
                "deleted raw object remains present",
            )));
        }
        Ok(())
    }
}
