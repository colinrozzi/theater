use chrono::{DateTime, Utc};
use ratatui::widgets::ListState;
use theater::chain::ChainEvent;
use std::collections::HashMap;

#[derive(Debug)]
pub struct EventExplorerApp {
    // Core data
    pub actor_id: String,
    pub events: Vec<ChainEvent>,
    pub filtered_events: Vec<usize>, // Indices into events vec

    // UI state
    pub list_state: ListState,
    pub selected_event_index: Option<usize>,
    pub should_quit: bool,

    // Display modes
    pub detail_mode: DetailMode,
    pub show_help: bool,

    // Filtering
    pub active_filters: EventFilters,
    pub search_query: String,
    pub filter_input_mode: bool,
    pub search_input_mode: bool,

    // Pagination
    pub page_size: usize,
    pub current_page: usize,

    // Real-time mode
    pub live_mode: bool,
    pub follow_mode: bool, // Auto-scroll to new events
    pub paused: bool,

    // Performance
    pub event_cache: HashMap<String, ProcessedEventData>,
}

#[derive(Debug, Clone)]
pub enum DetailMode {
    Overview,  // Basic event info
    JsonData,  // Pretty-printed JSON data
    RawData,   // Hex dump of raw bytes
    ChainView, // Show event in chain context
}

#[derive(Debug, Clone)]
pub struct EventFilters {
    pub event_type: Option<String>,
    pub time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    pub level_filter: Option<EventLevel>,
    pub has_error: Option<bool>,
    pub chain_filter: Option<ChainFilter>,
}

#[derive(Debug, Clone)]
pub enum ChainFilter {
    RootEvents,   // Events with no parent
    LeafEvents,   // Events with no children
    WithChildren, // Events that have children
    OrphanEvents, // Events with missing parents
}

#[derive(Debug, Clone)]
pub struct ProcessedEventData {
    pub formatted_data: String,
    pub data_type: DataType,
    pub size_bytes: usize,
    pub is_utf8: bool,
}

#[derive(Debug, Clone)]
pub enum DataType {
    Json,
    Binary,
    Text,
    Empty,
}

#[derive(Debug, Clone)]
pub enum EventLevel {
    Info,
    Warning,
    Error,
}

impl EventExplorerApp {
    pub fn new(actor_id: String, live_mode: bool, follow_mode: bool) -> Self {
        Self {
            actor_id,
            events: Vec::new(),
            filtered_events: Vec::new(),
            list_state: ListState::default(),
            selected_event_index: None,
            should_quit: false,
            detail_mode: DetailMode::Overview,
            show_help: false,
            active_filters: EventFilters::default(),
            search_query: String::new(),
            filter_input_mode: false,
            search_input_mode: false,
            page_size: 50,
            current_page: 0,
            live_mode,
            follow_mode,
            paused: false,
            event_cache: HashMap::new(),
        }
    }

    pub fn load_events(&mut self, events: Vec<ChainEvent>) {
        self.events = events;
        self.apply_filters();
        self.update_selection();
    }

    pub fn add_live_event(&mut self, event: ChainEvent) {
        if self.live_mode && !self.paused {
            self.events.push(event);
            self.apply_filters();

            if self.follow_mode {
                // Auto-scroll to newest event
                self.select_last_event();
            }
        }
    }

    pub fn apply_filters(&mut self) {
        self.filtered_events.clear();

        for (index, event) in self.events.iter().enumerate() {
            if self.event_matches_filters(event) {
                self.filtered_events.push(index);
            }
        }
    }

    pub fn event_matches_filters(&self, event: &ChainEvent) -> bool {
        // Apply all active filters
        if let Some(ref type_filter) = self.active_filters.event_type {
            if !event.event_type.contains(type_filter) {
                return false;
            }
        }

        if !self.search_query.is_empty() {
            let searchable = format!(
                "{} {}",
                event.event_type,
                event.description.as_deref().unwrap_or("")
            );
            if !searchable
                .to_lowercase()
                .contains(&self.search_query.to_lowercase())
            {
                return false;
            }
        }

        // Add more filter logic here in future phases
        true
    }

    pub fn get_selected_event(&self) -> Option<&ChainEvent> {
        self.selected_event_index
            .and_then(|idx| self.filtered_events.get(idx))
            .and_then(|&event_idx| self.events.get(event_idx))
    }

    pub fn select_next(&mut self) {
        if self.filtered_events.is_empty() {
            return;
        }

        let next_idx = match self.selected_event_index {
            Some(idx) => (idx + 1).min(self.filtered_events.len() - 1),
            None => 0,
        };

        self.selected_event_index = Some(next_idx);
        self.list_state.select(Some(next_idx));
    }

    pub fn select_previous(&mut self) {
        if self.filtered_events.is_empty() {
            return;
        }

        let prev_idx = match self.selected_event_index {
            Some(idx) => idx.saturating_sub(1),
            None => 0,
        };

        self.selected_event_index = Some(prev_idx);
        self.list_state.select(Some(prev_idx));
    }

    pub fn page_up(&mut self) {
        if self.filtered_events.is_empty() {
            return;
        }

        let current = self.selected_event_index.unwrap_or(0);
        let new_idx = current.saturating_sub(self.page_size);

        self.selected_event_index = Some(new_idx);
        self.list_state.select(Some(new_idx));
    }

    pub fn page_down(&mut self) {
        if self.filtered_events.is_empty() {
            return;
        }

        let current = self.selected_event_index.unwrap_or(0);
        let new_idx = (current + self.page_size).min(self.filtered_events.len() - 1);

        self.selected_event_index = Some(new_idx);
        self.list_state.select(Some(new_idx));
    }

    pub fn select_last_event(&mut self) {
        if !self.filtered_events.is_empty() {
            let last_idx = self.filtered_events.len() - 1;
            self.selected_event_index = Some(last_idx);
            self.list_state.select(Some(last_idx));
        }
    }

    pub fn cycle_detail_mode(&mut self) {
        self.detail_mode = match self.detail_mode {
            DetailMode::Overview => DetailMode::JsonData,
            DetailMode::JsonData => DetailMode::RawData,
            DetailMode::RawData => DetailMode::ChainView,
            DetailMode::ChainView => DetailMode::Overview,
        };
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn toggle_pause(&mut self) {
        if self.live_mode {
            self.paused = !self.paused;
        }
    }

    pub fn toggle_follow(&mut self) {
        if self.live_mode {
            self.follow_mode = !self.follow_mode;
            if self.follow_mode {
                self.select_last_event();
            }
        }
    }

    pub fn enter_search_mode(&mut self) {
        self.search_input_mode = true;
        // In future phases, this will enable input capture for search
    }

    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_input_mode = false;
        self.apply_filters();
    }

    pub fn enter_filter_mode(&mut self) {
        self.filter_input_mode = true;
        // In future phases, this will open filter dialog
    }

    pub fn set_event_type_filter(&mut self, event_type: String) {
        self.active_filters.event_type = Some(event_type);
        self.apply_filters();
    }

    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.apply_filters();
    }

    fn update_selection(&mut self) {
        if self.filtered_events.is_empty() {
            self.selected_event_index = None;
            self.list_state.select(None);
        } else if self.selected_event_index.is_none() {
            self.selected_event_index = Some(0);
            self.list_state.select(Some(0));
        }
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }
}

impl Default for EventFilters {
    fn default() -> Self {
        Self {
            event_type: None,
            time_range: None,
            level_filter: None,
            has_error: None,
            chain_filter: None,
        }
    }
}

impl DetailMode {
    pub fn display_name(&self) -> &'static str {
        match self {
            DetailMode::Overview => "Overview",
            DetailMode::JsonData => "JSON",
            DetailMode::RawData => "Raw",
            DetailMode::ChainView => "Chain",
        }
    }

    pub fn next_mode_name(&self) -> &'static str {
        match self {
            DetailMode::Overview => "JSON",
            DetailMode::JsonData => "Raw",
            DetailMode::RawData => "Chain",
            DetailMode::ChainView => "Overview",
        }
    }
}
