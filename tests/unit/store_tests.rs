use anyhow::Result;
use std::sync::Arc;
use tempfile::tempdir;
use theater::store::{ContentRef, ContentStore, Label};
use tokio::sync::Mutex;

#[tokio::test]
async fn test_content_store_basic() {
    let temp_dir = tempdir().unwrap();
    let store_path = temp_dir.path().join("test-store");
    
    let store = ContentStore::new(store_path.to_str().unwrap());
    
    // Test storing content
    let test_content = b"test content data".to_vec();
    let content_ref = store.store(test_content.clone()).await.unwrap();
    
    // Verify the hash
    let expected_hash = ContentRef::from_content(&test_content).hash();
    assert_eq!(content_ref.hash(), expected_hash);
    
    // Retrieve the content and verify
    let retrieved = store.get(&content_ref).await.unwrap();
    assert_eq!(retrieved, test_content);
}

#[tokio::test]
async fn test_content_store_deduplication() {
    let temp_dir = tempdir().unwrap();
    let store_path = temp_dir.path().join("test-store");
    
    let store = ContentStore::new(store_path.to_str().unwrap());
    
    // Store the same content twice
    let test_content = b"duplicate content test".to_vec();
    
    let ref1 = store.store(test_content.clone()).await.unwrap();
    let ref2 = store.store(test_content.clone()).await.unwrap();
    
    // References should be equal
    assert_eq!(ref1.hash(), ref2.hash());
    
    // Content should be retrievable with either reference
    let retrieved1 = store.get(&ref1).await.unwrap();
    let retrieved2 = store.get(&ref2).await.unwrap();
    
    assert_eq!(retrieved1, test_content);
    assert_eq!(retrieved2, test_content);
}

#[tokio::test]
async fn test_content_store_labeling() {
    let temp_dir = tempdir().unwrap();
    let store_path = temp_dir.path().join("test-store");
    
    let store = ContentStore::new(store_path.to_str().unwrap());
    
    // Store content
    let test_content1 = b"content one".to_vec();
    let test_content2 = b"content two".to_vec();
    
    let ref1 = store.store(test_content1.clone()).await.unwrap();
    let ref2 = store.store(test_content2.clone()).await.unwrap();
    
    // Add labels
    let label1 = Label::new("test-label-1");
    let label2 = Label::new("test-label-2");
    
    store.add_label(label1.clone(), ref1.clone()).await.unwrap();
    store.add_label(label2.clone(), ref2.clone()).await.unwrap();
    
    // Lookup by label
    let found_ref1 = store.lookup_label(label1).await.unwrap();
    let found_ref2 = store.lookup_label(label2).await.unwrap();
    
    assert_eq!(found_ref1, ref1);
    assert_eq!(found_ref2, ref2);
    
    // Update label
    store.add_label(Label::new("test-label-1"), ref2.clone()).await.unwrap();
    
    // Verify update
    let updated_ref = store.lookup_label(Label::new("test-label-1")).await.unwrap();
    assert_eq!(updated_ref, ref2);
}

#[tokio::test]
async fn test_content_store_delete() {
    let temp_dir = tempdir().unwrap();
    let store_path = temp_dir.path().join("test-store");
    
    let store = ContentStore::new(store_path.to_str().unwrap());
    
    // Store content
    let test_content = b"deletable content".to_vec();
    let content_ref = store.store(test_content.clone()).await.unwrap();
    
    // Verify it exists
    assert!(store.exists(&content_ref).await.unwrap());
    
    // Delete it
    store.delete(&content_ref).await.unwrap();
    
    // Verify it's gone
    assert!(!store.exists(&content_ref).await.unwrap());
    
    // Trying to get it should error
    let result = store.get(&content_ref).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_content_store_from_id() {
    // Create a store with a specific ID
    let store = ContentStore::from_id("test-id");
    
    // Test storing and retrieving
    let test_content = b"store from ID test".to_vec();
    let content_ref = store.store(test_content.clone()).await.unwrap();
    
    let retrieved = store.get(&content_ref).await.unwrap();
    assert_eq!(retrieved, test_content);
    
    // Verify store path contains the ID
    assert!(store.path().contains("test-id"));
}

#[tokio::test]
async fn test_content_ref_creation() {
    let content = b"test content for ref".to_vec();
    
    // Create content ref directly
    let content_ref = ContentRef::from_content(&content);
    
    // Hash should be deterministic
    let content_ref2 = ContentRef::from_content(&content);
    assert_eq!(content_ref.hash(), content_ref2.hash());
    
    // Hash should be different for different content
    let different_content = b"different content".to_vec();
    let different_ref = ContentRef::from_content(&different_content);
    assert_ne!(content_ref.hash(), different_ref.hash());
    
    // Try creating from hash string
    let hash = content_ref.hash();
    let from_hash = ContentRef::from_hash(hash);
    assert_eq!(content_ref, from_hash);
}

#[tokio::test]
async fn test_content_ref_serialization() {
    let content = b"serialization test".to_vec();
    let content_ref = ContentRef::from_content(&content);
    
    // Serialize and deserialize
    let serialized = serde_json::to_string(&content_ref).unwrap();
    let deserialized: ContentRef = serde_json::from_str(&serialized).unwrap();
    
    assert_eq!(content_ref, deserialized);
}

#[tokio::test]
async fn test_label_operations() {
    // Create simple label
    let label = Label::new("test-label");
    assert_eq!(label.value(), "test-label");
    
    // Create namespace label
    let namespaced = Label::namespaced("namespace", "value");
    assert_eq!(namespaced.value(), "namespace:value");
    
    // Test equality
    let label2 = Label::new("test-label");
    assert_eq!(label, label2);
    
    let different = Label::new("different");
    assert_ne!(label, different);
    
    // Serialize and deserialize
    let serialized = serde_json::to_string(&label).unwrap();
    let deserialized: Label = serde_json::from_str(&serialized).unwrap();
    
    assert_eq!(label, deserialized);
}