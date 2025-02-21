use crate::chain::ChainEvent;
use crate::ActorStore;
use anyhow::Result;
use serde::Serialize;
use std::sync::OnceLock;
use tokio::sync::mpsc;
use wasmtime::StoreContextMut;

static EVENT_SENDER: OnceLock<mpsc::UnboundedSender<(StoreId, String, Vec<u8>)>> = OnceLock::new();

#[derive(Clone, Hash, Eq, PartialEq)]
struct StoreId(u128);

pub fn init_host_wrapper() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    EVENT_SENDER
        .set(tx)
        .expect("Host wrapper already initialized");

    // Spawn a task to process events
    tokio::spawn(async move {
        while let Some((store_id, event_type, data)) = rx.recv().await {
            // Process events...
            // We'll need to figure out how to get the ActorStore reference here
        }
    });
}

pub struct HostFunctionBoundary {
    pub function_name: String,
    pub interface_name: String,
}

impl HostFunctionBoundary {
    pub fn new(interface_name: impl Into<String>, function_name: impl Into<String>) -> Self {
        Self {
            interface_name: interface_name.into(),
            function_name: function_name.into(),
        }
    }

    pub fn wrap<Args, Return, F>(
        &self,
        store: &mut StoreContextMut<'_, ActorStore>,
        args: Args,
        f: F,
    ) -> Result<Return>
    where
        Args: Serialize + Clone,
        Return: Serialize + Clone,
        F: FnOnce(Args) -> Result<Return>,
    {
        // Get the event sender
        let sender = EVENT_SENDER.get().expect("Host wrapper not initialized");

        // Record outbound call
        let args_json = serde_json::to_vec(&args)?;
        let store_id = StoreId(store.data().id.as_uuid().as_u128()); // We'll need to add this to TheaterId
        sender.send((
            store_id.clone(),
            format!("{}/{}_call", self.interface_name, self.function_name),
            args_json,
        ))?;

        // Execute the host function
        let result = f(args)?;

        // Record the return value
        let result_json = serde_json::to_vec(&result)?;
        sender.send((
            store_id,
            format!("{}/{}_return", self.interface_name, self.function_name),
            result_json,
        ))?;

        Ok(result)
    }
}

/// Helper macro to wrap a host function with boundary tracking
#[macro_export]
macro_rules! host_func_wrap {
    ($interface:expr, $name:expr, $ctx:expr, |$($param:ident: $ty:ty),*| $body:expr) => {{
        let boundary = HostFunctionBoundary::new($interface, $name);
        move |mut store: StoreContextMut<'_, ActorStore>, ($($param),*): ($($ty),*)| {
            let args = ($($param.clone()),*);
            boundary.wrap(&mut store, args, |($($param),*)| $body)
        }
    }};
}

