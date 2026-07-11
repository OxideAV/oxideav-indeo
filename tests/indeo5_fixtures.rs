//! Indeo 5 real-bitstream fixture tests.
//!
//! Decode the two vendored Intel/Ligos-encoded `IV50` INTRA keyframes
//! (see `tests/data/README.md`) end-to-end through the public
//! `decode_intra_picture` API and pin the structural outcome: every
//! band's MB-header phase and per-block coefficient phase must decode
//! without error and consume the band payload to within its trailing
//! padding. These frames are what arbitrated the r388 entropy-layer
//! readings (prefix-form codebooks, rv-table composite decode, the
//! MB-headers-then-block-data tile split, CBP-before-qdelta order,
//! §2.8 whole-tile explicit sizes).

use oxideav_indeo::indeo5::decode_intra_picture;

const EDUC: &[u8] = include_bytes!("data/intra-240x180-educ.iv50");
const INDEO5: &[u8] = include_bytes!("data/intra-320x240-indeo5.iv50");

#[test]
fn educ_240x180_black_frame_decodes() {
    let d = decode_intra_picture(EDUC).expect("decode");
    assert!(d.parse_complete);
    assert!(d.fully_reconstructed());

    // GOP: 240x180 YVU9, decomp 0 -> one band per plane, one tile
    // each; luma 15x12 MBs (mb 16), chroma 60x45 at mb 4 -> 15x12.
    assert_eq!(d.stats.bands, 3);
    assert_eq!(d.stats.empty_bands, 0);
    assert_eq!(d.stats.tiles, 3);
    assert_eq!(d.stats.mbs, 3 * 180);
    assert_eq!(d.stats.mbs_skipped, 0);

    // A black frame: a single coded block in the whole picture (whose
    // stream exercises the escape path once); everything else is
    // uncoded / DC-less.
    assert_eq!(d.stats.coded_blocks, 1);
    assert_eq!(d.stats.escapes, 1);

    // Byte-exact band exhaustion: consumed == declared for all three
    // bands (Y 126, U 55, V 55 — the documented band chain).
    let sizes: Vec<(u64, Option<u32>)> = d
        .band_traces
        .iter()
        .map(|t| (t.consumed, t.declared))
        .collect();
    assert_eq!(
        sizes,
        vec![(126, Some(126)), (55, Some(55)), (55, Some(55)),]
    );

    // Zero-coefficient reconstruction -> uniform planes (the vendor
    // decoder emits Y=16, U=V=128 for this frame through its own
    // output conversion; our planar output pins the mid-grey zero
    // state pending the spec/08 output-LUT staging).
    let out = d.output.as_ref().expect("output");
    assert_eq!(out.data.len(), 240 * 180 + 2 * 60 * 45);
    assert!(out.data.iter().all(|&b| b == 128));

    // spec/08 §7 reconstruction oracle (formula recovered by black-box
    // validation). The educ band checksums are Y=0x2c00, U=V=0. Our
    // uniform-128 reconstruction is byte-sum-exact for the two chroma
    // bands (their real content is neutral 128) but not the luma band
    // (whose real content is Y=16), so exactly two of the three bands
    // verify — a quantitative pin of the coefficient-transform frontier.
    use oxideav_indeo::indeo5::ChecksumStatus;
    assert_eq!(d.bands.len(), 3);
    assert_eq!(d.bands[0].plane_idx, 0);
    assert!(matches!(
        d.bands[0].checksum,
        ChecksumStatus::Mismatch {
            stored: 0x2c00,
            computed: 0
        }
    ));
    assert!(d.bands[1].checksum.verified());
    assert!(d.bands[2].checksum.verified());
    assert_eq!(d.bands_verified(), 2);
    // The whole-frame checksum stays a mismatch while any plane is
    // gated (stored 0x1800 vs the uniform-128 recompute).
    assert!(matches!(
        d.frame_checksum,
        ChecksumStatus::Mismatch { stored: 0x1800, .. }
    ));
}

#[test]
fn indeo5_320x240_intra_decodes_all_bands() {
    let d = decode_intra_picture(INDEO5).expect("decode");
    assert!(d.parse_complete);
    assert!(d.fully_reconstructed());

    // 320x240 YVU9, decomp 0: luma 20x15 MBs, chroma 80x60 at mb 4
    // -> 20x15 per chroma band.
    assert_eq!(d.stats.bands, 3);
    assert_eq!(d.stats.tiles, 3);
    assert_eq!(d.stats.mbs, 3 * 300);
    assert_eq!(d.stats.mbs_skipped, 0);

    // The r388 arbitration counts: 678 + 194 + 224 coded blocks.
    assert_eq!(d.stats.coded_blocks, 678 + 194 + 224);
    assert!(d.stats.coefficients > 0);

    // Band exhaustion: consumed <= declared with a small trailing
    // tail inside the last tile's explicit byte count (the tail bytes
    // are not zero padding; whether the vendor decoder reads them is
    // an open question — pinned here so any refinement shows up as a
    // diff).
    let traces: Vec<(u64, Option<u32>)> = d
        .band_traces
        .iter()
        .map(|t| (t.consumed, t.declared))
        .collect();
    assert_eq!(traces.len(), 3);
    assert_eq!(traces[0].1, Some(1064));
    assert_eq!(traces[1].1, Some(242));
    assert_eq!(traces[2].1, Some(298));
    for (consumed, declared) in &traces {
        let declared = u64::from(declared.unwrap());
        assert!(*consumed <= declared);
        assert!(
            declared - consumed <= 8,
            "band tail too large: {consumed}/{declared}"
        );
    }

    // Per-band decoded coefficient work list (spec/05 stream): every
    // walked block is surfaced with its scan-ordered coefficients + the
    // effective per-MB quantiser, in decode order, for the (docs-gapped)
    // coefficient->pixel transform stage.
    use oxideav_indeo::indeo5::{BlockCoding, ChecksumStatus};
    assert_eq!(d.bands.len(), 3);
    let y_band = &d.bands[0];
    assert_eq!(y_band.glob_quant, 9); // band+0x40 = 9 (r388 erratum)
    let coded = y_band
        .blocks
        .iter()
        .filter(|b| b.coding == BlockCoding::Coded)
        .count();
    assert_eq!(coded, 678); // matches stats.coded_blocks for the Y band
                            // Every coded block's scan positions stay within its budget
                            // and its quantiser is a valid 0..=31 value.
    for b in &y_band.blocks {
        assert!(b.quant <= 31);
        assert!(b.blk_size == 8);
    }
    // A coded block with non-zero coefficients exists (they are decoded
    // and carried, not discarded).
    assert!(y_band
        .blocks
        .iter()
        .any(|b| b.coding == BlockCoding::Coded && b.coeffs.iter().any(|&c| c != 0)));

    // spec/08 §7 reconstruction oracle. The Y band stores checksum
    // 0xee60 (the r388 --watch value); our uniform-128 recompute is 0,
    // so the luma band is a Mismatch — the coefficient transform is
    // gated. The two chroma bands' real content is near-neutral, so
    // their stored checksums do not match uniform-128 either; every
    // band with content is correctly flagged unverified.
    assert!(matches!(
        y_band.checksum,
        ChecksumStatus::Mismatch { stored: 0xee60, .. }
    ));
    assert!(matches!(d.frame_checksum, ChecksumStatus::Mismatch { .. }));
}

#[test]
fn session_decodes_fixture_then_null_repeat() {
    // The stateful session surface over a real INTRA keyframe, then a
    // NULL frame (spec/08 §6.4): byte-for-byte repeat of the held
    // output.
    use oxideav_indeo::indeo5::{ChecksumStatus, Indeo5Decoder};
    let mut dec = Indeo5Decoder::new();
    let f0 = dec.decode(INDEO5).expect("intra");
    assert!(f0.parse_complete);
    assert_eq!(f0.output.data.len(), 320 * 240 + 2 * 80 * 60);

    // The session path surfaces the same spec/08 §7 reconstruction
    // oracle as the one-shot INTRA path: three bands, luma unverified
    // (transform gated), the frame checksum a mismatch.
    assert_eq!(f0.bands.len(), 3);
    assert!(matches!(
        f0.bands[0].checksum,
        ChecksumStatus::Mismatch { stored: 0xee60, .. }
    ));
    assert!(matches!(f0.frame_checksum, ChecksumStatus::Mismatch { .. }));

    // NULL frame: PSC + frame_type 4 + a fresh frame number.
    let null = [0x1f | (4 << 5), 0x01, 0, 0, 0, 0, 0, 0];
    let f1 = dec.decode(&null).expect("null");
    assert!(f1.repeated_previous);
    assert_eq!(f1.output.data, f0.output.data);
    // A NULL repeat carries no coefficient work list / checksum.
    assert!(f1.bands.is_empty());
    assert_eq!(f1.frame_checksum, ChecksumStatus::Absent);
}

#[test]
fn truncated_fixture_prefixes_never_panic() {
    // Robustness: every truncation of both real bitstreams must
    // return (Ok or Err) without panicking or over-reading.
    for fixture in [EDUC, INDEO5] {
        for len in 0..fixture.len() {
            let _ = decode_intra_picture(&fixture[..len]);
        }
    }
}

#[test]
fn corrupted_fixture_bytes_never_panic() {
    // Deterministic single-byte corruptions across the smaller
    // fixture: the decoder may reject or mis-decode, never panic.
    let mut buf = EDUC.to_vec();
    for i in 0..buf.len() {
        for flip in [0x01u8, 0x80, 0xff] {
            buf[i] ^= flip;
            let _ = decode_intra_picture(&buf);
            buf[i] ^= flip;
        }
    }
}
