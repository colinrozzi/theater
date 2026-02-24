//! Test for the task-manager actor.
//!
//! This test verifies that the task-manager actor works correctly:
//! - init creates empty state
//! - add-task creates tasks and returns IDs
//! - complete-task marks tasks as completed
//! - list-tasks returns all tasks

use std::sync::Arc;
use std::sync::RwLock as SyncRwLock;
use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::chain::StateChain;
use theater::pack_bridge::{ActorResult, AsyncRuntime, Ctx, PackInstance, Value};
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use tokio::sync::mpsc;
use tracing::info;

#[tokio::test]
async fn test_task_manager() {
    // Initialize tracing for test output
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    // Load the task-manager actor WASM
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../actors/task-manager/target/wasm32-unknown-unknown/release/task_manager.wasm"
    );

    let wasm_bytes = match std::fs::read(wasm_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            panic!(
                "Failed to read task-manager WASM from {}: {}. \
                 Make sure to build it first with: \
                 cd ../actors/task-manager && cargo build --release",
                wasm_path, e
            );
        }
    };

    info!("Loaded WASM bytes: {} bytes", wasm_bytes.len());

    // Create the runtime
    let runtime = AsyncRuntime::new();

    // Create actor store components
    let actor_id = TheaterId::generate();
    let (theater_tx, _theater_rx) = mpsc::channel::<TheaterCommand>(100);
    let (operation_tx, _operation_rx) = mpsc::channel(10);
    let (info_tx, _info_rx) = mpsc::channel(10);
    let (control_tx, _control_rx) = mpsc::channel(10);
    let chain = Arc::new(SyncRwLock::new(StateChain::new(actor_id.clone(), theater_tx.clone())));
    let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);

    let actor_store = ActorStore::new(
        actor_id.clone(),
        theater_tx.clone(),
        actor_handle,
        chain.clone(),
        None, // No initial state
    );

    // Create the PackInstance with host functions
    let result = PackInstance::new(
        "task-manager",
        &wasm_bytes,
        &runtime,
        actor_store,
        |builder| {
            // Register the log host function
            builder
                .interface("theater:simple/runtime")?
                .func_typed("log", |_ctx: &mut Ctx<'_, ActorStore>, input: Value| {
                    // Extract the message from the Value
                    let msg = match input {
                        Value::String(s) => s,
                        _ => format!("{:?}", input),
                    };
                    info!("[ACTOR LOG] {}", msg);
                    // Return unit (empty tuple)
                    Value::Tuple(vec![])
                })?;
            Ok(())
        },
    )
    .await;

    let mut instance = match result {
        Ok(inst) => inst,
        Err(e) => {
            panic!("Failed to create PackInstance: {}", e);
        }
    };

    info!("PackInstance created successfully");

    // Step 1: Call init
    info!("=== Calling init ===");
    let result: ActorResult<()> = instance
        .call_typed("theater:simple/actor.init", None, Value::Tuple(vec![]))
        .await
        .expect("init should succeed");

    info!("init succeeded");
    assert!(result.state.is_some(), "init should return state");
    let state = result.state;

    // Step 2: Add first task
    info!("=== Adding task 'Buy groceries' ===");
    let result: ActorResult<i32> = instance
        .call_typed(
            "theater:simple/task-manager.add-task",
            state,
            Value::Tuple(vec![Value::String("Buy groceries".to_string())]),
        )
        .await
        .expect("add-task should succeed");

    info!("add-task returned id = {}", result.value);
    assert_eq!(result.value, 0, "First task should have id 0");
    let state = result.state;

    // Step 3: Add second task
    info!("=== Adding task 'Walk the dog' ===");
    let result: ActorResult<i32> = instance
        .call_typed(
            "theater:simple/task-manager.add-task",
            state,
            Value::Tuple(vec![Value::String("Walk the dog".to_string())]),
        )
        .await
        .expect("add-task should succeed");

    info!("add-task returned id = {}", result.value);
    assert_eq!(result.value, 1, "Second task should have id 1");
    let state = result.state;

    // Step 4: Add third task
    info!("=== Adding task 'Write code' ===");
    let result: ActorResult<i32> = instance
        .call_typed(
            "theater:simple/task-manager.add-task",
            state,
            Value::Tuple(vec![Value::String("Write code".to_string())]),
        )
        .await
        .expect("add-task should succeed");

    info!("add-task returned id = {}", result.value);
    assert_eq!(result.value, 2, "Third task should have id 2");
    let state = result.state;

    // Step 5: Complete the second task
    info!("=== Completing task 1 ===");
    let result: ActorResult<bool> = instance
        .call_typed(
            "theater:simple/task-manager.complete-task",
            state,
            Value::Tuple(vec![Value::S32(1)]),
        )
        .await
        .expect("complete-task should succeed");

    info!("complete-task returned found = {}", result.value);
    assert!(result.value, "Task 1 should be found and completed");
    let state = result.state;

    // Step 6: Try to complete a non-existent task
    info!("=== Completing task 99 (should not exist) ===");
    let result: ActorResult<bool> = instance
        .call_typed(
            "theater:simple/task-manager.complete-task",
            state,
            Value::Tuple(vec![Value::S32(99)]),
        )
        .await
        .expect("complete-task should succeed");

    info!("complete-task returned found = {}", result.value);
    assert!(!result.value, "Task 99 should not be found");
    let state = result.state;

    // Step 7: List all tasks
    info!("=== Listing tasks ===");
    let result: ActorResult<Vec<u8>> = instance
        .call_typed(
            "theater:simple/task-manager.list-tasks",
            state,
            Value::Tuple(vec![]),
        )
        .await
        .expect("list-tasks should succeed");

    // Parse the JSON response
    let tasks_json = String::from_utf8_lossy(&result.value);
    info!("list-tasks returned: {}", tasks_json);

    // Verify we have 3 tasks
    let tasks: serde_json::Value = serde_json::from_slice(&result.value)
        .expect("Should parse as JSON");

    let tasks_array = tasks.as_array().expect("Should be an array");
    assert_eq!(tasks_array.len(), 3, "Should have 3 tasks");

    // Verify task details
    assert_eq!(tasks_array[0]["id"], 0);
    assert_eq!(tasks_array[0]["title"], "Buy groceries");
    assert_eq!(tasks_array[0]["completed"], false);

    assert_eq!(tasks_array[1]["id"], 1);
    assert_eq!(tasks_array[1]["title"], "Walk the dog");
    assert_eq!(tasks_array[1]["completed"], true); // This one was completed

    assert_eq!(tasks_array[2]["id"], 2);
    assert_eq!(tasks_array[2]["title"], "Write code");
    assert_eq!(tasks_array[2]["completed"], false);

    info!("=== Task manager test passed! ===");
    info!("Created 3 tasks, completed 1, verified state correctly persisted");
}
