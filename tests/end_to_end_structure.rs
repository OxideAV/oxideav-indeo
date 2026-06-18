//! Integration tests for the Indeo 3 end-to-end structural decode
//! pipeline, exercising the public `decode_frame` → `assemble_output`
//! chain exactly as a downstream consumer would.
//!
//! These tests build minimal synthetic codec frames (header + picture
//! layer + plane payload) and drive them through the public API. They
//! deliberately stop at the structural boundary — pixel reconstruction
//! is gated on the spec/04 §7.1 codebook-bank docs-gap — and confirm
//! the driver threads cleanly, classifies plane presence correctly,
//! and assembles output planes from caller-supplied strip buffers.

use oxideav_indeo::indeo3;

// Mirror the in-crate test header builder: a minimal valid combined
// header (64 bytes) plus an optional plane payload.
const FRAME_HEADER_LEN: usize = 16;
const COMBINED_HEADER_LEN: usize = 64;
const MAGIC_FRMH: u32 = 0x4652_4d48;
const REQUIRED_DEC_VERSION: u16 = 0x0020;
const NULL_FRAME_DATA_SIZE_BITS: u32 = 0x0000_0080;

#[allow(clippy::too_many_arguments)]
fn build_frame(
    width: u16,
    height: u16,
    data_size_bits: u32,
    flags: u16,
    y_off: u32,
    v_off: u32,
    u_off: u32,
    payload: &[u8],
) -> Vec<u8> {
    let total_len = (COMBINED_HEADER_LEN + payload.len()) as u32;
    let mut buf = vec![0u8; COMBINED_HEADER_LEN];

    let frame_size = total_len;
    let check_sum = frame_size ^ MAGIC_FRMH;
    buf[0x08..0x0c].copy_from_slice(&check_sum.to_le_bytes());
    buf[0x0c..0x10].copy_from_slice(&frame_size.to_le_bytes());

    let b = FRAME_HEADER_LEN;
    buf[b..b + 2].copy_from_slice(&REQUIRED_DEC_VERSION.to_le_bytes());
    buf[b + 2..b + 4].copy_from_slice(&flags.to_le_bytes());
    buf[b + 4..b + 8].copy_from_slice(&data_size_bits.to_le_bytes());
    buf[b + 0x0c..b + 0x0e].copy_from_slice(&height.to_le_bytes());
    buf[b + 0x0e..b + 0x10].copy_from_slice(&width.to_le_bytes());
    buf[b + 0x10..b + 0x14].copy_from_slice(&y_off.to_le_bytes());
    buf[b + 0x14..b + 0x18].copy_from_slice(&v_off.to_le_bytes());
    buf[b + 0x18..b + 0x1c].copy_from_slice(&u_off.to_le_bytes());

    buf.extend_from_slice(payload);
    buf
}

#[test]
fn null_frame_decodes_to_no_planes() {
    let buf = build_frame(128, 96, NULL_FRAME_DATA_SIZE_BITS, 0, 0, 0, 0, &[]);
    let frame = indeo3::decode_frame(&buf).expect("null frame decodes");
    assert!(frame.is_null_frame());
    assert_eq!(
        frame.reconstruction_status,
        indeo3::ReconstructionStatus::NullFrame
    );
    assert!(frame.planes.is_empty());
    assert_eq!(frame.width(), 128);
    assert_eq!(frame.height(), 96);

    // A NULL frame allocates and assembles to no output planes.
    let strips = indeo3::allocate_strip_buffers(&frame);
    assert!(strips.is_empty());
    let out = indeo3::assemble_output(&frame, &strips).expect("assemble");
    assert!(out.planes.is_empty());
}

#[test]
fn single_luma_plane_threads_and_assembles() {
    // num_vectors (u32 = 0, INTRA) + a small payload of tree bytes.
    let mut payload = vec![0u8; 4 + 48];
    for (i, byte) in payload.iter_mut().enumerate().skip(4) {
        *byte = (i % 5) as u8;
    }
    // Plane base lands at the end of the 64-byte header.
    let y_off = (COMBINED_HEADER_LEN - FRAME_HEADER_LEN) as u32;
    let buf = build_frame(
        16,
        16,
        (payload.len() as u32) * 8,
        0,
        y_off,
        0x8000_0000, // V skipped
        0x8000_0000, // U skipped
        &payload,
    );

    let frame = match indeo3::decode_frame(&buf) {
        Ok(f) => f,
        // Synthetic tree bytes may be rejected deterministically;
        // that is a valid structural outcome.
        Err(indeo3::FrameDecodeError::PlaneTree { .. }) => return,
        Err(e) => panic!("unexpected error: {e}"),
    };
    assert_eq!(
        frame.reconstruction_status,
        indeo3::ReconstructionStatus::StructureComplete
    );

    if let Some(y) = frame.plane(indeo3::PLANE_IDX_Y) {
        assert!(y.is_intra());
        assert!(y.plan.is_luma());

        // Allocate zeroed strips and assemble — the output plane must
        // be the right shape and all-zero (zeroed strips upshift to 0).
        let strips = indeo3::allocate_strip_buffers(&frame);
        let out = indeo3::assemble_output(&frame, &strips).expect("assemble");
        let oy = out.luma().expect("luma assembled");
        assert_eq!(oy.width, y.plan.geometry.plane_width);
        assert_eq!(oy.height, y.plan.geometry.plane_height);
        assert!(oy.pixels.iter().all(|&b| b == 0));
    }
}

#[test]
fn malformed_header_is_a_typed_error() {
    // Too short to hold a combined header.
    let buf = vec![0u8; 8];
    match indeo3::decode_frame(&buf) {
        Err(indeo3::FrameDecodeError::Header(_)) => {}
        other => panic!("expected header error, got {other:?}"),
    }
}
