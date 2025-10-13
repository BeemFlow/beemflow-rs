//! Blob storage for large objects
//!
//! Provides filesystem and S3 blob storage backends.

pub mod s3;

use crate::{Result, constants};
use async_trait::async_trait;

pub use s3::S3BlobStore;

/// Blob storage trait
#[async_trait]
pub trait BlobStore: Send + Sync {
    /// Store a blob with optional MIME type and filename
    /// Returns a URL to the stored blob
    async fn put(
        &self,
        data: Vec<u8>,
        mime: Option<&str>,
        filename: Option<&str>,
    ) -> Result<String>;

    /// Retrieve a blob from a URL
    async fn get(&self, url: &str) -> Result<Vec<u8>>;
}

/// Filesystem blob storage
///
/// Stores blobs as files in a directory, matching Go's FilesystemBlobStore behavior:
/// - Atomic writes (write to .tmp, then rename)
/// - Returns file:// URLs
/// - Auto-generates filenames when not provided
pub struct FilesystemBlobStore {
    dir: String,
}

impl FilesystemBlobStore {
    /// Create a new filesystem blob store
    ///
    /// The directory will be created if it does not exist.
    pub async fn new(dir: String) -> Result<Self> {
        tokio::fs::create_dir_all(&dir).await?;
        Ok(Self { dir })
    }

    /// Create a new filesystem blob store (sync version for compatibility)
    pub fn new_sync(dir: String) -> Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }
}

#[async_trait]
impl BlobStore for FilesystemBlobStore {
    /// Store a blob as a file in the directory
    ///
    /// Uses atomic writes (write to .tmp, then rename) for safety.
    /// Returns a file:// URL.
    async fn put(
        &self,
        data: Vec<u8>,
        _mime: Option<&str>,
        filename: Option<&str>,
    ) -> Result<String> {
        use std::path::Path;

        // Generate filename if not provided (matches Go's behavior)
        let filename = match filename {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => {
                // Safe: duration_since(UNIX_EPOCH) only fails if system time is before 1970,
                // which is impossible on any modern system
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("System time is before UNIX_EPOCH")
                    .as_nanos();
                format!("blob-{}", timestamp)
            }
        };

        let path = Path::new(&self.dir).join(&filename);

        // Atomic write: write to .tmp then rename
        let tmp_path = path.with_extension("tmp");
        tokio::fs::write(&tmp_path, data).await?;
        tokio::fs::rename(&tmp_path, &path).await?;

        // Return file:// URL
        Ok(format!("file://{}", path.to_string_lossy()))
    }

    /// Retrieve a blob from a file:// URL
    async fn get(&self, url: &str) -> Result<Vec<u8>> {
        const PREFIX: &str = "file://";
        if !url.starts_with(PREFIX) {
            return Err(crate::BeemFlowError::validation(format!(
                "invalid file URL: {}",
                url
            )));
        }

        let path = &url[PREFIX.len()..];
        let data = tokio::fs::read(path).await?;
        Ok(data)
    }
}

// ============================================================================
// Blob Configuration & Factory
// ============================================================================

/// Blob storage configuration
#[derive(Debug, Clone)]
pub struct BlobConfig {
    /// Storage driver: "filesystem" or "s3"
    pub driver: Option<String>,
    /// Directory for filesystem storage
    pub directory: Option<String>,
    /// S3 bucket name
    pub bucket: Option<String>,
    /// S3 region
    pub region: Option<String>,
}

impl Default for BlobConfig {
    fn default() -> Self {
        Self {
            driver: Some("filesystem".to_string()),
            directory: Some(constants::default_blob_dir().to_string()),
            bucket: None,
            region: None,
        }
    }
}

/// Create a default blob store based on configuration
///
/// Matches Go's NewDefaultBlobStore behavior:
/// - Returns FilesystemBlobStore if config is None, empty, or driver is "filesystem"
/// - Returns S3BlobStore if driver is "s3"
/// - Uses default directory (~/.beemflow/files) if not specified
pub async fn new_default_blob_store(config: Option<&BlobConfig>) -> Result<Box<dyn BlobStore>> {
    // Determine driver and extract config values
    let driver = config
        .and_then(|c| c.driver.as_ref())
        .map(|s| s.as_str())
        .unwrap_or("filesystem");

    match driver {
        "" | "filesystem" => {
            // Use filesystem blob store
            let dir = config
                .and_then(|c| c.directory.as_ref())
                .cloned()
                .unwrap_or_else(|| constants::default_blob_dir().to_string());

            let store = FilesystemBlobStore::new(dir).await?;
            Ok(Box::new(store))
        }
        "s3" => {
            // Use S3 blob store
            let bucket = config.and_then(|c| c.bucket.as_ref()).ok_or_else(|| {
                crate::BeemFlowError::validation("s3 driver requires bucket and region".to_string())
            })?;

            let region = config.and_then(|c| c.region.as_ref()).ok_or_else(|| {
                crate::BeemFlowError::validation("s3 driver requires bucket and region".to_string())
            })?;

            if bucket.is_empty() || region.is_empty() {
                return Err(crate::BeemFlowError::validation(
                    "s3 driver requires non-empty bucket and region".to_string(),
                ));
            }

            let store = S3BlobStore::new(bucket.clone(), region.clone()).await?;
            Ok(Box::new(store))
        }
        other => Err(crate::BeemFlowError::validation(format!(
            "unsupported blob driver: {}",
            other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_filesystem_put_and_get() {
        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemBlobStore::new(temp_dir.path().to_string_lossy().to_string())
            .await
            .unwrap();

        // Store a blob
        let data = b"Hello, World!".to_vec();
        let url = store
            .put(data.clone(), Some("text/plain"), Some("test.txt"))
            .await
            .unwrap();

        // Verify URL format
        assert!(url.starts_with("file://"));
        assert!(url.ends_with("test.txt"));

        // Retrieve the blob
        let retrieved = store.get(&url).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_filesystem_auto_generate_filename() {
        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemBlobStore::new(temp_dir.path().to_string_lossy().to_string())
            .await
            .unwrap();

        // Store a blob without filename
        let data = b"Test data".to_vec();
        let url = store.put(data.clone(), None, None).await.unwrap();

        // Should have auto-generated filename starting with "blob-"
        assert!(url.contains("blob-"));

        // Should be retrievable
        let retrieved = store.get(&url).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_filesystem_empty_filename() {
        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemBlobStore::new(temp_dir.path().to_string_lossy().to_string())
            .await
            .unwrap();

        // Store a blob with empty filename (should auto-generate)
        let data = b"Test data".to_vec();
        let url = store.put(data.clone(), None, Some("")).await.unwrap();

        // Should have auto-generated filename
        assert!(url.contains("blob-"));
        let retrieved = store.get(&url).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_filesystem_binary_data() {
        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemBlobStore::new(temp_dir.path().to_string_lossy().to_string())
            .await
            .unwrap();

        // Store binary data
        let data: Vec<u8> = (0..=255).collect();
        let url = store
            .put(
                data.clone(),
                Some("application/octet-stream"),
                Some("binary.dat"),
            )
            .await
            .unwrap();

        let retrieved = store.get(&url).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_filesystem_invalid_url() {
        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemBlobStore::new(temp_dir.path().to_string_lossy().to_string())
            .await
            .unwrap();

        // Try to get with invalid URL
        let result = store.get("http://example.com/file").await;
        assert!(result.is_err());

        let result = store.get("not-a-url").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_filesystem_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemBlobStore::new(temp_dir.path().to_string_lossy().to_string())
            .await
            .unwrap();

        // Try to get non-existent file
        let url = format!(
            "file://{}/nonexistent.txt",
            temp_dir.path().to_string_lossy()
        );
        let result = store.get(&url).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_new_default_blob_store_filesystem() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlobConfig {
            driver: Some("filesystem".to_string()),
            directory: Some(temp_dir.path().to_string_lossy().to_string()),
            bucket: None,
            region: None,
        };

        let store = new_default_blob_store(Some(&config)).await.unwrap();

        // Test that it works
        let data = b"Test data".to_vec();
        let url = store
            .put(data.clone(), None, Some("test.txt"))
            .await
            .unwrap();
        let retrieved = store.get(&url).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_new_default_blob_store_none() {
        // Should default to filesystem
        let store = new_default_blob_store(None).await.unwrap();

        // Test that it works
        let data = b"Test data".to_vec();
        let url = store
            .put(data.clone(), None, Some("test.txt"))
            .await
            .unwrap();
        assert!(url.starts_with("file://"));
    }

    #[tokio::test]
    async fn test_blob_config_default() {
        let config = BlobConfig::default();
        assert_eq!(config.driver, Some("filesystem".to_string()));
        assert!(config.directory.is_some());
        assert!(config.bucket.is_none());
        assert!(config.region.is_none());
    }

    #[tokio::test]
    async fn test_filesystem_atomic_write() {
        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemBlobStore::new(temp_dir.path().to_string_lossy().to_string())
            .await
            .unwrap();

        // Store a blob
        let data = b"Atomic write test".to_vec();
        let url = store
            .put(data.clone(), None, Some("atomic.txt"))
            .await
            .unwrap();

        // Verify no .tmp files left behind
        let entries = std::fs::read_dir(temp_dir.path()).unwrap();
        for entry in entries {
            let entry = entry.unwrap();
            let filename = entry.file_name();
            let filename_str = filename.to_string_lossy();
            assert!(
                !filename_str.ends_with(".tmp"),
                "Found leftover .tmp file: {}",
                filename_str
            );
        }

        // Verify file exists and is readable
        let retrieved = store.get(&url).await.unwrap();
        assert_eq!(retrieved, data);
    }
}
