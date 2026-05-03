//! PSNR cross-decode against ffmpeg's reference Indeo 2 decoder.
//!
//! The fixture pair `tests/fixtures/rt21_vpar0019_10packets.bin` and
//! `tests/fixtures/rt21_vpar0019_10frames_yuv420p.yuv` was generated
//! once at fixture-creation time:
//!
//! 1. The first 10 video chunks of `VPAR0019.AVI` (the canonical
//!    public-domain RT21 reference sample from
//!    `samples.ffmpeg.org/V-codecs/RT21/`) were extracted into the
//!    `.bin` file as a sequence of `(u32 little-endian length) (raw
//!    chunk bytes)` records — a tiny self-describing container so we
//!    don't need an AVI demuxer in tests.
//! 2. The same 10 frames were decoded by the system `ffmpeg` binary
//!    with `ffmpeg -i VPAR0019.AVI -frames:v 10 -pix_fmt yuv420p -f
//!    rawvideo …`. The resulting raw `Yuv420P` pixel grid (160 × 120
//!    luma + 80 × 60 chroma, no padding) is the per-frame byte-for-byte
//!    ground truth.
//!
//! The expected outcome is an essentially perfect (≈infinite) PSNR for
//! intra frames since the codec is fully reproducible from the static
//! tables documented in §8 of the trace doc. We assert a generous
//! lower bound (≥ 35 dB) so chroma upsample-method drift never trips
//! a regression — the headline PSNR is logged for visibility.

use oxideav_core::{CodecId, Decoder, Frame, Packet, TimeBase};
use oxideav_indeo::v2::Indeo2Decoder;

const PACKETS: &[u8] = include_bytes!("fixtures/rt21_vpar0019_10packets.bin");
const REF_YUV: &[u8] = include_bytes!("fixtures/rt21_vpar0019_10frames_yuv420p.yuv");

const W: usize = 160;
const H: usize = 120;
const CW: usize = W / 2;
const CH: usize = H / 2;
const FRAME_BYTES: usize = W * H + 2 * CW * CH; // yuv420p

fn iter_packets(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut off = 0usize;
    std::iter::from_fn(move || {
        if off + 4 > data.len() {
            return None;
        }
        let n = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        if off + n > data.len() {
            return None;
        }
        let chunk = &data[off..off + n];
        off += n;
        Some(chunk)
    })
}

fn psnr_u8(reference: &[u8], decoded: &[u8]) -> f64 {
    assert_eq!(reference.len(), decoded.len());
    let mut mse_acc: u64 = 0;
    for (&r, &d) in reference.iter().zip(decoded.iter()) {
        let diff = r as i32 - d as i32;
        mse_acc += (diff * diff) as u64;
    }
    if mse_acc == 0 {
        return f64::INFINITY;
    }
    let mse = mse_acc as f64 / reference.len() as f64;
    10.0 * (255.0f64 * 255.0 / mse).log10()
}

#[test]
fn decodes_ten_frames_against_ffmpeg_reference_with_high_psnr() {
    assert_eq!(
        REF_YUV.len(),
        10 * FRAME_BYTES,
        "reference yuv must hold 10 yuv420p frames at {W}×{H}"
    );

    let mut dec = Indeo2Decoder::new(CodecId::new("indeo2"));
    let mut frame_idx = 0usize;
    let mut min_psnr = f64::INFINITY;
    let mut sum_psnr = 0.0f64;
    let mut count = 0u32;
    let mut min_y_psnr = f64::INFINITY;

    for packet_bytes in iter_packets(PACKETS) {
        let pkt = Packet::new(frame_idx as u32, TimeBase::new(1, 15), packet_bytes.to_vec());
        dec.send_packet(&pkt).expect("send_packet");
        let frame = dec.receive_frame().expect("receive_frame");
        let Frame::Video(vf) = frame else {
            panic!("expected video frame");
        };
        assert_eq!(vf.planes.len(), 3);
        assert_eq!(vf.planes[0].data.len(), W * H, "Y plane size");
        assert_eq!(vf.planes[1].data.len(), CW * CH, "U plane size");
        assert_eq!(vf.planes[2].data.len(), CW * CH, "V plane size");

        // Re-pack into one yuv420p contiguous buffer (Y, U, V) so we
        // can diff against the reference.
        let mut decoded = Vec::with_capacity(FRAME_BYTES);
        decoded.extend_from_slice(&vf.planes[0].data);
        decoded.extend_from_slice(&vf.planes[1].data);
        decoded.extend_from_slice(&vf.planes[2].data);
        assert_eq!(decoded.len(), FRAME_BYTES);

        let ref_frame = &REF_YUV[frame_idx * FRAME_BYTES..(frame_idx + 1) * FRAME_BYTES];
        let psnr = psnr_u8(ref_frame, &decoded);
        // Per-plane breakdown: the Y plane is byte-exact (the entropy
        // decode is fully deterministic from the §8 tables); chroma
        // PSNR can drop a little because we 2×2-replicate yuv410p →
        // yuv420p whereas ffmpeg's swscale chooses its own filter for
        // the same widening. The headline `psnr` reflects the merged
        // YUV buffer.
        let y_psnr = psnr_u8(&ref_frame[..W * H], &decoded[..W * H]);
        eprintln!(
            "frame {frame_idx}: PSNR = {psnr:.2} dB  (Y-plane only: {y_psnr:.2} dB)"
        );
        min_psnr = min_psnr.min(psnr);
        min_y_psnr = min_y_psnr.min(y_psnr);
        if psnr.is_finite() {
            sum_psnr += psnr;
            count += 1;
        }
        frame_idx += 1;
    }

    assert_eq!(frame_idx, 10, "expected 10 packets in fixture");
    let avg_psnr = if count > 0 {
        sum_psnr / count as f64
    } else {
        f64::INFINITY
    };
    eprintln!("min PSNR = {min_psnr:.2} dB, avg (finite-only) = {avg_psnr:.2} dB");
    eprintln!("min Y-plane PSNR = {min_y_psnr:.2} dB");
    assert!(
        min_psnr >= 35.0 || min_psnr.is_infinite(),
        "min PSNR {min_psnr:.2} dB below 35 dB floor"
    );
    // Y plane is bit-exact against ffmpeg's reference: every Indeo 2
    // pair / run codeword resolves deterministically against the
    // §8 tables, with no quantiser, no rounding, no DSP. Treat any
    // finite Y PSNR (i.e. any byte mismatch) as a regression.
    assert!(
        min_y_psnr.is_infinite(),
        "Y plane PSNR dropped from inf — minimum {min_y_psnr:.2} dB. Entropy or pair/run regression."
    );
}

#[test]
fn first_frame_y_plane_is_not_mid_grey() {
    // Round-1 acceptance regression guard: after the entropy decoder
    // landed in round 2 the Y plane MUST carry real pixel data, not
    // the round-1 `vec![128; w*h]` placeholder. A bit-exact check
    // against the reference is done by the PSNR test above; this is a
    // cheap structural check that fails loudly if the entropy
    // pipeline ever regresses to the placeholder.
    let mut dec = Indeo2Decoder::new(CodecId::new("indeo2"));
    let first = iter_packets(PACKETS).next().expect("at least one packet");
    let pkt = Packet::new(0, TimeBase::new(1, 15), first.to_vec());
    dec.send_packet(&pkt).unwrap();
    let frame = dec.receive_frame().unwrap();
    let Frame::Video(vf) = frame else {
        panic!("expected video");
    };
    let y = &vf.planes[0].data;
    let unique_values: std::collections::HashSet<u8> = y.iter().copied().collect();
    assert!(
        unique_values.len() > 32,
        "first-frame luma has only {} unique values — likely the round-1 mid-grey placeholder is back",
        unique_values.len()
    );
    let any_non_128 = y.iter().any(|&v| v != 128);
    assert!(any_non_128, "first-frame luma is uniformly 128 — placeholder regression");
}
