//! The Retryer struct allows to send a given packet to the client
//! 
//! In case of error, if it is due to a [`TrySendError::Full`] a resend closure will be provided to the user.
//! Thanks to that the pending packet is not lost.

use crate::errors::RetryerError;
use engineioxide::{sid_generator::Sid, SendPacket};
use std::{collections::VecDeque, fmt::Debug};
use tokio::{sync::mpsc::error::TrySendError, sync::mpsc::Sender};

//TODO: bin payload and payload should be in one VecDeque

/// The `Retryer` struct represents a retry mechanism for sending packets.
#[derive(Debug)]
pub struct Retryer {
    sid: Sid,
    sender: Sender<SendPacket>,
    packet: Option<SendPacket>,
    bin_payload: VecDeque<Vec<u8>>,
}

impl Retryer {
    /// Creates a new `Retryer` instance with the specified parameters.
    ///
    /// # Arguments
    ///
    /// * `sid` - The identifier for the network connection.
    /// * `sender` - The sender channel used for sending packets.
    /// * `packet` - An optional packet to be sent initially.
    /// * `bin_payload` - A queue of binary payloads to be sent.
    ///
    /// # Returns
    ///
    /// A new `Retryer` instance.
    pub(crate) fn new(
        sid: Sid,
        sender: Sender<SendPacket>,
        packet: Option<SendPacket>,
        bin_payload: VecDeque<Vec<u8>>,
    ) -> Retryer {
        Self {
            sid,
            sender,
            packet,
            bin_payload,
        }
    }

    /// Retries sending the packet and binary payloads.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if all packets were successfully sent.
    /// - `Err(RetryerError)` if there are remaining packets to be sent or the socket is closed.
    pub fn retry(mut self) -> Result<(), RetryerError> {
        let packet = self.packet.take();
        // Retry sending the main packet
        match packet.map(|p| self.sender.try_send(p)) {
            Some(Err(TrySendError::Full(packet))) => {
                self.packet = Some(packet);
                return Err(RetryerError::Remaining(self));
            }
            Some(Err(TrySendError::Closed(_))) => {
                return Err(RetryerError::SocketClosed { sid: self.sid })
            }
            _ => {}
        };

        // Retry sending binary payloads
        while let Some(payload) = self.bin_payload.pop_front() {
            match self.sender.try_send(SendPacket::Binary(payload)) {
                Err(TrySendError::Full(SendPacket::Binary(payload))) => {
                    self.bin_payload.push_front(payload);
                    return Err(RetryerError::Remaining(self));
                }
                Err(TrySendError::Full(SendPacket::Message(_))) => unreachable!(),
                Err(_) => return Err(RetryerError::SocketClosed { sid: self.sid }),
                _ => {}
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use crate::errors::RetryerError;
    use crate::packet::Packet;
    use crate::retryer::Retryer;
    use tokio::sync::mpsc::channel;

    #[tokio::test]
    async fn test_resend_bin() {
        let sid = 1i64.into();
        let (tx, mut rx) = channel(1);
        let err = Retryer::new(
            sid,
            tx.clone(),
            Some(
                Packet::event(
                    "ns".to_string(),
                    "lol".to_string(),
                    serde_json::to_value("\"someString2\"").unwrap(),
                )
                .try_into()
                .unwrap(),
            ),
            vec![vec![1, 2, 3], vec![4, 5, 6]].into(),
        )
        .retry()
        .unwrap_err();

        // only txt message sent
        let RetryerError::Remaining(retryer)  = err else {
        panic!("unexpected err");
    };
        // read txt
        rx.recv().await.unwrap();
        // send first bin, second bin fails
        let err = retryer.retry().unwrap_err();
        let RetryerError::Remaining(retryer)  = err else {
        panic!("unexpected err");
    };
        // read first bin
        rx.recv().await.unwrap();
        // successfully send last part
        retryer.retry().unwrap();
    }
}
