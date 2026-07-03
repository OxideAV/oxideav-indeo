//! Indeo 5 whole-frame decode integration tests.
//!
//! Drive `indeo5::decode_intra_picture` over synthetic IV50 bitstreams
//! exactly as a downstream consumer would: picture header stack →
//! per-band / per-tile / per-MB structural walk → wavelet recompose →
//! `spec/08` bias-and-clamp + planar pack. An all-zero-coefficient
//! frame reconstructs to the `spec/08 §3.3` mid-grey (`(0 + 0x200) >>
//! 2 = 128`) in every plane.

use oxideav_indeo::indeo5::{
    decode_intra_picture, FrontierReason, PlaneRole, TileDataSize, TileHeader,
};

/// LSB-first bit packer mirroring the decoder's bit order
/// (`spec/00 §3`).
struct BitWriter {
    bits: Vec<u8>,
}

impl BitWriter {
    fn new() -> Self {
        BitWriter { bits: Vec::new() }
    }
    fn put(&mut self, value: u32, n: u32) {
        for i in 0..n {
            self.bits.push(((value >> i) & 1) as u8);
        }
    }
    fn align(&mut self) {
        while self.bits.len() % 8 != 0 {
            self.bits.push(0);
        }
    }
    fn byte_len(&self) -> usize {
        assert!(self.bits.len() % 8 == 0, "aligned");
        self.bits.len() / 8
    }
    fn finish(mut self) -> Vec<u8> {
        self.align();
        let mut out = Vec::new();
        for chunk in self.bits.chunks(8) {
            let mut byte = 0u8;
            for (i, &b) in chunk.iter().enumerate() {
                byte |= b << i;
            }
            out.push(byte);
        }
        while out.len() < 8 {
            out.push(0);
        }
        out
    }
}

/// Emit the INTRA picture-header stack for a CIF (352x288) YVU9
/// no-decomposition GOP: picture start, GOP header, GOP trailer,
/// frame header (aligned exit). Leaves the writer byte-aligned at the
/// first band.
fn intra_cif_header(w: &mut BitWriter) {
    // spec/01 §3 picture start: PSC=0x1f, frame_type=0, frame_number=0.
    w.put(0x1f, 5);
    w.put(0, 3);
    w.put(0, 8);
    // spec/02 §1 GOP header: flags=0 (YVU9, no slice size), decomp=0
    // (1 luma + 1 chroma band), pic_size_id=5 (CIF 352x288), one luma
    // + one chroma band_info (mb 16 / blk 8, standard transform).
    w.put(0x00, 8);
    w.put(0, 3);
    w.put(5, 4);
    w.put(0b000000, 6);
    w.put(0b000000, 6);
    // spec/02 §1.9 GOP trailer.
    w.put(0, 8);
    w.put(0, 8);
    w.put(0, 3);
    w.put(0, 4);
    w.align(); // §2.1 pre-frame-header alignment
               // spec/02 §2 frame header: flags=0, value5=0, aligned exit.
    w.put(0x00, 8);
    w.put(0, 3);
    w.align();
}

const CIF_LUMA: usize = 352 * 288;
const CIF_CHROMA: usize = 88 * 72; // YVU9: ceil(352/4) x ceil(288/4)

#[test]
fn all_empty_bands_decode_to_mid_grey() {
    let mut w = BitWriter::new();
    intra_cif_header(&mut w);
    // Three bands (Y, U, V), each taking the spec/02 §3.3 empty fast
    // path (band_flags bit 0).
    for _ in 0..3 {
        w.put(0x01, 8);
        w.align();
    }
    let bitstream = w.finish();

    let decoded = decode_intra_picture(&bitstream).expect("decode");
    assert!(decoded.fully_reconstructed());
    assert_eq!(decoded.stats.bands, 3);
    assert_eq!(decoded.stats.empty_bands, 3);
    assert_eq!(decoded.stats.tiles, 0);

    let out = decoded.output.expect("INTRA frame produces output");
    assert_eq!(out.data.len(), CIF_LUMA + 2 * CIF_CHROMA);
    // spec/08 §3.3 — zero coefficients bias-and-clamp to 128.
    assert!(out.data.iter().all(|&b| b == 128));
    assert_eq!(out.plane_bytes(PlaneRole::Luma).len(), CIF_LUMA);
    assert_eq!(out.plane_bytes(PlaneRole::ChromaU).len(), CIF_CHROMA);
    assert_eq!(out.plane_bytes(PlaneRole::ChromaV).len(), CIF_CHROMA);
}

/// Emit a non-empty band header with no optional fields: flags=0,
/// checksum_flag=0, band_glob_quant.
fn plain_band_header(w: &mut BitWriter, quant: u32) {
    w.put(0x00, 8); // band_flags: non-empty, nothing optional
    w.put(0, 1); // checksum_flag
    w.put(quant, 5); // band_glob_quant
    w.align(); // tiles start byte-aligned (spec/03 §0)
}

#[test]
fn all_skipped_mbs_decode_to_mid_grey() {
    let mut w = BitWriter::new();
    intra_cif_header(&mut w);

    // Y band: 352x288, one tile (no slice size), mb 16 -> 22x18 MBs.
    plain_band_header(&mut w, 12);
    w.put(0, 1); // tile value24 = 0 (carries data)
    w.put(0, 1); // value25 = 0 -> implicit size
    for _ in 0..(22 * 18) {
        w.put(1, 1); // mb_coded = 1 -> skipped
    }
    w.align();

    // U and V bands: 88x72, mb 16 -> 6x5 MBs each.
    for _ in 0..2 {
        plain_band_header(&mut w, 12);
        w.put(0, 1);
        w.put(0, 1);
        for _ in 0..(6 * 5) {
            w.put(1, 1);
        }
        w.align();
    }
    let bitstream = w.finish();

    let decoded = decode_intra_picture(&bitstream).expect("decode");
    assert!(
        decoded.fully_reconstructed(),
        "no gates on a skip-only frame"
    );
    assert_eq!(decoded.stats.bands, 3);
    assert_eq!(decoded.stats.empty_bands, 0);
    assert_eq!(decoded.stats.tiles, 3);
    assert_eq!(decoded.stats.mbs, 22 * 18 + 2 * 6 * 5);
    assert_eq!(decoded.stats.mbs_skipped, 22 * 18 + 2 * 6 * 5);

    let out = decoded.output.expect("output");
    assert_eq!(out.data.len(), CIF_LUMA + 2 * CIF_CHROMA);
    assert!(out.data.iter().all(|&b| b == 128));
}

#[test]
fn coded_mb_without_ac_reconstructs() {
    // A coded MB whose 4-bit CBP is zero consumes no coefficient
    // bits (every block DC-only) — fully parseable today.
    let mut w = BitWriter::new();
    intra_cif_header(&mut w);

    // Y band, one tile, first MB coded with CBP=0, rest skipped.
    plain_band_header(&mut w, 12);
    w.put(0, 1); // value24
    w.put(0, 1); // value25 -> implicit
    w.put(0, 1); // MB 0: coded
    w.put(0b0000, 4); // CBP: all four blocks DC-only
    for _ in 1..(22 * 18) {
        w.put(1, 1); // skipped
    }
    w.align();
    for _ in 0..2 {
        w.put(0x01, 8); // empty chroma bands
        w.align();
    }
    let bitstream = w.finish();

    let decoded = decode_intra_picture(&bitstream).expect("decode");
    assert!(decoded.fully_reconstructed());
    assert_eq!(decoded.stats.mbs_coded_no_ac, 1);
    assert_eq!(decoded.stats.mbs_skipped, 22 * 18 - 1);
    let out = decoded.output.expect("output");
    assert!(out.data.iter().all(|&b| b == 128));
}

#[test]
fn coded_block_data_frontier_skipped_via_explicit_size() {
    let mut w = BitWriter::new();
    intra_cif_header(&mut w);

    // Y band, one tile with an explicit byte count. The first MB's
    // CBP requests AC data -> the driver must record the gated
    // frontier and skip to tile_start + size.
    plain_band_header(&mut w, 12);
    let tile_start = w.byte_len();
    w.put(0, 1); // value24
    w.put(1, 1); // value25 = 1 -> explicit
    w.put(6, 8); // value26 = 6 bytes (whole tile, §2.8 reading)
    w.put(0, 1); // MB 0: coded
    w.put(0b0001, 4); // CBP: block 0 carries AC -> gated
    w.align();
    while w.byte_len() < tile_start + 6 {
        w.put(0xaa, 8); // opaque (gated) coefficient bytes
    }
    // Chroma bands still decode after the skip.
    for _ in 0..2 {
        w.put(0x01, 8);
        w.align();
    }
    let bitstream = w.finish();

    let decoded = decode_intra_picture(&bitstream).expect("decode");
    assert!(decoded.parse_complete, "explicit size allows the skip");
    assert_eq!(decoded.frontiers.len(), 1);
    let f = decoded.frontiers[0];
    assert_eq!(f.plane_idx, 0);
    assert_eq!(f.band_idx, 0);
    assert_eq!(f.tile_idx, 0);
    assert_eq!(f.reason, FrontierReason::CodedBlockData);
    assert!(f.skipped_past);
    assert!(!decoded.fully_reconstructed());
    // The chroma bands after the skip were still walked.
    assert_eq!(decoded.stats.bands, 3);
    assert_eq!(decoded.stats.empty_bands, 2);
    // Output is still produced; gated regions stay zero -> mid-grey.
    let out = decoded.output.expect("output");
    assert!(out.data.iter().all(|&b| b == 128));
}

#[test]
fn implicit_frontier_without_band_size_stops_parse() {
    let mut w = BitWriter::new();
    intra_cif_header(&mut w);

    // Y band, implicit-size tile whose first MB requests AC data and
    // no band_data_size to bail to: the parse must stop (and report
    // it) rather than guess.
    plain_band_header(&mut w, 12);
    w.put(0, 1); // value24
    w.put(0, 1); // value25 -> implicit
    w.put(0, 1); // MB 0: coded
    w.put(0b1111, 4); // CBP: AC data follows -> gated, unskippable
    let bitstream = w.finish();

    let decoded = decode_intra_picture(&bitstream).expect("decode");
    assert!(!decoded.parse_complete);
    assert_eq!(decoded.frontiers.len(), 1);
    assert!(!decoded.frontiers[0].skipped_past);
    // Output is still assembled from the zero seed.
    let out = decoded.output.expect("output");
    assert_eq!(out.data.len(), CIF_LUMA + 2 * CIF_CHROMA);
    assert!(out.data.iter().all(|&b| b == 128));
}

#[test]
fn null_frame_produces_no_output() {
    // NULL frame: PSC + frame_type 4 + frame_number, nothing else.
    let mut w = BitWriter::new();
    w.put(0x1f, 5);
    w.put(4, 3);
    w.put(7, 8);
    let bitstream = w.finish();

    let decoded = decode_intra_picture(&bitstream).expect("decode");
    assert!(decoded.header.is_null());
    assert!(decoded.output.is_none());
    assert!(decoded.format.is_none());
    assert!(decoded.fully_reconstructed());
}

#[test]
fn tile_header_stages_round_trip_through_public_api() {
    // Cross-check the public TileHeader parse against the writer's
    // stage encoding (spec/03 §2.6 prefix code).
    use oxideav_indeo::indeo5::BitReader;
    let mut w = BitWriter::new();
    w.put(0, 1);
    w.put(1, 1);
    w.put(0xff, 8); // escape
    w.put(0x00_1234, 24);
    let bytes = w.finish();
    let mut r = BitReader::new(&bytes).unwrap();
    let th = TileHeader::parse(&mut r, false).unwrap();
    assert_eq!(th.size, TileDataSize::Explicit(0x1234));
}
