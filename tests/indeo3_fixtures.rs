//! Indeo 3 real-bitstream fixture tests.
//!
//! The vendored `IV32` corpora (see `tests/data/README.md`) are the
//! crate's first real-stream validation targets: coded access units
//! plus a black-box reference decode (`expected.yuv`, planar 4:1:0).
//!
//! What these tests pin (r451):
//!
//! * The spec/01 + spec/02 header stack parses **every access unit of
//!   both fixtures** — dimensions, plane layout, per-frame
//!   `alt_quant[]` / `cb_offset`, plane preludes.
//! * Byte-exact stream anchors for the first frame's plane payloads
//!   (the binary-tree bits + the first VQ_DATA leaf's codebook-index
//!   byte and data cursor).
//! * **The first real-stream pixel decode**: the first luma cell's
//!   mode-byte stream drives the staging-image row-delta arithmetic
//!   ([`StagingImage::row_delta`]) to the reference decode's exact
//!   pixels — 384 luma pixels byte-exact against `expected.yuv`,
//!   including the fixture-arbitrated `0x40` strip-boundary
//!   predictor and the `0xFD` predictor-propagation semantics.
//!
//! What remains gated (precise docs-gaps, reported in the r451 round
//! report): the cell-geometry bank tables (`IR32_32.DLL!0x100038f0`,
//! `spec/04 §5.3`/`§7.9` — undocumented) that drive the cell
//! sequencing past the first cell, the intra-frame contents of the
//! packed VQ-entry table at `inner_instance[0..0x3ff]` (`spec/04
//! §7.5`), and the variant-B/C/D mode-byte dispatch tables
//! (`spec/06 §4.1`, staged at the RVA level only).

use oxideav_indeo::indeo3::{
    CodebookSeedArea, FrameHeader, PictureLayer, PlanePresence, RowDeltaOutcome, StagingImage,
    VqArena,
};

const INTRA_BIN: &[u8] = include_bytes!("data/iv32-160x120-all-intra/samples.bin");
const INTRA_IDX: &str = include_str!("data/iv32-160x120-all-intra/samples-index.csv");
const INTRA_YUV: &[u8] = include_bytes!("data/iv32-160x120-all-intra/expected.yuv");

const INTER_BIN: &[u8] = include_bytes!("data/iv32-176x144-4frame-intra-period/samples.bin");
const INTER_IDX: &str = include_str!("data/iv32-176x144-4frame-intra-period/samples-index.csv");
const INTER_YUV: &[u8] = include_bytes!("data/iv32-176x144-4frame-intra-period/expected.yuv");

fn frames<'a>(bin: &'a [u8], idx: &str) -> Vec<&'a [u8]> {
    idx.lines()
        .skip(1)
        .map(|l| {
            let f: Vec<usize> = l.split(',').skip(1).map(|v| v.parse().unwrap()).collect();
            &bin[f[0]..f[0] + f[1]]
        })
        .collect()
}

#[test]
fn all_intra_fixture_headers_parse_on_every_frame() {
    let frames = frames(INTRA_BIN, INTRA_IDX);
    assert_eq!(frames.len(), 8);
    assert_eq!(INTRA_YUV.len(), 8 * (160 * 120 + 2 * 40 * 30));

    for (i, frame) in frames.iter().enumerate() {
        let header = FrameHeader::parse(frame).unwrap_or_else(|e| panic!("frame {i} header: {e}"));
        assert_eq!(header.bitstream.width, 160, "frame {i}");
        assert_eq!(header.bitstream.height, 120, "frame {i}");
        let pl = PictureLayer::parse(&header, frame).unwrap_or_else(|e| panic!("frame {i}: {e}"));
        // All three planes present, each with an empty MV prelude
        // (all-intra stream).
        for (p, plane) in pl.planes.iter().enumerate() {
            let PlanePresence::Present(prelude) = plane else {
                panic!("frame {i} plane {p} absent");
            };
            assert_eq!(prelude.num_vectors, 0, "frame {i} plane {p}");
        }
    }
}

#[test]
fn intra_period_fixture_headers_parse_on_every_frame() {
    let frames = frames(INTER_BIN, INTER_IDX);
    assert_eq!(frames.len(), 8);
    assert_eq!(INTER_YUV.len(), 8 * (176 * 144 + 2 * 44 * 36));

    for (i, frame) in frames.iter().enumerate() {
        let header = FrameHeader::parse(frame).unwrap_or_else(|e| panic!("frame {i} header: {e}"));
        assert_eq!(header.bitstream.width, 176, "frame {i}");
        assert_eq!(header.bitstream.height, 144, "frame {i}");
        let pl = PictureLayer::parse(&header, frame).unwrap_or_else(|e| panic!("frame {i}: {e}"));
        let mvs: usize = pl
            .planes
            .iter()
            .map(|p| match p {
                PlanePresence::Present(pr) => pr.num_vectors as usize,
                _ => 0,
            })
            .sum();
        // Frames 0 and 4 are the container-flagged intra frames: no
        // motion vectors. The inter frames carry MV preludes.
        if i % 4 == 0 {
            assert_eq!(mvs, 0, "intra frame {i} has MVs");
        } else {
            assert!(mvs > 0, "inter frame {i} has no MVs");
        }
    }
}

#[test]
fn first_frame_stream_anchors_are_byte_exact() {
    let frames = frames(INTRA_BIN, INTRA_IDX);
    let frame = frames[0];
    let header = FrameHeader::parse(frame).expect("header");

    // The per-frame codebook selection: cb_offset 0 and the ramped
    // alt_quant[] (both nibble sequences ascend across the 16 bands).
    assert_eq!(header.bitstream.cb_offset, 0);
    assert_eq!(
        header.bitstream.alt_quant,
        [2, 20, 38, 56, 74, 92, 110, 127, 130, 148, 166, 184, 202, 220, 238, 255]
    );
    // The overlay accepts the frame's selection over the built staging
    // image (spec/04 §6.1 over §5.2).
    let staging = StagingImage::build(&CodebookSeedArea::load());
    let mut arena = VqArena::new();
    let bias = arena
        .apply_alt_quant(
            &staging,
            &header.bitstream.alt_quant,
            header.bitstream.cb_offset,
        )
        .expect("overlay");
    assert_eq!(bias, 0);

    // Y-plane payload anchor: the first two bytes are the binary-tree
    // codes (INTRA root, then the alternating V/H split chain), the
    // third is the first VQ_DATA leaf's codebook-index byte 0x08, and
    // the mode-byte stream follows.
    let pl = PictureLayer::parse(&header, frame).expect("picture layer");
    let PlanePresence::Present(y) = &pl.planes[0] else {
        panic!("Y absent")
    };
    let payload = &frame[y.bitstream_offset..];
    assert_eq!(
        &payload[..8],
        &[0x91, 0x13, 0x08, 0x6c, 0x6c, 0xd3, 0xfd, 0x6c]
    );
}

#[test]
fn first_cell_pixels_decode_byte_exact_against_reference() {
    // The first luma cell of the all-intra fixture's first frame:
    // 24×16 pixels at (0,0), whose mode-byte stream is
    // [6c 6c] [d3] [FD]. Driving the staging-image row-delta
    // arithmetic (block 0 — the frame's cb_offset-0 window) with the
    // fixture-arbitrated 0x40 strip-boundary predictor:
    //
    //   row 0: two-byte form  6c+6c → 0x0A0A0A0A  (output 20)
    //   row 1: one-byte form  d3    → 0x08080808  (output 16)
    //   rows 2..15: 0xFD — null delta, repeat the row above (16s)
    //
    // — byte-exact against the black-box reference decode for all
    // 24×16 = 384 pixels.
    let staging = StagingImage::build(&CodebookSeedArea::load());

    let row0 = match staging.row_delta(0, 0x6c, Some(0x6c), 0x4040_4040) {
        Some(RowDeltaOutcome::Complete {
            value,
            used_continuation: true,
        }) => value,
        other => panic!("row 0: {other:?}"),
    };
    assert_eq!(row0, 0x0a0a_0a0a);
    // The one-byte probe first reports the continuation need.
    assert_eq!(
        staging.row_delta(0, 0x6c, None, 0x4040_4040),
        Some(RowDeltaOutcome::NeedsContinuation)
    );

    let row1 = match staging.row_delta(0, 0xd3, None, row0) {
        Some(RowDeltaOutcome::Complete {
            value,
            used_continuation: false,
        }) => value,
        other => panic!("row 1: {other:?}"),
    };
    assert_eq!(row1, 0x0808_0808);

    // Reference pixels: expected.yuv frame 0's luma plane, upshifted
    // 7-bit → 8-bit inverse: internal b ↔ output (b & 0x7f) << 1.
    let y = &INTRA_YUV[..160 * 120];
    let out0 = ((row0 & 0x7f) << 1) as u8;
    let out1 = ((row1 & 0x7f) << 1) as u8;
    let mut exact = 0usize;
    for row in 0..16usize {
        let want = if row == 0 { out0 } else { out1 };
        for col in 0..24usize {
            assert_eq!(
                y[row * 160 + col],
                want,
                "reference pixel ({col},{row}) disagrees with the decoded cell"
            );
            exact += 1;
        }
    }
    assert_eq!(exact, 384);
}

#[test]
fn chroma_boundary_predictor_is_mid_range() {
    // The all-intra fixture's chroma planes are dominated by neutral
    // 128 — internal 0x40 — reconstructed through repeat-row-above
    // chains that bottom out at the strip boundary predictor. A zero
    // boundary would surface as 0-valued output in those chains; the
    // reference decode has none. (This is the evidence pinning
    // TOP_OF_STRIP_PREDICTOR = 0x40; spec/07 §7.4 resolved.)
    let frame0_u = &INTRA_YUV[160 * 120..160 * 120 + 40 * 30];
    let neutral = frame0_u.iter().filter(|&&b| b == 128).count();
    assert!(
        neutral * 2 > frame0_u.len(),
        "U plane not neutral-dominated"
    );
    assert_eq!(frame0_u.iter().filter(|&&b| b == 0).count(), 0);
}

#[test]
fn fixture_frames_never_panic_the_structural_decoder() {
    // Robustness: every access unit of both fixtures must run the
    // whole-frame structural driver without panicking (typed results
    // only). The cell sequencing past the first data-bearing cell
    // rides the geometry-bank docs-gap, so the outcome is not pinned
    // here — only its safety.
    for (bin, idx) in [(INTRA_BIN, INTRA_IDX), (INTER_BIN, INTER_IDX)] {
        for frame in frames(bin, idx) {
            let _ = oxideav_indeo::indeo3::decode_frame(frame);
            let _ = oxideav_indeo::indeo3::decode_video_frame(frame, Some(0));
        }
    }
}

#[test]
fn five_plane_columns_decode_byte_exact_against_reference() {
    // The fixture-arbitrated big result (r451): the 160×120 luma
    // plane decomposes into eight full-height base columns — 24 px
    // plain and 16 px doubled, alternating — consumed in raster
    // order with no interleaved tree codes (the cell sequencing the
    // undocumented geometry banks drive). Decoding the first frame's
    // luma stream through the row-stream executor reproduces the
    // reference decode byte-exactly across the first five base
    // columns EXCEPT where the picture's detail region (the cube,
    // rows ~30..101) is re-coded by the subdivision/overlay mechanism
    // the staged docs do not yet pin — the reported docs-gap. The
    // exact per-column match counts are pinned below so any
    // refinement shows up as a diff.
    use oxideav_indeo::indeo3::{decode_cell_rows, expand_doubled_rows};

    let frames = frames(INTRA_BIN, INTRA_IDX);
    let frame = frames[0];
    let header = FrameHeader::parse(frame).expect("header");
    let pl = PictureLayer::parse(&header, frame).expect("picture layer");
    let PlanePresence::Present(y) = &pl.planes[0] else {
        panic!("Y absent")
    };
    // Payload: 2 bytes of tree codes + the codebook-index byte, then
    // the first cell's mode-byte stream.
    let payload = &frame[y.bitstream_offset..];
    let stream = &payload[3..];

    let staging = StagingImage::build(&CodebookSeedArea::load());
    let expected_y = &INTRA_YUV[..160 * 120];

    // Columns: (x, width, doubled). The first five decode cleanly;
    // their coded-row counts are 120 (plain) / 60 (doubled).
    let columns = [
        (0usize, 24usize, false),
        (24, 16, true),
        (40, 24, false),
        (64, 16, true),
        (80, 24, false),
    ];

    let mut cursor = 0usize;
    let mut exact = 0usize;
    for &(x, w, doubled) in &columns {
        let coded_rows = if doubled { 60 } else { 120 };
        let boundary = vec![0x40u8; w];
        let run = decode_cell_rows(&staging, 0, &boundary, &stream[cursor..], w, coded_rows)
            .unwrap_or_else(|e| panic!("column at x={x}: {e}"));
        cursor += run.bytes_consumed;
        let out_rows = if doubled {
            expand_doubled_rows(&run.rows)
        } else {
            run.rows.clone()
        };
        assert_eq!(out_rows.len(), 120, "column at x={x}");
        let mut col_exact = 0usize;
        for (row_idx, row) in out_rows.iter().enumerate() {
            for (col_idx, &b) in row.iter().enumerate() {
                let got = (b & 0x7f) << 1;
                let want = expected_y[row_idx * 160 + x + col_idx];
                if got == want {
                    col_exact += 1;
                }
                // Outside the detail region every pixel is byte-exact.
                if !(30..=101).contains(&row_idx) {
                    assert_eq!(
                        got,
                        want,
                        "pixel ({}, {row_idx}) diverges outside the detail region",
                        x + col_idx
                    );
                }
            }
        }
        // Pinned per-column match counts (frame 0): flip upward when
        // the subdivision/overlay mechanism lands.
        let expected_exact = match x {
            0 => 2644,
            24 => 1361,
            40 => 2503,
            64 => 1488,
            80 => 2408,
            _ => unreachable!(),
        };
        assert_eq!(col_exact, expected_exact, "column at x={x}");
        exact += col_exact;
    }
    assert_eq!(exact, 10404);
    // Each of the five columns' streams is byte-exactly the 4-byte
    // [6c 6c] [d3] [FD] pattern the reference encoder emitted.
    assert_eq!(cursor, 20);
}
