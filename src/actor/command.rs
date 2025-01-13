pub enum ActorCommand<T, U>
where
    T: wasmtime::component::Lower
        + wasmtime::component::ComponentNamedList
        + Send
        + Sync
        + Serialize
        + Debug,
    U: wasmtime::component::Lift
        + wasmtime::component::ComponentNamedList
        + Send
        + Sync
        + Serialize
        + Debug
        + Clone,
{
    Call {
        export_name: String,
        args: T,
        response_tx: oneshot::Sender<Result<U>>,
    },
}

pub enum ActorCommandWrapped {
    Call {
        export_name: String,
        handler: Box<dyn FnOnce(&mut WasmActor) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>,
    },
}
