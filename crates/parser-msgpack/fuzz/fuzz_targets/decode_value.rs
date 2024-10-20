#![no_main]

use libfuzzer_sys::fuzz_target;
use socketioxide_core::{parser::Parse, Value};
use socketioxide_parser_msgpack::MsgPackParser;
fuzz_target!(|data: (Value, bool)| {
    let mut data = data;
    MsgPackParser
        .decode_value::<rmpv::Value>(&mut data.0, data.1)
        .ok();
});
