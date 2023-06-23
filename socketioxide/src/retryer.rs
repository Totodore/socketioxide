use crate::errors::RetryerError;
use engineioxide::{sid_generator::Sid, SendPacket};
use std::{collections::VecDeque, fmt::Debug};
use tokio::{sync::mpsc::error::TrySendError, sync::mpsc::Sender};
// todo bin payload and payload should be in one VecDeque,
// todo sender should be the same for both types of SendPacket
#[derive(Debug)]
pub struct Retryer<T: Debug> {
    sid: Sid,
    sender: Sender<T>,
    packet: Option<T>,
    bin_payload: VecDeque<Vec<u8>>,
    bin_sender: Sender<SendPacket>,
}

impl<T: Debug> Retryer<T> {
    pub(crate) fn new(
        sid: Sid,
        sender: Sender<T>,
        packet: Option<T>,
        bin_payload: VecDeque<Vec<u8>>,
        bin_sender: Sender<SendPacket>,
    ) -> Self {
        Self {
            sid,
            sender,
            packet,
            bin_payload,
            bin_sender,
        }
    }

    pub fn retry(mut self) -> Result<(), RetryerError<T>> {
        match self.packet.map(|p| self.sender.try_send(p)) {
            Some(Err(TrySendError::Full(packet))) => {
                return Err(RetryerError::Remaining(Retryer::new(
                    self.sid,
                    self.sender,
                    Some(packet),
                    self.bin_payload,
                    self.bin_sender,
                )))
            }
            Some(Err(TrySendError::Closed(_))) => {
                return Err(RetryerError::SocketClosed { sid: self.sid })
            }
            _ => {}
        };

        while let Some(payload) = self.bin_payload.pop_front() {
            match self.bin_sender.try_send(SendPacket::Binary(payload)) {
                Err(TrySendError::Full(SendPacket::Binary(payload))) => {
                    self.bin_payload.push_front(payload);
                    return Err(RetryerError::Remaining(Retryer::new(
                        self.sid,
                        self.sender,
                        None,
                        self.bin_payload,
                        self.bin_sender,
                    )));
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
            tx,
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
