//! Indeo 2 (RT21 / IV20) decoder.
//!
//! Driven by `docs/video/indeo/indeo2/indeo2-trace-reverse-engineering.md`.
//! See the crate-level docs for round-1 status — what ships, and what's
//! deferred to round 2.

pub mod decoder;
pub mod header;

pub use decoder::{make_decoder, Indeo2Decoder};
pub use header::{FrameHeader, FrameType, FRAME_HEADER_BYTES, MAGIC_RF, VERSION_CONST};
