mod connection;
mod theater_client;

pub use connection::Connection;
pub use theater_client::TheaterClient;
pub use theater_server::{ManagementCommand, ManagementResponse};

pub mod cli_wrapper;
pub use cli_wrapper::CliTheaterClient;
