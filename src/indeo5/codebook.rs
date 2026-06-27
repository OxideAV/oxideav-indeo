//! Indeo 5 canonical-Huffman codebooks.
//!
//! Spec source: `docs/video/indeo/indeo5/spec/04-entropy.md` §1, §3.2,
//! §4.3.
//!
//! Both the per-MB Huffman context (`mb_huff_desc`, `spec/02 §2.6`) and
//! the per-band block-Huffman context (`blk_huff_desc`, `spec/02 §3.6`)
//! share one descriptor format and one canonical-Huffman build path
//! (`spec/04 §1, §3.2`). A descriptor is either:
//!
//! * one of seven preset row-length records (`huff_table_id 0..=6`,
//!   `spec/04 §1.4` Table B / `§1.5` Table A), selected directly; or
//! * an inline custom record (`huff_table_id == 7`): a `num_rows` count
//!   plus `num_rows` 4-bit per-row bit-lengths (`spec/04 §1.6`); or
//! * the implicit default (preset record 7) when the descriptor's
//!   present-flag is clear (`spec/04 §1.1`/`§1.2`).
//!
//! The descriptor is **per-row bit-lengths** (not per-bit-length
//! counts): row `i` carries `bit_length[i]`, the codeword length for
//! symbol `i`; a `0` length means "row `i` has no codeword, advance the
//! symbol counter anyway" (`spec/04 §3.2`/`§4.4`). [`Codebook::build`]
//! turns a row-length table into the canonical-Huffman codeword set and
//! a decode-ready lookup, and [`Codebook::decode`] consumes the
//! LSB-first bitstream the [`super::BitReader`] backs.
//!
//! This module lands the **shared entropy primitive** the MB-header
//! VLCs (`spec/03 §4`) and the per-block coefficient stream (`spec/05`)
//! both invoke. The coefficient-stream's `vlcEnd` / `vlcEsc` sentinel
//! derivation, the per-run level tables, and the rv-table run-value
//! mapping are deferred to `spec/05+` (`spec/04 §6` items 1, 2, 3).
//!
//! ## Preset-descriptor Kraft anomaly (reported docs gap)
//!
//! The spec/04 §1.3 / §3.2 builder treats each preset record's bytes as
//! per-row codeword bit-lengths assigned by "standard left-to-right
//! canonical Huffman". Under that rule a valid prefix-free code requires
//! the Kraft sum `Σ 2^-len ≤ 1`. The preset numeric data the spec lists
//! in §1.4 (Table B) and §1.5 (Table A) does **not** satisfy this for
//! most records (Kraft sums range `0.31..2.38`), so the records as
//! listed are not Kraft-valid per-row bit-length codebooks. The §3.2
//! builder is itself documented as *deduced from `mov` patterns*, and
//! its 4-byte table entry carries up-to-**three** symbols per 10-bit
//! prefix (`symbol_0/1/2`) with `0x10`/`0x20` overflow flags — a
//! non-plain-prefix-free decode whose exact code-space rule a dump of
//! the populated 4 KB table (`spec/04 §6` item 8, an Extractor-round
//! subject) would pin. [`Codebook::build`] therefore implements the
//! **standard** canonical-Huffman assignment (correct for the inline
//! custom descriptor the encoder emits as genuine per-row bit-lengths,
//! `spec/04 §1.6`) and reports [`CodebookError::Oversubscribed`] for a
//! non-Kraft-valid descriptor rather than inventing the binary's
//! multi-symbol-per-prefix semantics. The preset records are exposed as
//! documented numeric data ([`MB_HUFF_PRESETS`] / [`BLOCK_HUFF_PRESETS`])
//! for the Extractor cross-check, but are not asserted to build into
//! valid codebooks until the table dump resolves the assignment rule.

use super::bitreader::{BitReader, BitReaderError};

/// Spec/04 §1.3 — the maximum number of rows (codewords) a descriptor
/// can declare. The inline form's `num_rows` is a 4-bit field, so it
/// caps at 15; the presets observe `8..=13` (`spec/04 §1.4`/`§1.5`).
pub const MAX_ROWS: usize = 16;

/// Spec/04 §1.5 — the eight **mb-Huffman** preset row-length records
/// (`IR50_32.DLL!.rdata 0x1008d710`, Table A). Each record is
/// `[num_rows, bit_length[0..num_rows]]`. Numeric data per `spec/04
/// §1.5`. Record 7 is the implicit default (`spec/04 §1.1`).
pub const MB_HUFF_PRESETS: [&[u8]; 8] = [
    &[0, 4, 5, 4, 4, 4, 6, 6],
    &[0, 2, 2, 3, 3, 3, 3, 5, 3, 2, 2, 2],
    &[0, 2, 3, 4, 3, 3, 3, 3, 4, 3, 2, 2],
    &[0, 3, 4, 4, 3, 3, 3, 3, 3, 2, 2, 2],
    &[0, 4, 4, 3, 3, 3, 3, 2, 3, 3, 2, 1, 1],
    &[0, 4, 4, 4, 4, 3, 3, 3, 2],
    &[0, 4, 4, 4, 4, 3, 3, 2, 2, 2],
    &[0, 4, 4, 4, 3, 3, 2, 3, 2, 2, 2, 2],
];

/// Spec/04 §1.4 — the eight **block-Huffman** preset row-length records
/// (`IR50_32.DLL!.rdata 0x1008d798`, Table B). Numeric data per `spec/04
/// §1.4`. Record 7 is the implicit default (`spec/04 §1.2`).
pub const BLOCK_HUFF_PRESETS: [&[u8]; 8] = [
    &[1, 2, 3, 4, 4, 7, 5, 5, 4, 1],
    &[2, 3, 4, 4, 4, 7, 5, 4, 3, 3, 2],
    &[2, 4, 5, 5, 5, 5, 6, 4, 4, 3, 1, 1],
    &[3, 3, 4, 4, 5, 6, 6, 4, 4, 3, 2, 1, 1],
    &[3, 4, 4, 5, 5, 5, 6, 5, 4, 2, 2],
    &[3, 4, 5, 5, 5, 5, 6, 4, 3, 3, 2, 1, 1],
    &[3, 4, 5, 5, 5, 6, 5, 4, 3, 3, 2, 1, 1],
    &[3, 4, 4, 5, 5, 5, 6, 5, 5],
];

/// Spec/04 §1.1 / §1.2 — the implicit-default preset index (record 7).
pub const DEFAULT_PRESET_ID: usize = 7;

/// The Huffman context a descriptor belongs to (`spec/04 §0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HuffContext {
    /// Per-MB header VLCs (`mb_huff_desc`, Table A presets).
    Mb,
    /// Per-block coefficient VLCs (`blk_huff_desc`, Table B presets).
    Block,
}

impl HuffContext {
    /// The eight preset row-length records for this context.
    pub fn presets(self) -> &'static [&'static [u8]; 8] {
        match self {
            HuffContext::Mb => &MB_HUFF_PRESETS,
            HuffContext::Block => &BLOCK_HUFF_PRESETS,
        }
    }
}

/// Errors raised while building or decoding a canonical-Huffman
/// codebook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodebookError {
    /// A descriptor declared more than [`MAX_ROWS`] rows.
    TooManyRows {
        /// The row count found.
        found: usize,
    },
    /// A row carried a bit-length wider than the descriptor permits
    /// (`> 15`, beyond the inline 4-bit field range, `spec/04 §1.6`).
    BitLengthTooWide {
        /// The offending row index.
        row: usize,
        /// The bit-length found.
        bits: u8,
    },
    /// The row-length set is not a valid canonical-Huffman assignment:
    /// the codewords over-subscribe the code space (`sum 2^-len > 1`),
    /// so no prefix-free code exists. The `spec/04 §3.2` builder's
    /// running-code accumulator would overflow its bit-length window.
    Oversubscribed,
    /// A decode walked a bit pattern with no matching codeword (an
    /// alphabet "hole" per `spec/04 §4.4`).
    NoMatch,
    /// Underlying bit-reader fault during decode.
    BitReader(BitReaderError),
}

impl From<BitReaderError> for CodebookError {
    fn from(e: BitReaderError) -> Self {
        CodebookError::BitReader(e)
    }
}

impl core::fmt::Display for CodebookError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CodebookError::TooManyRows { found } => write!(
                f,
                "indeo5 codebook: {found} rows exceeds the maximum {MAX_ROWS} (spec/04 §1.3)"
            ),
            CodebookError::BitLengthTooWide { row, bits } => write!(
                f,
                "indeo5 codebook: row {row} bit-length {bits} exceeds 15 (spec/04 §1.6)"
            ),
            CodebookError::Oversubscribed => write!(
                f,
                "indeo5 codebook: row-length set over-subscribes the code space (spec/04 §3.2)"
            ),
            CodebookError::NoMatch => write!(
                f,
                "indeo5 codebook: bit pattern matched no codeword (alphabet hole, spec/04 §4.4)"
            ),
            CodebookError::BitReader(e) => write!(f, "indeo5 codebook: {e}"),
        }
    }
}

impl std::error::Error for CodebookError {}

/// One assigned codeword of a canonical-Huffman codebook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Codeword {
    /// The symbol index (the row index in the descriptor).
    pub symbol: u16,
    /// The codeword bit-length (`> 0`; `0`-length rows are not
    /// assigned codewords, `spec/04 §3.2`).
    pub length: u8,
    /// The codeword value, `length` bits wide, MSB-first as the
    /// canonical-Huffman assignment produces it.
    pub code: u32,
}

/// A built canonical-Huffman codebook (`spec/04 §1, §3.2`).
///
/// Construct from a row-length descriptor with [`Codebook::build`] (or
/// [`Codebook::from_preset`] / [`Codebook::from_huff_desc`]), then
/// decode symbols off an LSB-first [`BitReader`] with
/// [`Codebook::decode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Codebook {
    /// The assigned codewords, in ascending `(length, code)` order
    /// (canonical-Huffman order).
    codewords: Vec<Codeword>,
    /// The maximum codeword bit-length present (`0` for an empty
    /// codebook).
    max_length: u8,
    /// The number of declared rows (symbols), including `0`-length
    /// holes. The alphabet's symbol indices run `0..num_symbols`.
    num_symbols: u16,
}

impl Codebook {
    /// Build a canonical-Huffman codebook from a per-row bit-length
    /// descriptor (`spec/04 §3.2`).
    ///
    /// `row_lengths[i]` is the codeword length for symbol `i`; a `0`
    /// length means symbol `i` has no codeword but the symbol counter
    /// still advances (`spec/04 §3.2`/`§4.4`). The codewords are
    /// assigned by the standard canonical-Huffman left-to-right
    /// procedure: codewords are ordered by increasing length, and
    /// within a length by increasing symbol index; the running code is
    /// the previous code `+ 1`, shifted left when the length grows.
    pub fn build(row_lengths: &[u8]) -> Result<Self, CodebookError> {
        if row_lengths.len() > MAX_ROWS {
            return Err(CodebookError::TooManyRows {
                found: row_lengths.len(),
            });
        }
        for (row, &bits) in row_lengths.iter().enumerate() {
            if bits > 15 {
                return Err(CodebookError::BitLengthTooWide { row, bits });
            }
        }

        let num_symbols = row_lengths.len() as u16;
        let max_length = row_lengths.iter().copied().max().unwrap_or(0);

        // Collect the symbols that carry a codeword (length > 0),
        // grouped so we can walk them in canonical order: ascending by
        // length, then ascending by symbol index. The descriptor order
        // already yields ascending symbol index within a length when we
        // iterate symbols in order and bucket by length.
        let mut codewords: Vec<Codeword> = Vec::new();
        let mut code: u32 = 0;
        let mut prev_len: u8 = 0;
        for length in 1..=max_length {
            // Each new length shifts the running code left by the gap
            // from the previous assigned length (canonical-Huffman).
            if !codewords.is_empty() {
                code <<= length - prev_len;
            }
            prev_len = length;
            for (symbol, &bits) in row_lengths.iter().enumerate() {
                if bits == length {
                    // The running code must fit in `length` bits;
                    // overflow means the lengths over-subscribe the
                    // code space.
                    if code >> length != 0 {
                        return Err(CodebookError::Oversubscribed);
                    }
                    codewords.push(Codeword {
                        symbol: symbol as u16,
                        length,
                        code,
                    });
                    code += 1;
                }
            }
        }

        Ok(Codebook {
            codewords,
            max_length,
            num_symbols,
        })
    }

    /// Build the codebook for a context's preset record
    /// (`huff_table_id 0..=7`, `spec/04 §1.4`/`§1.5`).
    ///
    /// **Note.** Under the standard canonical-Huffman rule
    /// [`Codebook::build`] applies, most preset records are not
    /// Kraft-valid (see the module-level "Kraft anomaly" note) and this
    /// returns [`CodebookError::Oversubscribed`]. The presets are
    /// retained as the documented numeric data and exercised through
    /// [`raw_preset`] / [`HuffContext::presets`] until the §6-item-8
    /// table dump resolves the binary's multi-symbol assignment rule.
    pub fn from_preset(context: HuffContext, id: usize) -> Result<Self, CodebookError> {
        let presets = context.presets();
        let record = presets[id & 7];
        // Table A presets carry a leading num_rows-style `0` sentinel
        // row (`spec/04 §1.5`): the descriptor body is the row-length
        // array verbatim, the leading `0` being symbol 0's `0`-length
        // hole. Table B presets carry no such hole. Either way the
        // record is already the row-length array.
        Codebook::build(record)
    }

    /// Build the codebook a [`super::HuffDesc`] selects, defaulting to
    /// the context's preset 7 when `desc` is `None` (`spec/04
    /// §1.1`/`§1.2`).
    pub fn from_huff_desc(
        context: HuffContext,
        desc: Option<&super::HuffDesc>,
    ) -> Result<Self, CodebookError> {
        match desc {
            None => Codebook::from_preset(context, DEFAULT_PRESET_ID),
            Some(super::HuffDesc::Preset { id }) => Codebook::from_preset(context, *id as usize),
            Some(super::HuffDesc::Custom { row_lengths }) => Codebook::build(row_lengths),
        }
    }

    /// The raw per-row bit-length record for a context's preset
    /// (`spec/04 §1.4`/`§1.5`) — the documented numeric data, exposed
    /// for the §6-item-8 Extractor cross-check independent of whether it
    /// builds into a Kraft-valid codebook.
    pub fn raw_preset(context: HuffContext, id: usize) -> &'static [u8] {
        context.presets()[id & 7]
    }

    /// The Kraft sum `Σ 2^-len` (scaled by `2^max_len` to stay integral)
    /// over a per-row bit-length record. A prefix-free canonical-Huffman
    /// code requires the unscaled sum `≤ 1`; this diagnostic surfaces the
    /// preset Kraft anomaly (module-level note) without floating point.
    /// Returns `(scaled_sum, max_len)` where the code is valid iff
    /// `scaled_sum ≤ (1 << max_len)`.
    pub fn kraft_scaled(row_lengths: &[u8]) -> (u64, u8) {
        let max_len = row_lengths.iter().copied().max().unwrap_or(0);
        let sum = row_lengths
            .iter()
            .filter(|&&l| l > 0)
            .map(|&l| 1u64 << (max_len - l))
            .sum();
        (sum, max_len)
    }

    /// The maximum codeword bit-length present.
    pub fn max_length(&self) -> u8 {
        self.max_length
    }

    /// The number of declared symbols (rows), including `0`-length
    /// holes.
    pub fn num_symbols(&self) -> u16 {
        self.num_symbols
    }

    /// The assigned codewords (length `> 0` symbols), in canonical
    /// order.
    pub fn codewords(&self) -> &[Codeword] {
        &self.codewords
    }

    /// Decode one symbol from an LSB-first bit reader (`spec/04 §4.3`).
    ///
    /// The decoder reads bits one at a time, appending each to the low
    /// end of a running codeword accumulator, and matches against the
    /// assigned canonical-Huffman codes (treated MSB-first, the order
    /// [`Codebook::build`] assigns them in): the codeword grows one bit
    /// per step until a length-and-value match is found, and a
    /// prefix-free code guarantees the first match is unique.
    ///
    /// **Bit-order boundary.** The spec/04 §4.3 trace decodes via a
    /// 1024-entry prefix table indexed by the *low 10 bits* of the
    /// LSB-first accumulator, so the mapping from "first stream bit" to
    /// "codeword MSB" depends on how the §3.2 builder bit-orders each
    /// canonical code into the prefix index — a detail that requires a
    /// dump of the populated 4 KB table to pin (an Extractor-round
    /// subject, `spec/04 §6` item 8). This `decode` is the
    /// canonical-Huffman walk that is **self-consistent** with
    /// [`Codebook::build`]'s code assignment (encode-with-build →
    /// decode-with-decode round-trips); validating its stream bit-order
    /// against a real `IV50` fixture is the next step once a coefficient
    /// fixture is staged.
    pub fn decode(&self, r: &mut BitReader<'_>) -> Result<u16, CodebookError> {
        let mut acc: u32 = 0;
        let mut len: u8 = 0;
        while len < self.max_length {
            let bit = r.read_bit()? as u32;
            acc = (acc << 1) | bit;
            len += 1;
            for cw in &self.codewords {
                if cw.length == len && cw.code == acc {
                    return Ok(cw.symbol);
                }
            }
        }
        Err(CodebookError::NoMatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LSB-first bit packer that mirrors the header parsers' test
    /// harness so encoded codewords feed `BitReader` correctly.
    struct BitWriter {
        bits: Vec<u8>,
    }
    impl BitWriter {
        fn new() -> Self {
            BitWriter { bits: Vec::new() }
        }
        /// Append `n` bits LSB-first (low bit of `value` first).
        fn put(&mut self, value: u32, n: u32) {
            for i in 0..n {
                self.bits.push(((value >> i) & 1) as u8);
            }
        }
        /// Append a canonical-Huffman codeword: it is MSB-first, so the
        /// high bit (`length-1`) is emitted first into the stream.
        fn put_code(&mut self, code: u32, length: u8) {
            for i in (0..length).rev() {
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
            while out.len() < 4 {
                out.push(0);
            }
            out
        }
    }

    #[test]
    fn build_simple_two_symbol() {
        // Two symbols, each length 1: codes 0 and 1.
        let cb = Codebook::build(&[1, 1]).unwrap();
        assert_eq!(cb.max_length(), 1);
        assert_eq!(cb.num_symbols(), 2);
        let cws = cb.codewords();
        assert_eq!(cws.len(), 2);
        assert_eq!(
            cws[0],
            Codeword {
                symbol: 0,
                length: 1,
                code: 0
            }
        );
        assert_eq!(
            cws[1],
            Codeword {
                symbol: 1,
                length: 1,
                code: 1
            }
        );
    }

    #[test]
    fn build_canonical_mixed_lengths() {
        // Lengths [1, 2, 3, 3]: canonical codes
        //   sym0 len1 -> 0
        //   sym1 len2 -> 10
        //   sym2 len3 -> 110
        //   sym3 len3 -> 111
        let cb = Codebook::build(&[1, 2, 3, 3]).unwrap();
        let cws = cb.codewords();
        assert_eq!(
            cws[0],
            Codeword {
                symbol: 0,
                length: 1,
                code: 0b0
            }
        );
        assert_eq!(
            cws[1],
            Codeword {
                symbol: 1,
                length: 2,
                code: 0b10
            }
        );
        assert_eq!(
            cws[2],
            Codeword {
                symbol: 2,
                length: 3,
                code: 0b110
            }
        );
        assert_eq!(
            cws[3],
            Codeword {
                symbol: 3,
                length: 3,
                code: 0b111
            }
        );
    }

    #[test]
    fn build_skips_zero_length_holes() {
        // A leading 0-length row (the Table A sentinel) is skipped but
        // still consumes symbol index 0.
        let cb = Codebook::build(&[0, 1, 2, 2]).unwrap();
        let cws = cb.codewords();
        assert_eq!(cws.len(), 3);
        assert_eq!(cws[0].symbol, 1);
        assert_eq!(
            cws[0],
            Codeword {
                symbol: 1,
                length: 1,
                code: 0b0
            }
        );
        assert_eq!(
            cws[1],
            Codeword {
                symbol: 2,
                length: 2,
                code: 0b10
            }
        );
        assert_eq!(
            cws[2],
            Codeword {
                symbol: 3,
                length: 2,
                code: 0b11
            }
        );
        assert_eq!(cb.num_symbols(), 4);
    }

    #[test]
    fn build_rejects_oversubscribed() {
        // Three length-1 codewords cannot exist (only 0 and 1).
        assert_eq!(
            Codebook::build(&[1, 1, 1]),
            Err(CodebookError::Oversubscribed)
        );
    }

    #[test]
    fn build_rejects_too_wide_bitlength() {
        assert_eq!(
            Codebook::build(&[16]),
            Err(CodebookError::BitLengthTooWide { row: 0, bits: 16 })
        );
    }

    #[test]
    fn build_rejects_too_many_rows() {
        let rows = [1u8; 17];
        assert_eq!(
            Codebook::build(&rows),
            Err(CodebookError::TooManyRows { found: 17 })
        );
    }

    #[test]
    fn preset_max_lengths_match_spec() {
        // spec/04 §3.2: "max bit-length observed in any preset is 7,
        // from Table B records 0 and 1." Confirm the vendored numeric
        // data matches that claim (max <= 7 for block, <= 6 for mb).
        for id in 0..8 {
            let (_, max_blk) = Codebook::kraft_scaled(Codebook::raw_preset(HuffContext::Block, id));
            assert!(max_blk <= 7, "block preset {id} max {max_blk}");
            let (_, max_mb) = Codebook::kraft_scaled(Codebook::raw_preset(HuffContext::Mb, id));
            assert!(max_mb <= 6, "mb preset {id} max {max_mb}");
        }
        // The §3.2 "records 0 and 1 reach 7" cross-check.
        assert_eq!(Codebook::kraft_scaled(BLOCK_HUFF_PRESETS[0]).1, 7);
        assert_eq!(Codebook::kraft_scaled(BLOCK_HUFF_PRESETS[1]).1, 7);
    }

    #[test]
    fn presets_are_kraft_anomalous() {
        // Reported docs gap: most preset records are not Kraft-valid
        // per-row bit-length codebooks, so the standard canonical
        // builder rejects them (Oversubscribed). Pin a representative
        // over-subscribed record (block 0, Kraft scaled > 2^max) and an
        // under-subscribed one (block 7) — neither equals 2^max.
        let (sum0, max0) = Codebook::kraft_scaled(BLOCK_HUFF_PRESETS[0]);
        assert!(sum0 > (1u64 << max0), "block 0 should over-subscribe");
        assert_eq!(
            Codebook::build(BLOCK_HUFF_PRESETS[0]),
            Err(CodebookError::Oversubscribed)
        );
        let (sum7, max7) = Codebook::kraft_scaled(BLOCK_HUFF_PRESETS[7]);
        assert!(sum7 < (1u64 << max7), "block 7 should under-subscribe");
    }

    #[test]
    fn raw_preset_exposes_numeric_data() {
        // Table B record 0 is the documented numeric data verbatim.
        assert_eq!(
            Codebook::raw_preset(HuffContext::Block, 0),
            &[1, 2, 3, 4, 4, 7, 5, 5, 4, 1]
        );
        // Table A record 0 carries the leading 0-row sentinel.
        assert_eq!(
            Codebook::raw_preset(HuffContext::Mb, 0),
            &[0, 4, 5, 4, 4, 4, 6, 6]
        );
        // The masked id wraps into 0..8.
        assert_eq!(
            Codebook::raw_preset(HuffContext::Block, 7),
            Codebook::raw_preset(HuffContext::Block, 15)
        );
    }

    #[test]
    fn decode_round_trips_mixed() {
        let cb = Codebook::build(&[1, 2, 3, 3]).unwrap();
        // Emit symbols 2, 0, 3, 1 then padding.
        let mut w = BitWriter::new();
        for &sym in &[2u16, 0, 3, 1] {
            let cw = cb.codewords().iter().find(|c| c.symbol == sym).unwrap();
            w.put_code(cw.code, cw.length);
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes).unwrap();
        assert_eq!(cb.decode(&mut r).unwrap(), 2);
        assert_eq!(cb.decode(&mut r).unwrap(), 0);
        assert_eq!(cb.decode(&mut r).unwrap(), 3);
        assert_eq!(cb.decode(&mut r).unwrap(), 1);
    }

    #[test]
    fn decode_kraft_valid_full_round_trip() {
        // A complete Kraft-valid codebook (sum exactly 2^max): lengths
        // [2,2,2,2] -> four length-2 codes 00,01,10,11. Decode all four
        // back from their own codewords.
        let cb = Codebook::build(&[2, 2, 2, 2]).unwrap();
        assert_eq!(Codebook::kraft_scaled(&[2, 2, 2, 2]), (4, 2)); // 4 == 1<<2
        let mut w = BitWriter::new();
        let order: Vec<u16> = cb.codewords().iter().map(|c| c.symbol).collect();
        for cw in cb.codewords() {
            w.put_code(cw.code, cw.length);
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes).unwrap();
        for &sym in &order {
            assert_eq!(cb.decode(&mut r).unwrap(), sym);
        }
    }

    #[test]
    fn from_huff_desc_defaults_to_preset7() {
        // None routes to the default preset id (7); since preset 7 is
        // not Kraft-valid this matches the same (error) result the
        // direct preset-7 build returns — the default-selection wiring
        // is what's under test, independent of the Kraft anomaly.
        let def = Codebook::from_huff_desc(HuffContext::Block, None);
        let p7 = Codebook::from_preset(HuffContext::Block, 7);
        assert_eq!(def, p7);
    }

    #[test]
    fn from_huff_desc_preset_and_custom() {
        // Preset selection wiring: from_huff_desc(Preset{3}) ==
        // from_preset(.., 3) (both anomalous, but the routing matches).
        let desc = super::super::HuffDesc::Preset { id: 3 };
        let from_desc = Codebook::from_huff_desc(HuffContext::Block, Some(&desc));
        let direct = Codebook::from_preset(HuffContext::Block, 3);
        assert_eq!(from_desc, direct);

        // Custom (inline) descriptors are genuine per-row bit-lengths the
        // encoder emits Kraft-valid; this one builds successfully.
        let custom = super::super::HuffDesc::Custom {
            row_lengths: vec![1, 2, 2],
        };
        let cb = Codebook::from_huff_desc(HuffContext::Block, Some(&custom)).unwrap();
        assert_eq!(cb, Codebook::build(&[1, 2, 2]).unwrap());
    }

    #[test]
    fn decode_unmatched_pattern_errors() {
        // A 1-symbol length-1 codebook (code 0). Feed bit 1 -> no
        // match within max_length.
        let cb = Codebook::build(&[1]).unwrap();
        let mut w = BitWriter::new();
        w.put(1, 1);
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes).unwrap();
        assert_eq!(cb.decode(&mut r), Err(CodebookError::NoMatch));
    }
}
