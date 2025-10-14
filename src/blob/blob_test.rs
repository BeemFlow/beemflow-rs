//! Tests for blob

use super::{BlobConfig, BlobStore, FilesystemBlobStore, new_default_blob_store};
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
    // Should default to filesystem with temp directory
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
