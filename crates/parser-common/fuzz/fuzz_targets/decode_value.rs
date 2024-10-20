#![no_main]

use libfuzzer_sys::fuzz_target;
use socketioxide_core::{parser::Parse, Value};
use socketioxide_parser_common::CommonParser;

fuzz_target!(|data: (Value, bool)| {
    let mut data = data;
    CommonParser
        .decode_value::<serde_json::Value>(&mut data.0, data.1)
        .ok();
});
