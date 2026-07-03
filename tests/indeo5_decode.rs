//! Indeo 5 whole-frame decode integration tests.
//!
//! Drive `indeo5::decode_intra_picture` over synthetic IV50 bitstreams
//! exactly as a downstream consumer would: picture header stack →
//! per-band / per-tile / per-MB / per-block walk → wavelet recompose →
//! `spec/08` bias-and-clamp + planar pack. An all-zero-coefficient
//! frame reconstructs to the `spec/08 §3.3` mid-grey (`(0 + 0x200) >>
//! 2 = 128`) in every plane.

use oxideav_indeo::indeo5::{decode_intra_picture, PlaneRole, TileDataSize, TileHeader};

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
fn coded_block_stream_decodes_through_default_tables() {
    // A coded MB whose CBP requests one block of AC data, driven
    // through the default block codebook (preset 7) and the default
    // rv-table (slot 8): one (run 0, +1) coefficient then EOB. Under
    // slot 8, vlc 0 maps to composite 28 = run-0 midpoint = +1, and
    // vlc 4 is the EOB marker; both ride row 0 of preset 7 (xbits 3):
    // codewords "0 000" and "0 100" (prefix, MSB-first extras).
    let mut w = BitWriter::new();
    intra_cif_header(&mut w);

    // Y band, one tile with an explicit whole-tile byte count
    // (spec/03 §2.8 reading, behaviourally confirmed).
    plain_band_header(&mut w, 12);
    let tile_start = w.byte_len();
    w.put(0, 1); // value24
    w.put(1, 1); // value25 = 1 -> explicit
    w.put(56, 8); // value26: whole tile = 56 bytes
                  // Phase 1 — MB headers: MB 0 coded (CBP block 0), rest skipped.
    w.put(0, 1);
    w.put(0b0001, 4);
    for _ in 1..(22 * 18) {
        w.put(1, 1);
    }
    // Phase 2 — block streams: (run 0, +1) then EOB.
    w.put(0, 1); // vlc 0: prefix "0"
    w.put(0b000, 3); // extras (MSB-first) = 0
    w.put(0, 1); // vlc 4: prefix "0"
    w.put(0b001, 3); // extras (MSB-first) = 100b = 4
    w.align();
    while w.byte_len() < tile_start + 56 {
        w.put(0, 8); // trailing tile padding (skipped via §2.8)
    }
    for _ in 0..2 {
        w.put(0x01, 8); // empty chroma bands
        w.align();
    }
    let bitstream = w.finish();

    let decoded = decode_intra_picture(&bitstream).expect("decode");
    assert!(decoded.parse_complete);
    assert!(decoded.fully_reconstructed(), "no frontiers on intra");
    assert_eq!(decoded.stats.coded_blocks, 1);
    assert_eq!(decoded.stats.coefficients, 1);
    assert_eq!(decoded.stats.escapes, 0);
    assert_eq!(decoded.stats.mbs_skipped, 22 * 18 - 1);
    // Pixel reconstruction of the coefficient is gated on the
    // scan/dequant/transform docs-gap: output stays mid-grey.
    let out = decoded.output.expect("output");
    assert!(out.data.iter().all(|&b| b == 128));
}

#[test]
fn truncated_coefficient_stream_is_an_error() {
    // A coded MB whose CBP requests AC data but whose stream is
    // zero-padding runs the (run 0, +1) codeword off the block's
    // coefficient budget (or off the buffer) — a hard decode error,
    // not a silent guess.
    let mut w = BitWriter::new();
    intra_cif_header(&mut w);

    plain_band_header(&mut w, 12);
    w.put(0, 1); // value24
    w.put(0, 1); // value25 -> implicit
    w.put(0, 1); // MB 0: coded
    w.put(0b1111, 4); // CBP: AC data follows
    let bitstream = w.finish();

    assert!(decode_intra_picture(&bitstream).is_err());
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
