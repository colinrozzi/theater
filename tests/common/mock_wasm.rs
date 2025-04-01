use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use theater::actor_store::ActorStore;
use theater::chain::ChainEvent;
use theater::config::ManifestConfig;
use theater::id::TheaterId;
use theater::metrics::ActorMetrics;
use wasmtime::component::{Component, ComponentType, Lift, Lower};

/// A mock implementation of the WASM component for testing
pub struct MockActorComponent {
    store: Arc<Mutex<ActorStore>>,
    function_results: HashMap<String, Vec<u8>>,
    metrics: ActorMetrics,
    should_fail_instantiate: bool,
    fail_functions: Vec<String>,
}

impl MockActorComponent {
    pub fn new(store: ActorStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            function_results: HashMap::new(),
            metrics: ActorMetrics::default(),
            should_fail_instantiate: false,
            fail_functions: Vec::new(),
        }
    }

    /// Configure the component to fail when instantiated
    pub fn fail_on_instantiate(mut self) -> Self {
        self.should_fail_instantiate = true;
        self
    }

    /// Set predefined results for specific function calls
    pub fn with_function_results(mut self, results: HashMap<String, Vec<u8>>) -> Self {
        self.function_results = results;
        self
    }

    /// Set specific functions to fail when called
    pub fn with_failing_functions(mut self, functions: Vec<String>) -> Self {
        self.fail_functions = functions;
        self
    }

    /// Set the metrics that will be reported
    pub fn with_metrics(mut self, metrics: ActorMetrics) -> Self {
        self.metrics = metrics;
        self
    }

    /// Return the actor store
    pub fn get_store(&self) -> Arc<Mutex<ActorStore>> {
        self.store.clone()
    }

    /// Create a mock instance
    pub async fn instantiate(&self) -> Result<MockActorInstance> {
        if self.should_fail_instantiate {
            return Err(anyhow::anyhow!("Mock component failed to instantiate"));
        }

        Ok(MockActorInstance {
            store: self.store.clone(),
            function_results: self.function_results.clone(),
            metrics: self.metrics.clone(),
            fail_functions: self.fail_functions.clone(),
        })
    }
}

/// A mock WASM instance for testing
pub struct MockActorInstance {
    store: Arc<Mutex<ActorStore>>,
    function_results: HashMap<String, Vec<u8>>,
    metrics: ActorMetrics,
    fail_functions: Vec<String>,
}

impl MockActorInstance {
    /// Call a mock function with specific parameters
    pub async fn call_function<P, R>(&self, name: &str, params: &P) -> Result<R>
    where
        P: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        // Check if this function should fail
        if self.fail_functions.contains(&name.to_string()) {
            return Err(anyhow::anyhow!("Mock function {} failed", name));
        }

        // Serialize params for logging
        let params_json = serde_json::to_vec(params)
            .context("Failed to serialize parameters")?;

        // If we have a predefined result, return it
        if let Some(result_data) = self.function_results.get(name) {
            let result = serde_json::from_slice::<R>(result_data)
                .context("Failed to deserialize predefined result")?;
            return Ok(result);
        }

        // For init function, just return empty result
        if name == "ntwk:theater/actor.init" {
            // This is a special case where we don't need a result
            let empty_json = "{}";
            let result = serde_json::from_str::<R>(empty_json)
                .context("Failed to create empty result")?;
            return Ok(result);
        }

        // For any other function, create a default response
        // This won't work for all types, but is sufficient for testing
        let empty_json = "{}";
        let result = serde_json::from_str::<R>(empty_json)
            .context("Failed to create default result")?;
        
        Ok(result)
    }
    
    /// Get the metrics from the mock instance
    pub fn get_metrics(&self) -> ActorMetrics {
        self.metrics.clone()
    }
    
    /// Get the store data
    pub fn store(&self) -> &Arc<Mutex<ActorStore>> {
        &self.store
    }
    
    /// Get mutable access to store data
    pub fn store_data_mut(&self) -> ActorStore {
        self.store.lock().unwrap().clone()
    }
}

/// Create a mock actor component for testing
pub async fn create_mock_component(
    config: &ManifestConfig,
    actor_store: ActorStore
) -> Result<MockActorComponent> {
    let component = MockActorComponent::new(actor_store);
    Ok(component)
}

/// Helper to create predefined function results
pub fn mock_function_result<T>(value: &T) -> Result<Vec<u8>> 
where
    T: serde::Serialize
{
    let data = serde_json::to_vec(value)
        .context("Failed to serialize mock result")?;
    Ok(data)
}
