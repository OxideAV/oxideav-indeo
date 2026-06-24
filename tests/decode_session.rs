//! Integration tests for the Indeo 3 multi-frame decode session +
//! stateful decoder, exercising the public `indeo3::DecodeSession` /
//! `indeo3::Indeo3Decoder` API exactly as a downstream consumer driving
//! a whole IV31 / IV32 frame sequence would.
//!
//! These tests build minimal synthetic codec frames (with varying
//! `frame_number` / `frame_flags` / `data_size`) and feed them through
//! the public session + decoder entry points, asserting the inter-frame
//! sequencing contract: the first-frame / seek INTRA gate (spec/01
//! §3.2 / §4), the NULL-frame repeat-previous output (spec/07 §6.3), and
//! the previous-frame-bit-9 reference-bank ping-pong (spec/07 §6.1 /
//! spec/05 §4.2). The per-plane pixel reconstruction is exercised by the
//! crate's lib tests; here the focus is the sequencing + repeat-previous
//! path, so frames use all-skipped planes (the structural decode yields
//! an empty reconstruction).

use oxideav_indeo::indeo3;

const FRAME_HEADER_LEN: usize = 16;
const COMBINED_HEADER_LEN: usize = 64;
const MAGIC_FRMH: u32 = 0x4652_4d48;
const REQUIRED_DEC_VERSION: u16 = 0x0020;
const NULL_FRAME_DATA_SIZE_BITS: u32 = 0x0000_0080;
const INTRA: u16 = 0x0004;
const BUF_SEL: u16 = 0x0200;
const DATA: u32 = 4096;

/// Build a frame whose three planes are all skipped (high bit set), with
/// a checksum that accounts for the `frame_number` so multi-frame
/// sequences validate.
fn skipped_frame(frame_number: u32, frame_flags: u16, data_size_bits: u32) -> Vec<u8> {
    let mut buf = vec![0u8; COMBINED_HEADER_LEN];
    let unknown1: u32 = 0;
    let frame_size: u32 = COMBINED_HEADER_LEN as u32;
    let check_sum = frame_number ^ unknown1 ^ frame_size ^ MAGIC_FRMH;
    buf[0x00..0x04].copy_from_slice(&frame_number.to_le_bytes());
    buf[0x04..0x08].copy_from_slice(&unknown1.to_le_bytes());
    buf[0x08..0x0c].copy_from_slice(&check_sum.to_le_bytes());
    buf[0x0c..0x10].copy_from_slice(&frame_size.to_le_bytes());

    let b = FRAME_HEADER_LEN;
    buf[b..b + 2].copy_from_slice(&REQUIRED_DEC_VERSION.to_le_bytes());
    buf[b + 2..b + 4].copy_from_slice(&frame_flags.to_le_bytes());
    buf[b + 4..b + 8].copy_from_slice(&data_size_bits.to_le_bytes());
    buf[b + 0x0c..b + 0x0e].copy_from_slice(&64u16.to_le_bytes());
    buf[b + 0x0e..b + 0x10].copy_from_slice(&64u16.to_le_bytes());
    let neg = 0x8000_0000u32;
    buf[b + 0x10..b + 0x14].copy_from_slice(&neg.to_le_bytes());
    buf[b + 0x14..b + 0x18].copy_from_slice(&neg.to_le_bytes());
    buf[b + 0x18..b + 0x1c].copy_from_slice(&neg.to_le_bytes());
    buf
}

#[test]
fn session_drives_a_full_sequence() {
    // INTRA(0) → INTER(1) → NULL(2) → INTER(3) → seek INTRA(20).
    let mut session = indeo3::DecodeSession::new();

    let a0 = session
        .admit(&skipped_frame(0, INTRA, DATA))
        .expect("first INTRA");
    assert_eq!(a0.admission, indeo3::FrameAdmission::FirstFrame);
    assert!(a0.carries_picture());

    let a1 = session
        .admit(&skipped_frame(1, 0x0000, DATA))
        .expect("seq INTER");
    assert_eq!(a1.admission, indeo3::FrameAdmission::Sequential);

    let a2 = session
        .admit(&skipped_frame(2, 0x0000, NULL_FRAME_DATA_SIZE_BITS))
        .expect("NULL");
    assert_eq!(a2.admission, indeo3::FrameAdmission::NullRepeat);
    assert!(!a2.carries_picture());
    assert_eq!(a2.return_value.code(), 1);

    let a3 = session
        .admit(&skipped_frame(3, 0x0000, DATA))
        .expect("seq INTER");
    assert_eq!(a3.admission, indeo3::FrameAdmission::Sequential);

    let a4 = session
        .admit(&skipped_frame(20, INTRA, DATA))
        .expect("seek INTRA");
    assert_eq!(a4.admission, indeo3::FrameAdmission::Seek);
    assert!(a4.admission.is_resync_point());

    assert_eq!(session.saved_frame_number(), Some(20));
}

#[test]
fn session_rejects_inter_first_frame() {
    let mut session = indeo3::DecodeSession::new();
    let err = session.admit(&skipped_frame(0, 0x0000, DATA)).unwrap_err();
    assert!(matches!(
        err,
        indeo3::SessionError::FirstFrameNotIntra { .. }
    ));
    assert!(session.is_empty());
}

#[test]
fn decoder_repeats_previous_output_on_null_frame() {
    let mut decoder = indeo3::Indeo3Decoder::new();

    // Frame 0: INTRA key frame.
    let o0 = decoder
        .decode(&skipped_frame(0, INTRA, DATA))
        .expect("first");
    assert!(!o0.repeated_previous);
    let held = o0.frame.clone();

    // Frame 1: NULL → re-emits frame 0's reconstruction byte-for-byte.
    let o1 = decoder
        .decode(&skipped_frame(1, 0x0000, NULL_FRAME_DATA_SIZE_BITS))
        .expect("null");
    assert!(o1.repeated_previous);
    assert_eq!(o1.frame.clone(), held);
}

#[test]
fn decoder_reference_bank_follows_previous_frame_bit9() {
    let mut decoder = indeo3::Indeo3Decoder::new();
    // First frame bit-9 set → the NEXT frame reads the secondary bank.
    decoder
        .decode(&skipped_frame(0, INTRA | BUF_SEL, DATA))
        .expect("first");
    let o1 = decoder
        .decode(&skipped_frame(1, 0x0000, DATA))
        .expect("seq");
    assert_eq!(o1.read_bank(), indeo3::Bank::Secondary);
    // Frame 1 bit-9 clear → frame 2 reads the primary bank.
    let o2 = decoder
        .decode(&skipped_frame(2, 0x0000, DATA))
        .expect("seq");
    assert_eq!(o2.read_bank(), indeo3::Bank::Primary);
}

#[test]
fn decoder_rejects_seek_to_inter_and_keeps_previous() {
    let mut decoder = indeo3::Indeo3Decoder::new();
    decoder
        .decode(&skipped_frame(0, INTRA, DATA))
        .expect("first");
    let held = decoder.previous_frame().cloned();
    // Seek (frame 10) carrying INTER → no valid reference, rejected.
    let err = decoder
        .decode(&skipped_frame(10, 0x0000, DATA))
        .unwrap_err();
    assert!(matches!(
        err,
        indeo3::DecoderError::Session(indeo3::SessionError::SeekNotIntra { .. })
    ));
    // The held previous frame is unchanged.
    assert_eq!(decoder.previous_frame().cloned(), held);
}

#[test]
fn decoder_output_frame_assembles_for_every_admitted_frame() {
    // Drive a short sequence and pull each frame's OutputFrame, as a
    // playback consumer would. Skipped-plane frames produce empty output
    // frames, but the call must succeed for every admitted frame
    // (including the repeated NULL frame).
    let mut decoder = indeo3::Indeo3Decoder::new();

    let frames = [
        skipped_frame(0, INTRA, DATA),
        skipped_frame(1, 0x0000, NULL_FRAME_DATA_SIZE_BITS),
        skipped_frame(2, 0x0000, DATA),
    ];

    for (i, f) in frames.iter().enumerate() {
        let out = decoder
            .decode(f)
            .unwrap_or_else(|e| panic!("frame {i}: {e}"));
        let output = out.frame.to_output_frame();
        // All planes skipped → no output planes, but the assembly runs.
        assert!(output.planes.is_empty(), "frame {i} has no planes");
    }
}
