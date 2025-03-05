// src/store/content_store.rs

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::fs as std_fs;
use std::future::Future;
use std::io::Write as StdWrite;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, info, warn};
use wasmtime::component::{ComponentType, Lift, Lower};

/// A reference to content in the store
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct ContentRef {
    hash: String,
}

impl ContentRef {
    /// Create a new ContentRef from a hash
    pub fn new(hash: String) -> Self {
        Self { hash }
    }

    pub fn from_str(hash: &str) -> Self {
        Self::new(hash.to_string())
    }

    /// Get the hash as a string
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// Convert to a path for storage
    fn to_path(&self, base_path: &Path) -> PathBuf {
        base_path.join("data").join(&self.hash)
    }

    /// Create a ContentRef by hashing content
    pub fn from_content(content: &[u8]) -> Self {
        let mut hasher = Sha1::new();
        hasher.update(content);
        let hash = hasher.finalize();
        let hash_str = hex::encode(hash);
        Self { hash: hash_str }
    }
}

/// The core content store implementation that runs in its own thread
pub struct ContentStore {
    pub id: String,
    base_path: PathBuf,
}

impl ContentStore {
    /// Create a new store with the given base path
    pub fn new() -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let base_path = PathBuf::from("/Users/colinrozzi/work/theater/store").join(&id);
        Self { id, base_path }
    }

    pub fn from_id(id: &str) -> Self {
        let base_path = PathBuf::from("/Users/colinrozzi/work/theater/store").join(id);
        Self {
            id: id.to_string(),
            base_path,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Initialize the store (create necessary directories)
    async fn init(&self) -> Result<()> {
        // Create data directory
        let data_dir = self.base_path.join("data");
        fs::create_dir_all(&data_dir)
            .await
            .context("Failed to create data directory")?;

        // Create labels directory
        let labels_dir = self.base_path.join("labels");
        fs::create_dir_all(&labels_dir)
            .await
            .context("Failed to create labels directory")?;

        Ok(())
    }

    /// Store content and return its ContentRef
    pub async fn store(&self, content: Vec<u8>) -> Result<ContentRef> {
        let content_ref = ContentRef::from_content(&content);
        let path = content_ref.to_path(&self.base_path);

        // Check if content already exists
        if !fs::try_exists(&path).await.unwrap_or(false) {
            // Create parent directories if they don't exist
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .context("Failed to create parent directories")?;
            }

            // Write content to file
            let mut file = fs::File::create(&path)
                .await
                .context("Failed to create content file")?;
            file.write_all(&content)
                .await
                .context("Failed to write content")?;
            debug!("Stored content with hash: {}", content_ref.hash());
        } else {
            debug!("Content already exists: {}", content_ref.hash());
        }

        Ok(content_ref)
    }

    /// Store content synchronously and return its ContentRef
    pub fn store_sync(&self, content: Vec<u8>) -> Result<ContentRef> {
        let content_ref = ContentRef::from_content(&content);
        let path = content_ref.to_path(&self.base_path);

        // Check if content already exists
        if !std_fs::exists(&path).unwrap_or(false) {
            // Create parent directories if they don't exist
            if let Some(parent) = path.parent() {
                std_fs::create_dir_all(parent).context("Failed to create parent directories")?;
            }

            // Write content to file synchronously
            let mut file = std_fs::File::create(&path).context("Failed to create content file")?;
            file.write_all(&content)
                .context("Failed to write content")?;
            debug!("Stored content with hash: {}", content_ref.hash());
        } else {
            debug!("Content already exists: {}", content_ref.hash());
        }

        Ok(content_ref)
    }

    /// Retrieve content by its reference
    pub async fn get(&self, content_ref: &ContentRef) -> Result<Vec<u8>> {
        let path = content_ref.to_path(&self.base_path);
        fs::read(&path)
            .await
            .with_context(|| format!("Failed to read content at path: {:?}", path))
    }

    /// Check if content exists
    pub async fn exists(&self, content_ref: &ContentRef) -> bool {
        let path = content_ref.to_path(&self.base_path);
        fs::try_exists(&path).await.unwrap_or(false)
    }

    /// Get the path to a label file
    pub fn label_path(&self, label: &str) -> PathBuf {
        self.base_path.join("labels").join(label)
    }

    /// Attach a label to content (replaces any existing content at that label)
    pub async fn label(&self, label: &str, content_ref: &ContentRef) -> Result<()> {
        // Ensure content exists before labeling
        if !self.exists(content_ref).await {
            return Err(anyhow::anyhow!(
                "Content does not exist: {}",
                content_ref.hash()
            ));
        }

        let label_path = self.label_path(label);

        // Create parent directories if they don't exist
        if let Some(parent) = label_path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("Failed to create label parent directories")?;
        }

        // Write the single content ref hash to the label file
        fs::write(&label_path, content_ref.hash())
            .await
            .context("Failed to write label file")?;

        debug!("Set content {} for label '{}'", content_ref.hash(), label);

        Ok(())
    }

    pub async fn replace_content_at_label(
        &self,
        label: &str,
        content: Vec<u8>,
    ) -> Result<ContentRef> {
        let content_ref = ContentRef::from_content(&content);
        let path = content_ref.to_path(&self.base_path);

        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("Failed to create parent directories")?;
        }

        // Write content to file
        let mut file = fs::File::create(&path)
            .await
            .context("Failed to create content file")?;
        file.write_all(&content)
            .await
            .context("Failed to write content")?;
        debug!("Stored content with hash: {}", content_ref.hash());

        // Replace label with new content
        let label_path = self.label_path(label);
        let content = content_ref.hash();

        fs::write(&label_path, content)
            .await
            .context("Failed to write label file")?;

        debug!("Replaced content in label '{}'", label);

        Ok(content_ref)
    }

    pub async fn replace_at_label(&self, label: &str, content_ref: &ContentRef) -> Result<()> {
        // Ensure content exists before labeling
        if !self.exists(content_ref).await {
            return Err(anyhow::anyhow!(
                "Content does not exist: {}",
                content_ref.hash()
            ));
        }

        let label_path = self.label_path(label);

        // Create parent directories if they don't exist
        if let Some(parent) = label_path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("Failed to create label parent directories")?;
        }

        // Write updated refs to label file
        let content = content_ref.hash();

        fs::write(&label_path, content)
            .await
            .context("Failed to write label file")?;

        debug!("Replaced content in label '{}'", label);

        Ok(())
    }

    /// Get content reference by label
    pub async fn get_by_label(&self, label: &str) -> Result<Option<ContentRef>> {
        let label_path = self.label_path(label);

        // If label doesn't exist, return None
        if !fs::try_exists(&label_path).await.unwrap_or(false) {
            return Ok(None);
        }

        // Read and parse label file
        let content = fs::read_to_string(&label_path)
            .await
            .context("Failed to read label file")?;

        let content_hash = content.trim();
        if content_hash.is_empty() {
            return Ok(None);
        }

        // Return a single content reference
        let content_ref = ContentRef::new(content_hash.to_string());
        Ok(Some(content_ref))
    }

    /// Get content by label
    pub async fn get_content_by_label(&self, label: &str) -> Result<Option<Vec<u8>>> {
        if let Some(content_ref) = self.get_by_label(label).await? {
            let content = self.get(&content_ref).await?;
            Ok(Some(content))
        } else {
            Ok(None)
        }
    }

    pub async fn put_at_label(&self, label: &str, content: Vec<u8>) -> Result<ContentRef> {
        let content_ref = ContentRef::from_content(&content);
        let path = content_ref.to_path(&self.base_path);

        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("Failed to create parent directories")?;
        }

        // Write content to file
        let mut file = fs::File::create(&path)
            .await
            .context("Failed to create content file")?;
        file.write_all(&content)
            .await
            .context("Failed to write content")?;
        debug!("Stored content with hash: {}", content_ref.hash());

        // Write the single content ref hash to the label file
        let label_path = self.label_path(label);
        fs::write(&label_path, content_ref.hash())
            .await
            .context("Failed to write label file")?;

        debug!("Set content {} for label '{}'", content_ref.hash(), label);

        Ok(content_ref)
    }

    /// Remove a label
    pub async fn remove_label(&self, label: &str) -> Result<()> {
        let label_path = self.label_path(label);

        // If label doesn't exist, do nothing
        if !fs::try_exists(&label_path).await.unwrap_or(false) {
            debug!("Label does not exist: {}", label);
            return Ok(());
        }

        // Remove label file
        fs::remove_file(&label_path)
            .await
            .context("Failed to remove label file")?;

        debug!("Removed label: {}", label);
        Ok(())
    }

    /// Remove a specific content reference from a label
    /// If the content reference matches the one at the label, the label is removed
    pub async fn remove_from_label(&self, label: &str, content_ref: &ContentRef) -> Result<()> {
        let label_path = self.label_path(label);

        // If label doesn't exist, do nothing
        if !fs::try_exists(&label_path).await.unwrap_or(false) {
            debug!("Label does not exist: {}", label);
            return Ok(());
        }

        // Read the current content ref from the label file
        let content = fs::read_to_string(&label_path)
            .await
            .context("Failed to read label file")?;
        let current_hash = content.trim();

        // If the current content ref matches the one we want to remove, remove the label
        if current_hash == content_ref.hash() {
            fs::remove_file(&label_path)
                .await
                .context("Failed to remove label file")?;
            debug!(
                "Removed label '{}' that pointed to content {}",
                label,
                content_ref.hash()
            );
        } else {
            debug!(
                "Label '{}' does not point to content {}",
                label,
                content_ref.hash()
            );
        }

        Ok(())
    }

    /// List all labels recursively, including nested directories
    /// Returns paths relative to the labels directory
    pub async fn list_labels(&self) -> Result<Vec<String>> {
        let labels_dir = self.base_path.join("labels");

        // Ensure labels directory exists
        if !fs::try_exists(&labels_dir).await.unwrap_or(false) {
            return Ok(Vec::new());
        }

        let mut result = Vec::new();
        self.collect_labels_recursive(&labels_dir, &labels_dir, &mut result)
            .await?;

        Ok(result)
    }

    /// Helper method to recursively collect labels
    fn collect_labels_recursive<'a>(
        &'a self,
        base_path: &'a Path,
        current_path: &'a Path,
        result: &'a mut Vec<String>,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut entries = fs::read_dir(current_path)
                .await
                .with_context(|| format!("Failed to read directory: {:?}", current_path))?;

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();

                if let Ok(file_type) = entry.file_type().await {
                    if file_type.is_file() {
                        // It's a file/label, so add it to our results
                        if let Some(rel_path) = path.strip_prefix(base_path).ok() {
                            if let Some(rel_path_str) = rel_path.to_str() {
                                result.push(rel_path_str.to_string());
                            }
                        }
                    } else if file_type.is_dir() {
                        // It's a directory, so recursively collect labels from it
                        self.collect_labels_recursive(base_path, &path, result)
                            .await?;
                    }
                }
            }

            Ok(())
        })
    }

    /// List all content references in the store
    pub async fn list_all_content(&self) -> Result<Vec<ContentRef>> {
        let data_dir = self.base_path.join("data");

        // Ensure data directory exists
        if !fs::try_exists(&data_dir).await.unwrap_or(false) {
            return Ok(Vec::new());
        }

        let mut entries = fs::read_dir(&data_dir)
            .await
            .context("Failed to read data directory")?;

        let mut refs = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            if let Ok(file_type) = entry.file_type().await {
                if file_type.is_file() {
                    if let Some(name) = entry.file_name().to_str() {
                        refs.push(ContentRef::new(name.to_string()));
                    }
                }
            }
        }

        Ok(refs)
    }

    /// Calculate total size of all content in the store
    pub async fn calculate_total_size(&self) -> Result<u64> {
        let refs = self.list_all_content().await?;
        let mut total_size = 0;

        for content_ref in refs {
            let path = content_ref.to_path(&self.base_path);
            if let Ok(metadata) = fs::metadata(&path).await {
                total_size += metadata.len();
            }
        }

        Ok(total_size)
    }
}
