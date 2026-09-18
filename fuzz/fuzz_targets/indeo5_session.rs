#![no_main]
use libfuzzer_sys::fuzz_target;
use oxideav_indeo::indeo5::Indeo5Decoder;

// A sequence of frames through the stateful session: the input is cut
// into frames at a length prefix so INTER / NULL frames follow an
// INTRA one.
fuzz_target!(|data: &[u8]| {
    let mut dec = Indeo5Decoder::new();
    let mut rest = data;
    let mut frames = 0;
    while rest.len() >= 2 && frames < 6 {
        let len = usize::from(u16::from_le_bytes([rest[0], rest[1]])) % 4096;
        rest = &rest[2..];
        let take = len.min(rest.len());
        let (frame, tail) = rest.split_at(take);
        let _ = dec.decode(frame);
        rest = tail;
        frames += 1;
    }
});
