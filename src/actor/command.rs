use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::oneshot;
use wasmtime::component::{ComponentNamedList, Lift, Lower};

use super::wasm::WasmActor;

pub(super) enum ActorCommand<T, U>
where
    T: Lower + ComponentNamedList + Send + Sync + Serialize + Debug,
    U: Lift + ComponentNamedList + Send + Sync + Serialize + Debug + Clone,
{
    Call {
        export_name: String,
        params: T,
        response_tx: oneshot::Sender<Result<U>>,
    },
}
