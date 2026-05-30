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
//! cell's rows. Round 10 adds the per-cell sub-array wiring
//! (`spec/03` §5.1 / §5.3 / §5.5) — the cell-stack at
//! `[strip_slot + 0x40+]`: [`cell_stack_slot_offset`] /
//! [`cell_stack_array_offset`] enforce the §5 240-entry bound,
//! [`CellStackReadSite`] enumerates the three §5.3 read sites
//! within `IR32_32.DLL!0x10006538`, and [`CellStackTopDispatch`]
//! classifies the destination-slot stack-top load into the §5.4
//! strip-edge vs §5.5 inter-cell branch (with §5.5's
//! [`PER_CELL_EDGE_PREV_BR_OFFSET`] / [`PER_CELL_EDGE_PREV_BR_NEXT_OFFSET`]
//! / [`PER_CELL_EDGE_ROW_STRIDE`] / [`PER_CELL_EDGE_HEIGHT_STEP`]
//! constants surfaced). Round 11 adds the spec/03 §5.4 end-of-strip
//! edge fix-up parameter surface — [`StripEdgeFixupDims::for_slot`]
//! resolves the per-plane-role `sar 2` chroma divide, and
//! [`StripEdgeRowIter`] yields the per-row read/write byte-offsets
//! ([`STRIP_EDGE_BYTE_READ_OFFSET`] / [`STRIP_EDGE_BYTE_WRITE_OFFSET`])
//! the rightmost-column duplication walks. Round 12 adds the spec/05
//! §1 per-plane packed-MV table layout: [`MV_TABLE_BASE_OFFSET`] /
//! [`MV_TABLE_ENTRY_SIZE`] / [`MV_TABLE_BYTES`] /
//! [`MV_TABLE_MAX_BYTE_INDEXABLE_ENTRIES`] fix the §1.2 arena geometry,
//! [`MvTableParserArm::from_frame_flags`] resolves the §1.2 four-way
//! parser-arm dispatch on `frame_flags` bits 4 + 5 with the four
//! write-site RVAs surfaced, [`mv_table_entry_byte_offset`] /
//! [`MvIndexFetch::for_index`] model the §1.3
//! `xor eax,eax; mov al,[ebp]; shl eax,0x2; add eax,inner_instance`
//! INTER-leaf sequence up to (but not including) the table dereference,
//! and [`MvIndexValidity`] classifies an MV-index byte against the
//! plane's `num_vectors` per §1.4
//! (written-this-frame / stale-tail-entry / out-of-range). Round 13
//! adds the spec/05 §2.2 / §2.3 / §3.3 / §3.4 packed-MV bit-layout
//! decode and four-way MC dispatch: [`PackedMv::from_raw`] wraps the
//! 32-bit packed-MV DWORD fetched from `inner_instance[4*i]`,
//! [`PackedMv::pixel_offset`] recovers the §2.3 / §3.4 signed
//! strip-pixel byte offset via the dispatcher's `sar edx, 0x2`
//! ([`MV_PIXEL_OFFSET_SHIFT`] = `2`), [`PackedMv::mode`] returns
//! [`McDispatchMode`] — the §2.2 four-way fork (`FullPel` /
//! `VerticalHalfPel` / `HorizontalHalfPel` / `BothHalfPel`) selected
//! by [`MV_MODE_BITS_MASK`] (`0x3`) with each variant carrying its
//! inner-loop RVA (`0x1000670d` / `0x10006780` / `0x1000684b` /
//! `0x100068f8`); [`apply_mv_source_offset`] /
//! [`PackedMv::source_address`] model the §2.3
//! `src_addr = dst_cell_base + sign_extend(packed_MV >> 2)`, and
//! [`pack_mv_components`] surfaces the §3.3 constructive packer
//! `((176*vert + horiz) << 2) | (horiz_lsb << 1) | vert_lsb`. The
//! §3.3 row-stride constant [`MV_PIXEL_OFFSET_ROW_STRIDE`] (`176`)
//! aliases [`reconstruct::PREDICTOR_ROW_STRIDE`] with a `const _`
//! cross-check. Round 14 adds the spec/05 §5.1 / §5.2 / §5.3 MC
//! cell-copy inner-loop kernel: [`MC_ROW_STRIDE`] (`0xb0`) /
//! [`MC_INNER_LOOP_DWORDS_PER_ITER`] (`4`) /
//! [`MC_INNER_LOOP_BYTES_PER_ITER`] (`16`) / [`MC_BAND_ROWS`] (`4`) /
//! [`MC_BAND_BYTE_STRIDE`] (`0x2c0`) / [`MC_COLUMN_GROUP_PIXELS`]
//! (`4`) pin the §5.1 inner-loop shape; the
//! [`MC_FULL_PEL_ROW_OFFSETS`] table mirrors the four `mov [esi +
//! 0]`, `[esi + 0xb0]`, `[esi + 0x160]`, `[esi + 0x210]` immediates
//! the full-pel kernel at `IR32_32.DLL!0x1000670d..0x1000673d`
//! emits; [`mc_full_pel_row_dword`] / [`McKernelStep::for_row`]
//! expose the same offsets through a typed surface.
//! [`McKernelGeometry::new`] enforces the §5.1 multiple-of-4
//! width / height invariants and the §5.3 row-stride bound
//! ([`MC_MAX_CELL_WIDTH_BYTES`] = `0xb0`). The §5.2 per-DWORD
//! averaging kernels — [`mc_vert_half_pel_pair`]
//! ([`MC_VERT_HALF_PEL_NEIGHBOUR_OFFSET`] = `0xb0`),
//! [`mc_horiz_half_pel_pair`] ([`MC_HORIZ_HALF_PEL_NEIGHBOUR_OFFSET`]
//! = `1`, with the in-DWORD byte splice for the `[esi]` /
//! `[esi + 1]` neighbour pair), and [`mc_both_half_pel_quad`]
//! (the §2.2 2×2 box filter, composed as horizontal-pair-first /
//! vertical-pair-second) — share the §2.2 / §5.2 byte-parallel
//! `(a + b) >> 1` SWAR identity with the output-reconstruction
//! kernel's [`reconstruct::average_7bit`], confirming the
//! "no separate filter coefficient tables" §2.2 disposition.
//!
//! All offsets, field widths, validation rules, and sentinel
//! values are taken from the per-chapter spec under
//! `docs/video/indeo/indeo3/spec/`. Section references in
//! doc-comments below cite the chapter named in each module.

mod cell_loop;
mod cell_subarray;
mod entropy;
mod header;
mod macroblock;
mod mc_kernel;
mod mc_packed;
mod mc_table;
mod picture_layer;
mod reconstruct;
mod strip_context;
mod strip_edge;
mod vq;

pub use cell_loop::{
    advance_row, dispatch_cell_preamble, iterate_column_rows, read_cell_position_dword,
    read_cl_row_counter, CellLoopPreamble, CellLoopState, CellRowAdvance, CodebookBankView,
    CELL_BANK_LEN, CELL_DATA_TABLE, CELL_POSITION_MAX, CELL_POSITION_TABLE, CH_CONTROL_LUT,
    CL_ROW_COUNTER_LUT, INTRA_CONTEXT_CLEAR_MASK, INTRA_CONTEXT_FLAG, MIRROR_TABLE_OFFSET,
    SLOT_INDEX_LUT,
};
pub use cell_subarray::{
    cell_stack_array_offset, cell_stack_slot_offset, CellStackReadSite, CellStackTopDispatch,
    CELL_STACK_BEGIN_OFFSET, CELL_STACK_ENTRY_SIZE, CELL_STACK_MAX_ENTRIES,
    PER_CELL_EDGE_HEIGHT_STEP, PER_CELL_EDGE_PREV_BR_NEXT_OFFSET, PER_CELL_EDGE_PREV_BR_OFFSET,
    PER_CELL_EDGE_ROW_STRIDE,
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
pub use mc_kernel::{
    mc_both_half_pel_quad, mc_full_pel_row_dword, mc_horiz_half_pel_pair, mc_vert_half_pel_pair,
    McKernelGeometry, McKernelGeometryError, McKernelStep, MC_BAND_BYTE_STRIDE, MC_BAND_ROWS,
    MC_BYTES_PER_DWORD, MC_COLUMN_GROUP_PIXELS, MC_FULL_PEL_ROW_OFFSETS,
    MC_HORIZ_HALF_PEL_NEIGHBOUR_OFFSET, MC_INNER_LOOP_BYTES_PER_ITER,
    MC_INNER_LOOP_DWORDS_PER_ITER, MC_MAX_CELL_WIDTH_BYTES, MC_ROW_STRIDE,
    MC_VERT_HALF_PEL_NEIGHBOUR_OFFSET,
};
pub use mc_packed::{
    apply_mv_source_offset, pack_mv_components, McDispatchMode, PackedMv, MV_HORIZ_HALFPEL_BIT,
    MV_MODE_BITS_MASK, MV_PIXEL_OFFSET_ROW_STRIDE, MV_PIXEL_OFFSET_SHIFT, MV_VERT_HALFPEL_BIT,
};
pub use mc_table::{
    mv_table_entry_byte_offset, MvIndexFetch, MvIndexValidity, MvTableParserArm, MV_HALFPEL_HORIZ,
    MV_HALFPEL_MASK, MV_HALFPEL_VERT, MV_INDEX_SCALE_SHIFT, MV_TABLE_BASE_OFFSET, MV_TABLE_BYTES,
    MV_TABLE_ENTRY_SIZE, MV_TABLE_MAX_BYTE_INDEXABLE_ENTRIES,
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
pub use strip_edge::{
    strip_edge_byte_copy_offsets, strip_edge_chroma_shift, strip_edge_row_step, StripEdgeFixupDims,
    StripEdgeRow, StripEdgeRowIter, STRIP_EDGE_BYTE_READ_OFFSET, STRIP_EDGE_BYTE_WRITE_OFFSET,
    STRIP_EDGE_CHROMA_SHIFT, STRIP_EDGE_ROW_STRIDE,
};
pub use vq::{
    seed_dispatch_entries, CellVariant, CodebookEntry, DyadDeltaTable, SeedEntry, VqArena, VqError,
    VqNullRuntime, ARENA_BANDS_OFFSET, ARENA_BAND_COUNT, ARENA_BAND_LEN, ARENA_HALF_LEN, ARENA_LEN,
    DYAD_BANK15_VALID_ROWS, DYAD_BANK_COUNT, DYAD_BANK_STRIDE, DYAD_TABLE_LEN, PRIMARY_STRIDE,
    SECONDARY_STRIDE, SEED_PAIR_COUNT, SEED_TABLE_LEN,
};
