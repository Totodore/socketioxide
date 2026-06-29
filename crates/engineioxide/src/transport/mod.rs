//! All transports modules available in engineioxide

use engineioxide_core::{OpenPacket, Sid, TransportType};

use crate::config::EngineIoConfig;

pub mod polling;
pub mod ws;

fn make_open_packet(transport: TransportType, id: Sid, config: &EngineIoConfig) -> OpenPacket {
    let upgrades = if transport == TransportType::Polling {
        smallvec::smallvec![TransportType::Websocket]
    } else {
        smallvec::smallvec![]
    };
    OpenPacket {
        sid: id,
        upgrades,
        ping_timeout: config.ping_timeout,
        ping_interval: config.ping_interval,
        max_payload: config.max_payload,
    }
}
