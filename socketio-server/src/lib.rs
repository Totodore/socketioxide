use serde::Serialize;

pub mod layer;
pub mod client;
pub mod config;
pub mod ns;
pub mod socket;
mod handshake;
mod packet;
mod errors;

//TODO: Remove (used for testing purposes only)
#[derive(Serialize)]
pub struct Empty;