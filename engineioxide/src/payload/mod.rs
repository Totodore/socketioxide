use futures::Stream;
use tokio::sync::{mpsc::Receiver, MutexGuard};

use crate::{errors::Error, packet::Packet, service::ProtocolVersion};

mod buf;
mod decoder;
mod encoder;

#[cfg(feature = "v4")]
const PACKET_SEPARATOR_V4: u8 = b'\x1e';
#[cfg(feature = "v3")]
const PACKET_SEPARATOR_V3: u8 = b':';

pub fn decoder(
    body: impl http_body::Body<Error = impl std::fmt::Debug> + Unpin,
    #[allow(unused_variables)] protocol: ProtocolVersion,
    max_payload: u64,
) -> impl Stream<Item = Result<Packet, Error>> {
    #[cfg(all(feature = "v3", feature = "v4"))]
    {
        match protocol {
            ProtocolVersion::V4 => {
                futures::future::Either::Left(decoder::v4_decoder(body, max_payload))
            }
            ProtocolVersion::V3 => {
                futures::future::Either::Right(decoder::v3_decoder(body, max_payload))
            }
        }
    }

    #[cfg(all(feature = "v3", not(feature = "v4")))]
    {
        decoder::v3_decoder(body, max_payload)
    }
    #[cfg(all(feature = "v4", not(feature = "v3")))]
    {
        decoder::v4_decoder(body, max_payload)
    }
}

pub async fn encoder(
    rx: MutexGuard<'_, Receiver<Packet>>,
    #[allow(unused_variables)] protocol: ProtocolVersion,
    #[cfg(feature = "v3")] supports_binary: bool,
) -> Result<Vec<u8>, Error> {
    #[cfg(all(feature = "v3", feature = "v4"))]
    {
        match protocol {
            ProtocolVersion::V4 => encoder::v4_encoder(rx).await,
            ProtocolVersion::V3 if supports_binary => encoder::v3_binary_encoder(rx).await,
            ProtocolVersion::V3 => encoder::v3_string_encoder(rx).await,
        }
    }

    #[cfg(all(feature = "v3", not(feature = "v4")))]
    {
        if supports_binary {
            encoder::v3_binary_encoder(rx).await
        } else {
            encoder::v3_string_encoder(rx).await
        }
    }
    #[cfg(all(feature = "v4", not(feature = "v3")))]
    {
        encoder::v4_encoder(rx).await
    }
}
