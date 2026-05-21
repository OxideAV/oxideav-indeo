//! Indeo 3 (IV31 / IV32) — structural decoders.
//!
//! Round 1 lands `FrameHeader::parse`, which consumes the bytes
//! at offset 0 of an Indeo 3 codec frame and returns a typed view
//! of the 16-byte frame header plus the 48-byte bitstream header
//! that follows.
//!
//! All offsets, field widths, validation rules, and sentinel
//! values are taken from
//! `docs/video/indeo/indeo3/spec/01-file-header.md`. Section
//! references in doc-comments below cite that chapter unless
//! otherwise noted.

mod header;

pub use header::{
    alt_quant_indices, BitstreamHeader, FrameFlags, FrameHeader, FrameHeaderPreamble, HeaderError,
    BITSTREAM_HEADER_LEN, COMBINED_HEADER_LEN, FLAG_YVU9_8BIT, FRAME_HEADER_LEN, MAGIC_FRMH,
    MAX_HEIGHT, MAX_WIDTH, MIN_DIMENSION, NULL_FRAME_DATA_SIZE_BITS, REQUIRED_DEC_VERSION,
};
