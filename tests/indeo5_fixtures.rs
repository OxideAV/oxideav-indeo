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

    // A black frame: a single coded block in the whole picture — block
    // (0,0), whose stream is one escape-coded DC (level index 448 →
    // -224) and an EOB; everything else repeats that DC.
    assert_eq!(d.stats.coded_blocks, 1);
    assert_eq!(d.stats.escapes, 1);
    let first = &d.bands[0].blocks[0];
    assert_eq!((first.x, first.y), (0, 0));
    assert_eq!(first.coding, oxideav_indeo::indeo5::BlockCoding::Coded);
    assert_eq!(first.coeffs[0], -224);

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

    // Full pixel reconstruction. The vendor decoder produces
    // studio-black Y=16 with neutral chroma for this frame, and so do
    // we: block (0,0)'s escape-coded DC (-224 at step 1) reaches every
    // block through the intra DC chain, the inverse Slant's 1/2 DC
    // gain lands the band at a uniform -112, and the spec/08 §3.0
    // saturate-then-bias maps it to 16.
    let out = d.output.as_ref().expect("output");
    assert_eq!(out.data.len(), 240 * 180 + 2 * 60 * 45);
    let (luma, chroma) = out.data.split_at(240 * 180);
    assert!(luma.iter().all(|&b| b == 16));
    assert!(chroma.iter().all(|&b| b == 128));

    // spec/08 §7.3 reconstruction oracle: ALL FOUR stored checksums
    // verify byte-exactly (Y band 0x2c00, U=V bands 0, frame 0x1800)
    // — the first fully checksum-verified IV50 frame.
    use oxideav_indeo::indeo5::ChecksumStatus;
    assert_eq!(d.bands.len(), 3);
    assert_eq!(d.bands[0].plane_idx, 0);
    assert!(matches!(
        d.bands[0].checksum,
        ChecksumStatus::Match { value: 0x2c00 }
    ));
    assert!(d.bands[1].checksum.verified());
    assert!(d.bands[2].checksum.verified());
    assert_eq!(d.bands_verified(), 3);
    assert!(matches!(
        d.frame_checksum,
        ChecksumStatus::Match { value: 0x1800 }
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

    // Coded blocks per band under the byte-aligned tile phases
    // (r459): 682 luma + 199 V + 224 U. (The r388 counts 678 / 194 /
    // 224 came from MB headers read one alignment gap early.)
    assert_eq!(d.stats.coded_blocks, 682 + 199 + 224);
    assert!(d.stats.coefficients > 0);
    assert_eq!(d.stats.escapes, 2);

    // Byte-exact band exhaustion: with both tile phases byte-aligned
    // every band consumes exactly its declared `band_data_size` — the
    // r388 "3-8 unconsumed tail bytes" were the streams read out of
    // alignment.
    let traces: Vec<(u64, Option<u32>)> = d
        .band_traces
        .iter()
        .map(|t| (t.consumed, t.declared))
        .collect();
    assert_eq!(
        traces,
        vec![(1064, Some(1064)), (242, Some(242)), (298, Some(298))]
    );

    // Per-band decoded coefficient work list (spec/05 stream): every
    // walked block is surfaced with its scan-ordered coefficients + the
    // effective per-MB quantiser, in decode order (and, since r451,
    // reconstructed through the inverse Slant; the remaining gap is
    // the spec/06 §5.4 dequant scale).
    use oxideav_indeo::indeo5::{BlockCoding, ChecksumStatus};
    assert_eq!(d.bands.len(), 3);
    let y_band = &d.bands[0];
    assert_eq!(y_band.glob_quant, 9); // band+0x40 = 9 (r388 erratum)
    let coded = y_band
        .blocks
        .iter()
        .filter(|b| b.coding == BlockCoding::Coded)
        .count();
    assert_eq!(coded, 682); // matches stats.coded_blocks for the Y band
                            // Every coded block's quantiser is band_glob_quant or +1 (the 18
                            // per-MB +1 deltas the sandbox counted) and its size is 8.
    for b in &y_band.blocks {
        assert!(b.quant == 9 || b.quant == 10);
        assert!(b.blk_size == 8);
    }
    // A coded block with non-zero coefficients exists (they are decoded
    // and carried, not discarded).
    assert!(y_band
        .blocks
        .iter()
        .any(|b| b.coding == BlockCoding::Coded && b.coeffs.iter().any(|&c| c != 0)));

    // spec/08 §7.3 reconstruction oracle: all four stored checksums
    // verify (Y 0xee60, V 0x4be4, U 0xcf31, frame 0xc975) — the
    // quantised frame reconstructs byte-sum-exactly on every band
    // (r459: table dequantiser + byte-aligned phases + rv layout).
    assert!(matches!(
        y_band.checksum,
        ChecksumStatus::Match { value: 0xee60 }
    ));
    assert!(matches!(
        d.bands[1].checksum,
        ChecksumStatus::Match { value: 0x4be4 }
    ));
    assert!(matches!(
        d.bands[2].checksum,
        ChecksumStatus::Match { value: 0xcf31 }
    ));
    assert!(matches!(
        d.frame_checksum,
        ChecksumStatus::Match { value: 0xc975 }
    ));
    assert_eq!(d.bands_verified(), 3);
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
    // oracle as the one-shot INTRA path: three verified bands and a
    // verified frame checksum.
    assert_eq!(f0.bands.len(), 3);
    assert!(matches!(
        f0.bands[0].checksum,
        ChecksumStatus::Match { value: 0xee60 }
    ));
    assert!(matches!(
        f0.frame_checksum,
        ChecksumStatus::Match { value: 0xc975 }
    ));

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
