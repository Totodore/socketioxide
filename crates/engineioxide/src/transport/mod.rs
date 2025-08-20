//! All transports modules available in engineioxide

use engineioxide_core::{OpenPacket, Sid, TransportType};

use crate::config::EngineIoConfig;

pub mod polling;
pub mod ws;

fn make_open_packet(transport: TransportType, id: Sid, config: &EngineIoConfig) -> OpenPacket {
    let upgrades = if transport == TransportType::Polling {
        vec!["websocket".to_string()]
    } else {
        vec![]
    };
    OpenPacket {
        sid: id,
        upgrades,
        ping_timeout: config.ping_timeout.as_millis() as u64,
        ping_interval: config.ping_interval.as_millis() as u64,
        max_payload: config.max_payload,
    }
}
