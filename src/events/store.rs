use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StoreEventData {
    // Store content events
    StoreCall {
        content_size: usize,
    },
    StoreResult {
        hash: String,
        success: bool,
    },

    // Get content events
    GetCall {
        hash: String,
    },
    GetResult {
        hash: String,
        content_size: usize,
        success: bool,
    },

    // Exists events
    ExistsCall {
        hash: String,
    },
    ExistsResult {
        hash: String,
        exists: bool,
        success: bool,
    },

    // Label events
    LabelCall {
        label: String,
        hash: String,
    },
    LabelResult {
        label: String,
        hash: String,
        success: bool,
    },

    // Get by label events
    GetByLabelCall {
        label: String,
    },
    GetByLabelResult {
        label: String,
        refs_count: usize,
        success: bool,
    },

    // Remove label events
    RemoveLabelCall {
        label: String,
    },
    RemoveLabelResult {
        label: String,
        success: bool,
    },

    // Remove from label events
    RemoveFromLabelCall {
        label: String,
        hash: String,
    },
    RemoveFromLabelResult {
        label: String,
        hash: String,
        success: bool,
    },

    // Put at label events
    PutAtLabelCall {
        label: String,
        content_size: usize,
    },
    PutAtLabelResult {
        label: String,
        hash: String,
        success: bool,
    },

    // Replace content at label events
    ReplaceContentAtLabelCall {
        label: String,
        content_size: usize,
    },
    ReplaceContentAtLabelResult {
        label: String,
        hash: String,
        success: bool,
    },

    // Replace at label events
    ReplaceAtLabelCall {
        label: String,
        hash: String,
    },
    ReplaceAtLabelResult {
        label: String,
        hash: String,
        success: bool,
    },

    // List labels events
    ListLabelsCall {},
    ListLabelsResult {
        labels_count: usize,
        success: bool,
    },

    // List all content events
    ListAllContentCall {},
    ListAllContentResult {
        refs_count: usize,
        success: bool,
    },

    // Calculate total size events
    CalculateTotalSizeCall {},
    CalculateTotalSizeResult {
        size: u64,
        success: bool,
    },

    // Error events
    Error {
        operation: String,
        message: String,
    },
}

pub struct StoreEvent {
    pub data: StoreEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
