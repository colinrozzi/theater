// src/store/content_store.rs

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

/// A reference to content in the store
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentRef {
    hash: String,
}

impl ContentRef {
    /// Create a new ContentRef from a hash
    pub fn new(hash: String) -> Self {
        Self { hash }
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
struct ContentStoreImpl {
    base_path: PathBuf,
}

impl ContentStoreImpl {
    /// Create a new store with the given base path
    fn new(base_path: PathBuf) -> Self {
        Self { base_path }
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
    async fn store(&self, content: Vec<u8>) -> Result<ContentRef> {
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

    /// Retrieve content by its reference
    async fn get(&self, content_ref: &ContentRef) -> Result<Vec<u8>> {
        let path = content_ref.to_path(&self.base_path);
        fs::read(&path)
            .await
            .with_context(|| format!("Failed to read content at path: {:?}", path))
    }

    /// Check if content exists
    async fn exists(&self, content_ref: &ContentRef) -> bool {
        let path = content_ref.to_path(&self.base_path);
        fs::try_exists(&path).await.unwrap_or(false)
    }

    /// Get the path to a label file
    fn label_path(&self, label: &str) -> PathBuf {
        self.base_path.join("labels").join(label)
    }

    /// Attach a label to content
    async fn label(&self, label: &str, content_ref: &ContentRef) -> Result<()> {
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

        // Read existing content refs for this label
        let mut refs = self.get_by_label(label).await.unwrap_or_default();

        // Add new content ref if it doesn't already exist
        if !refs.contains(content_ref) {
            refs.push(content_ref.clone());

            // Write updated refs to label file
            let content = refs.iter().map(|r| r.hash()).collect::<Vec<_>>().join("\n");

            fs::write(&label_path, content)
                .await
                .context("Failed to write label file")?;

            debug!("Added content {} to label '{}'", content_ref.hash(), label);
        } else {
            debug!(
                "Content {} already in label '{}'",
                content_ref.hash(),
                label
            );
        }

        Ok(())
    }

    async fn replace_content_at_label(&self, label: &str, content: Vec<u8>) -> Result<ContentRef> {
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

    async fn replace_at_label(&self, label: &str, content_ref: &ContentRef) -> Result<()> {
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

    /// Get content references by label
    async fn get_by_label(&self, label: &str) -> Result<Vec<ContentRef>> {
        let label_path = self.label_path(label);

        // If label doesn't exist, return empty vec
        if !fs::try_exists(&label_path).await.unwrap_or(false) {
            return Ok(Vec::new());
        }

        // Read and parse label file
        let content = fs::read_to_string(&label_path)
            .await
            .context("Failed to read label file")?;

        let refs = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| ContentRef::new(line.trim().to_string()))
            .collect();

        Ok(refs)
    }

    /// Remove a label
    async fn remove_label(&self, label: &str) -> Result<()> {
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
    async fn remove_from_label(&self, label: &str, content_ref: &ContentRef) -> Result<()> {
        let label_path = self.label_path(label);

        // If label doesn't exist, do nothing
        if !fs::try_exists(&label_path).await.unwrap_or(false) {
            debug!("Label does not exist: {}", label);
            return Ok(());
        }

        // Read and parse label file
        let content = fs::read_to_string(&label_path)
            .await
            .context("Failed to read label file")?;

        let mut refs: Vec<ContentRef> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| ContentRef::new(line.trim().to_string()))
            .collect();

        // Remove content ref if it exists
        let original_len = refs.len();
        refs.retain(|r| r != content_ref);

        // If content ref was found and removed, write updated label file
        if refs.len() != original_len {
            if refs.is_empty() {
                // If no more refs, remove the label file
                fs::remove_file(&label_path)
                    .await
                    .context("Failed to remove empty label file")?;
                debug!("Removed empty label: {}", label);
            } else {
                // Write updated refs to label file
                let content = refs.iter().map(|r| r.hash()).collect::<Vec<_>>().join("\n");

                fs::write(&label_path, content)
                    .await
                    .context("Failed to write updated label file")?;

                debug!(
                    "Removed content {} from label '{}'",
                    content_ref.hash(),
                    label
                );
            }
        } else {
            debug!(
                "Content not found in label '{}': {}",
                label,
                content_ref.hash()
            );
        }

        Ok(())
    }

    /// List all labels
    async fn list_labels(&self) -> Result<Vec<String>> {
        let labels_dir = self.base_path.join("labels");

        // Ensure labels directory exists
        if !fs::try_exists(&labels_dir).await.unwrap_or(false) {
            return Ok(Vec::new());
        }

        let mut entries = fs::read_dir(&labels_dir)
            .await
            .context("Failed to read labels directory")?;

        let mut labels = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            if let Ok(file_type) = entry.file_type().await {
                if file_type.is_file() {
                    if let Some(name) = entry.file_name().to_str() {
                        labels.push(name.to_string());
                    }
                }
            }
        }

        Ok(labels)
    }

    /// List all content references in the store
    async fn list_all_content(&self) -> Result<Vec<ContentRef>> {
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
    async fn calculate_total_size(&self) -> Result<u64> {
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

/// Operations that can be performed on the content store
pub enum StoreOperation {
    // Core operations
    Store {
        content: Vec<u8>,
        resp: oneshot::Sender<Result<ContentRef>>,
    },
    Get {
        content_ref: ContentRef,
        resp: oneshot::Sender<Result<Vec<u8>>>,
    },
    Exists {
        content_ref: ContentRef,
        resp: oneshot::Sender<bool>,
    },

    // Label operations
    Label {
        label: String,
        content_ref: ContentRef,
        resp: oneshot::Sender<Result<()>>,
    },
    GetByLabel {
        label: String,
        resp: oneshot::Sender<Result<Vec<ContentRef>>>,
    },
    RemoveLabel {
        label: String,
        resp: oneshot::Sender<Result<()>>,
    },
    RemoveFromLabel {
        label: String,
        content_ref: ContentRef,
        resp: oneshot::Sender<Result<()>>,
    },
    PutAtLabel {
        label: String,
        content: Vec<u8>,
        resp: oneshot::Sender<Result<ContentRef>>,
    },
    ReplaceContentAtLabel {
        label: String,
        content: Vec<u8>,
        resp: oneshot::Sender<Result<ContentRef>>,
    },
    ReplaceAtLabel {
        label: String,
        content_ref: ContentRef,
        resp: oneshot::Sender<Result<()>>,
    },

    // Utility operations
    ListLabels {
        resp: oneshot::Sender<Result<Vec<String>>>,
    },
    ListAllContent {
        resp: oneshot::Sender<Result<Vec<ContentRef>>>,
    },
    CalculateTotalSize {
        resp: oneshot::Sender<Result<u64>>,
    },

    // Control operations
    Shutdown {
        resp: oneshot::Sender<()>,
    },
}

/// Client handle to interact with the content store
#[derive(Clone, Debug)]
pub struct ContentStore {
    sender: mpsc::Sender<StoreOperation>,
}

impl ContentStore {
    /// Start a new content store in its own thread and return a handle to it
    pub fn start(base_path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel(100); // Buffer size of 100 operations

        // Resolve the absolute path using THEATER_HOME if available
        let absolute_path = if base_path.is_absolute() {
            // Path is already absolute, use as is
            base_path
        } else {
            // Check for THEATER_HOME environment variable
            if let Ok(theater_home) = std::env::var("THEATER_HOME") {
                // THEATER_HOME exists, use it as base
                let theater_home_path = PathBuf::from(theater_home);
                debug!("Using THEATER_HOME for store path: {:?}", theater_home_path);
                theater_home_path.join(base_path)
            } else {
                // THEATER_HOME not available, fall back to current directory
                debug!("THEATER_HOME not set, using current directory as base");
                match std::env::current_dir() {
                    Ok(current_dir) => current_dir.join(base_path),
                    Err(e) => {
                        error!("Failed to get current directory: {}", e);
                        // Fallback to the original path if we can't get current dir
                        base_path
                    }
                }
            }
        };

        info!("Starting content store with path: {:?}", absolute_path);

        // Start store thread
        tokio::spawn(async move {
            run_store(absolute_path, receiver).await;
        });

        Self { sender }
    }

    /// Store content and return a reference to it
    pub async fn store(&self, content: Vec<u8>) -> Result<ContentRef> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(StoreOperation::Store {
                content: content.clone(),
                resp: resp_tx,
            })
            .await
            .context("Failed to send store operation")?;

        resp_rx.await.context("Store operation failed")??;

        // Create ContentRef by hashing content in the client
        // This allows us to check for existence before sending large content
        let content_ref = ContentRef::from_content(&content);
        Ok(content_ref)
    }

    /// Retrieve content by its reference
    pub async fn get(&self, content_ref: ContentRef) -> Result<Vec<u8>> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(StoreOperation::Get {
                content_ref,
                resp: resp_tx,
            })
            .await
            .context("Failed to send get operation")?;

        resp_rx.await.context("Get operation failed")?
    }

    /// Check if content exists
    pub async fn exists(&self, content_ref: ContentRef) -> Result<bool> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(StoreOperation::Exists {
                content_ref,
                resp: resp_tx,
            })
            .await
            .context("Failed to send exists operation")?;

        Ok(resp_rx.await.context("Exists operation failed")?)
    }

    /// Label content with a string identifier
    pub async fn label(&self, label: String, content_ref: ContentRef) -> Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(StoreOperation::Label {
                label,
                content_ref,
                resp: resp_tx,
            })
            .await
            .context("Failed to send label operation")?;

        resp_rx.await.context("Label operation failed")?
    }

    /// Get content references by label
    pub async fn get_by_label(&self, label: String) -> Result<Vec<ContentRef>> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(StoreOperation::GetByLabel {
                label,
                resp: resp_tx,
            })
            .await
            .context("Failed to send get_by_label operation")?;

        resp_rx.await.context("GetByLabel operation failed")?
    }

    /// Remove a label
    pub async fn remove_label(&self, label: String) -> Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(StoreOperation::RemoveLabel {
                label,
                resp: resp_tx,
            })
            .await
            .context("Failed to send remove_label operation")?;

        resp_rx.await.context("RemoveLabel operation failed")?
    }

    /// Remove a specific content reference from a label
    pub async fn remove_from_label(&self, label: String, content_ref: ContentRef) -> Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(StoreOperation::RemoveFromLabel {
                label,
                content_ref,
                resp: resp_tx,
            })
            .await
            .context("Failed to send remove_from_label operation")?;

        resp_rx.await.context("RemoveFromLabel operation failed")?
    }

    /// Store content and immediately label it
    pub async fn put_at_label(&self, label: String, content: Vec<u8>) -> Result<ContentRef> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(StoreOperation::PutAtLabel {
                label,
                content,
                resp: resp_tx,
            })
            .await
            .context("Failed to send put_at_label operation")?;

        resp_rx.await.context("PutAtLabel operation failed")?
    }

    /// Put content at a label, replacing any existing content
    pub async fn replace_content_at_label(
        &self,
        label: String,
        content: Vec<u8>,
    ) -> Result<ContentRef> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(StoreOperation::ReplaceContentAtLabel {
                label,
                content,
                resp: resp_tx,
            })
            .await
            .context("Failed to send replace_content_at_label operation")?;

        resp_rx
            .await
            .context("ReplaceContentAtLabel operation failed")?
    }

    /// Replace content at a label with a specific content reference
    pub async fn replace_at_label(&self, label: String, content_ref: ContentRef) -> Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(StoreOperation::ReplaceAtLabel {
                label,
                content_ref,
                resp: resp_tx,
            })
            .await
            .context("Failed to send replace_at_label operation")?;

        resp_rx.await.context("ReplaceAtLabel operation failed")?
    }

    /// List all labels
    pub async fn list_labels(&self) -> Result<Vec<String>> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(StoreOperation::ListLabels { resp: resp_tx })
            .await
            .context("Failed to send list_labels operation")?;

        resp_rx.await.context("ListLabels operation failed")?
    }

    /// List all content references
    pub async fn list_all_content(&self) -> Result<Vec<ContentRef>> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(StoreOperation::ListAllContent { resp: resp_tx })
            .await
            .context("Failed to send list_all_content operation")?;

        resp_rx.await.context("ListAllContent operation failed")?
    }

    /// Calculate total size of all content
    pub async fn calculate_total_size(&self) -> Result<u64> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(StoreOperation::CalculateTotalSize { resp: resp_tx })
            .await
            .context("Failed to send calculate_total_size operation")?;

        resp_rx
            .await
            .context("CalculateTotalSize operation failed")?
    }

    /// Shutdown the store thread
    pub async fn shutdown(&self) -> Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(StoreOperation::Shutdown { resp: resp_tx })
            .await
            .context("Failed to send shutdown operation")?;

        resp_rx.await.context("Shutdown operation failed")?;
        Ok(())
    }

    pub async fn resolve_reference(&self, reference: &str) -> Result<Vec<u8>> {
        if reference.starts_with("store:") {
            if reference.starts_with("store:hash:") {
                // Direct hash reference
                let hash = reference.strip_prefix("store:hash:").unwrap();
                let content_ref = ContentRef::new(hash.to_string());
                self.get(content_ref).await
            } else {
                // Label reference
                let label = reference.strip_prefix("store:").unwrap().to_string();
                let refs = self.get_by_label(label.clone()).await?;

                match refs.len() {
                    0 => Err(anyhow::anyhow!("No content found with label: {}", label)),
                    1 => self.get(refs[0].clone()).await,
                    _ => Err(anyhow::anyhow!(
                        "Ambiguous label reference, multiple matches found: {}",
                        label
                    )),
                }
            }
        } else {
            // Regular file path
            Ok(tokio::fs::read(reference)
                .await
                .expect(format!("Failed to read file: {}", reference).as_str()))
        }
    }
}

/// Run the content store in its own thread
async fn run_store(base_path: PathBuf, mut receiver: mpsc::Receiver<StoreOperation>) {
    // Create and initialize store
    let store = ContentStoreImpl::new(base_path);

    if let Err(e) = store.init().await {
        error!("Failed to initialize content store: {}", e);
        return;
    }

    info!("Content store initialized and ready for operations");

    // Process operations until shutdown
    while let Some(op) = receiver.recv().await {
        match op {
            StoreOperation::Store { content, resp } => {
                let result = store.store(content).await;
                let _ = resp.send(result);
            }

            StoreOperation::Get { content_ref, resp } => {
                let result = store.get(&content_ref).await;
                let _ = resp.send(result);
            }

            StoreOperation::Exists { content_ref, resp } => {
                let exists = store.exists(&content_ref).await;
                let _ = resp.send(exists);
            }

            StoreOperation::Label {
                label,
                content_ref,
                resp,
            } => {
                let result = store.label(&label, &content_ref).await;
                let _ = resp.send(result);
            }

            StoreOperation::GetByLabel { label, resp } => {
                let result = store.get_by_label(&label).await;
                let _ = resp.send(result);
            }

            StoreOperation::RemoveLabel { label, resp } => {
                let result = store.remove_label(&label).await;
                let _ = resp.send(result);
            }

            StoreOperation::RemoveFromLabel {
                label,
                content_ref,
                resp,
            } => {
                let result = store.remove_from_label(&label, &content_ref).await;
                let _ = resp.send(result);
            }

            StoreOperation::PutAtLabel {
                label,
                content,
                resp,
            } => {
                // Implement the combined operation directly in the worker
                // to avoid unnecessary message passing
                match store.store(content).await {
                    Ok(content_ref) => {
                        match store.label(&label, &content_ref).await {
                            Ok(_) => {
                                let _ = resp.send(Ok(content_ref));
                            }
                            Err(e) => {
                                // Label operation failed, but content was stored
                                warn!("Content stored but labeling failed: {}", e);
                                let _ = resp.send(Err(e));
                            }
                        }
                    }
                    Err(e) => {
                        let _ = resp.send(Err(e));
                    }
                }
            }

            StoreOperation::ReplaceContentAtLabel {
                label,
                content,
                resp,
            } => {
                // Implement the combined operation directly in the worker
                // to avoid unnecessary message passing
                match store.replace_content_at_label(&label, content).await {
                    Ok(content_ref) => {
                        let _ = resp.send(Ok(content_ref));
                    }
                    Err(e) => {
                        let _ = resp.send(Err(e));
                    }
                }
            }

            StoreOperation::ReplaceAtLabel {
                label,
                content_ref,
                resp,
            } => {
                let result = store.replace_at_label(&label, &content_ref).await;
                let _ = resp.send(result);
            }

            StoreOperation::ListLabels { resp } => {
                let result = store.list_labels().await;
                let _ = resp.send(result);
            }

            StoreOperation::ListAllContent { resp } => {
                let result = store.list_all_content().await;
                let _ = resp.send(result);
            }

            StoreOperation::CalculateTotalSize { resp } => {
                let result = store.calculate_total_size().await;
                let _ = resp.send(result);
            }

            StoreOperation::Shutdown { resp } => {
                info!("Content store shutting down");
                let _ = resp.send(());
                break;
            }
        }
    }

    info!("Content store thread terminated");
}
