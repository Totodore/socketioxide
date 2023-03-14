#[derive(Debug)]
pub enum Error {
	SerializeError(serde_json::Error),
}

impl From<serde_json::Error> for Error {
	fn from(err: serde_json::Error) -> Self {
		Error::SerializeError(err)
	}
}