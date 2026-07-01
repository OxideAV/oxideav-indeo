//! Indeo 5 output reconstruction — signed-coefficient → 8-bit plane.
//!
//! Spec source: `docs/video/indeo/indeo5/spec/08-output-reconstruction.md`.
//!
//! After the per-band wavelet recomposition ([`crate::indeo5::wavelet`],
//! `spec/06 §3`) each plane exists as a **signed 16-bit per-pixel
//! reconstruction buffer** in the codec's internal representation
//! (`spec/08 §0`, the `[ebx+0x130]` per-plane arena). This module
//! implements the first output-stage step: the per-plane
//! signed-to-unsigned conversion the eight per-plane writer kernels
//! share (`spec/08 §3.3`).
//!
//! The per-pixel arithmetic is the **bias-and-clamp** of `spec/08 §3.3`:
//!
//! ```text
//! output_byte = ((signed_coeff + 0x200) >> 2) & 0xff
//! ```
//!
//! The `+0x200` (+512) is the signed→unsigned bias that recentres the
//! codec's internal `[-512, +511]` pixel range into a 10-bit unsigned
//! value; the `>> 2` is the 10-bit→8-bit downshift. There is **no
//! explicit saturation** (`spec/08 §3.3`): values outside the expected
//! range wrap into the byte — the encoder is required to keep
//! coefficients in range (the same encoder-discipline mechanism Indeo 3
//! uses per `../indeo3/spec/07 §4`). The `& 0xff` truncation is the
//! only range operation applied.
//!
//! ## Per-plane stride (`spec/08 §1.1`)
//!
//! Each per-plane reconstruction buffer is allocated with a row stride
//! of `(plane_width + 0x1f) & ~0x1f` — the width rounded up to a
//! multiple of 32 (the same `0x1f`-padded convention as the per-band
//! buffer, `spec/06 §4.2`). The padding gives the MMX writer kernels a
//! 32-pixel right/bottom margin so their 2-pixel-at-a-time reads never
//! run past the buffer (`spec/08 §4.4`). This module carries the stride
//! explicitly and drops the padding when producing a tightly-packed
//! output plane.

/// `spec/08 §1.1` — the per-plane reconstruction-buffer row stride is
/// the plane width padded up to this alignment (`0x20` = 32).
pub const PLANE_STRIDE_ALIGN: u32 = 0x20;

/// `spec/08 §3.3` — the signed→unsigned recentre bias (`+512`).
pub const OUTPUT_BIAS: i32 = 0x200;

/// `spec/08 §3.3` — the 10-bit→8-bit downshift applied after the bias.
pub const OUTPUT_SHIFT: u32 = 2;

/// `spec/08 §1.1` — pad a plane width up to the 32-byte reconstruction
/// stride (`(width + 0x1f) & ~0x1f`).
#[inline]
pub fn plane_stride(width: u32) -> u32 {
    (width + (PLANE_STRIDE_ALIGN - 1)) & !(PLANE_STRIDE_ALIGN - 1)
}

/// `spec/08 §3.3` — the per-pixel bias-and-clamp: convert one signed
/// internal reconstruction coefficient to an 8-bit output byte via
/// `((coeff + 0x200) >> 2) & 0xff`.
///
/// The shift is a logical right shift; with the `+0x200` bias applied
/// first, an in-range coefficient (`[-512, +511]`) is non-negative
/// before the shift so the low-8-bit truncation matches the binary's
/// `add`/`shr`/byte-store sequence exactly.
#[inline]
pub fn bias_and_clamp(coeff: i32) -> u8 {
    (((coeff + OUTPUT_BIAS) >> OUTPUT_SHIFT) & 0xff) as u8
}

/// Errors the output-plane conversion can raise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputError {
    /// The reconstruction buffer's length does not match
    /// `stride * height` (`spec/08 §1.1`). A programming error in the
    /// caller that supplied a mis-sized buffer.
    BufferLengthMismatch {
        /// The buffer length supplied.
        got: usize,
        /// The `stride * height` length expected.
        expected: usize,
    },
    /// The visible width exceeds the buffer stride (`spec/08 §1.1`
    /// requires `width <= stride`). A malformed plane geometry.
    WidthExceedsStride {
        /// Visible plane width.
        width: u32,
        /// Buffer row stride.
        stride: u32,
    },
}

impl core::fmt::Display for OutputError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            OutputError::BufferLengthMismatch { got, expected } => write!(
                f,
                "indeo5 output: reconstruction buffer length {got} != stride*height {expected} (spec/08 §1.1)"
            ),
            OutputError::WidthExceedsStride { width, stride } => write!(
                f,
                "indeo5 output: plane width {width} exceeds buffer stride {stride} (spec/08 §1.1)"
            ),
        }
    }
}

impl std::error::Error for OutputError {}

/// A signed 16-bit per-pixel reconstruction plane (`spec/08 §0`/§1.1)
/// — the internal representation the wavelet recomposition produces,
/// prior to the output-format byte conversion.
///
/// Coefficients are held as `i32` for arithmetic headroom; the values
/// occupy the signed 16-bit range the codec's internal pixel space uses
/// (`spec/06 §1.4`). The buffer is `stride * height` samples with
/// `stride = plane_stride(width) >= width`; columns `width..stride` are
/// the right-edge padding (`spec/08 §4.4`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconstructionPlane {
    /// Visible plane width in samples.
    pub width: u32,
    /// Visible plane height in samples.
    pub height: u32,
    /// Row stride in samples (`>= width`, `spec/08 §1.1`).
    pub stride: u32,
    /// Row-major signed samples (`stride * height` entries).
    pub data: Vec<i32>,
}

impl ReconstructionPlane {
    /// Construct a reconstruction plane with the standard
    /// `plane_stride(width)` row stride and a zero-filled buffer
    /// (`spec/08 §1.1`).
    pub fn zeroed(width: u32, height: u32) -> Self {
        let stride = plane_stride(width);
        ReconstructionPlane {
            width,
            height,
            stride,
            data: vec![0; (stride * height) as usize],
        }
    }

    /// Construct from an explicit `(width, height, stride, data)`,
    /// validating the buffer length and the `width <= stride`
    /// invariant (`spec/08 §1.1`).
    pub fn new(width: u32, height: u32, stride: u32, data: Vec<i32>) -> Result<Self, OutputError> {
        if width > stride {
            return Err(OutputError::WidthExceedsStride { width, stride });
        }
        let expected = (stride * height) as usize;
        if data.len() != expected {
            return Err(OutputError::BufferLengthMismatch {
                got: data.len(),
                expected,
            });
        }
        Ok(ReconstructionPlane {
            width,
            height,
            stride,
            data,
        })
    }

    /// Sample at `(x, y)` in the padded buffer (row-major over the
    /// stride). `x` may address the right-edge padding (`x < stride`).
    #[inline]
    pub fn at(&self, x: u32, y: u32) -> i32 {
        self.data[(y * self.stride + x) as usize]
    }

    /// `spec/08 §3.3` — convert this plane to a tightly-packed 8-bit
    /// output plane by applying the per-pixel bias-and-clamp to every
    /// visible sample and dropping the right-edge stride padding.
    ///
    /// The result is `width * height` bytes in row-major order.
    pub fn to_output_plane(&self) -> OutputPlane {
        let w = self.width as usize;
        let h = self.height as usize;
        let stride = self.stride as usize;
        let mut pixels = Vec::with_capacity(w * h);
        for row in 0..h {
            let base = row * stride;
            for col in 0..w {
                pixels.push(bias_and_clamp(self.data[base + col]));
            }
        }
        OutputPlane {
            width: self.width,
            height: self.height,
            pixels,
        }
    }
}

/// A tightly-packed 8-bit output plane (`width * height` bytes,
/// row-major) — the post-bias-and-clamp result of one reconstruction
/// plane (`spec/08 §3.3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputPlane {
    /// Plane width in pixels.
    pub width: u32,
    /// Plane height in pixels.
    pub height: u32,
    /// Row-major 8-bit pixels (`width * height` bytes).
    pub pixels: Vec<u8>,
}

impl OutputPlane {
    /// Pixel at `(x, y)` (row-major, no padding).
    #[inline]
    pub fn at(&self, x: u32, y: u32) -> u8 {
        self.pixels[(y * self.width + x) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_stride_pads_to_32() {
        // spec/08 §1.1: (width + 0x1f) & ~0x1f.
        assert_eq!(plane_stride(1), 32);
        assert_eq!(plane_stride(32), 32);
        assert_eq!(plane_stride(33), 64);
        assert_eq!(plane_stride(64), 64);
        assert_eq!(plane_stride(176), 192); // QCIF luma width
        assert_eq!(plane_stride(352), 352); // CIF luma width (already aligned)
    }

    #[test]
    fn bias_and_clamp_midrange() {
        // spec/08 §3.3: (coeff + 0x200) >> 2 & 0xff.
        // coeff 0 -> (512 >> 2) = 128 (mid grey).
        assert_eq!(bias_and_clamp(0), 128);
        // coeff -512 (lower bound) -> (0 >> 2) = 0.
        assert_eq!(bias_and_clamp(-512), 0);
        // coeff +508 -> (1020 >> 2) = 255.
        assert_eq!(bias_and_clamp(508), 255);
        // coeff +4 -> (516 >> 2) = 129.
        assert_eq!(bias_and_clamp(4), 129);
    }

    #[test]
    fn bias_and_clamp_wraps_out_of_range() {
        // No explicit saturation (spec/08 §3.3): +512 -> (1024>>2)=256
        // truncates to 0.
        assert_eq!(bias_and_clamp(512), 0);
        // A large positive coeff wraps through the &0xff.
        assert_eq!(bias_and_clamp(516), 1);
    }

    #[test]
    fn zeroed_plane_geometry() {
        let p = ReconstructionPlane::zeroed(176, 144);
        assert_eq!(p.stride, 192);
        assert_eq!(p.data.len(), 192 * 144);
        assert_eq!(p.width, 176);
        assert_eq!(p.height, 144);
    }

    #[test]
    fn new_rejects_bad_length() {
        let e = ReconstructionPlane::new(4, 2, 32, vec![0; 10]).unwrap_err();
        assert_eq!(
            e,
            OutputError::BufferLengthMismatch {
                got: 10,
                expected: 64
            }
        );
    }

    #[test]
    fn new_rejects_width_over_stride() {
        let e = ReconstructionPlane::new(40, 2, 32, vec![0; 64]).unwrap_err();
        assert_eq!(
            e,
            OutputError::WidthExceedsStride {
                width: 40,
                stride: 32
            }
        );
    }

    #[test]
    fn convert_drops_padding_and_biases() {
        // 3x2 visible, stride 8. Fill visible with coeff 0 (-> 128),
        // padding with garbage that must be dropped.
        let mut data = vec![9999i32; 8 * 2];
        for row in 0..2 {
            for col in 0..3 {
                data[row * 8 + col] = 0;
            }
        }
        let p = ReconstructionPlane::new(3, 2, 8, data).unwrap();
        let out = p.to_output_plane();
        assert_eq!(out.width, 3);
        assert_eq!(out.height, 2);
        assert_eq!(out.pixels, vec![128; 6]);
    }

    #[test]
    fn output_plane_accessor() {
        let p = ReconstructionPlane::new(2, 2, 32, {
            let mut d = vec![0i32; 64];
            d[0] = -512; // (0,0) -> 0
            d[1] = 508; // (1,0) -> 255
            d[32] = 0; // (0,1) -> 128
            d[33] = 4; // (1,1) -> 129
            d
        })
        .unwrap();
        let out = p.to_output_plane();
        assert_eq!(out.at(0, 0), 0);
        assert_eq!(out.at(1, 0), 255);
        assert_eq!(out.at(0, 1), 128);
        assert_eq!(out.at(1, 1), 129);
    }
}
