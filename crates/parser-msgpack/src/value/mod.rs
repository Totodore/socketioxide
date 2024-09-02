use serde::{de::DeserializeOwned, Serialize};
use socketioxide_core::{
    parser::{is_de_tuple, is_ser_tuple, FirstElement},
    SocketIoValue,
};

mod de;
mod ser;

/// Deserialize any deserializable data from a generic [`SocketIoValue`] data with this form:
/// `[event, ...data`].
/// * If T is a tuple or a tuple struct, the end of the array will be parsed as the data: `T = (..data)`.
/// * If T is something else, the first element of the array will be parsed as the data: `T = data[0]`.
///
/// All adjacent binary data will be inserted into the output data.
pub fn from_value<T: DeserializeOwned>(
    value: SocketIoValue,
    with_event: bool,
) -> Result<T, rmp_serde::decode::Error> {
    let value = match value {
        SocketIoValue::Bytes(v) => v,
        SocketIoValue::Str(_) => panic!("unexpected string data"),
    };
    if is_de_tuple::<T>() {
        de::from_bytes(&value, with_event)
    } else {
        de::from_bytes_seed(&value, FirstElement::default(), with_event)
    }
}

/// Serialize any serializable data and an event to a generic [`SocketIoValue`] data.
pub fn to_value<T: Serialize>(
    data: &T,
    event: Option<&str>,
) -> Result<SocketIoValue, rmp_serde::encode::Error> {
    let data = if is_ser_tuple(data) {
        ser::into_bytes(data, event)?
    } else {
        ser::into_bytes(&(data,), event)?
    };
    Ok(SocketIoValue::Bytes(data.into()))
}
