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
//! ([`halve_fefefefe`]). Round 8 adds the spec/02 §4–§7
//! picture-decomposition state ([`strip_slot_index`],
//! [`StripSlotDescriptor`], [`StripGeometry`], [`PerPlaneDecodeCall`],
//! [`PlaneDecodeStatus`], [`luma_strip_slot_count`],
//! [`chroma_strip_slot_count`], [`chroma_plane_height`]) — the
//! strip-context array layout (§5), the per-plane decode-call
//! signature (§6), the codec-init strip-count arithmetic (§7), and
//! the §4.1 / §4.2 strip-geometry formulae. Round 9 adds the
//! outer per-cell row/column loop preamble (`spec/04` §3.3):
//! [`dispatch_cell_preamble`] runs the four-step
//! `IR32_32.DLL!0x1000665e..0x10006670` sequence — pick the
//! [`CodebookBankView`] (primary vs `+0xb00` mirror) from the
//! cell-stack top, load the cell-position DWORD with the `0xf423f`
//! sanity check ([`CELL_POSITION_MAX`]), load the new `cl` row
//! counter, and clear the intra-context flag — returning a
//! [`CellLoopState`] that bridges round 4's [`CodebookEntry`] to
//! round 7's [`emit_variant`]; [`advance_row`] /
//! [`iterate_column_rows`] step the `(cl, edi)` walk across a
//! cell's rows.
//!
//! All offsets, field widths, validation rules, and sentinel
//! values are taken from the per-chapter spec under
//! `docs/video/indeo/indeo3/spec/`. Section references in
//! doc-comments below cite the chapter named in each module.

mod cell_loop;
mod entropy;
mod header;
mod macroblock;
mod picture_layer;
mod reconstruct;
mod strip_context;
mod vq;

pub use cell_loop::{
    advance_row, dispatch_cell_preamble, iterate_column_rows, read_cell_position_dword,
    read_cl_row_counter, CellLoopPreamble, CellLoopState, CellRowAdvance, CodebookBankView,
    CELL_BANK_LEN, CELL_DATA_TABLE, CELL_POSITION_MAX, CELL_POSITION_TABLE, CH_CONTROL_LUT,
    CL_ROW_COUNTER_LUT, INTRA_CONTEXT_CLEAR_MASK, INTRA_CONTEXT_FLAG, MIRROR_TABLE_OFFSET,
    SLOT_INDEX_LUT,
};
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
pub use strip_context::{
    chroma_plane_height, chroma_strip_slot_count, luma_strip_slot_count, slot_field,
    strip_slot_index, PerPlaneDecodeCall, PlaneDecodeStatus, PlaneRole, StripGeometry,
    StripSlotDescriptor, DISPATCHABLE_SLOT_COUNT, INSTANCE_CHROMA_CODEBOOK_BANK,
    INSTANCE_LUMA_CODEBOOK_BANK, INSTANCE_SECONDARY_CODEBOOK_PTR, INSTANCE_STATE_LEN,
    INSTANCE_STRIP_ARRAY_VIEW_PTR, PIXEL_BUFFER_ARENA_LEN, PLANE_DECODE_STATUS_MALFORMED,
    PLANE_DECODE_STATUS_OK, PRIMARY_BANK_SLOTS, SECONDARY_BANK_SLOTS,
    STRIP_ARRAY_OFFSET_IN_INSTANCE, STRIP_SLOT_BASE_PTR_COUNT, STRIP_SLOT_COUNT,
    STRIP_SLOT_SENTINEL, STRIP_SLOT_STRIDE,
};
pub use vq::{
    seed_dispatch_entries, CellVariant, CodebookEntry, DyadDeltaTable, SeedEntry, VqArena, VqError,
    VqNullRuntime, ARENA_BANDS_OFFSET, ARENA_BAND_COUNT, ARENA_BAND_LEN, ARENA_HALF_LEN, ARENA_LEN,
    DYAD_BANK15_VALID_ROWS, DYAD_BANK_COUNT, DYAD_BANK_STRIDE, DYAD_TABLE_LEN, PRIMARY_STRIDE,
    SECONDARY_STRIDE, SEED_PAIR_COUNT, SEED_TABLE_LEN,
};
