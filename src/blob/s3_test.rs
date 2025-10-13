use super::*;

#[tokio::test]
#[ignore] // Requires AWS credentials and S3 bucket
async fn test_s3_put_get() {
    let bucket = std::env::var("TEST_S3_BUCKET")
        .unwrap_or_else(|_| "beemflow-test".to_string());
    let region = std::env::var("TEST_S3_REGION")
        .unwrap_or_else(|_| "us-east-1".to_string());

    let store = S3BlobStore::new(bucket, region).await.unwrap();

    let key = format!("test_{}.txt", uuid::Uuid::new_v4());
    let data = b"Hello, S3!".to_vec();

    // Put object
    let url = store.put(&key, data.clone()).await.unwrap();
    assert!(url.contains(&key));

    // Check exists
    assert!(store.exists(&key).await.unwrap());

    // Get object
    let retrieved = store.get(&key).await.unwrap();
    assert_eq!(retrieved, data);

    // Delete object
    store.delete(&key).await.unwrap();

    // Check no longer exists
    assert!(!store.exists(&key).await.unwrap());
}
}
