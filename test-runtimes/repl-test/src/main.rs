//! REPL Actor Integration Test
//!
//! This program tests the REPL actor by loading it with the Composite runtime
//! and sending eval requests.

use anyhow::{Context, Result};
use composite::abi::Value;
use composite::{AsyncRuntime, HostLinkerBuilder, LinkerError};
use std::sync::{Arc, Mutex};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

/// Simple store for the actor (minimal - just for testing)
#[derive(Clone, Default)]
struct TestStore {
    logs: Arc<Mutex<Vec<String>>>,
}

fn main() -> Result<()> {
    // Set up logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // Run the async tests
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(run_tests())
}

async fn run_tests() -> Result<()> {
    info!("Loading REPL actor...");

    // Load the WASM bytes
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/repl-actor/target/wasm32-unknown-unknown/release/repl_actor.wasm"
    );
    let wasm_bytes = std::fs::read(wasm_path)
        .context(format!("Failed to read WASM file: {}", wasm_path))?;

    info!("WASM loaded: {} bytes", wasm_bytes.len());

    // Create the Composite runtime
    let runtime = AsyncRuntime::new();

    // Load and instantiate the module
    let module = runtime
        .load_module(&wasm_bytes)
        .context("Failed to load WASM module")?;

    let store = TestStore::default();
    let logs = store.logs.clone();

    let mut instance = module
        .instantiate_with_host_async(store, |builder: &mut HostLinkerBuilder<'_, TestStore>| {
            setup_host_functions(builder)
        })
        .await
        .context("Failed to instantiate module")?;

    info!("Module instantiated!");

    // Test 1: Call init
    info!("\n=== Test 1: init ===");
    let init_input = Value::Option(None); // No initial state
    let init_result = instance
        .call_with_value_async("theater:simple/actor.init", &init_input, 0)
        .await
        .context("Failed to call init")?;
    info!("init result: {:?}", init_result);
    print_logs(&logs);

    // Test 2: Simple arithmetic
    info!("\n=== Test 2: (+ 1 2 3) ===");
    let result = call_eval(&mut instance, "(+ 1 2 3)").await?;
    info!("Result: {}", format_result(&result));
    print_logs(&logs);

    // Test 3: Nested arithmetic
    info!("\n=== Test 3: (* 2 (+ 3 4)) ===");
    let result = call_eval(&mut instance, "(* 2 (+ 3 4))").await?;
    info!("Result: {}", format_result(&result));
    print_logs(&logs);

    // Test 4a: Simple number
    info!("\n=== Test 4a: 42 ===");
    let result = call_eval(&mut instance, "42").await?;
    info!("Result: {}", format_result(&result));
    print_logs(&logs);

    // Test 4b: Just list
    info!("\n=== Test 4b: (list 1 2 3) ===");
    let result = call_eval(&mut instance, "(list 1 2 3)").await?;
    info!("Result: {}", format_result(&result));
    print_logs(&logs);

    // Test 4: List operations
    info!("\n=== Test 4: (car (list 1 2 3)) ===");
    let result = call_eval(&mut instance, "(car (list 1 2 3))").await?;
    info!("Result: {}", format_result(&result));
    print_logs(&logs);

    // Test 5: Comparison
    info!("\n=== Test 5: (< 1 2) ===");
    let result = call_eval(&mut instance, "(< 1 2)").await?;
    info!("Result: {}", format_result(&result));
    print_logs(&logs);

    // Test 6: Conditionals
    info!("\n=== Test 6: (if (< 1 2) 100 200) ===");
    let result = call_eval(&mut instance, "(if (< 1 2) 100 200)").await?;
    info!("Result: {}", format_result(&result));
    print_logs(&logs);

    // Test 7: Quote
    info!("\n=== Test 7: (quote (a b c)) ===");
    let result = call_eval(&mut instance, "(quote (a b c))").await?;
    info!("Result: {}", format_result(&result));
    print_logs(&logs);

    // Test 8: Quote shorthand
    info!("\n=== Test 8: '(hello world) ===");
    let result = call_eval(&mut instance, "'(hello world)").await?;
    info!("Result: {}", format_result(&result));
    print_logs(&logs);

    // Test 9: Cons
    info!("\n=== Test 9: (cons 1 (list 2 3)) ===");
    let result = call_eval(&mut instance, "(cons 1 (list 2 3))").await?;
    info!("Result: {}", format_result(&result));
    print_logs(&logs);

    // Test 10: Type predicates
    info!("\n=== Test 10: (num? 42) ===");
    let result = call_eval(&mut instance, "(num? 42)").await?;
    info!("Result: {}", format_result(&result));
    print_logs(&logs);

    // Test 11: Error case - unknown function
    info!("\n=== Test 11: (unknown-fn 1 2) ===");
    let result = call_eval(&mut instance, "(unknown-fn 1 2)").await?;
    info!("Result: {}", format_result(&result));
    print_logs(&logs);

    // Test 12: Floats
    info!("\n=== Test 12: (+ 1.5 2.5) ===");
    let result = call_eval(&mut instance, "(+ 1.5 2.5)").await?;
    info!("Result: {}", format_result(&result));
    print_logs(&logs);

    // Test 13: String
    info!("\n=== Test 13: \"hello world\" ===");
    let result = call_eval(&mut instance, "\"hello world\"").await?;
    info!("Result: {}", format_result(&result));
    print_logs(&logs);

    info!("\n=== All tests completed! ===");
    Ok(())
}

/// Set up host functions for the actor
fn setup_host_functions(
    builder: &mut HostLinkerBuilder<'_, TestStore>,
) -> Result<(), LinkerError> {
    builder
        .interface("theater:simple/runtime")?
        .func_typed("log", |ctx: &mut composite::Ctx<'_, TestStore>, input: Value| {
            if let Value::String(msg) = input {
                println!("  [ACTOR LOG] {}", msg);
                ctx.data().logs.lock().unwrap().push(msg);
            }
            Value::Tuple(vec![])
        })?
        .func_typed("get-chain", |_ctx: &mut composite::Ctx<'_, TestStore>, _input: Value| {
            // Return empty chain for testing
            Value::Record(vec![("events".to_string(), Value::List(vec![]))])
        })?
        .func_typed("shutdown", |_ctx: &mut composite::Ctx<'_, TestStore>, _input: Value| {
            // Return Ok(())
            Value::Variant {
                tag: 0,
                payload: Some(Box::new(Value::Tuple(vec![]))),
            }
        })?;

    Ok(())
}

/// Call the eval function with a string input
async fn call_eval(
    instance: &mut composite::AsyncInstance<TestStore>,
    input: &str,
) -> Result<Value> {
    let input_value = Value::String(input.to_string());
    instance
        .call_with_value_async("theater:simple/repl.eval", &input_value, 0)
        .await
        .context(format!("Failed to call eval with input: {}", input))
}

/// Format a result Value for display
fn format_result(value: &Value) -> String {
    match value {
        // eval-result variant: tag 0 = ok(sexpr), tag 1 = err(string)
        Value::Variant { tag: 0, payload: Some(sexpr) } => {
            format!("Ok({})", format_sexpr(sexpr))
        }
        Value::Variant { tag: 1, payload: Some(err) } => {
            if let Value::String(msg) = err.as_ref() {
                format!("Err({})", msg)
            } else {
                format!("Err({:?})", err)
            }
        }
        other => format!("{:?}", other),
    }
}

/// Format an SExpr Value for display
fn format_sexpr(value: &Value) -> String {
    match value {
        // sexpr variant tags: 0=sym, 1=num, 2=flt, 3=str, 4=list, 5=nil
        Value::Variant { tag: 0, payload: Some(p) } => {
            if let Value::String(s) = p.as_ref() {
                s.clone()
            } else {
                format!("{:?}", p)
            }
        }
        Value::Variant { tag: 1, payload: Some(p) } => {
            if let Value::S64(n) = p.as_ref() {
                format!("{}", n)
            } else {
                format!("{:?}", p)
            }
        }
        Value::Variant { tag: 2, payload: Some(p) } => {
            if let Value::F64(f) = p.as_ref() {
                format!("{}", f)
            } else {
                format!("{:?}", p)
            }
        }
        Value::Variant { tag: 3, payload: Some(p) } => {
            if let Value::String(s) = p.as_ref() {
                format!("\"{}\"", s)
            } else {
                format!("{:?}", p)
            }
        }
        Value::Variant { tag: 4, payload: Some(p) } => {
            if let Value::List(items) = p.as_ref() {
                let formatted: Vec<String> = items.iter().map(format_sexpr).collect();
                format!("({})", formatted.join(" "))
            } else {
                format!("{:?}", p)
            }
        }
        Value::Variant { tag: 5, payload: None } => "nil".to_string(),
        other => format!("{:?}", other),
    }
}

/// Print and clear accumulated logs
fn print_logs(logs: &Arc<Mutex<Vec<String>>>) {
    let mut logs = logs.lock().unwrap();
    logs.clear();
}
