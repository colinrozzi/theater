use anyhow::Result;
use theater::chain::ChainEvent;
use theater::config::permissions::HandlerPermission;
use theater::host::SimpleHandler;
use theater::messages::TheaterCommand;
use theater::theater_runtime::TheaterRuntime;
use theater::ChannelEvent;
use tokio::sync::mpsc::{Receiver, Sender};

mod simple_handler_wrapper;

pub struct SimpleTheater {
    runtime: TheaterRuntime<SimpleHandler, ChainEvent>,
}

impl SimpleTheater {
    pub async fn new(
        theater_tx: Sender<TheaterCommand>,
        theater_rx: Receiver<TheaterCommand>,
        channel_events_tx: Option<Sender<ChannelEvent>>,
        permissions: HandlerPermission,
    ) -> Result<Self> {
        let runtime =
            TheaterRuntime::new(theater_tx, theater_rx, channel_events_tx, permissions).await?;

        Ok(Self { runtime })
    }
}
