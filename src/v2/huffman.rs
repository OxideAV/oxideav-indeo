//! Canonical Huffman decoder for the Indeo 2 prefix code.
//!
//! The 143-entry codebook from `super::tables::VLC_TABLE` is assigned
//! the standard Deflate-style canonical bit-pattern (per §8.1 of the
//! trace document): codes are issued in declaration order, starting
//! with `code = 0` at the shortest length, and the recurrence
//! `code := (code + 1) << (next_len - cur_len)` whenever the current
//! length grows.
//!
//! The codebook is read MSB-first; the active 14-bit window is used to
//! address a 16,384-entry lookup table that returns `(symbol,
//! code_length)` in one branchless step. Roughly 0.71% of the lookup
//! window is left empty — those entries belong to the unused
//! "tree-terminator" prefix and a hit decodes as an error.

use crate::common::BitReader;
use crate::v2::tables::{MAX_CODE_LEN, VLC_TABLE};

/// Result of a single Huffman read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HuffSymbol {
    /// Pair-code symbol in the range `1..=0x7F`. Indexes the active
    /// delta table at offsets `2·sym` and `2·sym + 1`.
    Pair(u8),
    /// Run-code symbol in the range `0x80..=0x8F`; the run length in
    /// pixels is `(sym - 0x7F) * 2`, i.e. 2..=32 pixels.
    Run(u8),
}

impl HuffSymbol {
    /// Convert a raw 8-bit symbol from the codebook into the decoded
    /// pair/run dispatch. Returns `None` for `0x00` (reserved) or
    /// values above `0x8F` (out-of-range).
    pub fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            0x00 => None,
            0x01..=0x7F => Some(Self::Pair(raw)),
            0x80..=0x8F => Some(Self::Run(raw)),
            _ => None,
        }
    }

    /// Run length in *pixels* for a run code. Panics on a pair code.
    #[inline]
    pub fn run_pixels(self) -> usize {
        match self {
            Self::Run(c) => (c as usize - 0x7F) * 2,
            Self::Pair(_) => panic!("run_pixels() called on a pair code"),
        }
    }
}

/// One entry of the 14-bit lookup table.
///
/// `length == 0` means the slot is unassigned (a hit on it is a
/// stream error — the "tree-terminator" prefix has no leaf).
#[derive(Clone, Copy, Debug)]
struct LutEntry {
    symbol: u8,
    /// Code length in bits (1..=14). `0` ⇒ slot empty.
    length: u8,
}

/// 16,384-entry lookup table.
pub struct HuffTable {
    lut: Box<[LutEntry; 1 << MAX_CODE_LEN]>,
}

impl HuffTable {
    /// Build the table from `super::tables::VLC_TABLE` using the
    /// canonical Huffman recurrence. This is `O(143)` plus
    /// `O(2^MAX_CODE_LEN)` zero-fill — done once at decoder startup.
    pub fn build() -> Self {
        let mut lut = vec![
            LutEntry {
                symbol: 0,
                length: 0,
            };
            1 << MAX_CODE_LEN
        ];
        let mut code: u32 = 0;
        let mut prev_len: u8 = 0;
        for &(sym, len) in VLC_TABLE.iter() {
            assert!(len > 0 && len <= MAX_CODE_LEN, "bad codeword length");
            if prev_len == 0 {
                // First entry: code remains 0.
            } else if len == prev_len {
                code += 1;
            } else {
                code = (code + 1) << (len - prev_len);
            }
            prev_len = len;
            // The codeword `code` has `len` bits; left-shift to fill
            // the 14-bit window so every 14-bit pattern that begins
            // with this code maps to (sym, len).
            let shift = MAX_CODE_LEN - len;
            let base = code << shift;
            let span = 1u32 << shift;
            for i in 0..span {
                let idx = (base | i) as usize;
                debug_assert_eq!(lut[idx].length, 0, "canonical Huffman overlap");
                lut[idx] = LutEntry { symbol: sym, length: len };
            }
        }
        let lut: Box<[LutEntry; 1 << MAX_CODE_LEN]> =
            lut.into_boxed_slice().try_into().expect("LUT length");
        Self { lut }
    }

    /// Decode one codeword from `br`. Advances the cursor by the
    /// codeword's length on success.
    ///
    /// Returns `None` on bit-stream underflow or on a hit on the
    /// unassigned (zero-leaf) prefix.
    pub fn decode(&self, br: &mut BitReader<'_>) -> Option<HuffSymbol> {
        // Peek a 14-bit window. Pad with zeros if we're near EOF —
        // the canonical assignment never decodes a longer code so the
        // pad bits are harmless when the prefix matches; if a code is
        // truncated we'll detect underflow when we consume the bits.
        let window = peek_padded(br, MAX_CODE_LEN);
        let entry = self.lut[window as usize];
        if entry.length == 0 {
            return None;
        }
        if !br.skip_bits(entry.length as usize) {
            return None;
        }
        HuffSymbol::from_raw(entry.symbol)
    }
}

/// Peek up to `nbits` from `br`, padding with zeros if the buffer is
/// shorter than the window. Used so we can address the lookup with a
/// fixed 14-bit window even at the very last codeword.
fn peek_padded(br: &BitReader<'_>, nbits: u8) -> u32 {
    let avail = br.bits_remaining().min(nbits as usize);
    if avail == 0 {
        return 0;
    }
    let v = br.peek_bits(avail as u8).unwrap_or(0);
    // Left-justify into the nbits-wide window.
    v << (nbits as usize - avail)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pack a stream of 0/1 bit values into bytes LSB-first within
    /// each byte (matches `BitReader`'s wire order).
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
    fn known_codewords_decode() {
        let tbl = HuffTable::build();
        // §8.1 first four entries:
        //   000 -> 0x01, 001 -> 0x02, 010 -> 0x80, 011 -> 0x03
        let bits: Vec<u8> = vec![
            0, 0, 0, // 0x01
            0, 0, 1, // 0x02
            0, 1, 0, // 0x80
            0, 1, 1, // 0x03
        ];
        let buf = pack_bits(&bits);
        let mut br = BitReader::new(&buf);
        assert_eq!(tbl.decode(&mut br), Some(HuffSymbol::Pair(0x01)));
        assert_eq!(tbl.decode(&mut br), Some(HuffSymbol::Pair(0x02)));
        assert_eq!(tbl.decode(&mut br), Some(HuffSymbol::Run(0x80)));
        assert_eq!(tbl.decode(&mut br), Some(HuffSymbol::Pair(0x03)));
    }

    #[test]
    fn five_bit_codes_decode() {
        let tbl = HuffTable::build();
        // §8.1 five-bit codes start at 10000 = 0x04. Sequence:
        //   10000 -> 0x04, 10001 -> 0x81, 10010 -> 0x05
        let bits: Vec<u8> = vec![
            1, 0, 0, 0, 0, // 0x04
            1, 0, 0, 0, 1, // 0x81
            1, 0, 0, 1, 0, // 0x05
        ];
        let buf = pack_bits(&bits);
        let mut br = BitReader::new(&buf);
        assert_eq!(tbl.decode(&mut br), Some(HuffSymbol::Pair(0x04)));
        assert_eq!(tbl.decode(&mut br), Some(HuffSymbol::Run(0x81)));
        assert_eq!(tbl.decode(&mut br), Some(HuffSymbol::Pair(0x05)));
    }

    #[test]
    fn run_lengths_match_formula() {
        for c in 0x80u8..=0x8F {
            let s = HuffSymbol::Run(c);
            let expected = (c as usize - 0x7F) * 2;
            assert_eq!(s.run_pixels(), expected);
        }
    }

    #[test]
    fn unassigned_prefix_returns_none() {
        let tbl = HuffTable::build();
        // The 14-bit prefix `11111111111111` (all ones) is in the
        // never-assigned region (canonical assignment uses the
        // `1111110...` prefix for 13/14-bit codes; `1111111x...` is
        // unused). Decoding it must return None.
        let bits: Vec<u8> = vec![1; MAX_CODE_LEN as usize];
        let buf = pack_bits(&bits);
        let mut br = BitReader::new(&buf);
        assert!(tbl.decode(&mut br).is_none());
    }
}
