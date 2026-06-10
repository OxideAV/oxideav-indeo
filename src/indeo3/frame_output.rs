//! Indeo 3 output-buffer write: the spec/07 §4.3 1-bit output
//! upshift, the §5.6 IF09 / YVU9 passthrough surface, and the §5.7
//! strip-to-frame assembly executor.
//!
//! Spec source: `docs/video/indeo/indeo3/spec/07-output-reconstruction.md`.
//!
//! Round 28 lands the output stage the round-27 `frame_exit` handoff
//! targets (`FRAME_OUTPUT_RECONSTRUCTION_RVA`, spec/02 §6.2): once
//! all three planes have decoded into their 0xb0-stride strip pixel
//! buffers (spec/07 §5.1), the YVU-planar output path copies each
//! plane to the host buffer. This module covers, mapped to the
//! spec/07 sections:
//!
//! * §4.3 — the 1-bit upshift from the internal 7-bit-per-byte
//!   representation back to 8-bit output values
//!   ([`upshift_7bit_to_8bit`], `shl byte, 1`). The shift discards
//!   bit 7, so the §4.2 / §4.4 edge-marker sentinel is cleared as a
//!   side effect ([`super::EDGE_MARKER_BIT`] never reaches the output).
//! * §5.3 / §5.6 — the IF09 / YVU9 passthrough dispatch surface:
//!   the FOURCC the format dispatch compares against
//!   ([`IF09_FOURCC`], referenced at [`IF09_FOURCC_CASE_RVA`]) and
//!   the passthrough conversion function's entry RVA
//!   ([`IF09_PASSTHROUGH_RVA`], the §5.3 table's "IF09 passthrough"
//!   row).
//! * §5.6 step 2 — the output plane order **Y first, then V, then
//!   U** ([`OUTPUT_PLANE_ORDER`]), which is exactly the reverse of
//!   the §5.2 decode-time iteration order
//!   ([`super::PLANE_ITERATION_ORDER`], U → V → Y); a `const _`
//!   cross-check pins the reversal.
//! * §5.7 — the strip-to-frame assembly executor
//!   ([`assemble_plane_if09`]): the conversion-time loop that walks
//!   the plane's strips in left-to-right order, reads each strip's
//!   own 0xb0-stride pixel buffer, applies the §4.3 / §5.6 step 1b
//!   per-byte upshift, and writes the strip's rows into the
//!   corresponding horizontal slice of the caller's output plane
//!   raster. Per §5.7 the assembly happens at conversion time only —
//!   each strip is decoded into its own buffer independently and the
//!   strips meet for the first time here.
//!
//! What this module deliberately does **not** do (the spec/07
//! chapter boundaries):
//!
//! * No YUV → RGB conversion. The §5.3 RGB conversion functions are
//!   LUT-driven (§5.4) and their LUT contents are populated at
//!   codec-init time via register-indirect stores the audit could
//!   not pin (spec/07 §5.4 audit note + §7.2 open question) — the
//!   RGB paths stay deferred until the LUT-population evidence is
//!   staged.
//! * No chroma upsampling. §5.5's 4×4 box replication belongs to the
//!   RGB conversion loops; the IF09 passthrough this module models
//!   keeps the chroma planes at their 4:1:0 subsampling (§5.6
//!   closing paragraph).
//! * No frame finalisation. The §6 saved-frame-flags / frame-number
//!   state updates and the §6.3 return code are the next chapter
//!   slice above this one.
//! * No plane decode and no strip pixel-buffer ownership. The
//!   caller supplies the decoded strip buffers (spec/05 §4.1
//!   regions); this module only reads them.

use super::mc_arena::MC_ARENA_ROW_STRIDE;
use super::picture_layer::{PLANE_COUNT, PLANE_IDX_U, PLANE_IDX_V, PLANE_IDX_Y};
use super::strip_context::StripGeometry;

/// Spec/07 §4.3 — the output upshift's bit count. "The pixel values
/// shall be shifted one bit left to convert them back to 8-bit
/// values"; the YVU-planar output path applies `shl byte, 1` per
/// byte.
pub const OUTPUT_UPSHIFT_BITS: u32 = 1;

/// Spec/07 §5.6 — the IF09 FOURCC value the output-format dispatch
/// compares against (`'IF09'` = `0x39304649`, little-endian byte
/// order `I`, `F`, `0`, `9`).
pub const IF09_FOURCC: u32 = 0x3930_4649;

// §5.6 — the FOURCC constant spells "IF09" in stream byte order.
const _: () = assert!(IF09_FOURCC == u32::from_le_bytes(*b"IF09"));

/// Spec/07 §5.6 — the RVA of the `case 0x39304649` FOURCC reference
/// in the per-frame decode entry's output-format dispatch.
pub const IF09_FOURCC_CASE_RVA: u32 = 0x1000_4576;

/// Spec/07 §5.3 / §5.6 — the IF09 / YVU9 passthrough conversion
/// function's entry RVA (the §5.3 dispatch-table row "IF09
/// passthrough: 7-bit-to-8-bit upshift + plane copy").
pub const IF09_PASSTHROUGH_RVA: u32 = 0x1000_a53c;

/// Spec/07 §5.6 step 2 — the output-buffer plane order: "Plane
/// order in the output is Y first, then V, then U."
pub const OUTPUT_PLANE_ORDER: [usize; PLANE_COUNT] = [PLANE_IDX_Y, PLANE_IDX_V, PLANE_IDX_U];

// §5.2 vs §5.6 — the output plane order is exactly the reverse of
// the decode-time iteration order (U → V → Y, frame_exit's
// PLANE_ITERATION_ORDER).
const _: () = {
    assert!(OUTPUT_PLANE_ORDER[0] == super::frame_exit::PLANE_ITERATION_ORDER[2]);
    assert!(OUTPUT_PLANE_ORDER[1] == super::frame_exit::PLANE_ITERATION_ORDER[1]);
    assert!(OUTPUT_PLANE_ORDER[2] == super::frame_exit::PLANE_ITERATION_ORDER[0]);
};

// §5.6 — the output plane order is a permutation of 0..PLANE_COUNT.
const _: () = {
    assert!(OUTPUT_PLANE_ORDER[0] != OUTPUT_PLANE_ORDER[1]);
    assert!(OUTPUT_PLANE_ORDER[1] != OUTPUT_PLANE_ORDER[2]);
    assert!(OUTPUT_PLANE_ORDER[0] != OUTPUT_PLANE_ORDER[2]);
    assert!(OUTPUT_PLANE_ORDER[0] < PLANE_COUNT);
    assert!(OUTPUT_PLANE_ORDER[1] < PLANE_COUNT);
    assert!(OUTPUT_PLANE_ORDER[2] < PLANE_COUNT);
};

/// Spec/07 §5.1 — the strip pixel buffer's allocated row stride the
/// assembly reads at (`0xb0`), aliasing the spec/05 §4.1
/// [`MC_ARENA_ROW_STRIDE`].
pub const FRAME_OUTPUT_SRC_ROW_STRIDE: usize = MC_ARENA_ROW_STRIDE;

// §5.1 — one row stride across the whole pipeline.
const _: () = assert!(FRAME_OUTPUT_SRC_ROW_STRIDE == 0xb0);
const _: () = assert!(FRAME_OUTPUT_SRC_ROW_STRIDE == super::strip_edge::STRIP_EDGE_ROW_STRIDE);

/// Spec/07 §4.3 — upshift one internal 7-bit pixel byte to its
/// 8-bit output value (`shl byte, 1`).
///
/// The shift discards bit 7: per §4.4 the [`super::EDGE_MARKER_BIT`]
/// sentinel "is cleared by the shift" and only the 7-bit content
/// reaches the output. For every byte the result equals
/// `(b & 0x7f) << 1`.
pub const fn upshift_7bit_to_8bit(b: u8) -> u8 {
    b << OUTPUT_UPSHIFT_BITS
}

/// Spec/07 §5.1 / §5.7 — the minimum byte length of one strip's
/// pixel-buffer slice for an assembly walk of `plane_height` rows
/// of `strip_width` visible pixels: the last row only needs its
/// visible pixels, every earlier row advances by the full
/// [`FRAME_OUTPUT_SRC_ROW_STRIDE`].
pub const fn strip_min_buffer_bytes(strip_width: u32, plane_height: u32) -> usize {
    if plane_height == 0 || strip_width == 0 {
        0
    } else {
        (plane_height as usize - 1) * FRAME_OUTPUT_SRC_ROW_STRIDE + strip_width as usize
    }
}

/// Spec/07 §5.7 — failure modes of the strip-to-frame assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaneAssembleError {
    /// The caller supplied a different number of strip buffers than
    /// the §5.7 / spec/02 §4.1 geometry's `strip_count`.
    StripCountMismatch {
        /// `geometry.strip_count`.
        expected: u32,
        /// `strips.len()`.
        supplied: usize,
    },
    /// The geometry's per-strip widths do not sum to its
    /// `plane_width` — the §5.7 horizontal tiling would leave a gap
    /// or overrun the raster. (Never produced by
    /// [`StripGeometry::for_luma`] / [`StripGeometry::for_chroma`];
    /// guards hand-built geometry values.)
    StripWidthSumMismatch {
        /// `geometry.plane_width`.
        plane_width: u32,
        /// The sum over `geometry.iter_strip_widths()`.
        widths_total: u32,
    },
    /// One strip's width reaches past the strip buffer's allocated
    /// row stride (`0xb0`, spec/07 §5.1) — no such strip layout can
    /// exist.
    StripWidthExceedsRowStride {
        /// Index of the offending strip (left-to-right).
        strip_index: usize,
        /// The supplied strip width (in pixels).
        strip_width: u32,
    },
    /// One strip's pixel-buffer slice is shorter than the
    /// [`strip_min_buffer_bytes`] walk requires.
    StripBufferTooShort {
        /// Index of the offending strip (left-to-right).
        strip_index: usize,
        /// Bytes the walk requires.
        required: usize,
        /// Bytes supplied.
        supplied: usize,
    },
    /// The caller's output row stride is narrower than the plane
    /// width — the §5.7 full-width raster cannot fit one row.
    DstStrideTooNarrow {
        /// `geometry.plane_width`.
        plane_width: u32,
        /// The supplied output row stride (in bytes).
        dst_stride: usize,
    },
    /// The output slice is shorter than the assembled plane
    /// requires.
    DstBufferTooShort {
        /// Bytes the assembled plane requires.
        required: usize,
        /// Bytes supplied.
        supplied: usize,
    },
}

impl core::fmt::Display for PlaneAssembleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PlaneAssembleError::StripCountMismatch { expected, supplied } => write!(
                f,
                "spec/07 §5.7: geometry expects {expected} strip buffer(s); {supplied} supplied"
            ),
            PlaneAssembleError::StripWidthSumMismatch {
                plane_width,
                widths_total,
            } => write!(
                f,
                "spec/07 §5.7: per-strip widths sum to {widths_total}, \
                 not the plane width {plane_width}"
            ),
            PlaneAssembleError::StripWidthExceedsRowStride {
                strip_index,
                strip_width,
            } => write!(
                f,
                "spec/07 §5.1: strip {strip_index} width {strip_width} exceeds \
                 the allocated row stride 0xb0"
            ),
            PlaneAssembleError::StripBufferTooShort {
                strip_index,
                required,
                supplied,
            } => write!(
                f,
                "spec/07 §5.7: strip {strip_index} pixel buffer has {supplied} byte(s); \
                 the assembly walk requires at least {required}"
            ),
            PlaneAssembleError::DstStrideTooNarrow {
                plane_width,
                dst_stride,
            } => write!(
                f,
                "spec/07 §5.7: output row stride {dst_stride} is narrower than \
                 the plane width {plane_width}"
            ),
            PlaneAssembleError::DstBufferTooShort { required, supplied } => write!(
                f,
                "spec/07 §5.7: output slice has {supplied} byte(s); \
                 the assembled plane requires at least {required}"
            ),
        }
    }
}

impl std::error::Error for PlaneAssembleError {}

/// Spec/07 §5.6 / §5.7 — assemble one plane of the IF09 / YVU9
/// passthrough output from its decoded strip pixel buffers.
///
/// Walks the plane's strips in left-to-right order (§5.7), reading
/// `plane_height` rows of each strip's visible pixels from its own
/// [`FRAME_OUTPUT_SRC_ROW_STRIDE`]-stride buffer (§5.6 step 1a),
/// applying the §4.3 1-bit upshift per byte (§5.6 step 1b, which
/// also clears the §4.4 edge-marker bit), and writing the result to
/// the corresponding horizontal slice of `dst` (§5.6 step 1c), whose
/// rows are `dst_stride` bytes apart. Per §5.7, every strip except
/// possibly the last is `geometry.strip_width` pixels wide; the last
/// carries the spec/02 §4.1 remainder width.
///
/// Bytes of `dst` outside the plane's `plane_width × plane_height`
/// raster (stride padding, trailing slack) are left untouched.
///
/// Returns the number of pixel bytes written
/// (`plane_width × plane_height`).
pub fn assemble_plane_if09(
    geometry: &StripGeometry,
    strips: &[&[u8]],
    dst: &mut [u8],
    dst_stride: usize,
) -> Result<usize, PlaneAssembleError> {
    if strips.len() != geometry.strip_count as usize {
        return Err(PlaneAssembleError::StripCountMismatch {
            expected: geometry.strip_count,
            supplied: strips.len(),
        });
    }

    let widths_total: u32 = geometry.iter_strip_widths().sum();
    if widths_total != geometry.plane_width {
        return Err(PlaneAssembleError::StripWidthSumMismatch {
            plane_width: geometry.plane_width,
            widths_total,
        });
    }

    let plane_width = geometry.plane_width as usize;
    let plane_height = geometry.plane_height as usize;

    if plane_width > dst_stride {
        return Err(PlaneAssembleError::DstStrideTooNarrow {
            plane_width: geometry.plane_width,
            dst_stride,
        });
    }

    let dst_required = if plane_height == 0 || plane_width == 0 {
        0
    } else {
        (plane_height - 1) * dst_stride + plane_width
    };
    if dst.len() < dst_required {
        return Err(PlaneAssembleError::DstBufferTooShort {
            required: dst_required,
            supplied: dst.len(),
        });
    }

    let mut x0 = 0usize;
    let mut written = 0usize;
    for (strip_index, strip_width) in geometry.iter_strip_widths().enumerate() {
        if strip_width as usize > FRAME_OUTPUT_SRC_ROW_STRIDE {
            return Err(PlaneAssembleError::StripWidthExceedsRowStride {
                strip_index,
                strip_width,
            });
        }
        let required = strip_min_buffer_bytes(strip_width, geometry.plane_height);
        let strip = strips[strip_index];
        if strip.len() < required {
            return Err(PlaneAssembleError::StripBufferTooShort {
                strip_index,
                required,
                supplied: strip.len(),
            });
        }

        let w = strip_width as usize;
        if w == 0 {
            continue;
        }
        for row in 0..plane_height {
            let src_row = &strip[row * FRAME_OUTPUT_SRC_ROW_STRIDE..][..w];
            let dst_row = &mut dst[row * dst_stride + x0..][..w];
            for (d, s) in dst_row.iter_mut().zip(src_row) {
                *d = upshift_7bit_to_8bit(*s);
            }
        }
        x0 += w;
        written += w * plane_height;
    }

    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indeo3::{EDGE_MARKER_BIT, PLANE_ITERATION_ORDER};

    // ---- §4.3 upshift ------------------------------------------------

    #[test]
    fn upshift_doubles_in_range_values() {
        // §4.3: `shl byte, 1` on the 7-bit content doubles it.
        assert_eq!(upshift_7bit_to_8bit(0x00), 0x00);
        assert_eq!(upshift_7bit_to_8bit(0x01), 0x02);
        assert_eq!(upshift_7bit_to_8bit(0x3f), 0x7e);
        assert_eq!(upshift_7bit_to_8bit(0x7f), 0xfe);
    }

    #[test]
    fn upshift_clears_edge_marker() {
        // §4.4: "the marker bit is cleared by the per-plane upshift
        // (since `shl byte, 1` discards bit 7)".
        assert_eq!(upshift_7bit_to_8bit(EDGE_MARKER_BIT), 0x00);
        assert_eq!(upshift_7bit_to_8bit(EDGE_MARKER_BIT | 0x05), 0x0a);
        for b in 0..=u8::MAX {
            assert_eq!(
                upshift_7bit_to_8bit(b),
                upshift_7bit_to_8bit(b & !EDGE_MARKER_BIT),
                "marker-set and marker-clear bytes must upshift identically (b = {b:#04x})"
            );
            assert_eq!(upshift_7bit_to_8bit(b), (b & 0x7f) << 1);
        }
    }

    #[test]
    fn upshift_output_is_always_even() {
        // §4.3: a 1-bit left shift leaves bit 0 clear on every
        // output byte.
        for b in 0..=u8::MAX {
            assert_eq!(upshift_7bit_to_8bit(b) & 0x01, 0);
        }
    }

    // ---- §5.3 / §5.6 dispatch constants -------------------------------

    #[test]
    fn if09_fourcc_spells_if09() {
        assert_eq!(IF09_FOURCC, 0x3930_4649);
        assert_eq!(IF09_FOURCC.to_le_bytes(), *b"IF09");
    }

    #[test]
    fn if09_dispatch_rvas_match_spec() {
        // §5.6: FOURCC referenced at 0x10004576; §5.3 table: the
        // passthrough conversion function enters at 0x1000a53c.
        assert_eq!(IF09_FOURCC_CASE_RVA, 0x1000_4576);
        assert_eq!(IF09_PASSTHROUGH_RVA, 0x1000_a53c);
        // The conversion function lies after the dispatch that
        // selects it in code memory. (`black_box` defeats clippy's
        // constant-folding lint, mirroring mc_bounds.)
        assert!(core::hint::black_box(IF09_FOURCC_CASE_RVA) < IF09_PASSTHROUGH_RVA);
    }

    #[test]
    fn output_plane_order_is_y_v_u() {
        // §5.6 step 2: "Plane order in the output is Y first, then
        // V, then U."
        assert_eq!(
            OUTPUT_PLANE_ORDER,
            [
                crate::indeo3::PLANE_IDX_Y,
                crate::indeo3::PLANE_IDX_V,
                crate::indeo3::PLANE_IDX_U
            ]
        );
    }

    #[test]
    fn output_plane_order_reverses_decode_order() {
        // §5.2 decodes U → V → Y; §5.6 emits Y → V → U.
        let mut reversed = PLANE_ITERATION_ORDER;
        reversed.reverse();
        assert_eq!(OUTPUT_PLANE_ORDER, reversed);
    }

    #[test]
    fn src_row_stride_matches_pipeline() {
        assert_eq!(FRAME_OUTPUT_SRC_ROW_STRIDE, 0xb0);
        assert_eq!(FRAME_OUTPUT_SRC_ROW_STRIDE, MC_ARENA_ROW_STRIDE);
        assert_eq!(
            FRAME_OUTPUT_SRC_ROW_STRIDE,
            crate::indeo3::STRIP_EDGE_ROW_STRIDE
        );
    }

    // ---- strip_min_buffer_bytes ---------------------------------------

    #[test]
    fn strip_min_buffer_bytes_formula() {
        // Last row needs only its visible pixels.
        assert_eq!(strip_min_buffer_bytes(160, 8), 7 * 0xb0 + 160);
        assert_eq!(strip_min_buffer_bytes(40, 4), 3 * 0xb0 + 40);
        assert_eq!(strip_min_buffer_bytes(160, 1), 160);
        assert_eq!(strip_min_buffer_bytes(160, 0), 0);
        assert_eq!(strip_min_buffer_bytes(0, 8), 0);
    }

    // ---- §5.7 assembly: happy paths -----------------------------------

    /// Build a strip buffer of `rows` rows whose visible bytes are
    /// `base + row` (constant per row), with stride padding filled
    /// with `0x55`.
    fn make_strip(width: usize, rows: usize, base: u8) -> Vec<u8> {
        let mut buf = vec![0x55u8; strip_min_buffer_bytes(width as u32, rows as u32)];
        for r in 0..rows {
            for c in 0..width {
                buf[r * FRAME_OUTPUT_SRC_ROW_STRIDE + c] = base + r as u8;
            }
        }
        buf
    }

    #[test]
    fn single_strip_luma_assembly() {
        // 160×4 luma plane: one strip, tight output stride.
        let g = StripGeometry::for_luma(160, 4);
        assert_eq!(g.strip_count, 1);
        let s0 = make_strip(160, 4, 0x10);
        let mut dst = vec![0u8; 160 * 4];
        let written = assemble_plane_if09(&g, &[&s0], &mut dst, 160).unwrap();
        assert_eq!(written, 160 * 4);
        for r in 0..4 {
            for c in 0..160 {
                assert_eq!(dst[r * 160 + c], (0x10 + r as u8) << 1);
            }
        }
    }

    #[test]
    fn two_strip_luma_assembly_concatenates_left_to_right() {
        // §5.7: "For a 320-pixel-wide frame, there are exactly 2
        // strips per plane; the conversion loop processes them in
        // left-to-right order."
        let g = StripGeometry::for_luma(320, 2);
        assert_eq!(g.strip_count, 2);
        let s0 = make_strip(160, 2, 0x10);
        let s1 = make_strip(160, 2, 0x20);
        let mut dst = vec![0u8; 320 * 2];
        let written = assemble_plane_if09(&g, &[&s0, &s1], &mut dst, 320).unwrap();
        assert_eq!(written, 320 * 2);
        for r in 0..2 {
            for c in 0..160 {
                assert_eq!(dst[r * 320 + c], (0x10 + r as u8) << 1, "strip 0 r{r} c{c}");
                assert_eq!(
                    dst[r * 320 + 160 + c],
                    (0x20 + r as u8) << 1,
                    "strip 1 r{r} c{c}"
                );
            }
        }
    }

    #[test]
    fn remainder_last_strip_assembly() {
        // 176-wide luma: strips of width [160, 16] per the spec/02
        // §4.1 remainder formula.
        let g = StripGeometry::for_luma(176, 2);
        assert_eq!(g.strip_count, 2);
        assert_eq!(g.last_strip_width, 16);
        let s0 = make_strip(160, 2, 0x01);
        let s1 = make_strip(16, 2, 0x30);
        let mut dst = vec![0xEEu8; 176 * 2];
        let written = assemble_plane_if09(&g, &[&s0, &s1], &mut dst, 176).unwrap();
        assert_eq!(written, 176 * 2);
        assert_eq!(dst[159], 0x01 << 1);
        assert_eq!(dst[160], 0x30 << 1);
        assert_eq!(dst[175], 0x30 << 1);
        assert_eq!(dst[176], (0x01 + 1) << 1); // row 1, strip 0
    }

    #[test]
    fn chroma_plane_assembly() {
        // 80-wide chroma plane: two 40-wide strips (§5.7 strip
        // widths are 40 px for chroma).
        let g = StripGeometry::for_chroma(80, 3);
        assert_eq!(g.strip_count, 2);
        assert_eq!(g.strip_width, 40);
        let s0 = make_strip(40, 3, 0x05);
        let s1 = make_strip(40, 3, 0x45);
        let mut dst = vec![0u8; 80 * 3];
        let written = assemble_plane_if09(&g, &[&s0, &s1], &mut dst, 80).unwrap();
        assert_eq!(written, 80 * 3);
        // 0x45 has no bit 7; doubles to 0x8a.
        assert_eq!(dst[40], 0x8a);
        assert_eq!(dst[2 * 80 + 79], ((0x45u8 + 2) << 1));
    }

    #[test]
    fn wide_dst_stride_leaves_padding_untouched() {
        // Output rows at a 0xb0 stride: bytes past the plane width
        // must keep their pre-fill.
        let g = StripGeometry::for_luma(160, 2);
        let s0 = make_strip(160, 2, 0x08);
        let mut dst = vec![0xEEu8; 0xb0 * 2];
        assemble_plane_if09(&g, &[&s0], &mut dst, 0xb0).unwrap();
        for r in 0..2 {
            assert_eq!(dst[r * 0xb0], (0x08 + r as u8) << 1);
            for c in 160..0xb0 {
                assert_eq!(dst[r * 0xb0 + c], 0xEE, "padding touched at r{r} c{c}");
            }
        }
        // Trailing slack beyond the last row's visible pixels also
        // untouched.
        assert_eq!(dst[0xb0 + 160], 0xEE);
    }

    #[test]
    fn edge_marker_bytes_assemble_with_marker_dropped() {
        // §4.4: marker-bearing bytes reach the output as their
        // 7-bit content, doubled.
        let g = StripGeometry::for_luma(160, 1);
        let mut s0 = make_strip(160, 1, 0x00);
        s0[3] = EDGE_MARKER_BIT | 0x21;
        let mut dst = vec![0u8; 160];
        assemble_plane_if09(&g, &[&s0], &mut dst, 160).unwrap();
        assert_eq!(dst[3], 0x21 << 1);
    }

    #[test]
    fn zero_height_plane_writes_nothing() {
        let g = StripGeometry::for_luma(320, 0);
        assert_eq!(g.strip_count, 2);
        let empty: &[u8] = &[];
        let mut dst = [0xAAu8; 4];
        let written = assemble_plane_if09(&g, &[empty, empty], &mut dst, 320).unwrap();
        assert_eq!(written, 0);
        assert_eq!(dst, [0xAA; 4]);
    }

    #[test]
    fn zero_width_plane_writes_nothing() {
        let g = StripGeometry::for_luma(0, 8);
        assert_eq!(g.strip_count, 0);
        let mut dst = [0xAAu8; 4];
        let written = assemble_plane_if09(&g, &[], &mut dst, 0).unwrap();
        assert_eq!(written, 0);
        assert_eq!(dst, [0xAA; 4]);
    }

    // ---- §5.7 assembly: error paths -----------------------------------

    #[test]
    fn strip_count_mismatch_rejected() {
        let g = StripGeometry::for_luma(320, 2);
        let s0 = make_strip(160, 2, 0);
        let mut dst = vec![0u8; 320 * 2];
        let err = assemble_plane_if09(&g, &[&s0], &mut dst, 320).unwrap_err();
        assert_eq!(
            err,
            PlaneAssembleError::StripCountMismatch {
                expected: 2,
                supplied: 1
            }
        );
        assert!(err.to_string().contains("spec/07 §5.7"));
    }

    #[test]
    fn strip_width_sum_mismatch_rejected() {
        // Hand-built geometry whose widths tile 320 + 16 ≠ 400.
        let g = StripGeometry {
            role: crate::indeo3::PlaneRole::Luma,
            plane_width: 400,
            plane_height: 2,
            strip_width: 160,
            strip_count: 2,
            last_strip_width: 16,
        };
        let s = make_strip(160, 2, 0);
        let mut dst = vec![0u8; 400 * 2];
        let err = assemble_plane_if09(&g, &[&s, &s], &mut dst, 400).unwrap_err();
        assert_eq!(
            err,
            PlaneAssembleError::StripWidthSumMismatch {
                plane_width: 400,
                widths_total: 176
            }
        );
    }

    #[test]
    fn strip_width_exceeding_row_stride_rejected() {
        // Hand-built geometry with an impossible 0xb1-wide strip.
        let g = StripGeometry {
            role: crate::indeo3::PlaneRole::Luma,
            plane_width: 0xb1,
            plane_height: 1,
            strip_width: 0xb1,
            strip_count: 1,
            last_strip_width: 0xb1,
        };
        let s = vec![0u8; 0x200];
        let mut dst = vec![0u8; 0xb1];
        let err = assemble_plane_if09(&g, &[&s], &mut dst, 0xb1).unwrap_err();
        assert_eq!(
            err,
            PlaneAssembleError::StripWidthExceedsRowStride {
                strip_index: 0,
                strip_width: 0xb1
            }
        );
    }

    #[test]
    fn short_strip_buffer_rejected() {
        let g = StripGeometry::for_luma(160, 4);
        let s0 = vec![0u8; strip_min_buffer_bytes(160, 4) - 1];
        let mut dst = vec![0u8; 160 * 4];
        let err = assemble_plane_if09(&g, &[&s0], &mut dst, 160).unwrap_err();
        assert_eq!(
            err,
            PlaneAssembleError::StripBufferTooShort {
                strip_index: 0,
                required: strip_min_buffer_bytes(160, 4),
                supplied: strip_min_buffer_bytes(160, 4) - 1
            }
        );
    }

    #[test]
    fn narrow_dst_stride_rejected() {
        let g = StripGeometry::for_luma(320, 2);
        let s = make_strip(160, 2, 0);
        let mut dst = vec![0u8; 320 * 2];
        let err = assemble_plane_if09(&g, &[&s, &s], &mut dst, 319).unwrap_err();
        assert_eq!(
            err,
            PlaneAssembleError::DstStrideTooNarrow {
                plane_width: 320,
                dst_stride: 319
            }
        );
    }

    #[test]
    fn short_dst_buffer_rejected() {
        let g = StripGeometry::for_luma(160, 4);
        let s = make_strip(160, 4, 0);
        // Required: 3 * 160 + 160 = 640; supply one byte less.
        let mut dst = vec![0u8; 160 * 4 - 1];
        let err = assemble_plane_if09(&g, &[&s], &mut dst, 160).unwrap_err();
        assert_eq!(
            err,
            PlaneAssembleError::DstBufferTooShort {
                required: 160 * 4,
                supplied: 160 * 4 - 1
            }
        );
    }

    #[test]
    fn error_display_cites_spec_sections() {
        let errs: [PlaneAssembleError; 6] = [
            PlaneAssembleError::StripCountMismatch {
                expected: 2,
                supplied: 1,
            },
            PlaneAssembleError::StripWidthSumMismatch {
                plane_width: 400,
                widths_total: 176,
            },
            PlaneAssembleError::StripWidthExceedsRowStride {
                strip_index: 0,
                strip_width: 0xb1,
            },
            PlaneAssembleError::StripBufferTooShort {
                strip_index: 0,
                required: 10,
                supplied: 9,
            },
            PlaneAssembleError::DstStrideTooNarrow {
                plane_width: 320,
                dst_stride: 319,
            },
            PlaneAssembleError::DstBufferTooShort {
                required: 640,
                supplied: 639,
            },
        ];
        for e in errs {
            assert!(e.to_string().contains("spec/07"), "{e}");
        }
    }
}
