use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use theater::ActorStore;
use theater::ManifestConfig;
// use wasmtime::component::Lower; // Not used

/// A mock function result for testing
pub struct MockFunctionResult {
    pub result: Vec<u8>,
    pub error: Option<String>,
}

/// A mock WASM component for testing
pub struct MockActorComponent {
    pub store: ActorStore,
    pub function_results: Arc<Mutex<HashMap<String, MockFunctionResult>>>,
}

/// A mock WASM instance for testing
pub struct MockActorInstance {
    pub store: ActorStore,
    pub function_results: Arc<Mutex<HashMap<String, MockFunctionResult>>>,
}

/// Helper function to create mock function results
pub fn mock_function_result<T: serde::Serialize>(value: T) -> Result<MockFunctionResult> {
    Ok(MockFunctionResult {
        result: serde_json::to_vec(&value)?,
        error: None,
    })
}

/// Helper function to create mock error results
pub fn mock_error_result(error: &str) -> MockFunctionResult {
    MockFunctionResult {
        result: Vec::new(),
        error: Some(error.to_string()),
    }
}

impl MockActorComponent {
    pub async fn new(_config: &ManifestConfig, store: ActorStore) -> Result<Self> {
        Ok(Self {
            store,
            function_results: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Set the result for a function call
    pub fn set_function_result(&self, function_name: &str, result: MockFunctionResult) {
        let mut results = self.function_results.lock().unwrap();
        results.insert(function_name.to_string(), result);
    }

    /// Create a mock instance
    pub async fn instantiate(&self) -> Result<MockActorInstance> {
        Ok(MockActorInstance {
            store: self.store.clone(),
            function_results: self.function_results.clone(),
        })
    }
}

impl MockActorInstance {
    /// Mock calling a function
    pub async fn call_function<P, R>(&self, name: &str, params: P) -> Result<R>
    where
        P: serde::Serialize,
        R: for<'de> serde::Deserialize<'de>,
    {
        // Convert parameters to JSON
        let _params_json = serde_json::to_vec(&params)?;

        // Look up the function result
        let results = self.function_results.lock().unwrap();

        if let Some(result) = results.get(name) {
            // Check for error
            if let Some(error) = &result.error {
                return Err(anyhow::anyhow!("Function error: {}", error));
            }

            // Return result
            let result_value = serde_json::from_slice(&result.result)?;
            Ok(result_value)
        } else {
            Err(anyhow::anyhow!("Function not found: {}", name))
        }
    }

    /// Get the store
    pub fn store_data(&self) -> &ActorStore {
        &self.store
    }

    /// Get mutable store
    pub fn store_data_mut(&mut self) -> &mut ActorStore {
        &mut self.store
    }
}

/// A basic factory for creating mock actor components for testing
pub struct MockActorComponentFactory {}

impl MockActorComponentFactory {
    pub async fn create_test_component(
        config: &ManifestConfig,
        store: ActorStore,
        function_results: HashMap<String, MockFunctionResult>,
    ) -> Result<MockActorComponent> {
        let component = MockActorComponent::new(config, store).await?;

        // Set function results
        for (name, result) in function_results {
            component.set_function_result(&name, result);
        }

        Ok(component)
    }

    /// Create a basic component with standard function results
    pub async fn create_basic_component(store: ActorStore) -> Result<MockActorComponent> {
        let config = ManifestConfig {
            name: "test-actor".to_string(),
            package: "test-package.wasm".to_string(),
            version: "1.0.0".to_string(),
            handlers: Vec::new(),
            description: None,
            long_description: None,
            save_chain: None,
            permission_policy: Default::default(),
            init_state: None,
        };

        let component = MockActorComponent::new(&config, store).await?;

        // Set up standard functions
        component.set_function_result(
            "theater:simple/actor.init",
            mock_function_result(()).unwrap(),
        );

        component.set_function_result(
            "theater:simple/actor.handle_message",
            mock_function_result(true).unwrap(),
        );

        Ok(component)
    }
}
