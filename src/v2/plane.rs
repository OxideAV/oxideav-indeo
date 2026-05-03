//! Per-plane Indeo 2 entropy → pixel decoder.
//!
//! Implements §3.6 (intra) and §3.7 (inter) of the trace document:
//!
//! * **Intra row 0.** Pair codewords emit the two raw 8-bit table
//!   entries `[2c]` and `[2c+1]` directly (absolute palette). Run
//!   codewords emit a fixed-length run of mid-grey (value 128).
//! * **Intra rows ≥ 1.** Pair codewords add the two entries to the
//!   pixel directly above, treating the table as a signed delta with
//!   bias 128. Run codewords copy a 2..=32-pixel run from the row
//!   above.
//! * **Inter (any row).** Pair codewords add a *3/4-scaled* signed
//!   delta to the co-located pixel in the previous frame. Run
//!   codewords skip — i.e. leave the previous frame's pixels in
//!   place.
//!
//! Width must be even (§3.4); each codeword emits an even pixel count
//! and a row over-run is treated as a stream error.

use oxideav_core::{Error, Result};

use crate::common::{clip_u8, BitReader};
use crate::v2::huffman::{HuffSymbol, HuffTable};

/// Mid-grey neutral value used by intra row-0 run codewords (§3.6).
const NEUTRAL: u8 = 0x80;

/// Decode a single plane's pixels into `out` (which must be sized to
/// `width * height` and pre-initialised — for inter frames it must
/// hold the previous frame's pixels at the same coordinate; for intra
/// frames its initial contents are ignored).
///
/// Both `width` and `height` must be non-zero and `width` must be
/// even.
pub fn decode_plane(
    huff: &HuffTable,
    table: &[u8; 256],
    br: &mut BitReader<'_>,
    out: &mut [u8],
    width: usize,
    height: usize,
    intra: bool,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(Error::invalid("indeo2 plane: zero dimension"));
    }
    if width & 1 != 0 {
        return Err(Error::invalid("indeo2 plane: odd width"));
    }
    if out.len() < width * height {
        return Err(Error::invalid("indeo2 plane: output buffer too small"));
    }

    if intra {
        decode_intra(huff, table, br, out, width, height)
    } else {
        decode_inter(huff, table, br, out, width, height)
    }
}

fn decode_intra(
    huff: &HuffTable,
    table: &[u8; 256],
    br: &mut BitReader<'_>,
    out: &mut [u8],
    width: usize,
    height: usize,
) -> Result<()> {
    // Row 0: absolute palette / mid-grey fill.
    decode_row_0(huff, table, br, &mut out[..width], width)?;

    // Rows 1..H-1: signed delta vs. row above.
    for row in 1..height {
        let (above, current) = out[..(row + 1) * width].split_at_mut(row * width);
        let above_row = &above[(row - 1) * width..row * width];
        let current_row = &mut current[..width];
        decode_row_intra_delta(huff, table, br, above_row, current_row, width)?;
    }
    Ok(())
}

fn decode_inter(
    huff: &HuffTable,
    table: &[u8; 256],
    br: &mut BitReader<'_>,
    out: &mut [u8],
    width: usize,
    height: usize,
) -> Result<()> {
    // Inter: every row processed uniformly. `out` already holds the
    // previous frame's pixels.
    for row in 0..height {
        let row_buf = &mut out[row * width..(row + 1) * width];
        decode_row_inter(huff, table, br, row_buf, width)?;
    }
    Ok(())
}

fn decode_row_0(
    huff: &HuffTable,
    table: &[u8; 256],
    br: &mut BitReader<'_>,
    row: &mut [u8],
    width: usize,
) -> Result<()> {
    let mut x = 0usize;
    while x < width {
        let sym = huff
            .decode(br)
            .ok_or_else(|| Error::invalid("indeo2 row 0: unexpected codeword / underflow"))?;
        match sym {
            HuffSymbol::Pair(c) => {
                if x + 2 > width {
                    return Err(Error::invalid("indeo2 row 0: pair overflows row"));
                }
                let i = (c as usize) * 2;
                row[x] = table[i];
                row[x + 1] = table[i + 1];
                x += 2;
            }
            HuffSymbol::Run(_) => {
                let run = sym.run_pixels();
                if x + run > width {
                    return Err(Error::invalid("indeo2 row 0: run overflows row"));
                }
                for px in &mut row[x..x + run] {
                    *px = NEUTRAL;
                }
                x += run;
            }
        }
    }
    Ok(())
}

fn decode_row_intra_delta(
    huff: &HuffTable,
    table: &[u8; 256],
    br: &mut BitReader<'_>,
    above: &[u8],
    row: &mut [u8],
    width: usize,
) -> Result<()> {
    let mut x = 0usize;
    while x < width {
        let sym = huff
            .decode(br)
            .ok_or_else(|| Error::invalid("indeo2 intra: unexpected codeword / underflow"))?;
        match sym {
            HuffSymbol::Pair(c) => {
                if x + 2 > width {
                    return Err(Error::invalid("indeo2 intra: pair overflows row"));
                }
                let i = (c as usize) * 2;
                let d0 = table[i] as i32 - 128;
                let d1 = table[i + 1] as i32 - 128;
                row[x] = clip_u8(above[x] as i32 + d0);
                row[x + 1] = clip_u8(above[x + 1] as i32 + d1);
                x += 2;
            }
            HuffSymbol::Run(_) => {
                let run = sym.run_pixels();
                if x + run > width {
                    return Err(Error::invalid("indeo2 intra: run overflows row"));
                }
                row[x..x + run].copy_from_slice(&above[x..x + run]);
                x += run;
            }
        }
    }
    Ok(())
}

fn decode_row_inter(
    huff: &HuffTable,
    table: &[u8; 256],
    br: &mut BitReader<'_>,
    row: &mut [u8],
    width: usize,
) -> Result<()> {
    let mut x = 0usize;
    while x < width {
        let sym = huff
            .decode(br)
            .ok_or_else(|| Error::invalid("indeo2 inter: unexpected codeword / underflow"))?;
        match sym {
            HuffSymbol::Pair(c) => {
                if x + 2 > width {
                    return Err(Error::invalid("indeo2 inter: pair overflows row"));
                }
                let i = (c as usize) * 2;
                // 3/4-scaled signed delta on top of the previous
                // frame's pixel (which is already in `row[x]`).
                let d0 = table[i] as i32 - 128;
                let d1 = table[i + 1] as i32 - 128;
                row[x] = clip_u8(row[x] as i32 + ((d0 * 3) >> 2));
                row[x + 1] = clip_u8(row[x + 1] as i32 + ((d1 * 3) >> 2));
                x += 2;
            }
            HuffSymbol::Run(_) => {
                let run = sym.run_pixels();
                if x + run > width {
                    return Err(Error::invalid("indeo2 inter: run overflows row"));
                }
                // Skip — leave the previous frame's pixels intact.
                x += run;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::tables::DELTA_TABLES;

    /// Pack a sequence of bit values into bytes, LSB-first within
    /// each byte (matching `super::common::BitReader`'s wire order).
    /// `bits[0]` becomes bit 0 (LSB) of the first byte; `bits[7]`
    /// becomes bit 7 (MSB) of the first byte.
    fn pack_bits(bits: &[u8]) -> Vec<u8> {
        let mut out = vec![];
        let mut acc: u8 = 0;
        let mut nb = 0u8;
        for &b in bits {
            acc |= (b & 1) << nb;
            nb += 1;
            if nb == 8 {
                out.push(acc);
                acc = 0;
                nb = 0;
            }
        }
        if nb > 0 {
            out.push(acc);
        }
        out
    }

    #[test]
    fn intra_row_0_emits_palette_pair() {
        // Four `000` codewords → symbol 0x01 → table[2..=3] each.
        // Width = 8 ⇒ 4 pair codewords.
        let huff = HuffTable::build();
        let table = &DELTA_TABLES[0];
        let bits: Vec<u8> = (0..4).flat_map(|_| [0, 0, 0]).collect();
        let bytes = pack_bits(&bits);
        let mut br = BitReader::new(&bytes);
        let mut row = vec![0u8; 8];
        decode_row_0(&huff, table, &mut br, &mut row, 8).unwrap();
        // Symbol 0x01 indexes table[2] and table[3]; for table 0
        // both are 0x84.
        assert_eq!(row, vec![0x84, 0x84, 0x84, 0x84, 0x84, 0x84, 0x84, 0x84]);
    }

    #[test]
    fn intra_row_0_emits_run_neutral() {
        // One run-2 (`010`) and three pair-1 (`000`) codewords for
        // width = 8.
        let huff = HuffTable::build();
        let table = &DELTA_TABLES[0];
        let bits = vec![
            0, 1, 0, // run 2 -> two NEUTRAL pixels
            0, 0, 0, // pair 1 -> table[2..=3]
            0, 0, 0, 0, 0, 0,
        ];
        let bytes = pack_bits(&bits);
        let mut br = BitReader::new(&bytes);
        let mut row = vec![0u8; 8];
        decode_row_0(&huff, table, &mut br, &mut row, 8).unwrap();
        assert_eq!(row[0], NEUTRAL);
        assert_eq!(row[1], NEUTRAL);
        assert_eq!(row[2..], [0x84, 0x84, 0x84, 0x84, 0x84, 0x84]);
    }

    #[test]
    fn full_intra_plane_against_known_stream() {
        // 4-pixel wide, 2-row plane, intra. Row 0: two pair-1
        // codewords (`000` `000`). Row 1: two pair-1 codewords as
        // signed deltas vs row 0.
        let huff = HuffTable::build();
        let table = &DELTA_TABLES[0];
        let bits = vec![
            0, 0, 0, 0, 0, 0, // row 0 — two pairs of (0x84,0x84)
            0, 0, 0, 0, 0, 0, // row 1 — same; 0x84 means delta +4
        ];
        let bytes = pack_bits(&bits);
        let mut br = BitReader::new(&bytes);
        let mut plane = vec![0u8; 8];
        decode_plane(&huff, table, &mut br, &mut plane, 4, 2, true).unwrap();
        // Row 0: 0x84, 0x84, 0x84, 0x84
        // Row 1: row0 + (table[2..=3]-128) = 0x84 + 4 = 0x88
        assert_eq!(&plane[..4], &[0x84, 0x84, 0x84, 0x84]);
        assert_eq!(&plane[4..], &[0x88, 0x88, 0x88, 0x88]);
    }

    #[test]
    fn inter_skip_preserves_prev() {
        // Row of width 4, inter, two run-2 codewords (`010 010`).
        let huff = HuffTable::build();
        let table = &DELTA_TABLES[0];
        let bits = vec![0, 1, 0, 0, 1, 0];
        let bytes = pack_bits(&bits);
        let mut br = BitReader::new(&bytes);
        let mut row = vec![10u8, 20, 30, 40];
        decode_row_inter(&huff, table, &mut br, &mut row, 4).unwrap();
        assert_eq!(row, vec![10, 20, 30, 40]);
    }

    #[test]
    fn inter_pair_applies_three_quarter_delta() {
        // Two pair-1 codewords. table[2..=3] = (0x84, 0x84) ⇒ delta
        // +4, scaled to (+4 * 3) >> 2 = +3.
        let huff = HuffTable::build();
        let table = &DELTA_TABLES[0];
        let bits = vec![0, 0, 0, 0, 0, 0];
        let bytes = pack_bits(&bits);
        let mut br = BitReader::new(&bytes);
        let mut row = vec![100u8, 100, 100, 100];
        decode_row_inter(&huff, table, &mut br, &mut row, 4).unwrap();
        assert_eq!(row, vec![103, 103, 103, 103]);
    }

    #[test]
    fn rejects_odd_width() {
        let huff = HuffTable::build();
        let table = &DELTA_TABLES[0];
        let bytes = vec![0u8; 16];
        let mut br = BitReader::new(&bytes);
        let mut plane = vec![0u8; 9];
        assert!(decode_plane(&huff, table, &mut br, &mut plane, 3, 3, true).is_err());
    }
}
