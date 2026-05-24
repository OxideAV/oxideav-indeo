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
//! ([`VqNullRuntime`]). Round 5 adds the byte-level entropy module
//! (`spec/06`): the per-cell mode-byte stream classifier
//! ([`ModeByte`]), the variable-byte continuation rule
//! ([`continuation_needed`]), the eight RLE escapes ([`RleEscape`])
//! with their per-position acceptance matrix
//! ([`RleEscape::accepted_at`]), and the `0xFB` counter-byte category
//! table ([`fb_category_table`]). Round 6 adds the output-
//! reconstruction kernel (`spec/07` §1 + §2 + §4): the predictor
//! address ([`predictor_offset`]) and the softSIMD dyad-pair
//! `predictor + delta` add ([`apply_dyad_pair`]) with its
//! continuation / secondary-table fall-back and 7-bit-per-byte
//! overflow detection. Round 7 adds the four cell-shape variant
//! inner-loop emission kernels ([`emit_variant`], `spec/07` §2.2):
//! variant A's direct two-row store, variant B's `0x7f7f7f7f`-clamped
//! per-byte average ([`average_7bit`]), variant C's doubled-row
//! average, and variant D's `and 0xfefefefe; shr 1` halve
//! ([`halve_fefefefe`]).
//!
//! All offsets, field widths, validation rules, and sentinel
//! values are taken from the per-chapter spec under
//! `docs/video/indeo/indeo3/spec/`. Section references in
//! doc-comments below cite the chapter named in each module.

mod entropy;
mod header;
mod macroblock;
mod picture_layer;
mod reconstruct;
mod vq;

pub use entropy::{
    apply_continuation_xor, continuation_needed, fb_category, fb_category_table, variant_entry_rva,
    DyadAddress, FbCategory, FbCounter, HighNibbleAction, JumpTable, LiteralMode, ModeByte,
    ModeByteKind, PositionClass, RleEscape, ARENA_BAND_STRIDE, CONTINUATION_XOR, LITERAL_MODE_MAX,
    PRIMARY_TABLE_DISP, RLE_ESCAPE_MIN, SECONDARY_TABLE_DISP, VARIANT_A_ENTRY, VARIANT_B_ENTRY,
    VARIANT_C_ENTRY, VARIANT_D_ENTRY,
};
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
pub use reconstruct::{
    apply_dyad_pair, average_7bit, emit_variant, halve_fefefefe, jns_taken, pack_predictor,
    predictor_offset, unpack_pixels, DyadOutcome, RowEmission, SoftSimdSum, VariantEmission,
    CLAMP_7BIT_MASK, EDGE_MARKER_BIT, HALF_SENTINEL_MASK, HALVE_CARRY_MASK, PIXEL_VALUE_MAX,
    PREDICTOR_ROW_STRIDE, TOP_OF_STRIP_PREDICTOR,
};
pub use vq::{
    seed_dispatch_entries, CellVariant, CodebookEntry, DyadDeltaTable, SeedEntry, VqArena, VqError,
    VqNullRuntime, ARENA_BANDS_OFFSET, ARENA_BAND_COUNT, ARENA_BAND_LEN, ARENA_HALF_LEN, ARENA_LEN,
    DYAD_BANK15_VALID_ROWS, DYAD_BANK_COUNT, DYAD_BANK_STRIDE, DYAD_TABLE_LEN, PRIMARY_STRIDE,
    SECONDARY_STRIDE, SEED_PAIR_COUNT, SEED_TABLE_LEN,
};
