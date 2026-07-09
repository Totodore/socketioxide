#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use socketioxide_core::parser::Parse;
use socketioxide_parser_common::CommonParser;
fuzz_target!(|data: &[u8]| {
    CommonParser
        .decode_bin(&Default::default(), Bytes::copy_from_slice(data))
        .ok();
});
