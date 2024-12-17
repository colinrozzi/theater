use crate::actor::ActorMessage;
use anyhow::Result;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::mpsc;

pub trait HostHandler: Send + Sync {
    fn name(&self) -> &str;
    fn new(config: Value) -> Self
    where
        Self: Sized;
    fn start(
        &self,
        mailbox_tx: mpsc::Sender<ActorMessage>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    fn stop(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;
}
