use std::collections::HashMap;

use bytes::Bytes;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Number;

mod de;
mod from;
mod ser;

/// Wrapper that implements [`serde::Serialize`]; the output it produces has
/// [`PayloadValue::Binary()`] variants converted into binary placholder JSON objects.
pub(crate) struct AsJson<'a>(pub(crate) &'a PayloadValue);

/// Payload value representation, similar to [`serde_json::Value`], that can hold binary payloads.
///
/// Note that, if your data contains binary payloads, the [`serde::Serialize`] implementation for
/// this data type will *not* produce the type of JSON string you want, suitable for sending in a
/// socket.io binary event.  The default [`serde::Serialize`] implementation is set up so that the
/// data represented can be transformed into other types losslessly, which is at odds with the
/// correct JSON string representation needed for socket.io.
///
/// Instead, use [`to_json_string()`] to serialize properly for a socket.io event payload.
#[derive(Debug, Clone, Eq)]
pub enum PayloadValue {
    /// Represents a JSON `null` value.
    Null,
    /// Represents a JSON boolean value.
    Bool(bool),
    /// Represents a JSON number value.
    ///
    /// Both integers and floats are represented by the [`serde_json::Number`] type.
    Number(Number),
    /// Represents a JSON string value.
    String(String),
    /// Represents a binary payload.
    ///
    /// In socket.io, binary data is extracted from the JSON payload data, is replaced by a
    /// placeholder, and is sent over the wire in a more efficient way than is possible with JSON.
    /// When received, the original payload data is reconstructed with the binary data placed in
    /// the correct place.
    Binary(usize, Bytes),
    /// Represents a JSON array value.
    Array(Vec<PayloadValue>),
    /// Represents a JSON object value.
    Object(HashMap<String, PayloadValue>),
}

impl PayloadValue {
    /// Convert a `T` into a [`PayloadValue`].
    pub fn from_data<T: Serialize>(data: T) -> Result<PayloadValue, serde_json::Error> {
        ser::to_payload_value(data)
    }

    /// Interpret a [`PayloadValue`] as a `T`.
    pub fn into_data<T: DeserializeOwned>(self) -> Result<T, serde_json::Error> {
        T::deserialize(self)
    }

    /// Wraps `self` such that it can be serialized properly into a JSON string suitable for use in
    /// a socket.io event payload.
    pub(crate) fn as_json(&self) -> AsJson<'_> {
        AsJson(self)
    }

    /// Converts `self` to a [`serde_json::Value`]. Binary payloads are serialized as placeholder
    /// JSON objects.
    pub fn to_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(self.as_json())
    }

    /// Converts `self` to a JSON string. Binary payloads are serialized as placeholder JSON
    /// objects.
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.as_json())
    }

    /// Converts `self` to a JSON string that is pretty-printed for readability. Binary payloads
    /// are serialized as placeholder JSON objects.
    pub fn to_json_string_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.as_json())
    }

    /// Create a [`PayloadValue]` from a float.
    ///
    /// This may fail, if [`serde_json::Number`] cannot properly represent the number.
    pub fn from_f64(value: f64) -> Option<PayloadValue> {
        Number::from_f64(value).map(PayloadValue::Number)
    }

    /// Interprets `self` as a `bool`, if it contains boolean data.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PayloadValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Interprets `self` as a `u64`, if it contains unsigned integer data.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            PayloadValue::Number(n) => n.as_u64(),
            _ => None,
        }
    }

    /// Interprets `self` as a [`&str`], if it contains string ata.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PayloadValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Determines if `self` contains any binary payloads.
    pub fn has_binary(&self) -> bool {
        match self {
            PayloadValue::Binary(_, _) => true,
            PayloadValue::Array(a) => a.iter().any(PayloadValue::has_binary),
            PayloadValue::Object(o) => o.iter().any(|(_, v)| v.has_binary()),
            _ => false,
        }
    }

    /// Counts the number of binary payloads (or placeholders) contained in `self`.
    pub fn count_payloads(&self) -> usize {
        match self {
            PayloadValue::Binary(_, _) => 1,
            PayloadValue::Array(a) => a.iter().map(PayloadValue::count_payloads).sum(),
            PayloadValue::Object(o) => o.iter().map(|(_, v)| v.count_payloads()).sum(),
            _ => 0,
        }
    }

    /// Returns a list of all binary payloads in `self`, ordered as they will be sent over the wire
    /// as part of a socket.io binary event.
    pub fn get_binary_payloads(&self) -> Vec<Bytes> {
        fn rec(data: &PayloadValue) -> Vec<(usize, Bytes)> {
            let mut bins = Vec::<(usize, Bytes)>::new();

            match data {
                PayloadValue::Binary(num, bin) => {
                    bins.push((*num, bin.clone()));
                }
                PayloadValue::Array(a) => {
                    for value in a.iter() {
                        bins.extend(rec(value));
                    }
                }
                PayloadValue::Object(o) => {
                    for value in o.values() {
                        bins.extend(rec(value));
                    }
                }
                _ => (),
            }

            bins
        }

        let mut bins = rec(self);
        bins.sort_by(|(a, _), (b, _)| a.cmp(b));
        bins.into_iter().map(|(_, bin)| bin).collect()
    }
}

// PartialEq is implemented manually rather than being derived because the binary payload num
// should not be included in the comparison.  It's enough that the binary bytes in the node are
// equivalent.
impl PartialEq for PayloadValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PayloadValue::Null, PayloadValue::Null) => true,
            (PayloadValue::Bool(a), PayloadValue::Bool(b)) => a.eq(b),
            (PayloadValue::Number(a), PayloadValue::Number(b)) => a.eq(b),
            (PayloadValue::String(a), PayloadValue::String(b)) => a.eq(b),
            (PayloadValue::Array(a), PayloadValue::Array(b)) => a.eq(b),
            (PayloadValue::Object(a), PayloadValue::Object(b)) => a.eq(b),
            (PayloadValue::Binary(_, a), PayloadValue::Binary(_, b)) => a.eq(b),
            _ => false,
        }
    }
}

#[cfg(test)]
mod test {
    use bytes::Bytes;
    use serde::{Deserialize, Serialize};
    use serde_json::{json, Number, Value};

    use super::PayloadValue;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestSubPayload {
        more_binary: Bytes,
        opt_int: Option<i32>,
        opt_float: Option<f32>,
        opt_string: Option<String>,
        opt_boolean: Option<bool>,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestPayload {
        uint: u32,
        float: f64,
        binary: Bytes,
        string: String,
        boolean: bool,
        array: Vec<String>,
        sub_payload: TestSubPayload,
    }

    const BINARY_PAYLOAD: &[u8] = &[1, 2, 3, 4, 5, 6, 7];
    const MORE_BINARY_PAYLOAD: &[u8] = &[10, 9, 8, 7, 6, 5];

    fn build_test_payload(fill_options: bool) -> TestPayload {
        let fill_options = fill_options.then_some(true);
        TestPayload {
            uint: 42,
            float: 1.75,
            binary: Bytes::from_static(BINARY_PAYLOAD),
            string: "test string".to_string(),
            boolean: true,
            array: ["one", "two", "three"]
                .into_iter()
                .map(ToString::to_string)
                .collect(),
            sub_payload: TestSubPayload {
                more_binary: Bytes::from_static(MORE_BINARY_PAYLOAD),
                opt_int: fill_options.map(|_| 99),
                opt_float: fill_options.map(|_| 2.5),
                opt_string: fill_options.map(|_| "another test string".to_string()),
                opt_boolean: fill_options,
            },
        }
    }

    fn build_test_payload_value(fill_options: bool, fill_binary_data: bool) -> PayloadValue {
        let fill_options = fill_options.then_some(true);
        let fill_binary_data = fill_binary_data.then_some(true);
        PayloadValue::Object(
            [
                ("uint", PayloadValue::Number(42.into())),
                (
                    "float",
                    PayloadValue::Number(Number::from_f64(1.75).unwrap()),
                ),
                (
                    "binary",
                    PayloadValue::Binary(
                        0,
                        fill_binary_data
                            .map(|_| Bytes::from_static(BINARY_PAYLOAD))
                            .unwrap_or(Bytes::new()),
                    ),
                ),
                ("string", PayloadValue::String("test string".to_string())),
                ("boolean", PayloadValue::Bool(true)),
                (
                    "array",
                    PayloadValue::Array(
                        ["one", "two", "three"]
                            .into_iter()
                            .map(|s| PayloadValue::String(s.to_string()))
                            .collect(),
                    ),
                ),
                (
                    "sub_payload",
                    PayloadValue::Object(
                        [
                            (
                                "more_binary",
                                PayloadValue::Binary(
                                    1,
                                    fill_binary_data
                                        .map(|_| Bytes::from_static(MORE_BINARY_PAYLOAD))
                                        .unwrap_or(Bytes::new()),
                                ),
                            ),
                            (
                                "opt_int",
                                fill_options
                                    .map(|_| PayloadValue::Number(99.into()))
                                    .unwrap_or(PayloadValue::Null),
                            ),
                            (
                                "opt_float",
                                fill_options
                                    .map(|_| {
                                        PayloadValue::Number(
                                            Number::from_f64(2.5f32 as f64).unwrap(),
                                        )
                                    })
                                    .unwrap_or(PayloadValue::Null),
                            ),
                            (
                                "opt_string",
                                fill_options
                                    .map(|_| {
                                        PayloadValue::String("another test string".to_string())
                                    })
                                    .unwrap_or(PayloadValue::Null),
                            ),
                            (
                                "opt_boolean",
                                fill_options
                                    .map(PayloadValue::Bool)
                                    .unwrap_or(PayloadValue::Null),
                            ),
                        ]
                        .into_iter()
                        .map(|(k, v)| (k.to_string(), v))
                        .collect(),
                    ),
                ),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        )
    }

    fn build_test_payload_json_value(fill_options: bool) -> Value {
        let sub_payload = if fill_options {
            json!({
                "more_binary": {
                    "_placeholder": true,
                    "num": 1
                },
                "opt_int": 99,
                "opt_float": 2.5,
                "opt_string": "another test string",
                "opt_boolean": true
            })
        } else {
            json!({
                "more_binary": {
                    "_placeholder": true,
                    "num": 1
                },
                "opt_int": null,
                "opt_float": null,
                "opt_string": null,
                "opt_boolean": null
            })
        };

        let mut main_payload = json!({
            "uint": 42,
            "float": 1.75,
            "binary": {
                "_placeholder": true,
                "num": 0
            },
            "string": "test string",
            "boolean": true,
            "array": [
                "one",
                "two",
                "three"
            ]
        });

        if let Value::Object(ref mut o) = &mut main_payload {
            o.insert("sub_payload".to_string(), sub_payload);
        } else {
            panic!("test bug: not an object");
        }

        main_payload
    }

    #[test]
    pub fn test_payload_value_from_data() {
        let test_payload = build_test_payload(true);
        let payload_value = PayloadValue::from_data(test_payload).unwrap();
        assert_eq!(payload_value, build_test_payload_value(true, true));

        let test_payload = build_test_payload(false);
        let payload_value = PayloadValue::from_data(test_payload).unwrap();
        assert_eq!(payload_value, build_test_payload_value(false, true));
    }

    #[test]
    pub fn test_payload_value_into_data() {
        let payload_value = build_test_payload_value(true, true);
        let test_payload: TestPayload = payload_value.into_data().unwrap();
        assert_eq!(test_payload, build_test_payload(true));

        let payload_value = build_test_payload_value(false, true);
        let test_payload: TestPayload = payload_value.into_data().unwrap();
        assert_eq!(test_payload, build_test_payload(false));
    }

    #[test]
    pub fn test_payload_value_to_json_value() {
        let payload_value = build_test_payload_value(true, false);
        let json = payload_value.to_value().unwrap();
        assert_eq!(json, build_test_payload_json_value(true));

        let payload_value = build_test_payload_value(false, false);
        let json = payload_value.to_value().unwrap();
        assert_eq!(json, build_test_payload_json_value(false));
    }

    #[test]
    pub fn test_payload_value_from_json_value() {
        let json = build_test_payload_json_value(true);
        let payload_value: PayloadValue = serde_json::from_value(json).unwrap();
        assert_eq!(payload_value, build_test_payload_value(true, false));

        let json = build_test_payload_json_value(false);
        let payload_value: PayloadValue = serde_json::from_value(json).unwrap();
        assert_eq!(payload_value, build_test_payload_value(false, false));
    }

    #[test]
    pub fn test_count_payloads() {
        let payload_value = build_test_payload_value(true, false);
        assert_eq!(payload_value.count_payloads(), 2);
    }

    #[test]
    pub fn test_extract_binary_payloads() {
        let test_payload = build_test_payload(true);
        let payload_value = build_test_payload_value(true, true);
        let bins = payload_value.get_binary_payloads();

        assert_eq!(bins.len(), 2);
        assert_eq!(bins[0], *test_payload.binary);
        assert_eq!(bins[1], *test_payload.sub_payload.more_binary);
    }

    #[test]
    pub fn test_payload_value_redeser() {
        let payload_value_again: PayloadValue =
            build_test_payload_value(true, true).into_data().unwrap();
        assert_eq!(build_test_payload_value(true, true), payload_value_again);
    }
}
