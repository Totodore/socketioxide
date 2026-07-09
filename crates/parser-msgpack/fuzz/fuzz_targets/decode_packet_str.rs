#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use socketioxide_core::{parser::Parse, Str};
use socketioxide_parser_msgpack::MsgPackParser;

fuzz_target!(|data: &[u8]| {
    let data = unsafe { Str::from_bytes_unchecked(Bytes::copy_from_slice(data)) };
    MsgPackParser.decode_str(&Default::default(), data).ok();
});
