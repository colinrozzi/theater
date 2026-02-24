use anyhow::Result;
use clap::Parser;
use tokio::sync::mpsc;
use tracing::debug;

use crate::{error::CliError, CommandContext};
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::pack_bridge::{Value, ValueType};
use theater::theater_runtime::TheaterRuntime;
use theater::utils::resolve_reference;
use theater::ManifestConfig;
use theater_handler_runtime::RuntimeHandler;
use theater::config::actor_manifest::RuntimeHostConfig;

#[derive(Debug, Parser)]
pub struct RunArgs {
    /// Path to the actor manifest file
    #[arg(default_value = "manifest.toml")]
    pub manifest: String,

    /// Function to call (e.g., "theater:simple/task-manager.add-task")
    #[arg(short, long)]
    pub function: Option<String>,

    /// Input as JSON (e.g., '["Buy groceries"]' for a tuple with one string)
    #[arg(short, long, default_value = "[]")]
    pub input: String,

    /// Initial state as JSON string or path to JSON file
    #[arg(short, long)]
    pub state: Option<String>,

    /// Show raw Value output instead of JSON
    #[arg(long)]
    pub raw: bool,
}

/// Create a minimal handler registry with just runtime for logging
fn create_minimal_registry(
    theater_tx: mpsc::Sender<TheaterCommand>,
) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();

    // Runtime handler - provides log, get-chain, shutdown
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, theater_tx.clone(), None));

    registry
}

/// Parse a JSON array into a Value::Tuple
fn json_to_value(json_str: &str) -> Result<Value, CliError> {
    let parsed: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| CliError::server_error(format!("Invalid JSON input: {}", e)))?;

    json_value_to_pack_value(&parsed)
}

/// Convert serde_json::Value to pack Value
fn json_value_to_pack_value(json: &serde_json::Value) -> Result<Value, CliError> {
    match json {
        serde_json::Value::Null => Ok(Value::Option {
            inner_type: ValueType::String,
            value: None,
        }),
        serde_json::Value::Bool(b) => Ok(Value::Bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                    Ok(Value::S32(i as i32))
                } else {
                    Ok(Value::S64(i))
                }
            } else if let Some(f) = n.as_f64() {
                Ok(Value::F64(f))
            } else {
                Err(CliError::server_error("Invalid number in JSON"))
            }
        }
        serde_json::Value::String(s) => Ok(Value::String(s.clone())),
        serde_json::Value::Array(arr) => {
            let items: Result<Vec<Value>, _> = arr.iter().map(json_value_to_pack_value).collect();
            Ok(Value::Tuple(items?))
        }
        serde_json::Value::Object(obj) => {
            let fields: Result<Vec<(String, Value)>, _> = obj
                .iter()
                .map(|(k, v)| json_value_to_pack_value(v).map(|val| (k.clone(), val)))
                .collect();
            Ok(Value::Record {
                type_name: String::new(),
                fields: fields?,
            })
        }
    }
}

/// Convert pack Value to serde_json::Value for output
fn pack_value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::U8(n) => serde_json::Value::Number((*n).into()),
        Value::U16(n) => serde_json::Value::Number((*n).into()),
        Value::U32(n) => serde_json::Value::Number((*n).into()),
        Value::U64(n) => serde_json::Value::Number((*n).into()),
        Value::S8(n) => serde_json::Value::Number((*n).into()),
        Value::S16(n) => serde_json::Value::Number((*n).into()),
        Value::S32(n) => serde_json::Value::Number((*n).into()),
        Value::S64(n) => serde_json::Value::Number((*n).into()),
        Value::F32(n) => serde_json::json!(*n),
        Value::F64(n) => serde_json::json!(*n),
        Value::Char(c) => serde_json::Value::String(c.to_string()),
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::List { items, .. } => {
            serde_json::Value::Array(items.iter().map(pack_value_to_json).collect())
        }
        Value::Record { fields, .. } => {
            let obj: serde_json::Map<String, serde_json::Value> = fields
                .iter()
                .map(|(k, v)| (k.clone(), pack_value_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
        Value::Tuple(items) => {
            serde_json::Value::Array(items.iter().map(pack_value_to_json).collect())
        }
        Value::Variant { case_name, payload, .. } => {
            let mut obj = serde_json::Map::new();
            obj.insert("case".to_string(), serde_json::Value::String(case_name.clone()));
            if !payload.is_empty() {
                let payload_json: Vec<serde_json::Value> = payload.iter().map(pack_value_to_json).collect();
                if payload_json.len() == 1 {
                    obj.insert("payload".to_string(), payload_json.into_iter().next().unwrap());
                } else {
                    obj.insert("payload".to_string(), serde_json::Value::Array(payload_json));
                }
            }
            serde_json::Value::Object(obj)
        }
        Value::Option { value, .. } => match value {
            Some(v) => pack_value_to_json(v),
            None => serde_json::Value::Null,
        },
        Value::Result { value, .. } => match value {
            Ok(v) => {
                let mut obj = serde_json::Map::new();
                obj.insert("ok".to_string(), pack_value_to_json(v));
                serde_json::Value::Object(obj)
            }
            Err(e) => {
                let mut obj = serde_json::Map::new();
                obj.insert("err".to_string(), pack_value_to_json(e));
                serde_json::Value::Object(obj)
            }
        }
        Value::Flags(bits) => {
            serde_json::Value::Number((*bits).into())
        }
    }
}

/// Execute the run command - load actor, call function, print result, exit
pub async fn execute_async(args: &RunArgs, _ctx: &CommandContext) -> Result<(), CliError> {
    debug!("Running actor from manifest: {}", args.manifest);

    // Resolve the manifest reference
    let manifest_bytes = resolve_reference(&args.manifest).await.map_err(|e| {
        CliError::invalid_manifest(format!(
            "Failed to resolve manifest reference '{}': {}",
            args.manifest, e
        ))
    })?;

    let manifest_content = String::from_utf8(manifest_bytes).map_err(|e| {
        CliError::invalid_manifest(format!("Manifest content is not valid UTF-8: {}", e))
    })?;

    // Create the TheaterRuntime in-process
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);
    let handler_registry = create_minimal_registry(theater_tx.clone());

    let mut runtime = TheaterRuntime::new(
        theater_tx.clone(),
        theater_rx,
        None,
        handler_registry,
    )
    .await
    .map_err(|e| CliError::server_error(format!("Failed to create runtime: {}", e)))?;

    // Spawn the runtime event loop
    let _runtime_handle = tokio::spawn(async move {
        let _ = runtime.run().await;
    });

    // Parse the manifest
    let manifest = ManifestConfig::from_toml_str(&manifest_content).map_err(|e| {
        CliError::invalid_manifest(format!("Failed to parse manifest: {}", e))
    })?;

    // Resolve WASM path relative to manifest directory
    let wasm_path = if manifest.package.starts_with('/') || manifest.package.contains("://") {
        // Absolute path or URL - use as is
        manifest.package.clone()
    } else {
        // Relative path - resolve relative to manifest's directory
        let manifest_path = std::path::Path::new(&args.manifest);
        if let Some(manifest_dir) = manifest_path.parent() {
            manifest_dir.join(&manifest.package).to_string_lossy().to_string()
        } else {
            manifest.package.clone()
        }
    };

    // Load WASM bytes
    let wasm_bytes = resolve_reference(&wasm_path).await.map_err(|e| {
        CliError::server_error(format!(
            "Failed to load WASM from '{}': {}",
            wasm_path, e
        ))
    })?;

    // Spawn the actor
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    let (supervisor_tx, _supervisor_rx) = mpsc::channel(32);

    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes,
            name: Some(manifest.name.clone()),
            manifest: Some(manifest),
            response_tx,
            parent_id: None,
            supervisor_tx: Some(supervisor_tx),
            subscription_tx: None,
        })
        .await
        .map_err(|e| CliError::server_error(format!("Failed to send spawn command: {}", e)))?;

    // Wait for the actor to start
    let actor_id = match response_rx.await {
        Ok(Ok(id)) => {
            debug!("Actor started: {}", id);
            id
        }
        Ok(Err(e)) => {
            return Err(CliError::server_error(format!(
                "Failed to start actor: {}",
                e
            )));
        }
        Err(e) => {
            return Err(CliError::server_error(format!(
                "Failed to receive spawn response: {}",
                e
            )));
        }
    };

    // Get the actor handle
    let (handle_tx, handle_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorHandle {
            actor_id: actor_id.clone(),
            response_tx: handle_tx,
        })
        .await
        .map_err(|e| CliError::server_error(format!("Failed to get actor handle: {}", e)))?;

    let actor_handle = match handle_rx.await {
        Ok(Some(handle)) => handle,
        Ok(None) => {
            return Err(CliError::server_error("Actor handle not found".to_string()));
        }
        Err(e) => {
            return Err(CliError::server_error(format!(
                "Failed to receive actor handle: {}",
                e
            )));
        }
    };

    // Build init state
    let init_state = if let Some(ref state_arg) = args.state {
        let state_bytes = if std::path::Path::new(state_arg).exists() {
            std::fs::read(state_arg).map_err(|e| {
                CliError::file_operation_failed("read initial state", state_arg.clone(), e)
            })?
        } else {
            state_arg.as_bytes().to_vec()
        };
        Value::Option {
            inner_type: ValueType::List(Box::new(ValueType::U8)),
            value: Some(Box::new(Value::List {
                elem_type: ValueType::U8,
                items: state_bytes.into_iter().map(Value::U8).collect(),
            })),
        }
    } else {
        Value::Option {
            inner_type: ValueType::List(Box::new(ValueType::U8)),
            value: None,
        }
    };

    // Call init - the runtime manages state internally
    // Init params: tuple containing the initial state option
    let init_params = Value::Tuple(vec![init_state]);
    debug!("Calling init on actor {}", actor_id);
    let init_result = actor_handle
        .call_function("theater:simple/actor.init".to_string(), init_params)
        .await
        .map_err(|e| CliError::server_error(format!("Failed to call init: {:?}", e)))?;
    debug!("Init completed: {:?}", init_result);

    // If no function specified, just print init result and exit
    if args.function.is_none() {
        if args.raw {
            println!("{:?}", init_result);
        } else {
            let json = pack_value_to_json(&init_result);
            println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
        }
        return Ok(());
    }

    // Parse the input JSON into a Value (wrapped as params tuple)
    let input_value = json_to_value(&args.input)?;

    // Call the specified function
    // The runtime passes state internally; we just send the params
    let function_name = args.function.as_ref().unwrap();
    debug!("Calling function: {}", function_name);

    let result = actor_handle
        .call_function(function_name.clone(), input_value)
        .await
        .map_err(|e| CliError::server_error(format!("Failed to call {}: {:?}", function_name, e)))?;

    // Print the result (this is the function's output value, state is managed internally)
    if args.raw {
        println!("{:?}", result);
    } else {
        let json = pack_value_to_json(&result);
        println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
    }

    Ok(())
}
