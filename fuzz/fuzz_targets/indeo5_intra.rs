#![no_main]
use libfuzzer_sys::fuzz_target;

// One-shot INTRA decode of arbitrary bytes: must return, never panic.
fuzz_target!(|data: &[u8]| {
    let _ = oxideav_indeo::indeo5::decode_intra_picture(data);
});
