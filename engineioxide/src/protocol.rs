use std::str::FromStr;

use crate::errors::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum ProtocolVersion {
    V3 = 3,
    V4 = 4,
}

impl FromStr for ProtocolVersion {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "3" => Ok(ProtocolVersion::V3),
            "4" => Ok(ProtocolVersion::V4),
            _ => Err(Error::UnsupportedProtocolVersion),
        }
    }
}
