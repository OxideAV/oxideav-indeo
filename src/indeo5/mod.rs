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

mod band;
mod bitreader;
mod checksum;
mod chroma;
mod clip_table;
mod codebook;
mod finalise;
mod format;
mod frame;
mod gop;
mod header;
mod level_table;
mod output;
mod pack;
pub mod pic_size;
mod picture;
mod planes;
mod tile;
mod wavelet;

pub use band::{BandError, BandFlags, BandHeader, DEFAULT_RV_TAB_SEL, MAX_RV_CORR};
pub use bitreader::{BitReader, BitReaderError, MAX_READ_BITS};
pub use checksum::{
    frame_checksum_present, parse_band_checksum, parse_frame_checksum, ChecksumField,
    FRAME_CHECKSUM_FLAG,
};
pub use chroma::{upsample_chroma, ChromaSubsampling};
pub use clip_table::{
    build_clip_table, clip_lookup, CLIP_BIAS, CLIP_LOWER, CLIP_PIXEL_CENTRE, CLIP_TABLE_LEN,
    CLIP_UPPER,
};
pub use codebook::{
    Codebook, CodebookError, Codeword, HuffContext, BLOCK_HUFF_PRESETS, DEFAULT_PRESET_ID,
    MAX_ROWS, MB_HUFF_PRESETS,
};
pub use finalise::{
    frame_produces_output, is_output_written, mark_output_written, output_row_order,
    reference_rotation, DecodeReturn, ReferenceRotation, RowOrder, OUTPUT_WRITTEN_FLAG,
};
pub use format::{
    ChromaLayout, OutputFormat, BI_RGB, FOURCC_I420, FOURCC_IF09, FOURCC_IYUV, FOURCC_YUY2,
    FOURCC_YV12, FOURCC_YVU9,
};
pub use frame::{FrameError, FrameFlags, FrameHeader, GopTrailer, HuffDesc};
pub use gop::{
    BandInfo, DecompLevels, GopError, GopFlags, GopHeader, Subsampling, TransformId, Transparency,
    BLK_SIZE_TABLE, MB_SIZE_TABLE,
};
pub use header::{
    FormatDescriptor, FrameType, HeaderError, PictureStart, MAGIC_ALTERNATE, MAGIC_CANONICAL,
    MIN_HEIGHT, MIN_WIDTH, PICTURE_START_CODE,
};
pub use level_table::{build_level_table, level_value, LEVEL_TABLE_LEN};
pub use output::{
    bias_and_clamp, plane_stride, OutputError, OutputPlane, ReconstructionPlane, OUTPUT_BIAS,
    OUTPUT_SHIFT, PLANE_STRIDE_ALIGN,
};
pub use pack::{pack_planar, HostBuffer, PlanePlacement};
pub use picture::{PictureError, PictureHeader};
pub use planes::{num_bands, FramePlanes, PlaneRole, OUTPUT_ITERATION_ORDER, PLANE_RECORD_ORDER};
pub use tile::{tile_count, Tile, TileGrid};
pub use wavelet::{recompose_level, recompose_plane, synth_1d, Band, LevelBands};
