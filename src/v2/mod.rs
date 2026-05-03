//! Indeo 2 (RT21 / IV20) decoder.
//!
//! Driven end-to-end by
//! `docs/video/indeo/indeo2/indeo2-trace-reverse-engineering.md`:
//! [`header`] parses §3.3, [`tables`] holds the §8 numerical data
//! (143-entry codebook + four 256-byte delta tables), [`huffman`]
//! builds the canonical 14-bit lookup over those entries, [`plane`]
//! runs the §3.6 / §3.7 pair / run pixel decode, and [`decoder`]
//! glues it all together behind the [`oxideav_core::Decoder`] trait.

pub mod decoder;
pub mod header;
pub mod huffman;
pub mod plane;
pub mod tables;

pub use decoder::{make_decoder, Indeo2Decoder};
pub use header::{FrameHeader, FrameType, FRAME_HEADER_BYTES, MAGIC_RF, VERSION_CONST};
pub use huffman::{HuffSymbol, HuffTable};
pub use tables::{DELTA_TABLES, MAX_CODE_LEN, VLC_TABLE};
