//! Indeo 5 whole-frame structural decode driver.
//!
//! Spec source: `docs/video/indeo/indeo5/spec/01`..`spec/08` — the
//! per-frame thread of `spec/02 §4.4`:
//!
//! ```text
//! picture header (spec/01 + spec/02 §1/§2)
//!   for each plane in { Y, U, V }:
//!     for each band:
//!       band header (spec/02 §3)          — empty band -> zeros
//!       for each tile in raster order:
//!         per-tile size header (spec/03 §2) — empty tile -> zeros
//!         per-MB walk (spec/03 §3/§4)       — skipped MB -> zeros
//!         per-block coefficients (spec/05)  — GATED, see below
//!     wavelet recompose (spec/06 §3)
//!   bias-and-clamp + planar pack (spec/08) -> HostBuffer
//! ```
//!
//! [`decode_intra_picture`] drives an INTRA frame end-to-end and
//! produces real pixels through [`super::assemble_frame`] for every
//! region the staged spec fully determines: empty bands, empty tiles,
//! skipped macroblocks, and coded MBs whose CBP carries no AC data
//! all reconstruct (zero coefficients → the `spec/08 §3.3` mid-grey
//! `128`). The first genuinely gated element in a tile surfaces as a
//! [`DecodeFrontier`] instead of a guess:
//!
//! * [`FrontierReason::CodedBlockData`] — a CBP requests per-block
//!   coefficients; their rv-table contents (`spec/05 §7` items 1/2/8)
//!   and the fused-transform handler enumeration (`spec/06 §6` items
//!   2/3/7) are reported docs-gaps.
//! * [`FrontierReason::CodebookRequired`] — the band's VLC fields
//!   need a preset block-Huffman codebook, which is the reported
//!   `spec/04` Kraft-anomaly docs-gap (custom descriptors build
//!   fine).
//!
//! When a frontier hits a tile with an **explicit** size the driver
//! uses the `spec/03 §2.6` skip-to-next-tile path (reading the byte
//! count as spanning the whole tile from its start — the `§2.8`
//! operational semantics; the `§2.4`-vs-`§2.8` tension is a reported
//! docs-gap). An **implicit**-size tile can only be skipped via the
//! band's `band_data_size` (`spec/02 §3.2`); with neither available
//! the parse stops and `parse_complete` reports `false` (already
//! parsed planes still reconstruct; unparsed regions stay zero).
//!
//! **Band ordering note.** The per-plane band order is taken as the
//! `spec/06 §3.1`/`§3.4` decomposition order — the innermost LL band
//! first, then each level's `(HL, LH, HH)` triple innermost to
//! outermost — matching the wiki "Wavelet bands" LL/HL/LH/HH
//! default-transform mapping cited by `spec/02 §6` item 9. The
//! per-band tile grid uses `ceil(band_dim / slice_size)` (the
//! `spec/03 §1.1` luma examples); the `spec/02 §4.1` chroma
//! tile-count formula disagrees with the `spec/03 §1.1` chroma
//! examples and is a reported docs-gap.

use super::assemble::{assemble_frame, AssembleError};
use super::band::{BandError, BandHeader};
use super::bitreader::{BitReader, BitReaderError};
use super::codebook::{Codebook, HuffContext};
use super::format::OutputFormat;
use super::gop::{BandInfo, GopHeader, Subsampling};
use super::header::FrameType;
use super::level_table::{build_level_table, LEVEL_TABLE_LEN};
use super::mb::MbGrid;
use super::mb_header::{MbContext, MbHeader, MbHeaderError, QdeltaMode};
use super::mc::{mc_add_block, BandView, McError};
use super::mv::{resolve_mv, Mv, MvPredictor, MvResolution};
use super::output::{plane_stride, OutputError, ReconstructionPlane};
use super::pack::HostBuffer;
use super::picture::{PictureError, PictureHeader};
use super::tile::TileGrid;
use super::tile_header::{TileDataSize, TileHeader};
use super::wavelet::{recompose_plane, Band, LevelBands};

/// Errors raised by the whole-frame driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Fault in the spec/01/spec/02 picture-header stack.
    Picture(PictureError),
    /// The driver currently decodes INTRA (and NULL) frames only;
    /// INTER frames need the session-carried GOP state + reference
    /// planes (`spec/07`).
    NotIntra {
        /// The frame type found.
        found: FrameType,
    },
    /// Fault in a band header.
    Band {
        /// Plane index (0 = Y, 1 = U, 2 = V, `spec/02 §4.4`).
        plane_idx: usize,
        /// Band index within the plane.
        band_idx: usize,
        /// The underlying error.
        error: BandError,
    },
    /// Fault in a per-MB header.
    MbHeader {
        /// Plane index.
        plane_idx: usize,
        /// Band index.
        band_idx: usize,
        /// Tile index in raster order.
        tile_idx: usize,
        /// The underlying error.
        error: MbHeaderError,
    },
    /// An explicit tile size (`spec/03 §2.8`) or `band_data_size`
    /// (`spec/02 §3.2`) placed the next-payload target *behind* the
    /// parser's position — the stream and its size fields disagree.
    SizeFieldBehindCursor {
        /// The parser's byte position.
        at_byte: u64,
        /// The size field's target byte.
        target_byte: u64,
    },
    /// An INTER frame's reference band buffers did not match the
    /// GOP-derived band geometry (`spec/07 §1.2` workspace contract).
    ReferenceMismatch {
        /// Plane index.
        plane_idx: usize,
        /// Band index.
        band_idx: usize,
    },
    /// A motion-compensated fetch fell outside the reference band
    /// (`spec/07 §5.4` padding contract violated by the MV).
    Mc {
        /// Plane index.
        plane_idx: usize,
        /// Band index.
        band_idx: usize,
        /// Tile index in raster order.
        tile_idx: usize,
        /// The underlying error.
        error: McError,
    },
    /// Underlying bit-reader fault.
    BitReader(BitReaderError),
    /// Reconstruction-plane geometry fault (`spec/08`).
    Output(OutputError),
    /// Output-assembly fault (`spec/08`).
    Assemble(AssembleError),
}

impl From<PictureError> for DecodeError {
    fn from(e: PictureError) -> Self {
        DecodeError::Picture(e)
    }
}
impl From<BitReaderError> for DecodeError {
    fn from(e: BitReaderError) -> Self {
        DecodeError::BitReader(e)
    }
}
impl From<OutputError> for DecodeError {
    fn from(e: OutputError) -> Self {
        DecodeError::Output(e)
    }
}
impl From<AssembleError> for DecodeError {
    fn from(e: AssembleError) -> Self {
        DecodeError::Assemble(e)
    }
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::Picture(e) => write!(f, "indeo5 decode: {e}"),
            DecodeError::NotIntra { found } => write!(
                f,
                "indeo5 decode: frame type {found:?} needs session state (INTRA-only driver)"
            ),
            DecodeError::Band {
                plane_idx,
                band_idx,
                error,
            } => write!(
                f,
                "indeo5 decode: plane {plane_idx} band {band_idx}: {error}"
            ),
            DecodeError::MbHeader {
                plane_idx,
                band_idx,
                tile_idx,
                error,
            } => write!(
                f,
                "indeo5 decode: plane {plane_idx} band {band_idx} tile {tile_idx}: {error}"
            ),
            DecodeError::SizeFieldBehindCursor {
                at_byte,
                target_byte,
            } => write!(
                f,
                "indeo5 decode: size field targets byte {target_byte} behind cursor byte {at_byte} (spec/03 §2.8)"
            ),
            DecodeError::ReferenceMismatch {
                plane_idx,
                band_idx,
            } => write!(
                f,
                "indeo5 decode: plane {plane_idx} band {band_idx}: reference band geometry mismatch (spec/07 §1.2)"
            ),
            DecodeError::Mc {
                plane_idx,
                band_idx,
                tile_idx,
                error,
            } => write!(
                f,
                "indeo5 decode: plane {plane_idx} band {band_idx} tile {tile_idx}: {error}"
            ),
            DecodeError::BitReader(e) => write!(f, "indeo5 decode: {e}"),
            DecodeError::Output(e) => write!(f, "indeo5 decode: {e}"),
            DecodeError::Assemble(e) => write!(f, "indeo5 decode: {e}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Why the structural walk stopped short of pixels at some position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontierReason {
    /// A coded block's `(run, level)` coefficient stream follows —
    /// gated on the rv-table contents (`spec/05 §7` items 1/2/8) and
    /// the fused inverse-Slant handler enumeration (`spec/06 §6`
    /// items 2/3/7).
    CodedBlockData,
    /// The band's per-MB VLC fields selected a block-Huffman preset
    /// that does not build under the standard canonical rule — the
    /// `spec/04 §1.4`/`§3.2` Kraft-anomaly docs-gap (the multi-symbol
    /// 4 KB-table assignment rule). Custom descriptors and the
    /// Kraft-valid presets (including the default, preset 7) build
    /// and decode.
    CodebookRequired,
    /// A coded tile in an inter band with the MV-inheritance flag
    /// set — gated on the `spec/07 §3.4`/`§3.5` per-band
    /// `0x3604`/`0x3664` inheritance-MV tables docs-gap.
    MvInheritance,
}

/// One gated element the driver encountered (and, where possible,
/// skipped past via an explicit size field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeFrontier {
    /// Plane index (0 = Y, 1 = U, 2 = V).
    pub plane_idx: usize,
    /// Band index within the plane.
    pub band_idx: usize,
    /// Tile index in raster order.
    pub tile_idx: usize,
    /// The gate.
    pub reason: FrontierReason,
    /// `true` when an explicit tile size / `band_data_size` allowed
    /// the driver to reposition and keep parsing; `false` when the
    /// parse stopped here (`parse_complete == false`).
    pub skipped_past: bool,
}

/// Aggregate structural counts over one decoded frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DecodeStats {
    /// Bands walked (including empty ones).
    pub bands: u32,
    /// Bands taking the `spec/02 §3.3` empty fast path.
    pub empty_bands: u32,
    /// Tiles walked.
    pub tiles: u32,
    /// Tiles with the `spec/03 §2.2` empty flag.
    pub empty_tiles: u32,
    /// Macroblocks walked.
    pub mbs: u32,
    /// Macroblocks with the `spec/03 §4.1` skip flag.
    pub mbs_skipped: u32,
    /// Coded macroblocks whose CBP carried no AC data (fully
    /// reconstructable without the gated coefficient path).
    pub mbs_coded_no_ac: u32,
}

/// One decoded INTRA frame: the parsed header stack, the assembled
/// host buffer, and the structural coverage report.
#[derive(Debug, Clone)]
pub struct DecodedPicture {
    /// The parsed picture header.
    pub header: PictureHeader,
    /// The host output format the GOP subsampling selects
    /// (`None` for a NULL frame).
    pub format: Option<OutputFormat>,
    /// The assembled planar host buffer (`None` for a NULL frame —
    /// `spec/08 §6.4` no-output path).
    pub output: Option<HostBuffer>,
    /// Every gated element encountered, in decode order.
    pub frontiers: Vec<DecodeFrontier>,
    /// Structural coverage counts.
    pub stats: DecodeStats,
    /// `false` when a frontier could not be skipped past and parsing
    /// stopped early (later bands / planes reconstruct as zeros).
    pub parse_complete: bool,
}

impl DecodedPicture {
    /// `true` when every walked structural element reconstructed to
    /// pixels without hitting a gate.
    pub fn fully_reconstructed(&self) -> bool {
        self.parse_complete && self.frontiers.is_empty()
    }
}

/// Per-plane geometry derived from the GOP header (`spec/02 §1.5`/
/// `§1.6`).
struct PlaneGeom<'g> {
    width: u32,
    height: u32,
    levels: u32,
    band_info: &'g [BandInfo],
}

/// The band dimensions for band `band_idx` of a plane with `levels`
/// decomposition levels (`spec/06 §3.1`/`§4.1`): the innermost LL is
/// scaled down by `2^levels`; the level-`l` high-frequency triple
/// (innermost `l = 0`) by `2^(levels - l)`.
fn band_dims(plane_w: u32, plane_h: u32, levels: u32, band_idx: usize) -> (u32, u32) {
    let shift = if band_idx == 0 {
        levels
    } else {
        levels - (band_idx as u32 - 1) / 3
    };
    (
        plane_w.div_ceil(1 << shift).max(1),
        plane_h.div_ceil(1 << shift).max(1),
    )
}

/// The per-band tile grid counts: `ceil(band_dim / slice_size)` per
/// axis (`spec/03 §1.1`), one tile when the GOP carries no slice size
/// (whole picture = one slice).
fn band_tile_counts(band_w: u32, band_h: u32, slice_size: Option<u32>) -> (u32, u32) {
    match slice_size {
        None => (1, 1),
        Some(s) => (band_w.div_ceil(s).max(1), band_h.div_ceil(s).max(1)),
    }
}

/// Build the band's block-Huffman codebook where the staged spec
/// allows: a custom descriptor builds directly; the preset records
/// are the reported Kraft-anomaly docs-gap and yield `None`.
fn band_codebook(header: &BandHeader) -> Option<Codebook> {
    Codebook::from_huff_desc(HuffContext::Block, header.blk_huff_desc.as_ref()).ok()
}

/// Reposition the reader at an absolute byte target (used by the
/// explicit-size tile skip and the `band_data_size` band skip).
fn skip_to_byte(r: &mut BitReader<'_>, target_byte: u64) -> Result<(), DecodeError> {
    let target_bits = target_byte * 8;
    let at = r.bits_read();
    if target_bits < at {
        return Err(DecodeError::SizeFieldBehindCursor {
            at_byte: at / 8,
            target_byte,
        });
    }
    let mut remaining = target_bits - at;
    while remaining > 0 {
        let take = remaining.min(u64::from(super::bitreader::MAX_READ_BITS));
        r.skip(take as u32)?;
        remaining -= take;
    }
    Ok(())
}

/// Walk one coded tile's macroblock grid (`spec/03 §3/§4`),
/// returning the first gate hit (if any). Zero-coefficient regions
/// (skipped MBs, CBP-without-AC blocks) reconstruct implicitly — the
/// band buffer is already zeroed.
#[allow(clippy::too_many_arguments)]
fn walk_tile_mbs(
    r: &mut BitReader<'_>,
    grid: &MbGrid,
    ctx: &MbContext,
    codebook: Option<&Codebook>,
    fallback: &Codebook,
    level_table: &[i8; LEVEL_TABLE_LEN],
    stats: &mut DecodeStats,
    at: (usize, usize, usize),
) -> Result<Option<FrontierReason>, DecodeError> {
    // The VLC fields fire for every coded MB of this tile; without a
    // buildable codebook the tile is gated before any MB bits are
    // consumed (conservative: a fully-skipped tile with a VLC-needing
    // band would also stop here, but the gate is per-band anyway).
    let vlc_needed = ctx.qdelta_mode == QdeltaMode::Explicit || ctx.explicit_mv;
    if vlc_needed && codebook.is_none() {
        return Ok(Some(FrontierReason::CodebookRequired));
    }
    let cb = codebook.unwrap_or(fallback);
    let (plane_idx, band_idx, tile_idx) = at;

    for _mb in grid.iter() {
        let header =
            MbHeader::parse(r, ctx, cb, level_table).map_err(|error| DecodeError::MbHeader {
                plane_idx,
                band_idx,
                tile_idx,
                error,
            })?;
        stats.mbs += 1;
        if header.skipped {
            stats.mbs_skipped += 1;
            continue;
        }
        if let Some(cbp) = header.cbp {
            if cbp.coded_blocks(ctx.blocks_per_mb) > 0 {
                // The per-block (run, level) stream follows — the
                // gated spec/05/spec/06 path.
                return Ok(Some(FrontierReason::CodedBlockData));
            }
        }
        stats.mbs_coded_no_ac += 1;
    }
    Ok(None)
}

/// Crop a recomposed wavelet plane into a `spec/08 §1.1`
/// stride-padded reconstruction plane.
fn to_reconstruction_plane(
    band: &Band,
    w: u32,
    h: u32,
) -> Result<ReconstructionPlane, OutputError> {
    let stride = plane_stride(w);
    let mut data = vec![0i32; (stride * h) as usize];
    for y in 0..h {
        for x in 0..w {
            data[(y * stride + x) as usize] = band.at(x as usize, y as usize);
        }
    }
    ReconstructionPlane::new(w, h, stride, data)
}

/// Walk one coded **inter** tile's macroblock grid (`spec/03 §3/§4`
/// and `spec/07 §3/§5/§6`), applying the motion-compensated
/// predictor copy for every reconstructable MB. `work` holds the
/// band's coefficient layer seeded with the reference content;
/// `reference` is the previous frame's band at the same geometry.
#[allow(clippy::too_many_arguments)]
fn walk_tile_mbs_inter(
    r: &mut BitReader<'_>,
    grid: &MbGrid,
    ctx: &MbContext,
    codebook: Option<&Codebook>,
    fallback: &Codebook,
    level_table: &[i8; LEVEL_TABLE_LEN],
    stats: &mut DecodeStats,
    at: (usize, usize, usize),
    tile: &super::tile::Tile,
    mv_res: MvResolution,
    work: &mut [i16],
    reference: &[i16],
    band_w: usize,
) -> Result<Option<FrontierReason>, DecodeError> {
    let vlc_needed = ctx.qdelta_mode == QdeltaMode::Explicit || ctx.explicit_mv;
    if vlc_needed && codebook.is_none() {
        return Ok(Some(FrontierReason::CodebookRequired));
    }
    let cb = codebook.unwrap_or(fallback);
    let (plane_idx, band_idx, tile_idx) = at;

    // spec/07 §3.3 — zero-MV predictor reset at tile entry.
    let mut predictor = MvPredictor::new();

    for mb in grid.iter() {
        let header =
            MbHeader::parse(r, ctx, cb, level_table).map_err(|error| DecodeError::MbHeader {
                plane_idx,
                band_idx,
                tile_idx,
                error,
            })?;
        stats.mbs += 1;

        let mv = if header.skipped {
            stats.mbs_skipped += 1;
            // spec/07 §6.1 — skip inherits the left-neighbour MV.
            predictor.decode_mb(Mv::ZERO)
        } else {
            let delta = header
                .mv_delta
                .map(|(dx, dy)| Mv { x: dx, y: dy })
                .unwrap_or(Mv::ZERO);
            let mv = predictor.decode_mb(delta);
            if let Some(cbp) = header.cbp {
                if cbp.coded_blocks(ctx.blocks_per_mb) > 0 {
                    return Ok(Some(FrontierReason::CodedBlockData));
                }
            }
            stats.mbs_coded_no_ac += 1;
            mv
        };

        // A zero MV leaves the seed (= the reference content at the
        // same position, spec/07 §4.4 predictor carry). A non-zero MV
        // replaces the MB region with the MV-displaced prediction —
        // the residual is zero here (skip / CBP-without-AC), so the
        // spec/07 §5.5 residual-add over a zeroed region is a copy.
        if mv != Mv::ZERO {
            let resolved = resolve_mv(mv, mv_res);
            let dst_x = (tile.x + mb.x) as usize;
            let dst_y = (tile.y + mb.y) as usize;
            for row in 0..mb.height as usize {
                let base = (dst_y + row) * band_w + dst_x;
                work[base..base + mb.width as usize].fill(0);
            }
            mc_add_block(
                work,
                band_w,
                dst_x,
                dst_y,
                BandView {
                    data: reference,
                    stride: band_w,
                },
                dst_x as i32 + resolved.dx,
                dst_y as i32 + resolved.dy,
                mb.width as usize,
                mb.height as usize,
                resolved.mode,
            )
            .map_err(|error| DecodeError::Mc {
                plane_idx,
                band_idx,
                tile_idx,
                error,
            })?;
        }
    }
    Ok(None)
}

/// The decoded payload of one picture-carrying frame: the per-plane
/// band coefficient buffers (the `spec/07 §1.2` reference workspace
/// for the next frame) plus the recomposed reconstruction planes.
#[derive(Debug, Clone)]
pub(crate) struct PayloadOutcome {
    pub bands: Vec<Vec<Band>>,
    pub recon: Vec<ReconstructionPlane>,
    pub frontiers: Vec<DecodeFrontier>,
    pub stats: DecodeStats,
    pub parse_complete: bool,
}

/// Decode one INTRA frame to pixels.
///
/// Threads the `spec/02 §4.4` per-plane / per-band / per-tile walk
/// over the staged structural layers, recomposes each plane's bands
/// (`spec/06 §3`), and assembles the `spec/08` host buffer. See the
/// module docs for the gated-element (frontier) semantics. For
/// multi-frame sequences (INTER / NULL-repeat) use
/// [`super::Indeo5Decoder`].
pub fn decode_intra_picture(bitstream: &[u8]) -> Result<DecodedPicture, DecodeError> {
    let (header, mut r) = PictureHeader::parse_with_reader(bitstream, None)?;

    // NULL frame: repeat-previous, no coded payload (spec/08 §6.4).
    if header.is_null() {
        return Ok(DecodedPicture {
            header,
            format: None,
            output: None,
            frontiers: Vec::new(),
            stats: DecodeStats::default(),
            parse_complete: true,
        });
    }
    if header.frame_type() != FrameType::Intra {
        return Err(DecodeError::NotIntra {
            found: header.frame_type(),
        });
    }

    let gop = header.gop.as_ref().expect("INTRA frame carries a GOP");
    let frame = header
        .frame
        .as_ref()
        .expect("INTRA frame carries a frame header");
    let band_size_present = frame.flags.band_data_size_present();

    let payload = decode_payload(&mut r, gop, band_size_present, None)?;
    let format = output_format(gop);
    let output = assemble_frame(
        &payload.recon[0],
        &payload.recon[2],
        &payload.recon[1],
        format,
    )?;

    Ok(DecodedPicture {
        header,
        format: Some(format),
        output: Some(output),
        frontiers: payload.frontiers,
        stats: payload.stats,
        parse_complete: payload.parse_complete,
    })
}

/// The host output format the GOP subsampling selects
/// (`spec/08 §2.2`).
pub(crate) fn output_format(gop: &GopHeader) -> OutputFormat {
    match gop.flags.subsampling() {
        Subsampling::Yvu9 => OutputFormat::Yvu9,
        Subsampling::Yv12 => OutputFormat::Yv12,
    }
}

/// Decode a picture-carrying frame's per-band payload and recompose
/// its planes (`spec/02 §4.4` walk). `reference` is `None` for an
/// INTRA frame (zero seeds) and the previous frame's band buffers for
/// an INTER frame (`spec/07 §4.4` band-coefficient-layer prediction).
pub(crate) fn decode_payload(
    r: &mut BitReader<'_>,
    gop: &GopHeader,
    band_size_present: bool,
    reference: Option<&[Vec<Band>]>,
) -> Result<PayloadOutcome, DecodeError> {
    let slice_size = gop.slice_size();
    let level_table = build_level_table();
    // Never consulted (only passed when no VLC field is gated on).
    let fallback_codebook = Codebook::build(&[1, 1]).expect("trivial codebook");

    let planes_geom = [
        PlaneGeom {
            width: gop.width,
            height: gop.height,
            levels: gop.decomp.luma_levels,
            band_info: &gop.luma_band_info,
        },
        PlaneGeom {
            width: gop.chroma_width,
            height: gop.chroma_height,
            levels: gop.decomp.chroma_levels,
            band_info: &gop.chroma_band_info,
        },
        PlaneGeom {
            width: gop.chroma_width,
            height: gop.chroma_height,
            levels: gop.decomp.chroma_levels,
            band_info: &gop.chroma_band_info,
        },
    ];

    let mut stats = DecodeStats::default();
    let mut frontiers: Vec<DecodeFrontier> = Vec::new();
    let mut parse_complete = true;
    let mut recon_planes: Vec<ReconstructionPlane> = Vec::with_capacity(3);
    let mut all_bands: Vec<Vec<Band>> = Vec::with_capacity(3);

    for (plane_idx, geom) in planes_geom.iter().enumerate() {
        // Seed every band: zeros for INTRA; the previous frame's band
        // content for INTER (spec/07 §4.4 — empty bands / empty tiles
        // / skipped MBs carry the predictor forward).
        let mut bands: Vec<Band> = (0..geom.band_info.len())
            .map(|band_idx| {
                let (bw, bh) = band_dims(geom.width, geom.height, geom.levels, band_idx);
                match reference {
                    Some(prev) => prev
                        .get(plane_idx)
                        .and_then(|p| p.get(band_idx))
                        .filter(|b| b.width == bw as usize && b.height == bh as usize)
                        .cloned()
                        .ok_or(DecodeError::ReferenceMismatch {
                            plane_idx,
                            band_idx,
                        }),
                    None => Ok(Band::new(
                        bw as usize,
                        bh as usize,
                        vec![0i32; (bw * bh) as usize],
                    )),
                }
            })
            .collect::<Result<_, _>>()?;

        for (band_idx, binfo) in geom.band_info.iter().enumerate() {
            if !parse_complete {
                break;
            }
            r.align()?;
            let band_start = r.byte_pos();
            let band_header = BandHeader::parse(&mut *r, band_size_present).map_err(|error| {
                DecodeError::Band {
                    plane_idx,
                    band_idx,
                    error,
                }
            })?;
            stats.bands += 1;
            if band_header.empty {
                stats.empty_bands += 1;
                continue;
            }
            let band_end = band_header
                .band_data_size
                .map(|s| band_start + u64::from(s));
            let codebook = band_codebook(&band_header);
            let (bw, bh) = band_dims(geom.width, geom.height, geom.levels, band_idx);
            let (count_x, count_y) = band_tile_counts(bw, bh, slice_size);
            let tile_grid = TileGrid::build(bw, bh, count_x, count_y);

            let qdelta_mode = QdeltaMode::from_band_flags(
                band_header.flags.qdelta_present(),
                band_header.flags.qdelta_inherit(),
            );
            let inter = reference.is_some();
            // spec/03 §4.4 — an inter band carries explicit MVs only
            // when the per-band MV-inheritance flag is clear; the
            // inheritance path rides the docs-gapped spec/07 §3.4
            // per-band `0x3604`/`0x3664` tables.
            let explicit_mv = inter && !band_header.flags.mv_inherit();
            let mv_inherit_gated = inter && band_header.flags.mv_inherit();
            let mv_res = if binfo.mv_halfpel {
                MvResolution::HalfPel
            } else {
                MvResolution::FullPel
            };

            // The inter walk works on the band's coefficient layer as
            // i16 (the spec/07 §5 packed-word arithmetic); seed = the
            // reference carry already in `bands[band_idx]`.
            let mut work: Vec<i16> = if inter {
                bands[band_idx].data.iter().map(|&v| v as i16).collect()
            } else {
                Vec::new()
            };
            let ref_i16: Vec<i16> = if inter { work.clone() } else { Vec::new() };

            r.align()?;
            let mut band_aborted = false;
            let mut tile_idx = 0usize;
            'tiles: for row in 0..count_y {
                for col in 0..count_x {
                    let tile = tile_grid.tile(col, row).expect("in-range tile");
                    r.align()?;
                    let tile_start = r.byte_pos();
                    // The §2.7 predictor context flag reads the
                    // frame-level `[frame+0xec]` array whose origin is
                    // the spec/03 §6.3-item-5 open question; passed
                    // clear (intra frames force it clear anyway).
                    let th = TileHeader::parse(&mut *r, false)?;
                    stats.tiles += 1;

                    match th.size {
                        TileDataSize::Empty => {
                            // Intra: no coded blocks (zeros). Inter:
                            // the tile inherits the reference content
                            // already seeded (spec/03 §2.2).
                            stats.empty_tiles += 1;
                        }
                        TileDataSize::Implicit | TileDataSize::Explicit(_) => {
                            let grid = MbGrid::build(
                                tile.width,
                                tile.height,
                                binfo.mb_size,
                                binfo.blk_size,
                            );
                            let ctx = MbContext {
                                qdelta_mode,
                                explicit_mv,
                                blocks_per_mb: grid.blocks_per_mb(),
                            };
                            let frontier = if mv_inherit_gated {
                                // Coded tile in an MV-inheritance band:
                                // the per-MB MV/transform-id markers
                                // ride the docs-gapped spec/07 §3.4
                                // tables.
                                Some(FrontierReason::MvInheritance)
                            } else if inter {
                                walk_tile_mbs_inter(
                                    &mut *r,
                                    &grid,
                                    &ctx,
                                    codebook.as_ref(),
                                    &fallback_codebook,
                                    &level_table,
                                    &mut stats,
                                    (plane_idx, band_idx, tile_idx),
                                    tile,
                                    mv_res,
                                    &mut work,
                                    &ref_i16,
                                    bw as usize,
                                )?
                            } else {
                                walk_tile_mbs(
                                    &mut *r,
                                    &grid,
                                    &ctx,
                                    codebook.as_ref(),
                                    &fallback_codebook,
                                    &level_table,
                                    &mut stats,
                                    (plane_idx, band_idx, tile_idx),
                                )?
                            };
                            r.align()?;

                            if let Some(reason) = frontier {
                                let skipped_past = match th.size {
                                    TileDataSize::Explicit(n) => {
                                        // spec/03 §2.6 skip path (§2.8
                                        // whole-tile byte-count reading).
                                        skip_to_byte(&mut *r, tile_start + u64::from(n))?;
                                        true
                                    }
                                    TileDataSize::Implicit => {
                                        if let Some(end) = band_end {
                                            // spec/02 §3.2 band-level skip.
                                            skip_to_byte(&mut *r, end)?;
                                            band_aborted = true;
                                            true
                                        } else {
                                            parse_complete = false;
                                            false
                                        }
                                    }
                                    TileDataSize::Empty => unreachable!(),
                                };
                                frontiers.push(DecodeFrontier {
                                    plane_idx,
                                    band_idx,
                                    tile_idx,
                                    reason,
                                    skipped_past,
                                });
                                if band_aborted || !parse_complete {
                                    break 'tiles;
                                }
                            } else if let TileDataSize::Explicit(n) = th.size {
                                // §2.8 reconciliation: pad-skip when the
                                // encoder's count exceeds what the walk
                                // consumed; a target behind the cursor
                                // is the §2.8 error return.
                                skip_to_byte(&mut *r, tile_start + u64::from(n))?;
                            }
                        }
                    }
                    tile_idx += 1;
                }
            }

            // Band exit: reconcile against band_data_size when known.
            if parse_complete && !band_aborted {
                if let Some(end) = band_end {
                    r.align()?;
                    if r.byte_pos() < end {
                        skip_to_byte(&mut *r, end)?;
                    }
                }
            }
            // Intra bands whose coefficients are gated keep their
            // zero seed; an inter band writes back its worked
            // coefficient layer (reference carry + MC updates).
            if inter {
                bands[band_idx] = Band::new(
                    bw as usize,
                    bh as usize,
                    work.iter().map(|&v| v as i32).collect(),
                );
            }
        }

        // spec/06 §3.4 — bottom-up recompose: bands[0] is the
        // innermost LL; each level's (HL, LH, HH) triple follows.
        let levels: Vec<LevelBands> = (0..geom.levels as usize)
            .map(|l| LevelBands {
                hl: bands[1 + 3 * l].clone(),
                lh: bands[2 + 3 * l].clone(),
                hh: bands[3 + 3 * l].clone(),
            })
            .collect();
        let plane = recompose_plane(&bands[0], &levels);
        recon_planes.push(to_reconstruction_plane(&plane, geom.width, geom.height)?);
        all_bands.push(bands);
    }

    Ok(PayloadOutcome {
        bands: all_bands,
        recon: recon_planes,
        frontiers,
        stats,
        parse_complete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_dims_per_level() {
        // 0-level plane: one band at plane resolution.
        assert_eq!(band_dims(352, 288, 0, 0), (352, 288));
        // 1-level: LL + HF triple all at half resolution.
        assert_eq!(band_dims(352, 288, 1, 0), (176, 144));
        assert_eq!(band_dims(352, 288, 1, 1), (176, 144));
        assert_eq!(band_dims(352, 288, 1, 3), (176, 144));
        // 2-level: LL + inner triple at quarter, outer triple at half.
        assert_eq!(band_dims(352, 288, 2, 0), (88, 72));
        assert_eq!(band_dims(352, 288, 2, 2), (88, 72));
        assert_eq!(band_dims(352, 288, 2, 4), (176, 144));
        assert_eq!(band_dims(352, 288, 2, 6), (176, 144));
    }

    #[test]
    fn tile_counts_from_slice() {
        // No slice -> whole band is one tile.
        assert_eq!(band_tile_counts(176, 144, None), (1, 1));
        // spec/03 §1.1: 176x144 band at 64-px slices -> 3x3.
        assert_eq!(band_tile_counts(176, 144, Some(64)), (3, 3));
        // Band smaller than the slice -> one tile.
        assert_eq!(band_tile_counts(44, 36, Some(64)), (1, 1));
    }
}
