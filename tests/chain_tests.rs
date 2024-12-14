use chrono::Utc;
use pretty_assertions::assert_eq;
use serde_json::json;
use theater::chain::{ChainEvent, HashChain};
use theater::{ActorInput, ActorOutput};

// [Previous tests remain unchanged...]

#[test]
fn test_error_handling() {
    let mut chain = HashChain::new();

    // Initial request
    chain.add_event(ChainEvent::Input {
        input: ActorInput::HttpRequest {
            method: "GET".to_string(),
            uri: "/api/users/invalid-id".to_string(),
            headers: vec![],
            body: None,
        },
        timestamp: Utc::now(),
    });

    // State change to processing
    chain.add_event(ChainEvent::StateChange {
        old_state: json!(null),
        new_state: json!({
            "status": "processing",
            "request_id": "req456",
            "method": "GET",
            "path": "/api/users/invalid-id"
        }),
        timestamp: Utc::now(),
    });

    // Message to database service
    chain.add_event(ChainEvent::MessageSent {
        target_actor: "database".to_string(),
        target_chain_state: "ready".to_string(),
        source_chain_state: "processing".to_string(),
        payload: json!({
            "operation": "find",
            "table": "users",
            "id": "invalid-id"
        }),
        timestamp: Utc::now(),
    });

    // Error response from database
    chain.add_event(ChainEvent::MessageReceived {
        source_actor: "database".to_string(),
        source_chain_state: "error".to_string(),
        our_chain_state: "processing".to_string(),
        payload: json!({
            "error": "not_found",
            "message": "User not found"
        }),
        timestamp: Utc::now(),
    });

    // State change to error
    chain.add_event(ChainEvent::StateChange {
        old_state: chain.get_current_state().unwrap(),
        new_state: json!({
            "status": "error",
            "request_id": "req456",
            "method": "GET",
            "path": "/api/users/invalid-id",
            "error": {
                "type": "not_found",
                "message": "User not found"
            }
        }),
        timestamp: Utc::now(),
    });

    // Error response to client
    chain.add_event(ChainEvent::Output {
        output: ActorOutput::HttpResponse {
            status: 404,
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: Some(br#"{"error":"not_found","message":"User not found"}"#.to_vec()),
        },
        chain_state: "error".to_string(),
        timestamp: Utc::now(),
    });

    let full_chain = chain.get_full_chain();
    assert_eq!(full_chain.len(), 6);

    // Verify error state
    let final_state = chain.get_current_state().unwrap();
    assert_eq!(final_state["status"], "error");
    assert_eq!(final_state["error"]["type"], "not_found");
}

#[test]
fn test_interleaved_requests() {
    let mut chain = HashChain::new();

    // Request 1 starts
    chain.add_event(ChainEvent::Input {
        input: ActorInput::Message(json!({"request_id": "1", "action": "start"})),
        timestamp: Utc::now(),
    });

    // Request 2 starts
    chain.add_event(ChainEvent::Input {
        input: ActorInput::Message(json!({"request_id": "2", "action": "start"})),
        timestamp: Utc::now(),
    });

    // State tracks both requests
    chain.add_event(ChainEvent::StateChange {
        old_state: json!(null),
        new_state: json!({
            "requests": {
                "1": {"status": "processing"},
                "2": {"status": "processing"}
            }
        }),
        timestamp: Utc::now(),
    });

    // Request 1 completes
    chain.add_event(ChainEvent::Output {
        output: ActorOutput::Message(json!({"request_id": "1", "status": "completed"})),
        chain_state: "processing".to_string(),
        timestamp: Utc::now(),
    });

    // State updated for request 1
    chain.add_event(ChainEvent::StateChange {
        old_state: chain.get_current_state().unwrap(),
        new_state: json!({
            "requests": {
                "1": {"status": "completed"},
                "2": {"status": "processing"}
            }
        }),
        timestamp: Utc::now(),
    });

    // Request 2 completes
    chain.add_event(ChainEvent::Output {
        output: ActorOutput::Message(json!({"request_id": "2", "status": "completed"})),
        chain_state: "processing".to_string(),
        timestamp: Utc::now(),
    });

    // Final state
    chain.add_event(ChainEvent::StateChange {
        old_state: chain.get_current_state().unwrap(),
        new_state: json!({
            "requests": {
                "1": {"status": "completed"},
                "2": {"status": "completed"}
            }
        }),
        timestamp: Utc::now(),
    });

    let full_chain = chain.get_full_chain();
    assert_eq!(full_chain.len(), 7);

    let final_state = chain.get_current_state().unwrap();
    assert_eq!(final_state["requests"]["1"]["status"], "completed");
    assert_eq!(final_state["requests"]["2"]["status"], "completed");
}

#[test]
fn test_state_rollback() {
    let mut chain = HashChain::new();

    // Initial state
    chain.add_event(ChainEvent::StateChange {
        old_state: json!(null),
        new_state: json!({
            "counter": 0,
            "status": "ready"
        }),
        timestamp: Utc::now(),
    });

    // Start operation
    chain.add_event(ChainEvent::StateChange {
        old_state: chain.get_current_state().unwrap(),
        new_state: json!({
            "counter": 1,
            "status": "processing"
        }),
        timestamp: Utc::now(),
    });

    // Error occurs
    chain.add_event(ChainEvent::StateChange {
        old_state: chain.get_current_state().unwrap(),
        new_state: json!({
            "counter": 0,
            "status": "ready",
            "last_error": "Operation failed"
        }),
        timestamp: Utc::now(),
    });

    let final_state = chain.get_current_state().unwrap();
    assert_eq!(final_state["counter"], 0);
    assert_eq!(final_state["status"], "ready");
    assert!(final_state.get("last_error").is_some());
}

#[test]
fn test_chain_integrity() {
    let mut chain = HashChain::new();

    // Add a sequence of events
    let hash1 = chain.add_event(ChainEvent::StateChange {
        old_state: json!(null),
        new_state: json!({"value": 1}),
        timestamp: Utc::now(),
    });

    let hash2 = chain.add_event(ChainEvent::StateChange {
        old_state: json!({"value": 1}),
        new_state: json!({"value": 2}),
        timestamp: Utc::now(),
    });

    let hash3 = chain.add_event(ChainEvent::StateChange {
        old_state: json!({"value": 2}),
        new_state: json!({"value": 3}),
        timestamp: Utc::now(),
    });

    // Verify chain linkage
    let full_chain = chain.get_full_chain();

    // Most recent event should have previous event as parent
    assert_eq!(full_chain[0].1.parent, Some(hash2));

    // Middle event should have first event as parent
    assert_eq!(full_chain[1].1.parent, Some(hash1));

    // First event should have no parent
    assert_eq!(full_chain[2].1.parent, None);

    // Head should point to most recent event
    assert_eq!(chain.get_head(), Some(hash3.as_str()));
}
