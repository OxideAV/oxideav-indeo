#![no_main]
use libfuzzer_sys::fuzz_target;
use oxideav_indeo::indeo3::Indeo3PictureDecoder;

// A sequence of frames through the stateful picture decoder, cut at a
// length prefix so inter / NULL frames follow an intra one.
fuzz_target!(|data: &[u8]| {
    let mut dec = Indeo3PictureDecoder::new();
    let mut rest = data;
    let mut frames = 0;
    while rest.len() >= 2 && frames < 6 {
        let len = usize::from(u16::from_le_bytes([rest[0], rest[1]])) % 8192;
        rest = &rest[2..];
        let take = len.min(rest.len());
        let (frame, tail) = rest.split_at(take);
        let _ = dec.decode(frame);
        rest = tail;
        frames += 1;
    }
});
