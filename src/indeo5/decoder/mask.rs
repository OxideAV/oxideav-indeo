//! The transparency trailer that follows the last band of every frame
//! of a stream with gop_flags bit 3 (spec/02 §5, spec/08 §8.9): the
//! rectangles that bound the opaque part of the picture, and a bitmask
//! that says which pixels are opaque.
//!
//! Layout, in read order:
//!
//! | Bits | Field | Condition |
//! |------|-------|-----------|
//! | 24 | trailer size in bytes, from its first byte | frame_flags bit 0 |
//! | 3 | rectangle count n | always |
//! | 4 x 16 | n rectangles: x, y, width, height | |
//! | 1 | row delta: each mask row is XORed with the row above | always |
//! | 1 | a code descriptor follows | always |
//! | 4 + 4k | the descriptor: row count k, k row widths | the bit above |
//!
//! Then, from a byte boundary, the mask: one bit that says the mask is a
//! single value, and that value in the next bit; or else a bit that says
//! a size follows, the size (8 bits, or 0xff and 24 bits: the bytes of
//! the mask data counted from its first byte), an align, the value of
//! the first run in one bit, and run lengths coded with the descriptor's
//! code until the runs cover the picture. A run code of 0 stands for 255
//! pixels and keeps the value for the run after it; any other code is
//! that many pixels and the value flips after the run. Runs go row by
//! row over the width rounded up to a multiple of 32; the samples past
//! the width are dropped. The row delta applies to run-coded masks only:
//! the vendor driver shows a single-value mask with the bit set as one
//! value everywhere.
//!
//! spec/08 §8.9 reads the run syntax from the code and leaves it
//! unchecked; the streams settle it. Rent-A-Hero has 21 IV50 movies
//! with transparency, 4 150 frames. In every run-coded frame the runs
//! end exactly at the end of the picture and the data ends exactly at
//! the trailer size. On 1 844 of the frames (every frame of 20 movies
//! and the first 600 of the credits), the mask's transparent pixels are
//! exactly the pixels the vendor driver fills with the transparency
//! colour when it converts the frame to RGB (see
//! [`Iv50Transparency`](super::Iv50Transparency)).
//!
//! The descriptor stays in force for later frames that carry none. The
//! rectangles' union limits the part of the picture a frame rebuilds
//! (spec/08 §8.3); the mask changes no plane sample.

use super::Iv50Error;
use super::bits::Bits;
use super::huff::{Huff, read_descriptor};

/// One transparency rectangle of a frame, in luma pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Iv50Rect {
    /// Left column.
    pub x: u32,
    /// Top row.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in rows.
    pub height: u32,
}

/// Mask value of an opaque pixel.
pub(crate) const OPAQUE: u8 = 0xff;

/// The trailer's fields before the mask data.
pub(crate) struct TrailerHeader {
    pub rects: Vec<Iv50Rect>,
    pub row_delta: bool,
    pub descriptor: Option<Vec<u8>>,
}

/// Read the trailer up to the mask data, including the align before it.
pub(crate) fn read_header(r: &mut Bits, frame_flags: u32) -> TrailerHeader {
    if frame_flags & 1 != 0 {
        r.read(24);
    }
    let n = r.read(3);
    let rects = (0..n)
        .map(|_| Iv50Rect {
            x: r.read(16),
            y: r.read(16),
            width: r.read(16),
            height: r.read(16),
        })
        .collect();
    let row_delta = r.bit();
    let descriptor = r.bit().then(|| read_descriptor(r));
    r.align();
    TrailerHeader {
        rects,
        row_delta,
        descriptor,
    }
}

/// Row length of the mask in samples: the picture width rounded up to a
/// multiple of 32.
pub(crate) fn stride(width: usize) -> usize {
    (width + 31) & !31
}

/// Decode the mask data into `mask`, `stride(width)` samples per row,
/// [`OPAQUE`] or 0. `code` is the descriptor in force, if any.
pub(crate) fn read_mask(
    r: &mut Bits,
    code: Option<&Huff>,
    row_delta: bool,
    stride: usize,
    mask: &mut [u8],
) -> Result<(), Iv50Error> {
    if r.bit() {
        let v = if r.bit() { OPAQUE } else { 0 };
        mask.fill(v);
        return if r.overrun() {
            Err(Iv50Error::Truncated)
        } else {
            Ok(())
        };
    }
    if r.bit() && r.read(8) == 0xff {
        // The size of the mask data: informational.
        r.read(24);
    }
    r.align();
    let code = code.ok_or(Iv50Error::BadHeader(
        "transparency mask without a code descriptor",
    ))?;
    let mut v = if r.bit() { OPAQUE } else { 0 };
    let mut pos = 0;
    while pos < mask.len() {
        let c = code
            .decode(r)
            .ok_or(Iv50Error::BadHeader("transparency mask code"))?;
        if r.overrun() {
            return Err(Iv50Error::Truncated);
        }
        // A run that would pass the end of the mask is cut there; the
        // streams seen end exactly at it.
        let (n, flip) = if c == 0 {
            (255, false)
        } else {
            (c as usize, true)
        };
        let end = mask.len().min(pos + n);
        mask[pos..end].fill(v);
        pos = end;
        if flip {
            v ^= OPAQUE;
        }
    }
    if row_delta {
        for i in stride..mask.len() {
            mask[i] ^= mask[i - stride];
        }
    }
    Ok(())
}
