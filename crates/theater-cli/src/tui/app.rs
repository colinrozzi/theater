use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use theater::ChainEvent;
use theater_server::ManagementResponse;

#[derive(Debug, Clone)]
pub struct TuiApp {
    // Core state
    pub actor_id: String,
    pub manifest_path: String,
    pub start_time: DateTime<Utc>,

    // Event tracking
    pub events: VecDeque<DisplayEvent>,
    pub max_events: usize,
    pub auto_scroll: bool,

    // Lifecycle tracking
    pub lifecycle_events: Vec<LifecycleEvent>,
    pub current_status: ActorStatus,
    pub error_count: usize,
    pub event_count: usize,

    // UI state
    pub should_quit: bool,
    pub paused: bool,
}

#[derive(Debug, Clone)]
pub struct DisplayEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub message: String,
    pub details: Option<String>,
    pub level: EventLevel,
}

#[derive(Debug, Clone)]
pub struct LifecycleEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: LifecycleEventType,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum LifecycleEventType {
    ActorStarted,
    ActorStopped,
    ActorError,
    ActorResult,
    StatusUpdate,
}

#[derive(Debug, Clone)]
pub enum EventLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub enum ActorStatus {
    Starting,
    Running,
    Paused,
    Stopped,
    Error,
}

impl TuiApp {
    pub fn new(actor_id: String, manifest_path: String) -> Self {
        Self {
            actor_id,
            manifest_path,
            start_time: Utc::now(),
            events: VecDeque::new(),
            max_events: 1000,
            auto_scroll: true,
            lifecycle_events: Vec::new(),
            current_status: ActorStatus::Starting,
            error_count: 0,
            event_count: 0,
            should_quit: false,
            paused: false,
        }
    }

    pub fn add_event(&mut self, event: DisplayEvent) {
        if !self.paused {
            self.event_count += 1;

            if event.level == EventLevel::Error {
                self.error_count += 1;
            }

            self.events.push_back(event);

            // Keep only the last max_events
            while self.events.len() > self.max_events {
                self.events.pop_front();
            }
        }
    }

    pub fn add_lifecycle_event(&mut self, event: LifecycleEvent) {
        match &event.event_type {
            LifecycleEventType::ActorStarted => {
                self.current_status = ActorStatus::Running;
            }
            LifecycleEventType::ActorStopped => {
                self.current_status = ActorStatus::Stopped;
            }
            LifecycleEventType::ActorError => {
                self.current_status = ActorStatus::Error;
                self.error_count += 1;
            }
            _ => {}
        }

        self.lifecycle_events.push(event);

        // Keep only the last 50 lifecycle events
        if self.lifecycle_events.len() > 50 {
            self.lifecycle_events.remove(0);
        }
    }

    pub fn handle_management_response(&mut self, response: ManagementResponse) {
        let timestamp = Utc::now();

        match response {
            ManagementResponse::ActorStarted { id } => {
                let lifecycle_event = LifecycleEvent {
                    timestamp,
                    event_type: LifecycleEventType::ActorStarted,
                    message: format!("Actor started with ID: {}", id),
                };
                self.add_lifecycle_event(lifecycle_event);
            }
            ManagementResponse::ActorEvent { event } => {
                let display_event = self.chain_event_to_display_event(event, timestamp);
                self.add_event(display_event);
            }
            ManagementResponse::ActorError { error } => {
                let lifecycle_event = LifecycleEvent {
                    timestamp,
                    event_type: LifecycleEventType::ActorError,
                    message: format!("Actor error: {}", error),
                };
                self.add_lifecycle_event(lifecycle_event);
            }
            ManagementResponse::ActorStopped { id } => {
                let lifecycle_event = LifecycleEvent {
                    timestamp,
                    event_type: LifecycleEventType::ActorStopped,
                    message: format!("Actor stopped: {}", id),
                };
                self.add_lifecycle_event(lifecycle_event);
            }
            ManagementResponse::ActorResult(result) => {
                let lifecycle_event = LifecycleEvent {
                    timestamp,
                    event_type: LifecycleEventType::ActorResult,
                    message: format!("Actor result: {}", result),
                };
                self.add_lifecycle_event(lifecycle_event);
            }
            _ => {
                // Handle other response types if needed
            }
        }
    }

    fn chain_event_to_display_event(
        &self,
        event: ChainEvent,
        timestamp: DateTime<Utc>,
    ) -> DisplayEvent {
        let description = event.description.as_deref().unwrap_or("Unknown event");
        let level = if description.contains("error") {
            EventLevel::Error
        } else if description.contains("warn") {
            EventLevel::Warning
        } else {
            EventLevel::Info
        };

        DisplayEvent {
            timestamp,
            event_type: event.event_type.clone(),
            message: description.to_string(),
            details: Some(format!("{:?}", event.data)),
            level,
        }
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    pub fn toggle_auto_scroll(&mut self) {
        self.auto_scroll = !self.auto_scroll;
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn reset_events(&mut self) {
        self.events.clear();
        self.event_count = 0;
    }
}

impl PartialEq for EventLevel {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (EventLevel::Info, EventLevel::Info)
                | (EventLevel::Warning, EventLevel::Warning)
                | (EventLevel::Error, EventLevel::Error)
        )
    }
}
