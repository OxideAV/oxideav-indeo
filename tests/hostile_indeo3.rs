//! Hostile-input robustness sweeps for the Indeo 3 decode surface.
//!
//! The structural decoder's contract is: every input — arbitrary
//! garbage, truncations, bit flips, adversarial cell trees, hostile
//! mode-byte streams — either decodes or returns a *typed* error.
//! Nothing panics, nothing overflows, nothing loops unboundedly.
//! These sweeps drive that contract with deterministic
//! pseudo-random (LCG) corpora over the public API:
//!
//! * `indeo3::decode_frame` (spec/01 → spec/02 → spec/03) and the
//!   `decode_frame` → `reconstruct_frame` → `to_output_frame` chain,
//! * the multi-frame `indeo3::Indeo3Decoder` session,
//! * the cell executors (`reconstruct_cell_stateful`, `unpack_cell`,
//!   `run_cell_sequence`) over hostile mode-byte streams,
//! * adversarial cell trees (all-splits payloads at maximum picture
//!   dimensions — the deepest recursion the halving geometry allows).

use oxideav_indeo::indeo3;

const FRAME_HEADER_LEN: usize = 16;
const COMBINED_HEADER_LEN: usize = 64;
const MAGIC_FRMH: u32 = 0x4652_4d48;
const REQUIRED_DEC_VERSION: u16 = 0x0020;

/// Deterministic 64-bit LCG (no external crates; reproducible corpus).
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        // Constants from the classic 64-bit LCG family.
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn next_u8(&mut self) -> u8 {
        (self.next_u64() >> 56) as u8
    }
    fn next_range(&mut self, upper: usize) -> usize {
        if upper == 0 {
            return 0;
        }
        (self.next_u64() % upper as u64) as usize
    }
    fn fill(&mut self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            *b = self.next_u8();
        }
    }
}

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

/// Drive one buffer through the whole single-frame surface; a decode
/// success additionally drives reconstruction and output assembly.
fn exercise_frame(buf: &[u8]) {
    if let Ok(frame) = indeo3::decode_frame(buf) {
        if let Ok(recon) = indeo3::reconstruct_frame(&frame) {
            let _ = recon.to_output_frame();
        }
        let strips = indeo3::allocate_strip_buffers(&frame);
        let _ = indeo3::assemble_output(&frame, &strips);
    }
}

#[test]
fn arbitrary_garbage_buffers_never_panic() {
    let mut rng = Lcg::new(0x1d3_0433);
    for _ in 0..4000 {
        let len = rng.next_range(300);
        let mut buf = vec![0u8; len];
        rng.fill(&mut buf);
        exercise_frame(&buf);
    }
}

#[test]
fn random_payload_after_valid_header_never_panics() {
    let mut rng = Lcg::new(0xC0DE_C0DE);
    let dims: [(u16, u16); 6] = [
        (16, 16),
        (160, 120),
        (320, 240),
        (640, 480), // above MAX_WIDTH/HEIGHT — must be rejected, typed
        (17, 19),   // non-multiple-of-4 dims
        (0, 0),     // degenerate — typed rejection
    ];
    for i in 0..1500 {
        let (w, h) = dims[i % dims.len()];
        let payload_len = 4 + rng.next_range(160);
        let mut payload = vec![0u8; payload_len];
        rng.fill(&mut payload);
        // Sometimes force num_vectors = 0 (INTRA-shaped prelude).
        if i % 3 == 0 && payload.len() >= 4 {
            payload[..4].copy_from_slice(&0u32.to_le_bytes());
        }
        let y_off = (COMBINED_HEADER_LEN - FRAME_HEADER_LEN) as u32;
        let v_off = if i % 4 == 0 {
            0x8000_0000
        } else {
            rng.next_u64() as u32
        };
        let u_off = 0x8000_0000;
        let flags = (rng.next_u64() as u16) & 0x0330;
        let buf = build_frame(
            w,
            h,
            (payload.len() as u32) * 8,
            flags,
            y_off,
            v_off,
            u_off,
            &payload,
        );
        exercise_frame(&buf);
    }
}

#[test]
fn truncation_sweep_never_panics() {
    // A syntactically valid small frame, then decode every prefix.
    let mut payload = vec![0u8; 4 + 40];
    for (i, byte) in payload.iter_mut().enumerate().skip(4) {
        *byte = (i % 7) as u8;
    }
    let y_off = (COMBINED_HEADER_LEN - FRAME_HEADER_LEN) as u32;
    let buf = build_frame(
        16,
        16,
        (payload.len() as u32) * 8,
        0,
        y_off,
        0x8000_0000,
        0x8000_0000,
        &payload,
    );
    for len in 0..=buf.len() {
        exercise_frame(&buf[..len]);
    }
}

#[test]
fn single_byte_mutation_sweep_never_panics() {
    let mut payload = vec![0u8; 4 + 32];
    for (i, byte) in payload.iter_mut().enumerate().skip(4) {
        *byte = (i % 5) as u8;
    }
    let y_off = (COMBINED_HEADER_LEN - FRAME_HEADER_LEN) as u32;
    let base = build_frame(
        32,
        32,
        (payload.len() as u32) * 8,
        0,
        y_off,
        0x8000_0000,
        0x8000_0000,
        &payload,
    );
    // Flip every bit of every byte (8 * len mutants).
    for pos in 0..base.len() {
        for bit in 0..8 {
            let mut mutant = base.clone();
            mutant[pos] ^= 1 << bit;
            exercise_frame(&mutant);
        }
    }
}

#[test]
fn all_splits_payload_at_max_dimensions_terminates() {
    // An all-zero-bits payload reads as H_SPLIT at every node — the
    // deepest tree the halving geometry allows at the maximum picture
    // size (spec/01 MAX_WIDTH × MAX_HEIGHT = 640×480). The walk must
    // terminate with a typed outcome (DegenerateSplit once a split
    // would produce a zero-size child, or truncation), never
    // overflow the stack.
    let payload = vec![0u8; 4 + 4096];
    let y_off = (COMBINED_HEADER_LEN - FRAME_HEADER_LEN) as u32;
    let buf = build_frame(
        0x0280,
        0x01e0,
        (payload.len() as u32) * 8,
        0,
        y_off,
        0x8000_0000,
        0x8000_0000,
        &payload,
    );
    exercise_frame(&buf);

    // The alternating-splits variant (bits 00 01 00 01 … = byte 0x11
    // pattern) drives both split arms.
    let mut payload = vec![0u8; 4 + 4096];
    for byte in payload.iter_mut().skip(4) {
        *byte = 0x11;
    }
    let buf = build_frame(
        0x0280,
        0x01e0,
        (payload.len() as u32) * 8,
        0,
        y_off,
        0x8000_0000,
        0x8000_0000,
        &payload,
    );
    exercise_frame(&buf);
}

#[test]
fn multi_frame_session_hostile_sequences_never_panic() {
    let mut rng = Lcg::new(0x5E55_1011);
    let mut decoder = indeo3::Indeo3Decoder::new();
    for i in 0..600 {
        let hostile = rng.next_range(4) == 0;
        let buf = if hostile {
            let len = rng.next_range(200);
            let mut b = vec![0u8; len];
            rng.fill(&mut b);
            b
        } else {
            let mut payload = vec![0u8; 4 + rng.next_range(80)];
            rng.fill(&mut payload);
            if payload.len() >= 4 {
                payload[..4].copy_from_slice(&0u32.to_le_bytes());
            }
            let y_off = (COMBINED_HEADER_LEN - FRAME_HEADER_LEN) as u32;
            // Alternate picture-carrying and NULL frames.
            let bits = if i % 5 == 0 {
                0x0000_0080
            } else {
                (payload.len() as u32) * 8
            };
            build_frame(
                48,
                32,
                bits,
                (rng.next_u64() as u16) & 0x0330,
                y_off,
                0x8000_0000,
                0x8000_0000,
                &payload,
            )
        };
        if let Ok(out) = decoder.decode(&buf) {
            let _ = out.to_output_frame();
            let _ = out.to_yuv_frame();
        }
    }
}

#[test]
fn hostile_mode_byte_streams_never_panic_executors() {
    let table = indeo3::DyadDeltaTable::load();
    let arena = indeo3::VqArena::new();
    let mut rng = Lcg::new(0xBADC_0FFE);
    let variants = [
        indeo3::CellVariant::Plain,
        indeo3::CellVariant::WithEdge,
        indeo3::CellVariant::DoubledRow,
        indeo3::CellVariant::FullyDoubled,
    ];
    for i in 0..3000 {
        let width = 1 + rng.next_range(4);
        let rows = 1 + rng.next_range(8);
        let top = rng.next_range(4) * 0xb0;
        let strip_rows = 2 + rng.next_range(20);
        let mut strip = vec![0u8; 0xb0 * strip_rows];
        let mut stream = vec![0u8; rng.next_range(40)];
        rng.fill(&mut stream);

        let geometry = indeo3::CellReconstructGeometry {
            width_dwords: width,
            source_rows: rows,
            top_left_offset: top,
        };

        // Static executor (stateless + stateful + carry-in).
        let _ = indeo3::reconstruct_cell_static(&mut strip, geometry, &stream, &table);
        if let Ok(run) =
            indeo3::reconstruct_cell_stateful(&mut strip, geometry, &stream, &table, i % 7 == 0)
        {
            assert!(run.bytes_consumed <= stream.len());
        }

        // Arena unpacker across all four variants.
        let variant = variants[i % variants.len()];
        if let Ok(run) = indeo3::unpack_cell(
            &mut strip,
            geometry,
            variant,
            &stream,
            &table,
            &arena,
            i % 11 == 0,
        ) {
            assert!(run.bytes_consumed <= stream.len());
        }

        // Sequence driver over a few consecutive cells.
        let cells = [geometry, geometry, geometry];
        if let Ok(report) = indeo3::run_cell_sequence(&mut strip, &cells, &stream, &table) {
            assert!(report.bytes_consumed <= stream.len());
            assert!(report.steps.len() <= cells.len());
        }
    }
}

#[test]
fn hostile_streams_with_planted_arena_content_never_panic() {
    // Same executor sweep, but over an arena filled with adversarial
    // (sign-heavy) content so the continuation / fault paths run hot.
    let table = indeo3::DyadDeltaTable::load();
    let mut arena = indeo3::VqArena::new();
    let mut rng = Lcg::new(0xA5A5_5A5A);
    rng.fill(&mut arena.bytes_mut()[..]);
    for i in 0..2000 {
        let geometry = indeo3::CellReconstructGeometry {
            width_dwords: 1 + rng.next_range(3),
            source_rows: 1 + rng.next_range(4),
            top_left_offset: rng.next_range(3) * 0xb0,
        };
        let mut strip = vec![0u8; 0xb0 * (2 + rng.next_range(12))];
        let mut stream = vec![0u8; 1 + rng.next_range(24)];
        rng.fill(&mut stream);
        // Bias half the corpus toward literal (non-escape) bytes so
        // the dyad path dominates.
        if i % 2 == 0 {
            for b in stream.iter_mut() {
                *b &= 0x7F;
            }
        }
        let variant = if i % 2 == 0 {
            indeo3::CellVariant::Plain
        } else {
            indeo3::CellVariant::WithEdge
        };
        if let Ok(run) = indeo3::unpack_cell(
            &mut strip, geometry, variant, &stream, &table, &arena, false,
        ) {
            assert!(run.bytes_consumed <= stream.len());
        }
    }
}
