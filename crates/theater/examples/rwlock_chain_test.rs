//! Standalone test for RwLock chain functionality
//! Run with: cargo test --bin rwlock_test

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use theater::actor::store::ActorStore;
use theater::actor::handle::ActorHandle;
use theater::events::{ChainEventData, EventData};
use theater::events::runtime::RuntimeEventData;
use theater::id::TheaterId;

#[tokio::main]
async fn main() {
    println!("🧪 Testing RwLock Chain Implementation...\n");
    
    test_concurrent_access().await;
    test_query_methods().await; 
    test_no_deadlock().await;
    
    println!("\n✅ All RwLock chain tests passed!");
}

async fn test_concurrent_access() {
    println!("🔄 Testing concurrent read/write access...");
    
    // Create actor store
    let actor_id = TheaterId::generate();
    let (theater_tx, _theater_rx) = mpsc::channel(100);
    let (op_tx, _op_rx) = mpsc::channel(100);
    let actor_handle = ActorHandle::new(op_tx);
    
    let store = Arc::new(
        ActorStore::new(actor_id.clone(), theater_tx, actor_handle)
            .expect("Failed to create ActorStore")
    );

    // Add some initial events
    for i in 0..5 {
        let event_data = ChainEventData {
            event_type: format!("test_event_{}", i),
            data: EventData::Runtime(RuntimeEventData::Log {
                level: "info".to_string(),
                message: format!("Test event {}", i),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some(format!("Test event {}", i)),
        };
        store.record_event(event_data);
    }

    // Test concurrent reads while writing
    let store_clone = store.clone();
    let reader_handle = tokio::spawn(async move {
        for _ in 0..10 {
            // These should all work concurrently without blocking each other
            let all_events = store_clone.get_all_events();
            let recent_events = store_clone.get_recent_events(3);
            let by_type = store_clone.get_events_by_type("test_event_0");
            let verified = store_clone.verify_chain();
            let has_type = store_clone.has_event_type("test_event_1");
            
            assert!(all_events.len() >= 5);
            assert!(recent_events.len() <= 3);
            assert!(by_type.len() <= 1);
            assert!(verified);
            assert!(has_type);
            
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });

    // Concurrent writer
    let store_clone2 = store.clone();
    let writer_handle = tokio::spawn(async move {
        for i in 5..10 {
            let event_data = ChainEventData {
                event_type: format!("concurrent_event_{}", i),
                data: EventData::Runtime(RuntimeEventData::Log {
                    level: "info".to_string(),
                    message: format!("Concurrent event {}", i),
                }),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                description: Some(format!("Concurrent event {}", i)),
            };
            store_clone2.record_event(event_data);
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    });

    // Wait for both to complete
    let (_, _) = tokio::join!(reader_handle, writer_handle);

    // Final verification
    let final_events = store.get_all_events();
    assert!(final_events.len() >= 10);
    assert!(store.verify_chain());
    
    println!("   ✅ Concurrent access test passed with {} total events", final_events.len());
}

async fn test_query_methods() {
    println!("🔍 Testing new query methods...");
    
    // Create actor store
    let actor_id = TheaterId::generate();
    let (theater_tx, _theater_rx) = mpsc::channel(100);
    let (op_tx, _op_rx) = mpsc::channel(100);
    let actor_handle = ActorHandle::new(op_tx);
    
    let store = ActorStore::new(actor_id.clone(), theater_tx, actor_handle)
        .expect("Failed to create ActorStore");

    let now = chrono::Utc::now().timestamp_millis() as u64;

    // Add events of different types and times
    for i in 0..10 {
        let event_type = if i % 2 == 0 { "even_event" } else { "odd_event" };
        let event_data = ChainEventData {
            event_type: event_type.to_string(),
            data: EventData::Runtime(RuntimeEventData::Log {
                level: if i % 2 == 0 { "info" } else { "debug" }.to_string(),
                message: format!("Query test event {} of type {}", i, event_type),
            }),
            timestamp: now + (i * 1000), // Events spaced 1 second apart
            description: Some(format!("Event {} of type {}", i, event_type)),
        };
        store.record_event(event_data);
    }

    // Test get_events_by_type
    let even_events = store.get_events_by_type("even_event");
    let odd_events = store.get_events_by_type("odd_event");
    assert_eq!(even_events.len(), 5);
    assert_eq!(odd_events.len(), 5);

    // Test get_recent_events
    let recent_3 = store.get_recent_events(3);
    assert_eq!(recent_3.len(), 3);
    // Should be in reverse chronological order (most recent first)
    assert!(recent_3[0].timestamp >= recent_3[1].timestamp);
    assert!(recent_3[1].timestamp >= recent_3[2].timestamp);

    // Test get_events_since
    let since_5th_event = store.get_events_since(now + 5000);
    assert_eq!(since_5th_event.len(), 4); // Events 6,7,8,9 (4 events after timestamp)

    // Test has_event_type
    assert!(store.has_event_type("even_event"));
    assert!(store.has_event_type("odd_event"));
    assert!(!store.has_event_type("nonexistent_event"));

    println!("   ✅ Query methods test passed - found {} even events, {} odd events", 
            even_events.len(), odd_events.len());
}

async fn test_no_deadlock() {
    println!("🔒 Testing deadlock prevention...");
    
    // Test that multiple readers don't deadlock when a writer is waiting
    let actor_id = TheaterId::generate();
    let (theater_tx, _theater_rx) = mpsc::channel(100);
    let (op_tx, _op_rx) = mpsc::channel(100);
    let actor_handle = ActorHandle::new(op_tx);
    
    let store = Arc::new(
        ActorStore::new(actor_id.clone(), theater_tx, actor_handle)
            .expect("Failed to create ActorStore")
    );

    // Add initial event
    let initial_event = ChainEventData {
        event_type: "initial".to_string(),
        data: EventData::Runtime(RuntimeEventData::Log {
            level: "info".to_string(),
            message: "Initial test event".to_string(),
        }),
        timestamp: chrono::Utc::now().timestamp_millis() as u64,
        description: Some("Initial event".to_string()),
    };
    store.record_event(initial_event);

    // Spawn multiple concurrent readers
    let mut reader_handles = Vec::new();
    for i in 0..5 {
        let store_clone = store.clone();
        let handle = tokio::spawn(async move {
            for _ in 0..20 {
                let _events = store_clone.get_all_events();
                let _verified = store_clone.verify_chain();
                let _recent = store_clone.get_recent_events(1);
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            i
        });
        reader_handles.push(handle);
    }

    // Spawn a writer that runs concurrently
    let store_clone = store.clone();
    let writer_handle = tokio::spawn(async move {
        for i in 0..10 {
            let event_data = ChainEventData {
                event_type: format!("writer_event_{}", i),
                data: EventData::Runtime(RuntimeEventData::Log {
                    level: "debug".to_string(),
                    message: format!("Writer event {}", i),
                }),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                description: Some(format!("Writer event {}", i)),
            };
            store_clone.record_event(event_data);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

    // Wait for all tasks to complete (should not deadlock)
    let timeout_duration = Duration::from_secs(10);
    let result = tokio::time::timeout(timeout_duration, async {
        for handle in reader_handles {
            handle.await.expect("Reader task failed");
        }
        writer_handle.await.expect("Writer task failed");
    }).await;

    assert!(result.is_ok(), "Test timed out - possible deadlock detected!");
    
    // Verify final state
    let final_events = store.get_all_events();
    assert_eq!(final_events.len(), 11); // 1 initial + 10 writer events
    assert!(store.verify_chain());
    
    println!("   ✅ No deadlock test passed - {} events in final chain", final_events.len());
}
