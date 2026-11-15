//! Hostile input for the Indeo 5 decoder: every staged frame truncated
//! at every length, with every byte changed and with single bits
//! flipped, alone and inside a sequence. The decoder must return (a
//! picture or an error), never panic, and never take more memory than
//! its picture-area bound allows.

use oxideav_core::{CodecId, CodecParameters, DecoderLimits, Error, Packet, TimeBase};
use oxideav_indeo::indeo5::{self, FrameType, Iv50Decoder, Iv50Error, MAX_PICTURE_PIXELS};

const EDUC: &[u8] = include_bytes!("data/intra-240x180-educ.iv50");
const INDEO5: &[u8] = include_bytes!("data/intra-320x240-indeo5.iv50");
const INTER: &[u8] = include_bytes!("data/iv50-inter-176x144-8frame-vendor-enc/samples.bin");
const INTER_INDEX: &str =
    include_str!("data/iv50-inter-176x144-8frame-vendor-enc/samples-index.csv");

/// A picture-area bound that fits the staged frames and keeps a
/// corrupted size field from costing a large allocation per decode.
const SWEEP_PIXELS: u64 = 512 * 512;

/// The sweeps visit every position in an optimised build and every
/// `STRIDE`-th one past the headers in an unoptimised one, where a
/// decode costs some thirty times more.
const STRIDE: usize = if cfg!(debug_assertions) { 11 } else { 1 };

/// Positions `0..len`: all of the first 64, then every `STRIDE`-th.
fn positions(len: usize) -> impl Iterator<Item = usize> {
    (0..len).filter(|&i| i < 64 || i % STRIDE == 0)
}

fn decoder() -> Iv50Decoder {
    Iv50Decoder::new().with_max_pixels(SWEEP_PIXELS)
}

fn inter_frames() -> Vec<&'static [u8]> {
    INTER_INDEX
        .lines()
        .skip(1)
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            let (off, size): (usize, usize) = (f[1].parse().unwrap(), f[2].parse().unwrap());
            &INTER[off..off + size]
        })
        .collect()
}

/// Whatever the decoder holds must be readable at its stated size.
fn touch(dec: &Iv50Decoder) {
    for p in [dec.y(), dec.u(), dec.v()].into_iter().flatten() {
        assert!(p.stride >= p.width);
        assert!(p.data.len() >= p.stride * (p.height - 1) + p.width);
        let _ = p.row(p.height - 1);
    }
    let _ = dec.yuy2();
}

/// A deterministic generator for the sweeps.
fn lcg(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
    *state >> 8
}

#[test]
fn every_truncation_of_every_staged_frame_returns() {
    let frames = inter_frames();
    for frame in [EDUC, INDEO5].into_iter().chain(frames.iter().copied()) {
        for len in positions(frame.len()) {
            // Alone on a fresh decoder.
            let mut dec = decoder();
            let _ = dec.decode(&frame[..len]);
            touch(&dec);
        }
    }
    // An inter frame cut short after its intra frame decoded: the
    // error leaves the previous picture in place.
    let mut dec = decoder();
    dec.decode(frames[0]).unwrap();
    let before = dec.yuy2().unwrap();
    for len in positions(frames[1].len()).skip(2) {
        if dec.decode(&frames[1][..len]).is_err() {
            assert_eq!(dec.yuy2().unwrap(), before, "cut at {len}");
        } else {
            // A cut that still decodes changed the picture; start over.
            dec = decoder();
            dec.decode(frames[0]).unwrap();
        }
    }
}

#[test]
fn every_byte_changed_in_every_staged_frame_returns() {
    let frames = inter_frames();
    // One decoder for the whole sweep: each corrupted frame also meets
    // whatever the ones before it left behind.
    let mut dec = decoder();
    for frame in [EDUC, INDEO5, frames[0]] {
        let mut work = frame.to_vec();
        for i in positions(frame.len()) {
            // Every byte gets one of three changes; the first 64 bytes
            // (the headers) get all three.
            let masks = [0x01u8, 0x80, 0xff];
            let picks = if i < 64 {
                &masks[..]
            } else {
                &masks[i % 3..i % 3 + 1]
            };
            for &mask in picks {
                work[i] = frame[i] ^ mask;
                let _ = dec.decode(&work);
                touch(&dec);
            }
            work[i] = frame[i];
        }
        // The clean frame still decodes on the decoder the sweep used.
        assert_eq!(dec.decode(frame), Ok(FrameType::Intra));
    }
}

#[test]
fn every_header_bit_flipped_returns() {
    // The first 48 bytes hold the picture start, the GOP header, the
    // frame header and the first band header of each staged keyframe.
    let frames = inter_frames();
    for frame in [EDUC, INDEO5, frames[0]] {
        for bit in 0..48 * 8 {
            let mut work = frame.to_vec();
            work[bit / 8] ^= 1 << (bit % 8);
            let mut dec = decoder();
            let _ = dec.decode(&work);
            touch(&dec);
        }
    }
}

#[test]
fn corrupted_inter_frames_in_a_sequence_return() {
    let frames = inter_frames();
    let mut st = 0x1234_5678;
    let mut dec = decoder();
    for round in 0..400 / STRIDE {
        // Restart from the intra frame now and then, so the corrupted
        // frames also meet a clean reference.
        if round % 8 == 0 {
            dec.decode(frames[0]).unwrap();
        }
        let k = 1 + (lcg(&mut st) as usize) % 7;
        let mut work = frames[k].to_vec();
        for _ in 0..1 + lcg(&mut st) % 4 {
            let at = (lcg(&mut st) as usize) % work.len();
            work[at] ^= 1 << (lcg(&mut st) % 8);
        }
        // A fresh frame number, so the frame is not taken for a repeat.
        work[1] = (round % 251) as u8 + 1;
        let _ = dec.decode(&work);
        touch(&dec);
    }
    // Every bit of one inter frame, each against the clean reference.
    let mut dec = decoder();
    for (n, bit) in positions(frames[1].len() * 8).enumerate() {
        if n % 64 == 0 {
            dec.decode(frames[0]).unwrap();
        }
        let mut work = frames[1].to_vec();
        work[bit / 8] ^= 1 << (bit % 8);
        work[1] = (bit % 250) as u8 + 2;
        let _ = dec.decode(&work);
        touch(&dec);
    }
}

#[test]
fn random_bytes_after_a_valid_start_return() {
    let mut st = 99;
    let frames = inter_frames();
    let mut dec = decoder();
    dec.decode(frames[0]).unwrap();
    for round in 0..2000u32 {
        let len = 2 + (lcg(&mut st) as usize) % 300;
        let mut work: Vec<u8> = (0..len).map(|_| lcg(&mut st) as u8).collect();
        // A picture start of every frame type, the invalid ones too.
        work[0] = 0x1f | ((round % 8) as u8) << 5;
        let _ = dec.decode(&work);
        touch(&dec);
    }
    // The real GOP and frame headers followed by random band data.
    for _ in 0..300 {
        let mut work = INDEO5[..20].to_vec();
        let len = (lcg(&mut st) as usize) % 400;
        work.extend((0..len).map(|_| lcg(&mut st) as u8));
        let mut dec = decoder();
        let _ = dec.decode(&work);
        touch(&dec);
    }
}

/// A GOP header alone: gop_flags, `decomp_levels` 0, a custom size, two
/// band infos, the trailing fields, and a frame header.
fn gop_only(width: u32, height: u32) -> Vec<u8> {
    let mut bits: Vec<u8> = Vec::new();
    let mut put = |value: u32, n: u32| {
        for i in 0..n {
            bits.push(((value >> i) & 1) as u8);
        }
    };
    put(0x1f, 5);
    put(0, 3);
    put(0, 8);
    put(0, 8); // gop_flags
    put(0, 3);
    put(15, 4);
    put(height, 13);
    put(width, 13);
    put(0, 6);
    put(6, 6);
    while !bits.len().is_multiple_of(8) {
        bits.push(0);
    }
    let mut put = |value: u32, n: u32| {
        for i in 0..n {
            bits.push(((value >> i) & 1) as u8);
        }
    };
    put(2 << 19, 24);
    put(0, 16); // frame header
    bits.chunks(8)
        .map(|c| c.iter().enumerate().fold(0, |b, (i, &v)| b | (v << i)))
        .collect()
}

#[test]
fn picture_area_is_bounded_before_allocation() {
    // The largest size a GOP header can state.
    let huge = gop_only(8191, 8191);
    assert!(matches!(
        Iv50Decoder::new().decode(&huge),
        Err(Iv50Error::Unsupported(_))
    ));
    const { assert!(8191 * 8191 > MAX_PICTURE_PIXELS) };
    // A lowered bound refuses what the default accepts, exactly at the
    // bound.
    let frame = gop_only(64, 48);
    let mut tight = Iv50Decoder::new().with_max_pixels(64 * 48 - 1);
    assert!(matches!(
        tight.decode(&frame),
        Err(Iv50Error::Unsupported(_))
    ));
    assert_eq!(tight.dimensions(), None);
    // At the bound the header is taken (the frame then lacks its bands).
    let mut exact = Iv50Decoder::new().with_max_pixels(64 * 48);
    assert_eq!(exact.decode(&frame), Err(Iv50Error::Truncated));
    assert_eq!(exact.dimensions(), Some((64, 48)));
    // The bound cannot be raised above the crate's.
    let mut raised = Iv50Decoder::new().with_max_pixels(u64::MAX);
    assert!(matches!(
        raised.decode(&huge),
        Err(Iv50Error::Unsupported(_))
    ));
    // A staged frame still decodes under a bound that fits it.
    let mut dec = Iv50Decoder::new().with_max_pixels(320 * 240);
    assert_eq!(dec.decode(INDEO5), Ok(FrameType::Intra));
}

#[test]
fn registry_decoder_honours_the_stream_limits() {
    let packet = |data: &[u8]| Packet {
        stream_index: 0,
        time_base: TimeBase::new(1, 1000),
        pts: Some(0),
        dts: Some(0),
        duration: None,
        flags: Default::default(),
        data: data.to_vec(),
    };
    let mut params = CodecParameters::video(CodecId::new(indeo5::CODEC_ID_STR));
    params.limits = DecoderLimits::default().with_max_pixels_per_frame(320 * 240 - 1);
    let mut dec = indeo5::make_decoder(&params).unwrap();
    dec.send_packet(&packet(INDEO5)).unwrap();
    assert!(matches!(dec.receive_frame(), Err(Error::InvalidData(_))));
    // The limit survives a seek.
    dec.reset().unwrap();
    dec.send_packet(&packet(INDEO5)).unwrap();
    assert!(dec.receive_frame().is_err());
    // The smaller staged frame fits.
    dec.reset().unwrap();
    dec.send_packet(&packet(EDUC)).unwrap();
    assert!(dec.receive_frame().is_ok());
    // Default limits take the frame.
    let params = CodecParameters::video(CodecId::new(indeo5::CODEC_ID_STR));
    let mut dec = indeo5::make_decoder(&params).unwrap();
    dec.send_packet(&packet(INDEO5)).unwrap();
    assert!(dec.receive_frame().is_ok());
}
