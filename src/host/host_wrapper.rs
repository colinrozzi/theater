use crate::ActorStore;
use wasmtime::StoreContextMut;
use serde::Serialize;
use anyhow::Result;

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
        // Record outbound call
        let args_json = serde_json::to_vec(&args)?;
        store.data_mut().record_event(
            format!("{}/{}_call", self.interface_name, self.function_name),
            args_json
        );

        // Execute the host function
        let result = f(args)?;

        // Record the return value
        let result_json = serde_json::to_vec(&result)?;
        store.data_mut().record_event(
            format!("{}/{}_return", self.interface_name, self.function_name),
            result_json
        );

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