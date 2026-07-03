//! Indeo 5 per-macroblock header (`spec/03 §4`).
//!
//! Spec source: `docs/video/indeo/indeo5/spec/03-tile-and-macroblock-layer.md`
//! §4 plus the `spec/04 §5.2`/`§5.3` symbol-to-signed-value mapping.
//!
//! Each macroblock of a coded tile is prefixed by a small
//! variable-width header (`spec/03 §4.5` field order):
//!
//! | Field         | Width  | Condition                                |
//! | ------------- | ------ | ---------------------------------------- |
//! | `mb_coded`    | 1 bit  | always (`0` = coded, `1` = **skipped**)  |
//! | `mb_qdelta`   | VLC    | `qdelta_present` AND coded               |
//! | `mv_x_delta`  | VLC    | inter tile AND coded                     |
//! | `mv_y_delta`  | VLC    | inter tile AND coded                     |
//! | `cbp`         | 4 bits | coded AND `blocks_per_mb == 4`           |
//! | `block_coded` | 1 bit  | coded AND `blocks_per_mb == 1`           |
//!
//! The VLC fields decode through the band's block-Huffman codebook
//! ([`super::Codebook`], `spec/03 §4.2`/`§4.4`) and the decoded
//! codeword index is folded to a signed value via the shared level
//! zig-zag table (`spec/04 §3.4`/`§5.2`/`§5.3`,
//! [`super::build_level_table`]).

use super::bitreader::{BitReader, BitReaderError};
use super::codebook::{Codebook, CodebookError};
use super::level_table::{level_value, LEVEL_TABLE_LEN};

/// Spec/03 §5 / spec/06 §5.2 — the effective per-MB quantiser valid
/// range bound (`0..=31`).
pub const MAX_QUANT: u8 = 31;

/// Errors raised while parsing a per-MB header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MbHeaderError {
    /// A VLC field failed to decode against the band codebook.
    Vlc(CodebookError),
    /// Underlying bit-reader fault.
    BitReader(BitReaderError),
    /// A decoded VLC symbol exceeded the 256-entry level zig-zag
    /// table's index space (`spec/04 §3.4`).
    SymbolOutOfRange {
        /// The decoded symbol.
        symbol: u16,
    },
}

impl From<CodebookError> for MbHeaderError {
    fn from(e: CodebookError) -> Self {
        MbHeaderError::Vlc(e)
    }
}

impl From<BitReaderError> for MbHeaderError {
    fn from(e: BitReaderError) -> Self {
        MbHeaderError::BitReader(e)
    }
}

impl core::fmt::Display for MbHeaderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MbHeaderError::Vlc(e) => write!(f, "indeo5 mb header: vlc: {e}"),
            MbHeaderError::BitReader(e) => write!(f, "indeo5 mb header: {e}"),
            MbHeaderError::SymbolOutOfRange { symbol } => write!(
                f,
                "indeo5 mb header: vlc symbol {symbol} exceeds the level table (spec/04 §3.4)"
            ),
        }
    }
}

impl std::error::Error for MbHeaderError {}

/// Spec/03 §4.2 — the three per-MB-qdelta modes selected by the band
/// record's `[band+0x2c]` (`qdelta_present`, `band_flags` bit 2) and
/// `[band+0x30]` (`qdelta_inherit`, bit 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QdeltaMode {
    /// `[band+0x2c] == 0` — no qdelta; the MB inherits the band's
    /// `band_glob_quant` directly.
    Absent,
    /// `[band+0x2c] == 1 && [band+0x30] == 0` — an explicit per-MB
    /// qdelta VLC is read.
    Explicit,
    /// `[band+0x30] == 1` — the per-MB qdelta is inherited from the
    /// parent band's per-MB-qdelta map (no bits read here; the
    /// inheritance-source walk is the `spec/03 §4.2`-deferred path).
    Inherit,
}

impl QdeltaMode {
    /// Derive the mode from the two band flags (`spec/03 §4.2`).
    pub fn from_band_flags(qdelta_present: bool, qdelta_inherit: bool) -> Self {
        if !qdelta_present {
            QdeltaMode::Absent
        } else if qdelta_inherit {
            QdeltaMode::Inherit
        } else {
            QdeltaMode::Explicit
        }
    }
}

/// Spec/03 §4.3 — the coded-block-pattern of one coded macroblock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cbp {
    /// Case B — four-block MB: one bit per block, LSB-first in block
    /// raster order (bit 0 = block 0 top-left … bit 3 = block 3
    /// bottom-right). A `1` means the block is coded (AC coefficients
    /// follow); a `0` means DC-only.
    FourBlock(u8),
    /// Case A — single-block MB: the 1-bit `block_coded` flag.
    /// `false` (bit 0) = the block is fully coded, `true` (bit 1) =
    /// DC-only (`spec/03 §4.3` case-A pseudocode).
    SingleBlock {
        /// `true` when the single block carries no AC residual.
        dc_only: bool,
    },
}

impl Cbp {
    /// `true` when block `block_idx` carries AC coefficient data.
    pub fn block_coded(&self, block_idx: u32) -> bool {
        match self {
            Cbp::FourBlock(bits) => bits & (1 << block_idx) != 0,
            Cbp::SingleBlock { dc_only } => !dc_only,
        }
    }

    /// Number of coded (AC-carrying) blocks.
    pub fn coded_blocks(&self, blocks_per_mb: u32) -> u32 {
        (0..blocks_per_mb).filter(|&b| self.block_coded(b)).count() as u32
    }
}

/// The per-tile context the per-MB header parse needs (`spec/03
/// §4.5` conditions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MbContext {
    /// The band's qdelta mode (`spec/03 §4.2`).
    pub qdelta_mode: QdeltaMode,
    /// `true` for an inter tile carrying explicit MVs (`frame_type !=
    /// 0` AND the per-band MV-inheritance flag clear, `spec/03 §4.4`).
    pub explicit_mv: bool,
    /// Blocks per macroblock (1 or 4, `spec/03 §3.1`).
    pub blocks_per_mb: u32,
}

/// One parsed per-MB header (`spec/03 §4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MbHeader {
    /// `true` when the 1-bit `mb_coded` flag was `1` — the MB is
    /// skipped and no further bits were consumed (`spec/03 §4.1`).
    pub skipped: bool,
    /// The decoded signed per-MB quantiser delta (`Explicit` mode
    /// only).
    pub qdelta: Option<i8>,
    /// The decoded signed MV delta pair `(mv_x_delta, mv_y_delta)`
    /// (explicit-MV inter tiles only).
    pub mv_delta: Option<(i8, i8)>,
    /// The coded-block pattern (`None` for a skipped MB).
    pub cbp: Option<Cbp>,
}

/// Decode one VLC through the band codebook and fold the codeword
/// index to a signed value via the level zig-zag table
/// (`spec/04 §5.2`/`§5.3`).
fn decode_signed_vlc(
    r: &mut BitReader<'_>,
    codebook: &Codebook,
    level_table: &[i8; LEVEL_TABLE_LEN],
) -> Result<i8, MbHeaderError> {
    let symbol = codebook.decode(r)?;
    if symbol as usize >= LEVEL_TABLE_LEN {
        return Err(MbHeaderError::SymbolOutOfRange { symbol });
    }
    Ok(level_value(level_table, symbol as u8))
}

impl MbHeader {
    /// Parse one per-MB header in the `spec/03 §4.5` field order.
    ///
    /// `codebook` is the band's active block-Huffman codebook (the
    /// `blk_huff_desc` selection, `spec/02 §3.6`) shared by the qdelta
    /// and MV-delta VLCs; `level_table` is the shared zig-zag fold
    /// (`spec/04 §3.4`, [`super::build_level_table`]).
    pub fn parse(
        r: &mut BitReader<'_>,
        ctx: &MbContext,
        codebook: &Codebook,
        level_table: &[i8; LEVEL_TABLE_LEN],
    ) -> Result<Self, MbHeaderError> {
        // §4.1 — the 1-bit MB-coded flag (1 = skipped).
        if r.read_bit()? == 1 {
            return Ok(MbHeader {
                skipped: true,
                qdelta: None,
                mv_delta: None,
                cbp: None,
            });
        }

        // §4.2 — conditional per-MB quantiser delta.
        let qdelta = match ctx.qdelta_mode {
            QdeltaMode::Explicit => Some(decode_signed_vlc(r, codebook, level_table)?),
            QdeltaMode::Absent | QdeltaMode::Inherit => None,
        };

        // §4.4 — conditional MV delta pair (x then y, §4.5 order).
        let mv_delta = if ctx.explicit_mv {
            let dx = decode_signed_vlc(r, codebook, level_table)?;
            let dy = decode_signed_vlc(r, codebook, level_table)?;
            Some((dx, dy))
        } else {
            None
        };

        // §4.3 — CBP: 4-bit field (case B) or 1-bit flag (case A).
        let cbp = if ctx.blocks_per_mb == 4 {
            Some(Cbp::FourBlock(r.read(4)? as u8))
        } else {
            Some(Cbp::SingleBlock {
                dc_only: r.read_bit()? == 1,
            })
        };

        Ok(MbHeader {
            skipped: false,
            qdelta,
            mv_delta,
            cbp,
        })
    }
}

/// Spec/06 §5.2 — the effective per-MB quantiser:
/// `band_glob_quant + mb_qdelta`, clamped to the `0..=31` valid range.
pub fn effective_mb_quant(band_glob_quant: u8, qdelta: i8) -> u8 {
    (i32::from(band_glob_quant) + i32::from(qdelta)).clamp(0, i32::from(MAX_QUANT)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indeo5::level_table::build_level_table;

    /// LSB-first bit packer mirroring the reader's bit order.
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
        /// Emit a codeword MSB-first (the order `Codebook::decode`
        /// consumes single bits in).
        fn put_codeword(&mut self, code: u32, len: u8) {
            for i in (0..len).rev() {
                self.bits.push(((code >> i) & 1) as u8);
            }
        }
        fn finish(mut self) -> Vec<u8> {
            while self.bits.len() % 8 != 0 {
                self.bits.push(0);
            }
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

    /// A tiny Kraft-valid codebook: four symbols, each with a 2-bit
    /// codeword (`00`, `01`, `10`, `11`).
    fn flat_codebook() -> Codebook {
        Codebook::build(&[2, 2, 2, 2]).unwrap()
    }

    fn intra_ctx(blocks_per_mb: u32, qdelta_mode: QdeltaMode) -> MbContext {
        MbContext {
            qdelta_mode,
            explicit_mv: false,
            blocks_per_mb,
        }
    }

    #[test]
    fn qdelta_mode_derivation() {
        assert_eq!(
            QdeltaMode::from_band_flags(false, false),
            QdeltaMode::Absent
        );
        assert_eq!(
            QdeltaMode::from_band_flags(true, false),
            QdeltaMode::Explicit
        );
        assert_eq!(QdeltaMode::from_band_flags(true, true), QdeltaMode::Inherit);
        // qdelta_inherit without qdelta_present: no qdelta at all.
        assert_eq!(QdeltaMode::from_band_flags(false, true), QdeltaMode::Absent);
    }

    #[test]
    fn skipped_mb_consumes_one_bit() {
        let mut w = BitWriter::new();
        w.put(1, 1); // mb_coded = 1 -> skipped
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes).unwrap();
        let table = build_level_table();
        let cb = flat_codebook();
        let ctx = intra_ctx(4, QdeltaMode::Explicit);
        let mb = MbHeader::parse(&mut r, &ctx, &cb, &table).unwrap();
        assert!(mb.skipped);
        assert_eq!(mb.cbp, None);
        assert_eq!(r.bits_read(), 1);
    }

    #[test]
    fn coded_mb_four_block_cbp() {
        let mut w = BitWriter::new();
        w.put(0, 1); // coded
        w.put(0b1010, 4); // CBP: blocks 1 and 3 coded
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes).unwrap();
        let table = build_level_table();
        let cb = flat_codebook();
        let ctx = intra_ctx(4, QdeltaMode::Absent);
        let mb = MbHeader::parse(&mut r, &ctx, &cb, &table).unwrap();
        assert!(!mb.skipped);
        assert_eq!(mb.qdelta, None);
        assert_eq!(mb.mv_delta, None);
        let cbp = mb.cbp.unwrap();
        assert!(!cbp.block_coded(0));
        assert!(cbp.block_coded(1));
        assert!(!cbp.block_coded(2));
        assert!(cbp.block_coded(3));
        assert_eq!(cbp.coded_blocks(4), 2);
        assert_eq!(r.bits_read(), 5);
    }

    #[test]
    fn coded_mb_single_block_flag() {
        // Case A: block_coded bit 0 = fully coded, 1 = DC-only.
        let mut w = BitWriter::new();
        w.put(0, 1); // coded
        w.put(1, 1); // block_coded = 1 -> DC-only
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes).unwrap();
        let table = build_level_table();
        let cb = flat_codebook();
        let ctx = intra_ctx(1, QdeltaMode::Absent);
        let mb = MbHeader::parse(&mut r, &ctx, &cb, &table).unwrap();
        let cbp = mb.cbp.unwrap();
        assert_eq!(cbp, Cbp::SingleBlock { dc_only: true });
        assert!(!cbp.block_coded(0));
        assert_eq!(cbp.coded_blocks(1), 0);
    }

    #[test]
    fn explicit_qdelta_zig_zag_fold() {
        // Symbol 0 folds to -0x80? level table index 0 = -0x80; the
        // small header alphabets use the early indices: index 1 =
        // -0x7f (i=2, even -> 2/2-0x80 = -0x7f)... Use the table
        // itself as ground truth.
        let table = build_level_table();
        let cb = flat_codebook();
        // Encode symbol 2 (third 2-bit codeword).
        let cw = cb.codewords().iter().find(|c| c.symbol == 2).unwrap();
        let mut w = BitWriter::new();
        w.put(0, 1); // coded
        w.put_codeword(cw.code, cw.length); // qdelta VLC
        w.put(0b0000, 4); // CBP
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes).unwrap();
        let ctx = intra_ctx(4, QdeltaMode::Explicit);
        let mb = MbHeader::parse(&mut r, &ctx, &cb, &table).unwrap();
        assert_eq!(mb.qdelta, Some(level_value(&table, 2)));
    }

    #[test]
    fn inherit_mode_reads_no_qdelta_bits() {
        let mut w = BitWriter::new();
        w.put(0, 1); // coded
        w.put(0b1111, 4); // CBP directly (no qdelta VLC)
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes).unwrap();
        let table = build_level_table();
        let cb = flat_codebook();
        let ctx = intra_ctx(4, QdeltaMode::Inherit);
        let mb = MbHeader::parse(&mut r, &ctx, &cb, &table).unwrap();
        assert_eq!(mb.qdelta, None);
        assert_eq!(mb.cbp, Some(Cbp::FourBlock(0b1111)));
        assert_eq!(r.bits_read(), 5);
    }

    #[test]
    fn inter_mv_delta_pair_order() {
        // Field order (spec/03 §4.5): qdelta, mv_x, mv_y, cbp.
        let table = build_level_table();
        let cb = flat_codebook();
        let cw1 = *cb.codewords().iter().find(|c| c.symbol == 1).unwrap();
        let cw3 = *cb.codewords().iter().find(|c| c.symbol == 3).unwrap();
        let mut w = BitWriter::new();
        w.put(0, 1); // coded
        w.put_codeword(cw1.code, cw1.length); // mv_x symbol 1
        w.put_codeword(cw3.code, cw3.length); // mv_y symbol 3
        w.put(0b0101, 4); // CBP
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes).unwrap();
        let ctx = MbContext {
            qdelta_mode: QdeltaMode::Absent,
            explicit_mv: true,
            blocks_per_mb: 4,
        };
        let mb = MbHeader::parse(&mut r, &ctx, &cb, &table).unwrap();
        assert_eq!(
            mb.mv_delta,
            Some((level_value(&table, 1), level_value(&table, 3)))
        );
        assert_eq!(mb.cbp, Some(Cbp::FourBlock(0b0101)));
    }

    #[test]
    fn effective_quant_clamps() {
        // spec/06 §5.2 — band_glob_quant + qdelta clamped to 0..=31.
        assert_eq!(effective_mb_quant(16, 4), 20);
        assert_eq!(effective_mb_quant(16, -4), 12);
        assert_eq!(effective_mb_quant(30, 5), 31);
        assert_eq!(effective_mb_quant(2, -5), 0);
        assert_eq!(effective_mb_quant(31, 0), 31);
    }
}
