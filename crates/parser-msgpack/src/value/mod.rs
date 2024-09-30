use serde::{Deserialize, Serialize};
use socketioxide_core::{
    parser::{is_de_tuple, is_ser_tuple, FirstElement},
    Value,
};

mod de;
mod ser;

/// Deserialize any deserializable data from a generic [`SocketIoValue`] data with this form:
/// `[event, ...data`].
/// * If T is a tuple or a tuple struct, the end of the array will be parsed as the data: `T = (..data)`.
/// * If T is something else, the first element of the array will be parsed as the data: `T = data[0]`.
pub fn from_value<'de, T: Deserialize<'de>>(
    value: &'de Value,
    with_event: bool,
) -> Result<T, rmp_serde::decode::Error> {
    let value = match value {
        Value::Bytes(v) => v,
        Value::Str(_, _) => panic!("unexpected string data"),
    };
    if is_de_tuple::<T>() {
        de::from_bytes(value, with_event)
    } else {
        de::from_bytes_seed(value, FirstElement::default(), with_event)
    }
}

/// Serialize any serializable data and an event to a generic [`SocketIoValue`] data.
pub fn to_value<T: ?Sized + Serialize>(
    data: &T,
    event: Option<&str>,
) -> Result<Value, rmp_serde::encode::Error> {
    let data = if is_ser_tuple(data) {
        ser::into_bytes(data, event)?
    } else {
        ser::into_bytes(&(data,), event)?
    };
    Ok(Value::Bytes(data.into()))
}

pub fn read_event(data: &Value) -> Result<&str, rmp_serde::decode::Error> {
    let data = data.as_bytes().expect("bytes data for common parser");
    de::read_event(data)
}

#[cfg(test)]
mod tests {

    use super::*;
    use bytes::Bytes;
    use rmp_serde::to_vec_named;
    use serde::de::DeserializeOwned;
    use serde_json::json;

    fn to_bytes(data: impl Serialize, event: Option<&str>) -> Bytes {
        to_value(&data, event).unwrap().as_bytes().unwrap().clone()
    }
    fn from_bytes_event<T: DeserializeOwned>(data: impl Serialize) -> T {
        println!("{:?}", &to_vec_named(&data).unwrap());
        println!("{:?}", std::any::type_name::<T>());
        from_value::<T>(&Value::Bytes(to_vec_named(&data).unwrap().into()), true).unwrap()
    }
    fn from_bytes_ack<T: DeserializeOwned>(data: impl Serialize) -> T {
        println!("{:?}", &to_vec_named(&data).unwrap());
        println!("{:?}", std::any::type_name::<T>());
        from_value::<T>(&Value::Bytes(to_vec_named(&data).unwrap().into()), false).unwrap()
    }

    const BIN: Bytes = Bytes::from_static(&[1, 2, 3, 4]);
    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
    struct Data {
        data: Bytes,
    }
    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
    struct Data2 {
        data: Vec<Bytes>,
        test: String,
    }

    #[test]
    fn to_value_event() {
        assert_eq!(
            to_bytes("hello", Some("event")),
            to_vec_named(&json!(["event", "hello"])).unwrap()
        );
        assert_eq!(
            to_bytes(("hello", 1, 2, 3), Some("event")),
            to_vec_named(&json!(["event", "hello", 1, 2, 3])).unwrap()
        );
        assert_eq!(
            to_bytes(vec![1, 2, 3], Some("event")),
            to_vec_named(&json!(["event", vec![1, 2, 3]])).unwrap()
        );
        assert_eq!(
            to_bytes(json!({ "test": "null" }), Some("event")),
            to_vec_named(&json!(["event", { "test": "null" }])).unwrap()
        );
    }

    #[test]
    fn from_value_event() {
        assert_eq!(
            from_bytes_event::<String>(json!(["event", "hello"])),
            "hello"
        );
        assert_eq!(
            from_bytes_event::<(String, usize, usize, usize)>(json!(["event", "hello", 1, 2, 3])),
            ("hello".into(), 1, 2, 3)
        );
        assert_eq!(
            from_bytes_event::<Vec<usize>>(json!(["event", vec![1, 2, 3]])),
            vec![1, 2, 3]
        );
        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct Test {
            test: String,
        }
        assert_eq!(
            from_bytes_event::<Test>(json!(["event", { "test": "null" }])),
            Test {
                test: "null".into()
            }
        );
    }

    #[test]
    fn to_value_binary() {
        assert_eq!(
            to_bytes(("hello", &BIN), Some("event")),
            to_vec_named(&("event", "hello", BIN)).unwrap()
        );

        println!(
            "{:?}",
            to_bytes(("hello", 1, 2, 3, Data { data: BIN }), Some("event"))
                .to_vec()
                .as_slice()
        );
        assert_eq!(
            to_bytes(("hello", 1, 2, 3, Data { data: BIN }), Some("event")),
            to_vec_named(&("event", "hello", 1, 2, 3, Data { data: BIN })).unwrap()
        );
        assert_eq!(
            to_bytes(vec![Data { data: BIN }; 3], Some("event")),
            to_vec_named(&("event", vec![Data { data: BIN }; 3])).unwrap()
        );

        let data = Data2 {
            data: vec![BIN; 5],
            test: "null".to_string(),
        };
        assert_eq!(
            to_bytes(&data, Some("event")),
            to_vec_named(&("event", data)).unwrap()
        );
    }

    #[test]
    fn from_value_binary() {
        assert_eq!(from_bytes_event::<Bytes>(("event", BIN)), BIN);
        assert_eq!(
            from_bytes_event::<(String, Bytes)>(("event", "hello", BIN)),
            ("hello".into(), BIN)
        );
        assert_eq!(
            from_bytes_event::<(String, usize, usize, usize, Bytes)>((
                "event", "hello", 1, 2, 3, BIN
            )),
            ("hello".to_string(), 1, 2, 3, BIN),
        );
        assert_eq!(
            from_bytes_event::<Vec<Data>>(("event", vec![Data { data: BIN }; 3])),
            vec![Data { data: BIN }; 3]
        );
        let data = Data2 {
            data: vec![BIN; 5],
            test: "null".to_string(),
        };
        assert_eq!(from_bytes_event::<Data2>(("event", &data)), data);
    }

    #[test]
    fn to_value_binary_ack() {
        assert_eq!(to_bytes(BIN, None), to_vec_named(&[BIN]).unwrap());
        assert_eq!(
            to_bytes(("hello", 1, 2, 3, BIN), None),
            to_vec_named(&("hello", 1, 2, 3, BIN)).unwrap()
        );
    }

    #[test]
    fn from_value_binary_ack() {
        assert_eq!(from_bytes_ack::<Bytes>([BIN]), BIN);
        assert_eq!(
            from_bytes_ack::<(String, usize, usize, usize, Bytes)>(("hello", 1, 2, 3, BIN)),
            ("hello".to_string(), 1, 2, 3, BIN)
        );
    }

    #[test]
    fn to_value_ack() {
        assert_eq!(to_bytes("hello", None), to_vec_named(&["hello"]).unwrap());
        assert_eq!(
            to_bytes(("hello", 1, 2, 3), None),
            to_vec_named(&("hello", 1, 2, 3)).unwrap()
        );
    }

    #[test]
    fn from_value_ack() {
        assert_eq!(from_bytes_ack::<String>(["hello"]), "hello".to_string());
        assert_eq!(
            from_bytes_ack::<(String, usize, usize, usize)>(("hello", 1, 2, 3)),
            ("hello".to_string(), 1, 2, 3)
        );
    }
}
