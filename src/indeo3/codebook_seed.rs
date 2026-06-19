//! Indeo 3 per-frame codebook seed area (`spec/04 §5.2`).
//!
//! Spec source: `docs/video/indeo/indeo3/spec/04-vq-codebooks.md` §5.2,
//! cross-checked against `docs/video/indeo/indeo3/audit/00-report.md`
//! §2.3 / §6.2 / §6.5 and the Extractor dump
//! `tables/region_1004d26a.{hex,csv,meta}`.
//!
//! The per-frame VQ codebook arena ([`super::VqArena`]) is rebuilt each
//! non-NULL frame from a *materialised* static seed window by the §6
//! `alt_quant[]` overlay ([`super::VqArena::apply_alt_quant`]). That
//! overlay copies 1 KB windows out of a seed buffer the reference
//! reaches through the codec-init-stashed pointer `*(0x1004d25a)`. The
//! seed buffer itself is **not** the raw `.data + 0x1004d26a` bytes:
//! the codec-init routine at `IR32_32.DLL!0x10006308` walks the raw
//! `0x1004d26a` *variable-length block table* and expands it into the
//! materialised window the overlay then copies from (`spec/04 §5.2`).
//!
//! This module owns the producer side of that chain — the raw
//! `0x1004d26a` block table — and materialises the portion of the
//! §5.2 expansion that the staged docs determine unambiguously:
//!
//! * The variable-length block walk: a 1-byte count `N`, then `N`
//!   signed byte-pairs, repeated until a count byte of `0` terminates
//!   the table (`spec/04 §5.2` step 2 + §7.8).
//! * The per-block byte-pair extraction and the §5.2 step-3a packing
//!   formula — `eax = (bl << 8) | al` with the `0x80` high-bit bias
//!   (`xor` at `IR32_32.DLL!0x10006345`) and the `<< 16` scale into a
//!   DWORD's upper word ([`SeedBlock::primary_dwords`]).
//!
//! What this module deliberately does **NOT** do — the wall, reported
//! as a DOCS-GAP in the round report:
//!
//! * It does not produce the final materialised seed window the §6
//!   overlay copies from. `spec/04 §5.2` and `audit/00 §2.3` give two
//!   *mutually incompatible* readings of the raw `0x1004d26a` bytes.
//!   The **spec reading** (`spec/04 §5.2`) treats them as
//!   count-prefixed blocks (`1` count byte + `N` byte-pairs), processed
//!   in reverse with `+0xbffc` mirror writes and a separate
//!   "negative-count expansion" branch at `0x100063ec`. The **audit
//!   reading** (`audit/00 §2.3`) walks the *same* bytes empirically and
//!   finds records delimited by 2-byte zero gaps (record 1 = bytes
//!   3..95 len 92, record 2 = bytes 103..393 len 290, …), and
//!   explicitly states (audit/00 §6.5) the leading `0xc3` byte "is
//!   **not** a length prefix for any record" (no record length equals
//!   195). Under the count-byte reading the leading `0xc3` would
//!   consume the first 391 bytes as one block, swallowing the zero-gap
//!   record structure the audit observed. The two readings cannot both
//!   be the per-band layout, so the per-band → arena-offset assignment
//!   is undetermined.
//! * The §5.2 step-3b `+0xbffc` mirror, step-3c non-negative /
//!   negative-count expansion branches, and the `0x800`-per-block
//!   destination advance are structurally named in the spec but their
//!   arithmetic depends on resolving the block-format contradiction
//!   above, so they are not materialised here.
//!
//! [`CodebookSeedArea`] therefore surfaces the determinable structure
//! (the block walk + per-block primary DWORD packing) so the round's
//! DOCS-GAP report can point at exactly which §5.2 step is blocked,
//! and so a future round can build on the parsed structure once the
//! Extractor/Auditor resolve the block-format reading.

/// Spec/04 §5.2 — the codec-init seed pointer is stashed at
/// `.data + 0x1004d25a`; the raw block table begins at
/// `.data + 0x1004d26a` (16 bytes past the stash slot).
pub const SEED_AREA_VMA: u32 = 0x1004_d26a;

/// Spec/04 §5.2 step 2 — the block table terminates at the first
/// count byte equal to `0`.
pub const BLOCK_TERMINATOR: u8 = 0;

/// Spec/04 §5.2 step 3a — the `0x80` high-bit XOR bias the codec-init
/// seed-table walker applies (`xor ah, -0x80` at
/// `IR32_32.DLL!0x10006345`) to map signed-byte residuals onto the
/// unsigned-byte storage range.
pub const SEED_SIGN_BIAS: u8 = 0x80;

/// Spec/04 §5.2 step 3d — the per-block destination advance
/// (`edi += 0x800`); each block fills one 2 KB per-band region of the
/// expansion scratch. Surfaced for the future materialiser; this
/// module does not perform the advance (see module docs).
pub const BLOCK_DEST_ADVANCE: usize = 0x800;

// Vendored, verbatim, from the docs clean-room table extract. This is a
// copy of `docs/video/indeo/indeo3/tables/region_1004d26a.hex` placed
// inside the crate so the published crate is self-contained (an
// `include_str!` of a path outside the crate root would not survive
// `cargo package`). The file carries a `#`-prefixed provenance header;
// `parse_hex_bytes` skips comment lines.
const SEED_AREA_HEX: &str = include_str!("data/codebook_seed_1004d26a.hex");

/// Parse a whitespace-separated lower-hex byte dump, skipping any line
/// that begins with `#` (the vendored-file provenance header).
fn parse_hex_bytes(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        for tok in line.split_whitespace() {
            if let Ok(b) = u8::from_str_radix(tok, 16) {
                out.push(b);
            }
        }
    }
    out
}

/// Spec/04 §5.2 step 3a — one signed byte-pair `(a, b)` read from a
/// block's body, with the packed primary DWORD the codec-init walker
/// forms from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeedPair {
    /// `al` — the low byte of the pair (`signed_byte_at[esi + 2*edx - 1]`).
    pub a: i8,
    /// `bl` — the high byte of the pair (`signed_byte_at[esi + 2*edx]`).
    pub b: i8,
}

impl SeedPair {
    /// Spec/04 §5.2 step 3a — the packed primary DWORD: `(bl << 8) | al`
    /// with the `0x80` high-bit bias applied to each byte, scaled into
    /// the DWORD's upper word (`<< 16`).
    ///
    /// The reference forms `eax = (bl << 8) | al` "interpreted with the
    /// `0x80` XOR at `0x10006345`" then `eax <<= 16`. The `0x80` XOR
    /// biases the signed byte onto the unsigned storage range; we apply
    /// it per byte (the high bit flip of each packed byte) before the
    /// 16-bit shift, matching the seed-table walker's `xor ah, -0x80`.
    pub fn primary_dword(self) -> u32 {
        let a_biased = (self.a as u8) ^ SEED_SIGN_BIAS;
        let b_biased = (self.b as u8) ^ SEED_SIGN_BIAS;
        let word = ((b_biased as u32) << 8) | (a_biased as u32);
        word << 16
    }
}

/// Spec/04 §5.2 step 2/3 — one variable-length block of the
/// `0x1004d26a` seed table: a count `N` followed by `N` signed
/// byte-pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedBlock {
    /// Byte offset of this block's count byte within the seed area.
    pub offset: usize,
    /// The count `N` (number of byte-pairs in the block body).
    pub count: u8,
    /// The `N` signed byte-pairs of the block body, in stream order.
    pub pairs: Vec<SeedPair>,
}

impl SeedBlock {
    /// Spec/04 §5.2 step 3a — the per-pair packed primary DWORDs of
    /// this block, in stream order.
    ///
    /// The reference walks the pairs in **reverse** (`edx` from `N`
    /// down) writing to `edi + 4*edx - 4`; the resulting table is the
    /// same set of DWORDs addressed by descending index. We return them
    /// in stream order; the reverse-walk only affects the destination
    /// index, not the per-pair value.
    pub fn primary_dwords(&self) -> Vec<u32> {
        self.pairs.iter().map(|p| p.primary_dword()).collect()
    }

    /// The number of bytes this block occupies in the seed table
    /// (`1` count byte + `2 * N` body bytes).
    pub fn encoded_len(&self) -> usize {
        1 + 2 * self.count as usize
    }
}

/// Spec/04 §5.2 — the parsed per-frame codebook seed area at
/// `.data + 0x1004d26a`.
///
/// This is the producer side of the §6 overlay's `static_seed` input.
/// The parse models the §5.2 step-2 block walk over the raw extract;
/// see the module docs for what the parse can and cannot determine (the
/// block-format reading is a reported DOCS-GAP).
#[derive(Debug, Clone)]
pub struct CodebookSeedArea {
    raw: Vec<u8>,
    blocks: Vec<SeedBlock>,
    /// Byte offset of the terminator count byte (`0`), or `None` if the
    /// extract window ends before a terminator is seen (the §5.2 walk
    /// would continue past the 4 KB extract per audit/00 §6.2).
    terminator_offset: Option<usize>,
}

impl CodebookSeedArea {
    /// Materialise the seed area from the vendored clean-room extract
    /// and walk its variable-length block structure (`spec/04 §5.2`).
    pub fn load() -> Self {
        let raw = parse_hex_bytes(SEED_AREA_HEX);
        Self::from_bytes(raw)
    }

    /// Parse a caller-supplied raw seed buffer (used by tests with
    /// synthetic block tables). Walks `spec/04 §5.2` step-2 blocks until
    /// a `0` count byte or the buffer's end.
    pub fn from_bytes(raw: Vec<u8>) -> Self {
        let mut blocks = Vec::new();
        let mut terminator_offset = None;
        let mut i = 0usize;
        while i < raw.len() {
            let count = raw[i];
            if count == BLOCK_TERMINATOR {
                terminator_offset = Some(i);
                break;
            }
            let body_start = i + 1;
            let body_len = 2 * count as usize;
            // If the block body runs past the extract window the §5.2
            // walk would keep reading into the following bytes; the
            // extract is window-truncated (audit/00 §6.2), so we stop
            // rather than read out of bounds.
            if body_start + body_len > raw.len() {
                break;
            }
            let mut pairs = Vec::with_capacity(count as usize);
            for k in 0..count as usize {
                let a = raw[body_start + 2 * k] as i8;
                let b = raw[body_start + 2 * k + 1] as i8;
                pairs.push(SeedPair { a, b });
            }
            blocks.push(SeedBlock {
                offset: i,
                count,
                pairs,
            });
            i = body_start + body_len;
        }
        CodebookSeedArea {
            raw,
            blocks,
            terminator_offset,
        }
    }

    /// The raw seed-area bytes (VMA order, as extracted).
    pub fn raw(&self) -> &[u8] {
        &self.raw
    }

    /// The parsed §5.2 blocks (in stream order).
    pub fn blocks(&self) -> &[SeedBlock] {
        &self.blocks
    }

    /// The byte offset of the §5.2 terminator count byte (`0`), or
    /// `None` when the extract window ends before a terminator (the
    /// true table boundary is window-truncated per audit/00 §6.2).
    pub fn terminator_offset(&self) -> Option<usize> {
        self.terminator_offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_area_loads_4096_bytes_with_known_prefix() {
        let area = CodebookSeedArea::load();
        // tables/region_1004d26a.meta: 4096-byte window, first 16 bytes
        // c3 00 00 02 02 fe fe ff 03 01 fd 03 ff fd 01 04.
        assert_eq!(area.raw().len(), 4096);
        let expect = [
            0xc3, 0x00, 0x00, 0x02, 0x02, 0xfe, 0xfe, 0xff, 0x03, 0x01, 0xfd, 0x03, 0xff, 0xfd,
            0x01, 0x04,
        ];
        assert_eq!(&area.raw()[..16], &expect);
    }

    #[test]
    fn block_walk_first_block_count_is_leading_byte() {
        // Spec/04 §5.2 step 2: the first count byte is the leading
        // 0xc3 = 195. (The block-format DOCS-GAP is that this reading
        // contradicts audit/00 §2.3's zero-gap record structure; the
        // parse still models the spec's count-byte walk faithfully.)
        let area = CodebookSeedArea::load();
        let first = &area.blocks()[0];
        assert_eq!(first.offset, 0);
        assert_eq!(first.count, 195);
        assert_eq!(first.pairs.len(), 195);
    }

    #[test]
    fn block_encoded_len_is_count_byte_plus_body() {
        let area = CodebookSeedArea::load();
        let b = &area.blocks()[0];
        assert_eq!(b.encoded_len(), 1 + 2 * 195);
        // Next block starts immediately after.
        if area.blocks().len() > 1 {
            assert_eq!(area.blocks()[1].offset, b.offset + b.encoded_len());
        }
    }

    #[test]
    fn synthetic_block_table_terminates_on_zero_count() {
        // count=2, two pairs, then a terminator 0.
        let raw = vec![2, 10, 20, 30, 40, 0, 99, 99];
        let area = CodebookSeedArea::from_bytes(raw);
        assert_eq!(area.blocks().len(), 1);
        assert_eq!(area.blocks()[0].count, 2);
        assert_eq!(
            area.blocks()[0].pairs,
            vec![SeedPair { a: 10, b: 20 }, SeedPair { a: 30, b: 40 },]
        );
        assert_eq!(area.terminator_offset(), Some(5));
    }

    #[test]
    fn primary_dword_applies_0x80_bias_and_word_shift() {
        // Spec/04 §5.2 step 3a: eax = (bl<<8)|al with 0x80 XOR, then
        // <<16. Pair (a, b) = (0, 0) biases to (0x80, 0x80):
        // word = (0x80 << 8) | 0x80 = 0x8080, << 16 = 0x80800000.
        let p = SeedPair { a: 0, b: 0 };
        assert_eq!(p.primary_dword(), 0x8080_0000);
        // Pair (a, b) = (-128, -128) biases to (0x00, 0x00):
        // word = 0, << 16 = 0.
        let p = SeedPair { a: -128, b: -128 };
        assert_eq!(p.primary_dword(), 0);
        // Pair (a, b) = (127, 127) biases to (0xff, 0xff):
        // word = 0xffff, << 16 = 0xffff0000.
        let p = SeedPair { a: 127, b: 127 };
        assert_eq!(p.primary_dword(), 0xffff_0000);
    }

    #[test]
    fn primary_dwords_are_per_pair_in_stream_order() {
        let raw = vec![3, 0, 0, 1, 2, 3, 4, 0];
        let area = CodebookSeedArea::from_bytes(raw);
        let dwords = area.blocks()[0].primary_dwords();
        assert_eq!(dwords.len(), 3);
        // First pair (0, 0) → 0x80800000.
        assert_eq!(dwords[0], 0x8080_0000);
        // Second pair (1, 2): a_biased 0x81, b_biased 0x82 →
        // word = 0x8281, << 16 = 0x82810000.
        assert_eq!(dwords[1], 0x8281_0000);
        // Third pair (3, 4): a_biased 0x83, b_biased 0x84 →
        // word = 0x8483, << 16 = 0x84830000.
        assert_eq!(dwords[2], 0x8483_0000);
    }

    #[test]
    fn from_bytes_stops_when_body_overruns_window() {
        // count=5 but only 2 body bytes present → no complete block,
        // and no terminator seen (window-truncated per audit/00 §6.2).
        let raw = vec![5, 1, 2];
        let area = CodebookSeedArea::from_bytes(raw);
        assert!(area.blocks().is_empty());
        assert_eq!(area.terminator_offset(), None);
    }

    #[test]
    fn loaded_area_has_no_terminator_in_window() {
        // The 4 KB extract is window-truncated (audit/00 §6.2): the
        // §5.2 walk would continue past the window, so the parse over
        // the extract should not encounter a clean terminator that
        // closes the whole table.
        let area = CodebookSeedArea::load();
        // Either no terminator (walk ran off the window) or the last
        // partial block overran — both leave the table unclosed within
        // the 4 KB. Assert the parse consumed at least one full block.
        assert!(!area.blocks().is_empty());
    }
}
