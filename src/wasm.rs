use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fmt::Debug;
use thiserror::Error;
use tokio::sync::mpsc::Sender;
use wasmtime::component::{Component, ComponentExportIndex, ComponentType, Lift, Lower, Linker};
use wasmtime::{Engine, Store};

use crate::config::ManifestConfig;
use crate::messages::TheaterCommand;
use crate::store::ActorStore;
use tracing::{error, info};

// [Rest of the file remains the same]
