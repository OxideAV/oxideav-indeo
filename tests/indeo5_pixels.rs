//! Indeo 5 pixel-exact fixture comparison against the vendor decoder's
//! output (`tests/data/*.expected.yuy2` — the staged `expected.yuv` of
//! each fixture: the vendor's packed `YUY2` host buffer, `Y0 U Y1 V`
//! per 4-byte unit, chroma carried at 4:2:2 by the vendor's own
//! writer). Luma is compared sample-for-sample; the chroma columns of
//! the packed view are the vendor's upsampled planes and are covered
//! by the `spec/08 §7` band checksums instead.

use oxideav_indeo::indeo5::{decode_intra_picture, pack_yuy2, BlockCoding, ChecksumStatus};

const INDEO5: &[u8] = include_bytes!("data/intra-320x240-indeo5.iv50");
const INDEO5_YUY2: &[u8] = include_bytes!("data/intra-320x240-indeo5.expected.yuy2");
const EDUC: &[u8] = include_bytes!("data/intra-240x180-educ.iv50");
const EDUC_YUY2: &[u8] = include_bytes!("data/intra-240x180-educ.expected.yuy2");

/// Count luma samples equal to the vendor's (`Y` at every even byte of
/// the packed view), overall and per row.
fn luma_exact(ours: &[u8], yuy2: &[u8], w: usize, h: usize) -> (usize, Vec<usize>) {
    let mut exact = 0;
    let mut per_row = vec![0usize; h];
    for y in 0..h {
        for x in 0..w {
            if ours[y * w + x] == yuy2[(y * w + x) * 2] {
                exact += 1;
                per_row[y] += 1;
            }
        }
    }
    (exact, per_row)
}

#[test]
fn educ_240x180_luma_pixel_exact() {
    let d = decode_intra_picture(EDUC).expect("decode");
    let out = d.output.as_ref().expect("output");
    let (exact, _) = luma_exact(&out.data[..240 * 180], EDUC_YUY2, 240, 180);
    assert_eq!(exact, 240 * 180);
    // The packed view's chroma columns are 128 throughout, as are our
    // native chroma planes.
    assert!(EDUC_YUY2.iter().skip(1).step_by(2).all(|&c| c == 128));
    assert!(out.data[240 * 180..].iter().all(|&c| c == 128));
}

#[test]
fn indeo5_320x240_luma_pixel_exact() {
    // r459: the whole luma plane reproduces the vendor decoder
    // sample-for-sample — 76 800 / 76 800 — through the byte-aligned
    // tile phases, the r388 rv-table level layout, the table
    // dequantiser (spec/05 §2.3) and the previous-block DC chain.
    let d = decode_intra_picture(INDEO5).expect("decode");
    let out = d.output.as_ref().expect("output");
    let (exact, rows) = luma_exact(&out.data[..320 * 240], INDEO5_YUY2, 320, 240);
    assert_eq!(exact, 320 * 240, "per-row exact counts: {rows:?}");
    assert!(rows.iter().all(|&r| r == 320));

    // Every one of the tile's 1 200 luma blocks is walked; 682 carry a
    // coefficient stream (the CBPs read from the byte-aligned header
    // phase), the rest repeat the running DC.
    let y = &d.bands[0];
    assert_eq!(y.blocks.len(), 1200);
    let coded = y
        .blocks
        .iter()
        .filter(|b| b.coding == BlockCoding::Coded)
        .count();
    assert_eq!(coded, 682);
    // Block (0,0) opens the band with its DC as an escape-coded delta
    // from the zero seed: level index 216 → -108 at step 1.
    let first = &y.blocks[0];
    assert_eq!((first.x, first.y), (0, 0));
    assert_eq!(first.coding, BlockCoding::Coded);
    assert_eq!(first.coeffs[0], -108);
    assert_eq!(first.quant, 10);
}

#[test]
fn indeo5_320x240_chroma_verifies_by_checksum() {
    // The vendor's packed view carries interpolated 4:2:2 chroma, so
    // the native 80x60 chroma planes are pinned through the stored
    // band checksums (byte-sum-exact) and the frame checksum.
    let d = decode_intra_picture(INDEO5).expect("decode");
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
    // Chroma DC seeds: V opens at -21 (level index 42), U at +129
    // (escape-coded level index 257) — the three first-block reads
    // the sandbox measured on this fixture.
    assert_eq!(d.bands[1].blocks[0].coeffs[0], -21);
    assert_eq!(d.bands[2].blocks[0].coeffs[0], 129);
}

#[test]
fn indeo5_320x240_yuy2_host_buffer_byte_exact() {
    // r459: the vendor's whole 153 600-byte YUY2 host buffer — luma
    // plus the cosited, truncating 2x chroma interpolation of the
    // native 4:1:0 planes, row-duplicated to 4:2:2 — reproduces
    // byte-for-byte.
    let d = decode_intra_picture(INDEO5).expect("decode");
    let out = d.output.as_ref().expect("output");
    let packed = pack_yuy2(out, 320, 240).expect("yuy2");
    assert_eq!(packed.len(), INDEO5_YUY2.len());
    let mismatches = packed
        .iter()
        .zip(INDEO5_YUY2)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(mismatches, 0);
}

#[test]
fn educ_240x180_yuy2_host_buffer_byte_exact() {
    let d = decode_intra_picture(EDUC).expect("decode");
    let out = d.output.as_ref().expect("output");
    let packed = pack_yuy2(out, 240, 180).expect("yuy2");
    assert_eq!(packed, EDUC_YUY2);
}
