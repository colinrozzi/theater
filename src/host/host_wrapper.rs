use crate::events::ChainEventData;
use crate::ActorStore;
use anyhow::Result;
use serde::Serialize;
use wasmtime::StoreContextMut;

#[derive(Clone)]
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
        event_data: ChainEventData,
    ) -> Result<()> {
        store.data_mut().record_event(event_data);
        Ok(())
    }
}
