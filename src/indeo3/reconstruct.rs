//! Indeo 3 output reconstruction: predictor formation + the softSIMD
//! dyad-pair `predictor + delta` add.
//!
//! Spec source: `docs/video/indeo/indeo3/spec/07-output-reconstruction.md`.
//!
//! Round 6 lands the first half of spec/07: the pixel-emission
//! arithmetic that spec/06 (round 5) deferred. spec/06's
//! [`super::continuation_needed`] / [`super::apply_continuation_xor`]
//! answered the *entropy* question — how many bytes a literal mode
//! byte costs and the XOR back-out shape. This module answers the
//! *arithmetic* question: given a predictor pixel-pair and the dyad
//! delta entry the per-frame arena (spec/04 §2.1) holds, how the
//! decoder forms the output pixel-pair DWORD.
//!
//! What this round covers, mapped to the spec/07 sections:
//!
//! * §0 / §1.1 — the predictor address. The predictor for the pixel
//!   about to be written at `[edi]` is `[edi - 0xb0]` (one row above,
//!   same column, in the strip pixel buffer). [`PREDICTOR_ROW_STRIDE`]
//!   is the `0xb0` (176) row stride; [`predictor_offset`] computes the
//!   row-above byte index from a write index.
//! * §1.3 / §9 — the top-of-strip predictor seed. Cells whose
//!   row-above slot falls outside the strip read the zero-initialised
//!   padding, so the top-of-strip predictor is the constant
//!   [`TOP_OF_STRIP_PREDICTOR`] (`0x00`).
//! * §2.1 / §2.3 — the softSIMD dyad-pair add. [`apply_dyad_pair`]
//!   computes `predictor + primary_delta` as a byte-parallel DWORD
//!   add, detects the 16-bit-half overflow sentinel (`jns` on the
//!   full DWORD, then `js` on the low 16-bit half after the secondary
//!   add), runs the continuation fall-back against the secondary-table
//!   word, and faults if the secondary add is still sign-set.
//! * §4.1 / §4.2 — the 7-bit-per-byte range. There is no explicit
//!   saturation; bit 7 of every pixel byte is reserved as the
//!   edge / boundary marker, and the overflow sentinel (bit 15 of
//!   each 16-bit half) doubles as the continuation trigger.
//!   [`SoftSimdSum`] records both halves' overflow state.
//!
//! What this round deliberately does **not** do (the spec/07 chapter
//! boundary on the output side, plus the multi-cell scope):
//!
//! * No cell-stack walk, no per-cell-variant inner loop (variants
//!   A–D, §2.2), no row-band advance, and no inter-cell edge fix-up
//!   (§1.3). This module is the per-position arithmetic kernel the
//!   variant loops call, not the loops themselves.
//! * No strip-buffer allocation, no plane assembly, no 7-bit→8-bit
//!   upshift, and no YUV→RGB / IF09 conversion (§4.3, §5). Those are
//!   the output-buffer-write stage.
//! * No motion compensation (`spec/05`).
//! * No static dyad delta-table (`.data + 0x1003d088`) value
//!   interpretation beyond what spec/04's [`super::DyadDeltaTable`]
//!   already materialises (§3 here is the same table spec/04 §1.3
//!   vendored).
//!
//! The contract: given a 4-byte predictor DWORD (four horizontally
//! adjacent predictor pixels in softSIMD layout) and the primary /
//! optional secondary dyad entries the arena holds, this module
//! produces the output pixel-pair DWORD (or a fault) exactly as the
//! `add eax, [esi + 4*edx + 0x400]` chain at
//! `IR32_32.DLL!0x10006e0f..0x10006e2e` does.

use super::CONTINUATION_XOR;

/// Spec/07 §0 / §1.1 — the strip pixel buffer's row stride, `0xb0`
/// (176) bytes. The predictor for `[edi]` is at `[edi - 0xb0]`.
pub const PREDICTOR_ROW_STRIDE: usize = 0xb0;

/// Spec/07 §1.3 / §9 — the top-of-strip predictor seed. The strip
/// allocator zero-fills the buffer (codec-init zero-fill at
/// `IR32_32.DLL!0x10004013`), so a cell whose row-above slot falls in
/// the pre-allocated padding reads the constant `0x00` (pixel value 0
/// in the internal 7-bit range, i.e. black).
pub const TOP_OF_STRIP_PREDICTOR: u8 = 0x00;

/// Spec/07 §4.2 — the internal pixel range is 7 bits per byte
/// (`0..=0x7f`). Bit 7 of every pixel byte is reserved as the
/// edge / boundary marker (set by the VQ_NULL `01` mark-skip path and
/// the `0xF8`/`0xF9`/`0xFA` RLE escapes, spec/06 §4.2).
pub const PIXEL_VALUE_MAX: u8 = 0x7f;

/// Spec/07 §4.2 — the edge / boundary marker bit (bit 7) reserved on
/// every internal pixel byte.
pub const EDGE_MARKER_BIT: u8 = 0x80;

/// Spec/07 §2.3 / §4.1 — the per-16-bit-half overflow sentinel mask.
///
/// A softSIMD DWORD holds two 16-bit halves, each carrying a 2-pixel
/// dyad-pair. Bit 15 of each half is the overflow / continuation
/// sentinel: after the `add eax, [primary]`, a set bit 15 in either
/// half means the primary table could not represent the delta in one
/// byte and the continuation path is taken (`xor eax, 0x80008000`).
pub const HALF_SENTINEL_MASK: u32 = 0x8000_8000;

/// Spec/07 §1.1 — the byte offset of the predictor for a write at byte
/// index `write_index` within the strip pixel buffer.
///
/// The predictor is the pixel one row above (`[edi - 0xb0]`). Returns
/// `None` when `write_index < PREDICTOR_ROW_STRIDE`, i.e. the write is
/// in the strip's top row and the predictor falls into the
/// pre-allocated padding region (where the seed is the constant
/// [`TOP_OF_STRIP_PREDICTOR`] per §1.3).
pub fn predictor_offset(write_index: usize) -> Option<usize> {
    write_index.checked_sub(PREDICTOR_ROW_STRIDE)
}

/// Spec/07 §2.3 / §4.1 — the overflow state of a softSIMD DWORD add.
///
/// After `predictor + primary_delta`, each 16-bit half's bit-15
/// sentinel is checked independently. The decoder's `jns` at
/// `IR32_32.DLL!0x10006e16` tests bit 31 (the *high* half's sentinel);
/// the low half's sentinel is consulted by the subsequent `js` after
/// the 16-bit `add ax` in the continuation path. This type records
/// both so the per-position kernel can reproduce the exact branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoftSimdSum {
    /// The raw 32-bit sum (`predictor + primary_delta`), wrapping.
    pub raw: u32,
    /// Bit 15 of the low 16-bit half (`raw & 0x0000_8000`).
    pub low_half_overflow: bool,
    /// Bit 15 of the high 16-bit half (`raw & 0x8000_0000`, i.e.
    /// bit 31). The `jns 0x10006e2f` tests this bit.
    pub high_half_overflow: bool,
}

impl SoftSimdSum {
    /// Spec/07 §2.1 — form `predictor + primary_delta` as a wrapping
    /// 32-bit add and record both halves' sentinel bits.
    pub fn add(predictor: u32, primary_delta: u32) -> Self {
        let raw = predictor.wrapping_add(primary_delta);
        SoftSimdSum {
            raw,
            low_half_overflow: raw & 0x0000_8000 != 0,
            high_half_overflow: raw & 0x8000_0000 != 0,
        }
    }

    /// Spec/07 §2.3 — `true` if *either* 16-bit half has its bit-15
    /// sentinel set, i.e. the documented "continuation needed"
    /// condition (§2.3 / §4.1). This is the per-half semantic; the
    /// literal `jns` instruction tests only [`Self::high_half_overflow`]
    /// (see [`jns_taken`]).
    pub fn any_half_overflow(self) -> bool {
        self.low_half_overflow || self.high_half_overflow
    }
}

/// Spec/07 §2.1 / §2.3 — the literal `jns 0x10006e2f` test at
/// `IR32_32.DLL!0x10006e16`: the jump (no continuation) is taken when
/// the 32-bit sum's sign bit (bit 31, the high half's sentinel) is
/// **clear**. This mirrors [`super::continuation_needed`] exactly —
/// kept here so the reconstruction kernel can document the instruction
/// it reproduces without reaching across modules.
pub fn jns_taken(sum: u32) -> bool {
    sum & 0x8000_0000 == 0
}

/// Spec/07 §2 — the outcome of applying a dyad-pair codebook entry to
/// the predictor at one cell position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DyadOutcome {
    /// The primary-table add stayed in range (the high-half sentinel
    /// was clear, `jns` taken). The output pixel-pair DWORD is the raw
    /// sum; no continuation byte was consumed.
    Primary {
        /// The output pixel-pair DWORD written to `[edi]`.
        pixels: u32,
    },
    /// The primary-table add overflowed; the continuation byte drove a
    /// secondary-table add that stayed in range. The output DWORD is
    /// the post-`xor`/post-secondary-add value; one continuation byte
    /// was consumed (`inc ebp`).
    Continuation {
        /// The output pixel-pair DWORD written to `[edi]`.
        pixels: u32,
    },
    /// Both the primary and the secondary add left the low half's sign
    /// bit set; the decoder faults to error code 2 at
    /// `IR32_32.DLL!0x1000855f` (§2.3 step 3, §4.1). An encoder that
    /// emits such a sequence produces a bitstream the decoder rejects.
    Fault,
}

/// Spec/07 §2.1 / §2.3 / §4.1 — apply a dyad-pair codebook entry to a
/// predictor DWORD, reproducing the inner-loop body at
/// `IR32_32.DLL!0x10006e0f..0x10006e2e`:
///
/// ```text
/// mov eax, [edi - 0xb0]              ; predictor DWORD
/// add eax, [esi + 4*edx + 0x400]     ; predictor + primary delta
/// jns 0x10006e2f                     ; if no overflow → Primary
/// mov dl, [ebp + 0x1]                ; read continuation byte
/// xor eax, 0x80008000                ; flip the two half-high bits
/// add ax, [esi + 4*edx + 0x402]      ; add secondary 16-bit word
/// js  0x1000855f                     ; still negative → Fault
/// inc ebp                            ; consume continuation byte
/// mov [edi], eax                     ; → Continuation
/// ```
///
/// `predictor` is the 4-byte row-above DWORD (`[edi - 0xb0]`).
/// `primary_delta` is the per-frame-arena primary-table DWORD at
/// `[esi + 4*edx + 0x400]` (spec/04 §2.1; the arena lookup itself is
/// the caller's job). `secondary_word` is the low-16-bit secondary
/// word at `[esi + 4*edx + 0x402]`, consulted only on a continuation.
///
/// Returns a [`DyadOutcome`] describing which path the decoder took
/// and the resulting pixel-pair DWORD; the caller stores `pixels` to
/// `[edi]` and, on [`DyadOutcome::Continuation`], advances the
/// bitstream cursor by one (`inc ebp`).
pub fn apply_dyad_pair(predictor: u32, primary_delta: u32, secondary_word: u16) -> DyadOutcome {
    // `add eax, [primary]`.
    let sum = predictor.wrapping_add(primary_delta);
    // `jns 0x10006e2f`: no high-half overflow → take the primary path.
    if jns_taken(sum) {
        return DyadOutcome::Primary { pixels: sum };
    }
    // Continuation path. `xor eax, 0x80008000` backs out the two
    // half-high sentinel bits before the secondary add (§2.3 step 1).
    let backed_out = sum ^ CONTINUATION_XOR;
    // `add ax, [secondary]`: a *16-bit* add affecting only the low
    // half (§2.3 step 2). The high half is preserved.
    let low = (backed_out & 0x0000_ffff) as u16;
    let new_low = low.wrapping_add(secondary_word);
    let combined = (backed_out & 0xffff_0000) | (new_low as u32);
    // `js 0x1000855f`: if the low half's sign bit (bit 15) is still
    // set after the secondary add, fault to error code 2 (§2.3 step 3,
    // §4.1). The `js` here tests the 16-bit result's sign, i.e. bit 15
    // of the low half.
    if new_low & 0x8000 != 0 {
        return DyadOutcome::Fault;
    }
    // `inc ebp` (continuation byte consumed); `mov [edi], eax`.
    DyadOutcome::Continuation { pixels: combined }
}

/// Spec/07 §1.1 / §0 — pack four horizontally adjacent predictor
/// pixel bytes into the little-endian softSIMD DWORD the inner loop
/// loads with `mov eax, [edi - 0xb0]`.
///
/// The four bytes are laid out low-to-high: `bytes[0]` is the leftmost
/// pixel (the low byte of the DWORD), matching the x86 little-endian
/// `mov eax, [mem]` load of four consecutive buffer bytes.
pub fn pack_predictor(bytes: [u8; 4]) -> u32 {
    u32::from_le_bytes(bytes)
}

/// Spec/07 §2.4 — unpack an output pixel-pair / quad DWORD back into
/// its four pixel bytes in raster (left-to-right) order, the inverse
/// of [`pack_predictor`]. The dyad iteration order (`wiki/Indeo_3.wiki`
/// §"VQ data codes") writes pixels left-to-right within the row, so
/// `bytes[0]` is the leftmost emitted pixel.
pub fn unpack_pixels(dword: u32) -> [u8; 4] {
    dword.to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predictor_stride_and_seed_constants() {
        assert_eq!(PREDICTOR_ROW_STRIDE, 0xb0);
        assert_eq!(PREDICTOR_ROW_STRIDE, 176);
        assert_eq!(TOP_OF_STRIP_PREDICTOR, 0x00);
        assert_eq!(PIXEL_VALUE_MAX, 0x7f);
        assert_eq!(EDGE_MARKER_BIT, 0x80);
        assert_eq!(HALF_SENTINEL_MASK, 0x8000_8000);
    }

    #[test]
    fn predictor_offset_is_row_above() {
        // A write at byte 0xb0 reads the predictor at byte 0.
        assert_eq!(predictor_offset(0xb0), Some(0));
        // A write at byte 0x160 (2 rows down) reads byte 0xb0.
        assert_eq!(predictor_offset(0x160), Some(0xb0));
        // Top-row writes (< stride) have no in-buffer predictor: the
        // seed is the constant 0x00 (§1.3).
        assert_eq!(predictor_offset(0), None);
        assert_eq!(predictor_offset(PREDICTOR_ROW_STRIDE - 1), None);
    }

    #[test]
    fn softsimd_sum_records_both_half_sentinels() {
        // No overflow in either half.
        let s = SoftSimdSum::add(0x0001_0001, 0x0002_0002);
        assert_eq!(s.raw, 0x0003_0003);
        assert!(!s.low_half_overflow);
        assert!(!s.high_half_overflow);
        assert!(!s.any_half_overflow());

        // Low-half-only overflow (bit 15 of the low half set).
        let s = SoftSimdSum::add(0x0000_7fff, 0x0000_0001);
        assert_eq!(s.raw, 0x0000_8000);
        assert!(s.low_half_overflow);
        assert!(!s.high_half_overflow);
        assert!(s.any_half_overflow());

        // High-half-only overflow (bit 31 set).
        let s = SoftSimdSum::add(0x7fff_0000, 0x0001_0000);
        assert_eq!(s.raw, 0x8000_0000);
        assert!(!s.low_half_overflow);
        assert!(s.high_half_overflow);
        assert!(s.any_half_overflow());
    }

    #[test]
    fn jns_taken_matches_high_half_sentinel() {
        // jns is taken (no continuation) when bit 31 is clear.
        assert!(jns_taken(0x0000_0000));
        assert!(jns_taken(0x7fff_ffff));
        assert!(!jns_taken(0x8000_0000));
        assert!(!jns_taken(0xffff_ffff));
        // It is exactly the inverse of spec/06's continuation_needed.
        for v in [0x0000_0000u32, 0x7fff_ffff, 0x8000_0000, 0xffff_ffff] {
            assert_eq!(jns_taken(v), !super::super::continuation_needed(v));
        }
    }

    #[test]
    fn primary_path_when_high_half_in_range() {
        // predictor + primary stays with bit 31 clear → Primary, raw
        // sum, no continuation byte.
        let out = apply_dyad_pair(0x0010_0010, 0x0005_0005, 0xffff);
        assert_eq!(
            out,
            DyadOutcome::Primary {
                pixels: 0x0015_0015
            }
        );
    }

    #[test]
    fn primary_path_ignores_secondary_word() {
        // When the primary add does not overflow, the secondary word is
        // never consulted (the continuation byte is not consumed).
        let a = apply_dyad_pair(0x0001_0001, 0x0001_0001, 0x0000);
        let b = apply_dyad_pair(0x0001_0001, 0x0001_0001, 0x7fff);
        assert_eq!(a, b);
        assert_eq!(
            a,
            DyadOutcome::Primary {
                pixels: 0x0002_0002
            }
        );
    }

    #[test]
    fn continuation_path_backs_out_and_adds_secondary() {
        // Force a high-half overflow so the continuation path runs.
        // predictor 0x7fff_0000 + primary 0x0001_0000 = 0x8000_0000:
        // bit 31 set → continuation.
        // xor 0x80008000 → 0x0000_8000. Low half = 0x8000.
        // add ax, secondary: pick secondary so the low half clears its
        // sign bit. 0x8000 + 0x8000 = 0x0000 (wrapping), bit 15 clear.
        let out = apply_dyad_pair(0x7fff_0000, 0x0001_0000, 0x8000);
        // backed_out high half = 0x0000; low half 0x8000 + 0x8000 = 0.
        assert_eq!(
            out,
            DyadOutcome::Continuation {
                pixels: 0x0000_0000
            }
        );
    }

    #[test]
    fn continuation_preserves_high_half() {
        // High half after xor must survive the 16-bit secondary add.
        // predictor 0x1234_0000 + primary 0x8000_0000 = 0x9234_0000:
        // bit 31 set → continuation. xor 0x80008000 → 0x1234_8000.
        // low 0x8000 + secondary 0x8001 = 0x0001 (bit 15 clear).
        let out = apply_dyad_pair(0x1234_0000, 0x8000_0000, 0x8001);
        assert_eq!(
            out,
            DyadOutcome::Continuation {
                pixels: 0x1234_0001
            }
        );
    }

    #[test]
    fn fault_when_secondary_still_sign_set() {
        // Continuation path, but the secondary add leaves bit 15 set →
        // Fault (error code 2).
        // predictor 0x7fff_0000 + primary 0x0001_0000 = 0x8000_0000.
        // xor → 0x0000_8000, low = 0x8000. secondary 0x0001:
        // 0x8000 + 0x0001 = 0x8001, bit 15 still set → Fault.
        let out = apply_dyad_pair(0x7fff_0000, 0x0001_0000, 0x0001);
        assert_eq!(out, DyadOutcome::Fault);
    }

    #[test]
    fn pack_and_unpack_predictor_round_trip() {
        let bytes = [0x12u8, 0x34, 0x56, 0x70];
        let dword = pack_predictor(bytes);
        // Little-endian: leftmost pixel is the low byte.
        assert_eq!(dword, 0x7056_3412);
        assert_eq!(unpack_pixels(dword), bytes);
    }

    #[test]
    fn realistic_in_range_dyad_pair() {
        // Two adjacent predictor pixels 0x20, 0x30 in the low half; the
        // primary delta adds 0x05, 0x07. softSIMD: low half holds
        // (pixel1<<8)|pixel0 = 0x3020; delta 0x0705 → 0x3725. Stays in
        // 7-bit-per-byte range (no bit-7 carry), no overflow.
        let predictor = pack_predictor([0x20, 0x30, 0x00, 0x00]);
        // primary delta in matching softSIMD layout.
        let primary = pack_predictor([0x05, 0x07, 0x00, 0x00]);
        let out = apply_dyad_pair(predictor, primary, 0xffff);
        match out {
            DyadOutcome::Primary { pixels } => {
                let p = unpack_pixels(pixels);
                assert_eq!(p[0], 0x25);
                assert_eq!(p[1], 0x37);
                // Pixel values stay within the 7-bit range.
                assert!(p[0] <= PIXEL_VALUE_MAX);
                assert!(p[1] <= PIXEL_VALUE_MAX);
            }
            other => panic!("expected Primary, got {other:?}"),
        }
    }
}
