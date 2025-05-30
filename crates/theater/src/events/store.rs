use crate::store::ContentRef;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StoreEventData {
    NewStoreCall {},
    NewStoreResult {
        store_id: String,
        success: bool,
    },

    // Store content events
    StoreCall {
        store_id: String,
        content: Vec<u8>,
    },
    StoreResult {
        store_id: String,
        content_ref: ContentRef,
        success: bool,
    },

    // Get content events
    GetCall {
        store_id: String,
        content_ref: ContentRef,
    },
    GetResult {
        store_id: String,
        content_ref: ContentRef,
        content: Option<Vec<u8>>,
        success: bool,
    },

    // Exists events
    ExistsCall {
        store_id: String,
        content_ref: ContentRef,
    },
    ExistsResult {
        store_id: String,
        content_ref: ContentRef,
        exists: bool,
        success: bool,
    },

    // Label events
    LabelCall {
        store_id: String,
        label: String,
        content_ref: ContentRef,
    },
    LabelResult {
        store_id: String,
        label: String,
        content_ref: ContentRef,
        success: bool,
    },

    // Get by label events
    GetByLabelCall {
        store_id: String,
        label: String,
    },
    GetByLabelResult {
        store_id: String,
        label: String,
        content_ref: Option<ContentRef>,
        success: bool,
    },

    // Remove label events
    RemoveLabelCall {
        store_id: String,
        label: String,
    },
    RemoveLabelResult {
        store_id: String,
        label: String,
        success: bool,
    },

    // Put at label events
    StoreAtLabelCall {
        store_id: String,
        label: String,
        content: Vec<u8>,
    },
    StoreAtLabelResult {
        store_id: String,
        label: String,
        content_ref: ContentRef,
        success: bool,
    },

    // Replace content at label events
    ReplaceContentAtLabelCall {
        store_id: String,
        label: String,
        content: Vec<u8>,
    },
    ReplaceContentAtLabelResult {
        store_id: String,
        label: String,
        content_ref: ContentRef,
        success: bool,
    },

    // Replace at label events
    ReplaceAtLabelCall {
        store_id: String,
        label: String,
        content_ref: ContentRef,
    },
    ReplaceAtLabelResult {
        store_id: String,
        label: String,
        content_ref: ContentRef,
        success: bool,
    },

    // List labels events
    ListLabelsCall {
        store_id: String,
    },
    ListLabelsResult {
        store_id: String,
        labels: Vec<String>,
        success: bool,
    },

    // List all content events
    ListAllContentCall {
        store_id: String,
    },
    ListAllContentResult {
        store_id: String,
        content_refs: Vec<ContentRef>,
        success: bool,
    },

    // Calculate total size events
    CalculateTotalSizeCall {
        store_id: String,
    },
    CalculateTotalSizeResult {
        store_id: String,
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
