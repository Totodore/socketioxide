use serde::Serialize;

#[derive(Debug, PartialEq, PartialOrd)]
pub enum Packet<'a> {
	Open(OpenPacket<'a>),
	Close,
	Ping,
	Pong,
	Message(String),
	Upgrade,
	Noop,
}

impl TryInto<String> for Packet<'_> {

	type Error = serde_json::Error;
	fn try_into(self) -> Result<String, Self::Error> {
		let res = match self {
			Packet::Open(open) => "0".to_string() + &serde_json::to_string(&open)?,
			Packet::Close => "1".to_string(),
			Packet::Ping => "2".to_string(),
			Packet::Pong => "3".to_string(),
			Packet::Message(msg) =>  "4".to_string() + &serde_json::to_string(&msg)?,
			Packet::Upgrade => "5".to_string(),
			Packet::Noop => "6".to_string(),
		};
		Ok(res)
	}
}


#[derive(Debug, Serialize, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct OpenPacket<'a> {
	sid: String,
	upgrades: [&'a str; 1],	// Only websocket upgrade is supported
	ping_interval: u64,
	ping_timeout: u64,
	max_payload: u64,
}

impl OpenPacket<'_> {
	pub fn new() -> Self {
		OpenPacket {
			sid: "ybnazdiudbnazdi".to_string(),		//TODO: Implement SID generation
			upgrades: ["websocket"],
			ping_interval: 300,
			ping_timeout: 200,
			max_payload: 1000000,
		}
	}
}

#[derive(Debug, PartialEq)]
pub enum TransportType {
	Websocket,
	Polling,
}