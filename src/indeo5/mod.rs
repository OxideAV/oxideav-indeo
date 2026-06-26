//! Intel Indeo Video Interactive 5 (`IV50`) — clean-room decode.
//!
//! Spec source: `docs/video/indeo/indeo5/spec/` (chapters 00..08).
//!
//! Indeo 5 is a wavelet-subband codec, structurally distinct from the
//! VQ-based Indeo 3 ([`crate::indeo3`]). A coded frame is a
//! bit-packed header stack (`spec/01` picture-start triplet, `spec/02`
//! GOP / frame / band headers) followed by per-band, per-tile
//! coefficient data that an inverse Slant transform and wavelet
//! recomposition turn into pixels (`spec/05`-`spec/08`).
//!
//! This module builds the decode stack from the bottom up. Landed so
//! far:
//!
//! * [`BitReader`] — the LSB-first 32-bit-accumulator bit reader
//!   (`spec/00 §3`, `spec/01 §3.1`).
//! * [`FormatDescriptor`] — the `spec/01 §2` format-descriptor
//!   preamble (magic + dimensions validation).
//! * [`PictureStart`] — the `spec/01 §3` picture-start triplet (PSC +
//!   frame_type + frame_number + the §3.4 NULL soft-correction).
//! * [`pic_size`] — the `spec/02 §1.6` standard picture-size tables.
//!
//! The bitstream payload of a real Indeo 5 frame (GOP / frame / band
//! headers and the per-tile coefficient stream) is parsed by the
//! later modules as they land.

mod bitreader;
mod header;
pub mod pic_size;

pub use bitreader::{BitReader, BitReaderError, MAX_READ_BITS};
pub use header::{
    FormatDescriptor, FrameType, HeaderError, PictureStart, MAGIC_ALTERNATE, MAGIC_CANONICAL,
    MIN_HEIGHT, MIN_WIDTH, PICTURE_START_CODE,
};
