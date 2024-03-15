pub(crate) mod de;
pub(crate) mod ser;

#[cfg(test)]
mod test {
    use bytes::Bytes;
    use serde::{Deserialize, Serialize};
    use serde_json::{json, Value};

    use super::de::from_value;
    use super::ser::to_value;

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

    fn build_bins() -> Vec<Bytes> {
        vec![
            Bytes::from_static(BINARY_PAYLOAD),
            Bytes::from_static(MORE_BINARY_PAYLOAD),
        ]
    }

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

    fn build_test_value(fill_options: bool) -> Value {
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
    pub fn test_value_from_data() {
        let (value, bins) = to_value(build_test_payload(true)).unwrap();
        assert_eq!(build_test_value(true), value);
        assert_eq!(bins.len(), 2);
        assert_eq!(bins[0], Bytes::from_static(BINARY_PAYLOAD));
        assert_eq!(bins[1], Bytes::from_static(MORE_BINARY_PAYLOAD));
    }

    #[test]
    pub fn test_value_into_data() {
        let data: TestPayload =
            from_value(build_test_value(true), build_bins().as_slice()).unwrap();
        assert_eq!(data, build_test_payload(true));
    }

    /*

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
    */
}
