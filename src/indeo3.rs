//! Indeo 3 (IV31 / IV32) — structural decoders.
//!
//! Round 1 lands `FrameHeader::parse` (`spec/01`). Round 2 adds
//! `PictureLayer::parse` (`spec/02`). Round 3 adds
//! `decode_plane_tree` (`spec/03`), the binary-tree walk over a
//! plane's bitstream payload that produces a typed [`CellTree`] of
//! INTRA / INTER leaf cells (the INTRA cells carry their VQ
//! sub-tree leaves inline). Round 4 adds the VQ codebook
//! materialisation (`spec/04`): the static dyad-mode delta table
//! ([`DyadDeltaTable`]), the packed-codebook-DWORD decode
//! ([`CodebookEntry`]), the static codebook seed-dispatch table
//! ([`seed_dispatch_entries`]), the per-frame arena + `alt_quant[]`
//! overlay ([`VqArena`]), and the VQ_NULL runtime sub-codes
//! ([`VqNullRuntime`]).
//!
//! All offsets, field widths, validation rules, and sentinel
//! values are taken from the per-chapter spec under
//! `docs/video/indeo/indeo3/spec/`. Section references in
//! doc-comments below cite the chapter named in each module.

mod header;
mod macroblock;
mod picture_layer;
mod vq;

pub use header::{
    alt_quant_indices, BitstreamHeader, FrameFlags, FrameHeader, FrameHeaderPreamble, HeaderError,
    BITSTREAM_HEADER_LEN, COMBINED_HEADER_LEN, FLAG_YVU9_8BIT, FRAME_HEADER_LEN, MAGIC_FRMH,
    MAX_HEIGHT, MAX_WIDTH, MIN_DIMENSION, NULL_FRAME_DATA_SIZE_BITS, REQUIRED_DEC_VERSION,
};
pub use macroblock::{
    decode_plane_tree, Cell, CellTree, MacroblockError, NodeCode, VqCell, VqLeaf, VqNull,
    CHROMA_STRIP_WIDTH, LUMA_STRIP_WIDTH,
};
pub use picture_layer::{
    MotionVector, PictureLayer, PictureLayerError, PlanePrelude, PlanePresence,
    MC_VECTOR_ENTRY_LEN, MIN_PRELUDE_LEN, NUM_VECTORS_FIELD_LEN, PLANE_COUNT, PLANE_IDX_U,
    PLANE_IDX_V, PLANE_IDX_Y,
};
pub use vq::{
    seed_dispatch_entries, CellVariant, CodebookEntry, DyadDeltaTable, SeedEntry, VqArena, VqError,
    VqNullRuntime, ARENA_BANDS_OFFSET, ARENA_BAND_COUNT, ARENA_BAND_LEN, ARENA_HALF_LEN, ARENA_LEN,
    DYAD_BANK15_VALID_ROWS, DYAD_BANK_COUNT, DYAD_BANK_STRIDE, DYAD_TABLE_LEN, PRIMARY_STRIDE,
    SECONDARY_STRIDE, SEED_PAIR_COUNT, SEED_TABLE_LEN,
};
