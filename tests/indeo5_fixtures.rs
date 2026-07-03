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
    let out = d.output.expect("output");
    assert_eq!(out.data.len(), 240 * 180 + 2 * 60 * 45);
    assert!(out.data.iter().all(|&b| b == 128));
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
}
