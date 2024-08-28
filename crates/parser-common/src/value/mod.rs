use bytes::Bytes;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use socketioxide_core::{SocketIoValue, Str};

use crate::de::FirstElement;

mod de;
mod ser;

/// Deserialize any deserializable data from a generic [`SocketIoValue`] data with this form:
/// `[event, ...data`].
/// * If T is a tuple or a tuple struct, the end of the array will be parsed as the data: `T = (..data)`.
/// * If T is something else, the first element of the array will be parsed as the data: `T = data[0]`.
///
/// All adjacent binary data will be inserted into the output data.
pub fn from_value<T: DeserializeOwned>(value: SocketIoValue) -> serde_json::Result<T> {
    let (value, bins) = match value {
        SocketIoValue::Str(v) => v,
        SocketIoValue::Bytes(_) => panic!("unexpected binary data"),
    };
    let is_tuple = de::is_tuple::<T>();
    if is_tuple {
        let mut de = de::Deserializer::new(value.as_str(), bins);
        T::deserialize(&mut de)
    } else {
        let mut de = de::Deserializer::new(value.as_str(), bins);
        FirstElement::<T>::deserialize(&mut de).map(|v| v.0)
    }
}

/// Serialize any serializable data and an event to a generic [`SocketIoValue`] data.
pub fn to_value<T: Serialize>(data: &T, event: Option<&str>) -> serde_json::Result<SocketIoValue> {
    let mut writer = Vec::new();
    let mut ser = ser::Serializer::new(&mut writer, event);
    if ser::is_tuple(data) {
        data.serialize(&mut ser)?;
    } else {
        (data,).serialize(&mut ser)?;
    }
    let binary = ser.into_binary();
    let data = unsafe { Str::from_bytes_unchecked(Bytes::from(writer)) };
    Ok(SocketIoValue::Str((data, binary)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn to_str(data: impl Serialize, event: Option<&str>) -> Str {
        to_value(&data, event).unwrap().as_str().unwrap().clone()
    }
    #[test]
    fn to_value_event() {
        assert_eq!(
            to_str("hello", Some("event")).as_str(),
            json!(["event", "hello"]).to_string().as_str()
        );
        assert_eq!(
            to_str(("hello", 1, 2, 3), Some("event")).as_str(),
            json!(["event", "hello", 1, 2, 3]).to_string().as_str()
        );
        assert_eq!(
            to_str(vec![1, 2, 3], Some("event")).as_str(),
            json!(["event", vec![1, 2, 3]]).to_string().as_str()
        );
        assert_eq!(
            to_str(json!({ "test": "null" }), Some("event")).as_str(),
            json!(["event", { "test": "null" }]).to_string().as_str()
        );
    }

    #[test]
    fn to_value_ack() {
        assert_eq!(
            to_str("hello", None).as_str(),
            json!(["hello"]).to_string().as_str()
        );
        assert_eq!(
            to_str(("hello", 1, 2, 3), None).as_str(),
            json!(["hello", 1, 2, 3]).to_string().as_str()
        );
    }

    #[test]
    fn is_tuple() {
        assert!(ser::is_tuple(&(1, 2, 3)));
        assert!(de::is_tuple::<(usize, usize, usize)>());

        assert!(ser::is_tuple(&[1, 2, 3]));
        assert!(de::is_tuple::<[usize; 3]>());

        #[derive(Serialize, Deserialize)]
        struct TupleStruct(&'static str);
        assert!(ser::is_tuple(&TupleStruct("test")));
        assert!(de::is_tuple::<TupleStruct>());

        assert!(!ser::is_tuple(&vec![1, 2, 3]));
        assert!(!de::is_tuple::<Vec<usize>>());

        #[derive(Serialize, Deserialize)]
        struct UnitStruct;
        assert!(!ser::is_tuple(&UnitStruct));
        assert!(!de::is_tuple::<UnitStruct>());
    }
}
