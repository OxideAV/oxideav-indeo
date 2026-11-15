//! Band decoding: tiles, macroblock headers, block coefficients,
//! dequantisation, DC prediction, inverse transforms and motion
//! compensation (`spec/03` to `spec/07` of the staged specification).

use super::Iv50Error;
use super::bits::Bits;
use super::headers::{BandHeader, BandInfo};
use super::huff::{Huff, signed};
use super::tables::{RV_SLOTS, SCAN_0, SCAN_1, SCAN_2, SCAN_3};
use super::xform;

/// Where a band lives in its plane area and how it is coded.
#[derive(Debug, Clone)]
pub(crate) struct BandGeom {
    /// Band height in samples.
    pub height: usize,
    /// Row pitch in samples: the band width rounded up so that a row
    /// of 16-bit samples is a multiple of 32 bytes (spec/06 §4.2).
    pub pitch: usize,
    /// Offset of sample (0, 0) in the plane area.
    pub offset: usize,
    pub info: BandInfo,
    /// Quantiser matrix group `g` (spec/06 §5.1).
    pub matrix: usize,
    /// Tiles of the band, in raster order.
    pub tiles: Vec<TileGeom>,
}

/// One tile of a band, in band samples.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TileGeom {
    pub x: usize,
    pub y: usize,
    /// Macroblock columns and rows.
    pub cols: usize,
    pub rows: usize,
    /// Width in samples of the area an empty tile copies.
    pub copy_width: usize,
    /// Index of the tile's first macroblock in the luma band 0 records.
    pub rec_base: usize,
}

/// What a luma band 0 macroblock leaves for the bands that inherit from
/// it (spec/06 §5.2; spec/07 §3.4). The records persist across frames.
#[derive(Debug, Clone, Copy)]
pub(crate) struct B0Rec {
    pub intra: bool,
    pub mvx: i32,
    pub mvy: i32,
    /// The stored quantiser delta.
    pub delta: i32,
}

impl Default for B0Rec {
    fn default() -> Self {
        B0Rec {
            intra: true,
            mvx: 0,
            mvy: 0,
            delta: 0,
        }
    }
}

/// A decoded macroblock header.
#[derive(Debug, Clone, Copy, Default)]
struct Mb {
    intra: bool,
    cbp: u32,
    q: u32,
    mvx: i32,
    mvy: i32,
}

/// Meaning of a block symbol after the run/value table and the
/// swap pairs (spec/05 §2.2 and §2.4).
#[derive(Clone, Copy, Default)]
struct RvSym {
    run: u8,
    /// 0 normal, 1 end of block, 2 escape, 3 invalid.
    kind: u8,
    code: u16,
}

/// The run/value map of a band.
pub(crate) struct RvMap([RvSym; 256]);

impl RvMap {
    /// Build the map for slot `slot` with the band's correction pairs.
    pub fn new(slot: usize, corrections: &[(u8, u8)]) -> Result<RvMap, Iv50Error> {
        let t = &RV_SLOTS[slot];
        // Rank to (run, level index), on the stack: a band builds its map
        // once per frame.
        let mut by_rank = [(0u8, 0u16); 256];
        let mut ranks = 2;
        for (i, &c) in t.runs.iter().enumerate() {
            let run = (i + 1) as u8;
            let c = c as usize;
            if ranks + 2 * c > 256 {
                return Err(Iv50Error::BadHeader("run/value table"));
            }
            for k in 0..c {
                by_rank[ranks + k] = (run, (2 * c - 2 * k) as u16);
                by_rank[ranks + c + k] = (run, (2 * k + 1) as u16);
            }
            ranks += 2 * c;
        }
        let by_rank = &by_rank[..ranks];
        let mut map = [RvSym {
            run: 0,
            kind: 3,
            code: 0,
        }; 256];
        for (s, m) in map.iter_mut().enumerate() {
            let rank = t.rank[s] as usize;
            if rank == 0 {
                m.kind = 1;
            } else if rank == 1 {
                m.kind = 2;
            } else if rank < by_rank.len() {
                let (run, code) = by_rank[rank];
                *m = RvSym { run, kind: 0, code };
            }
        }
        // Corrections exchange the meaning of two symbols. End of block
        // and escape stay with their symbols; a pair that involves one of
        // them leaves the other symbol without a valid meaning. (spec/05
        // §2.4 exchanges the table entries with no such check; no staged
        // stream names a sentinel in a pair.)
        for &(a, b) in corrections {
            let (a, b) = (a as usize, b as usize);
            let (ka, kb) = (map[a].kind, map[b].kind);
            if ka == 1 || ka == 2 || kb == 1 || kb == 2 {
                if ka == 0 || ka == 3 {
                    map[a].kind = 3;
                }
                if kb == 0 || kb == 3 {
                    map[b].kind = 3;
                }
                continue;
            }
            map.swap(a, b);
        }
        Ok(RvMap(map))
    }
}

#[cfg(test)]
impl RvMap {
    /// The symbol that stands for (run, value code), for building test
    /// streams.
    pub fn symbol_for(&self, run: u8, code: u16) -> Option<u32> {
        (0..256u32).find(|&s| {
            let e = self.0[s as usize];
            e.kind == 0 && e.run == run && e.code == code
        })
    }

    /// The end-of-block symbol.
    pub fn eob(&self) -> u32 {
        (0..256u32).find(|&s| self.0[s as usize].kind == 1).unwrap()
    }

    /// The escape symbol.
    pub fn esc(&self) -> u32 {
        (0..256u32).find(|&s| self.0[s as usize].kind == 2).unwrap()
    }
}

/// Quantiser steps, `bank[g][class][q][position]` (spec/06 §5.1;
/// `tables/quant_matrices_1007b000`).
pub(crate) type Bank = [[[[u8; 64]; 24]; 2]; 6];

/// Build the step bank from the base matrices and scale factors.
pub(crate) fn build_bank() -> Box<Bank> {
    use super::tables::{QUANT_BASE, QUANT_SCALE};
    let mut bank = Box::new([[[[0u8; 64]; 24]; 2]; 6]);
    for m in 0..6 {
        for cls in 0..2 {
            for q in 0..24 {
                let scale = QUANT_SCALE[m][cls][q] as u32;
                for p in 0..64 {
                    let base = if m == 5 {
                        let (r, k) = (p / 8, p % 8);
                        if r < 4 && k < 4 {
                            QUANT_BASE[m][cls][r * 4 + k] as u32
                        } else {
                            0
                        }
                    } else {
                        QUANT_BASE[m][cls][p] as u32
                    };
                    bank[m][cls][q][p] = ((base * scale) >> 8).clamp(1, 255) as u8;
                }
            }
        }
    }
    bank
}

/// The coefficient value of level index `code` at step `s` (spec/05
/// §2.3; `tables/recon_lut_formula`): `k = (code + 1) >> 1`, magnitude
/// `k * s + s / 2 - (s & 1 if s > 1)`, positive for odd `code`. The
/// per-step tables are contiguous with `8192 / s` levels each (spec/06
/// §5.1), so a level past the end of step `s` continues into the table
/// of step `s + 1`.
#[inline]
pub(super) fn dequant(code: u32, s: u32) -> Result<i32, Iv50Error> {
    if code == 0 {
        return Err(Iv50Error::BadBlockData("value code 0"));
    }
    let mut idx = code - 1;
    let mut s = s;
    // Every step up to 255 has at least 32 levels, so the first 64
    // level indices never leave their table: no division for them.
    while idx >= 64 {
        let n = 2 * (8192 / s);
        if idx < n {
            break;
        }
        idx -= n;
        s += 1;
        if s > 256 {
            return Err(Iv50Error::BadBlockData("value code past the table"));
        }
    }
    debug_assert!(s <= 256 && idx < 2 * (8192 / s));
    let k = idx / 2 + 1;
    let adj = if s & 1 == 1 && s > 1 { 1 } else { 0 };
    let mag = (k * s + s / 2 - adj) as i32;
    Ok(if idx & 1 == 0 { mag } else { -mag })
}

/// The scan of a transform type (spec/06 §2.5;
/// `tables/scan_tables_10098528`).
fn scan_for(xform: u32) -> Result<&'static [u8], Iv50Error> {
    match xform {
        0 => Ok(&SCAN_0),
        1 => Ok(&SCAN_1),
        2 | 3 => Ok(&SCAN_2),
        4 => Ok(&SCAN_3),
        _ => Err(Iv50Error::Unsupported("transform types 5 to 7")),
    }
}

/// Everything one band decode needs besides the bitstream.
pub(crate) struct BandJob<'a> {
    pub geom: &'a BandGeom,
    pub hdr: &'a BandHeader,
    /// The band's macroblocks do not lie one to one over luma band 0's
    /// (half-size chroma with another macroblock size ratio).
    pub foreign_grid: bool,
    pub is_b0: bool,
    pub frame_type: u32,
    pub frame_flags: u32,
    /// Macroblock size of luma band 0, for inherited vectors.
    pub b0_mb_size: u32,
    /// Luma band 0 has 1-block macroblocks in rows of odd length, whose
    /// records are not laid out one per macroblock (not covered by the
    /// staged specification).
    pub b0_drifts: bool,
    pub mb_huff: &'a Huff,
    pub blk_huff: &'a Huff,
    pub bank: &'a Bank,
    /// The plane area being decoded into, and the reference's.
    pub cur: &'a mut [i16],
    pub refp: &'a [i16],
    pub b0: &'a mut [B0Rec],
}

/// Scale an inherited vector component to a band with smaller
/// macroblocks: divide by 2^n, halves away from zero (the staged
/// inter fixture's chroma bands inherit this way; spec/07 §3.4 names
/// the inheritance but not the rounding).
#[inline]
fn scale_mv(v: i32, n: u32) -> i32 {
    if n == 0 {
        return v;
    }
    let mag = (v.abs() + (1 << (n - 1))) >> n;
    if v < 0 { -mag } else { mag }
}

impl BandJob<'_> {
    /// Decode the band's tiles from `r`, which stands at the first tile.
    pub fn decode(mut self, r: &mut Bits) -> Result<(), Iv50Error> {
        let info = self.geom.info;
        let four = info.mb_size != info.blk_size;
        let ks = info.mb_size as i32 / 8 - self.b0_mb_size as i32 / 8;
        if self.hdr.inherit() && ks > 0 {
            return Err(Iv50Error::Unsupported(
                "inheriting into larger macroblocks than luma band 0's",
            ));
        }
        let ks = (-ks) as u32;
        let reads_b0 = self.hdr.inherit() || (self.hdr.has_delta() && !self.hdr.reads_delta());
        if reads_b0 && !self.is_b0 && self.b0_drifts {
            return Err(Iv50Error::Unsupported(
                "inheriting from 1-block luma band 0 macroblocks in odd rows",
            ));
        }
        if reads_b0 && self.foreign_grid {
            return Err(Iv50Error::Unsupported(
                "half-size chroma inheriting across different macroblock grids",
            ));
        }
        let scan = scan_for(info.xform)?;
        let xf_blk = if info.xform >= 4 { 4 } else { 8 };
        if xf_blk != info.blk_size {
            return Err(Iv50Error::Unsupported(
                "transform size differs from block size",
            ));
        }
        if self.geom.matrix > 5 {
            return Err(Iv50Error::Unsupported("quantisation matrix ids 6 to 8"));
        }
        let rv = RvMap::new(self.hdr.rv_table, &self.hdr.corrections)?;
        let ntiles = self.geom.tiles.len();
        let mut mbs: Vec<Mb> = Vec::new();
        for t in 0..ntiles {
            let tile = self.geom.tiles[t];
            let tile_start = r.byte_pos();
            if r.bit() {
                // Empty tile: `value24` set (spec/03 §2.2).
                if self.frame_type != 0 {
                    self.copy_tile(&tile);
                    if self.is_b0 && !self.hdr.has_delta() {
                        let gq = self.hdr.gq as i32;
                        for rec in
                            &mut self.b0[tile.rec_base..tile.rec_base + tile.cols * tile.rows]
                        {
                            *rec = B0Rec {
                                intra: false,
                                mvx: 0,
                                mvy: 0,
                                delta: gq,
                            };
                        }
                    }
                }
                continue;
            }
            let size = if r.bit() {
                let s = r.read(8);
                if s == 0xff { r.read(24) } else { s }
            } else {
                0
            };
            r.align();
            self.parse_mbs(r, &tile, four, ks, &mut mbs)?;
            r.align();
            self.decode_blocks(r, &tile, four, scan, &rv, &mbs)?;
            r.align();
            if r.overrun() {
                return Err(Iv50Error::Truncated);
            }
            if size != 0 {
                if r.byte_pos() - tile_start != size as usize {
                    return Err(Iv50Error::BadBlockData("tile size mismatch"));
                }
            } else if t + 1 < ntiles {
                return Err(Iv50Error::Unsupported("coded tile without a size"));
            }
        }
        Ok(())
    }

    /// Copy an empty tile's area from the reference (spec/07 §6.4).
    fn copy_tile(&mut self, tile: &TileGeom) {
        let g = self.geom;
        let mb = g.info.mb_size as usize;
        let rows = (tile.rows * mb).min(g.height.next_multiple_of(16) - tile.y);
        for row in 0..rows {
            let o = g.offset + (tile.y + row) * g.pitch + tile.x;
            self.cur[o..o + tile.copy_width].copy_from_slice(&self.refp[o..o + tile.copy_width]);
        }
    }

    /// Parse the macroblock headers of a tile (spec/03 §4.0 phase 2 and
    /// §4.1 to §4.5; the type bit and the class it selects are spec/06
    /// §5.2).
    fn parse_mbs(
        &mut self,
        r: &mut Bits,
        tile: &TileGeom,
        four: bool,
        ks: u32,
        mbs: &mut Vec<Mb>,
    ) -> Result<(), Iv50Error> {
        let hdr = self.hdr;
        let inherit = hdr.inherit();
        let reads_delta = hdr.reads_delta();
        let has_delta = hdr.has_delta();
        let is_b0 = self.is_b0;
        let intra_frame = self.frame_type == 0;
        let delta_always = is_b0 && self.frame_flags & 8 != 0;
        let gq = hdr.gq as i32;
        let huff = self.mb_huff;
        let mut p: u32 = 0x8080;
        mbs.clear();
        let n = tile.cols * tile.rows;
        for k in 0..n {
            let src = if inherit || (has_delta && !reads_delta) {
                self.b0.get(tile.rec_base + k).copied().unwrap_or_default()
            } else {
                B0Rec::default()
            };
            let mut mb = Mb::default();
            let mut delta = 0;
            if r.bit() {
                mb.intra = intra_frame;
                if reads_delta && delta_always {
                    delta = signed(huff.decode(r).ok_or(Iv50Error::BadMbData)?);
                }
                if inherit {
                    mb.mvx = scale_mv(src.mvx, ks);
                    mb.mvy = scale_mv(src.mvy, ks);
                }
            } else {
                mb.intra = if intra_frame {
                    true
                } else if inherit {
                    src.intra
                } else {
                    !r.bit()
                };
                mb.cbp = if four { r.read(4) } else { r.read(1) };
                if reads_delta && (mb.cbp != 0 || delta_always) {
                    delta = signed(huff.decode(r).ok_or(Iv50Error::BadMbData)?);
                }
                if !mb.intra {
                    if inherit {
                        mb.mvx = scale_mv(src.mvx, ks);
                        mb.mvy = scale_mv(src.mvy, ks);
                    } else {
                        let dy = signed(huff.decode(r).ok_or(Iv50Error::BadMbData)?);
                        let dx = signed(huff.decode(r).ok_or(Iv50Error::BadMbData)?);
                        // The vector is one word with biased byte
                        // components; carries cross from x into y
                        // (spec/07 §2.4 and §3.2: the delta is added
                        // to the previous macroblock's vector).
                        p = p
                            .wrapping_add((((dy + 128) & 0xff) as u32) << 8)
                            .wrapping_add(((dx + 128) & 0xff) as u32)
                            .wrapping_sub(0x8080);
                        mb.mvx = (p & 0xff) as i32 - 128;
                        mb.mvy = ((p >> 8) & 0xff) as i32 - 128;
                    }
                }
            }
            // The quantiser (spec/06 §5.2).
            let stored;
            mb.q = if reads_delta {
                if !(-24..=23).contains(&delta) {
                    return Err(Iv50Error::BadMbData);
                }
                let q = (gq + delta).clamp(0, 23);
                stored = q - gq;
                q as u32
            } else if has_delta {
                stored = 0;
                (gq + src.delta).clamp(0, 23) as u32
            } else {
                stored = 0;
                gq as u32
            };
            if is_b0 {
                let rec = &mut self.b0[tile.rec_base + k];
                rec.intra = mb.intra;
                if !mb.intra {
                    rec.mvx = mb.mvx;
                    rec.mvy = mb.mvy;
                }
                rec.delta = stored;
            }
            mbs.push(mb);
        }
        if r.overrun() {
            return Err(Iv50Error::Truncated);
        }
        Ok(())
    }

    /// Decode the block data of a tile and reconstruct its blocks (spec
    /// 05, spec/06 and spec/07; spec/03 §4.0 phase 3).
    fn decode_blocks(
        &mut self,
        r: &mut Bits,
        tile: &TileGeom,
        four: bool,
        scan: &[u8],
        rv: &RvMap,
        mbs: &[Mb],
    ) -> Result<(), Iv50Error> {
        let g = self.geom;
        let info = g.info;
        let mb_size = info.mb_size as usize;
        let blk = info.blk_size as usize;
        let pitch = g.pitch;
        let xform = info.xform;
        let dc_pred = xform == 0 || xform == 4;
        let nblk = if four { 4 } else { 1 };
        let max_pos = scan.len() as i32 - 1;
        let half_pel = info.half_pel;
        let bank = &self.bank[g.matrix];
        let mut pred: i32 = 0;
        let mut coefs;
        for row in 0..tile.rows {
            for col in 0..tile.cols {
                let mb = &mbs[row * tile.cols + col];
                // The step table of the macroblock's class and quantiser
                // (spec/06 §5.1, §5.2): class 1 for an intra macroblock,
                // class 0 for an inter one. The 48 tables are one array,
                // class 1 after class 0, so an inter macroblock at a
                // quantiser of 24 to 31 lands on the class 1 table of
                // level q - 24; an intra one would land past the array.
                let steps = match (mb.q, mb.intra) {
                    (0..=23, intra) => &bank[intra as usize][mb.q as usize],
                    (24..=47, false) => &bank[1][mb.q as usize - 24],
                    _ => &bank[1][23],
                };
                let no_steps = mb.intra && mb.q > 23;
                for j in 0..nblk {
                    let bx = tile.x + col * mb_size + (j & 1) * blk;
                    let by = tile.y + row * mb_size + (j >> 1) * blk;
                    let off = g.offset + by * pitch + bx;
                    if mb.cbp & (1 << j) != 0 {
                        if no_steps {
                            return Err(Iv50Error::Unsupported(
                                "band quantiser above 23 on an intra macroblock",
                            ));
                        }
                        coefs = [0; 64];
                        let mut occ = xform::Occupancy { rows: 0, cols: 0 };
                        let mut pos: i32 = -1;
                        loop {
                            let s = self
                                .blk_huff
                                .decode(r)
                                .ok_or(Iv50Error::BadBlockData("code without a symbol"))?;
                            let e = rv.0[s as usize];
                            let (run, code) = match e.kind {
                                0 => (e.run as i32, e.code as u32),
                                1 => break,
                                2 => {
                                    let run = self
                                        .blk_huff
                                        .decode(r)
                                        .ok_or(Iv50Error::BadBlockData("escape"))?
                                        as i32
                                        + 1;
                                    let lo = self
                                        .blk_huff
                                        .decode(r)
                                        .ok_or(Iv50Error::BadBlockData("escape"))?;
                                    let hi = self
                                        .blk_huff
                                        .decode(r)
                                        .ok_or(Iv50Error::BadBlockData("escape"))?;
                                    (run, hi * 64 + lo)
                                }
                                _ => return Err(Iv50Error::BadBlockData("corrected symbol")),
                            };
                            pos += run;
                            if pos > max_pos {
                                return Err(Iv50Error::BadBlockData("scan position past the end"));
                            }
                            let p = scan[pos as usize] as usize;
                            coefs[p] = dequant(code, steps[p] as u32)? as i16;
                            occ.set(p);
                            if r.overrun() {
                                return Err(Iv50Error::Truncated);
                            }
                        }
                        if dc_pred && mb.intra {
                            let d = coefs[0] as i32;
                            coefs[0] = (pred + d) as i16;
                            pred += d;
                            occ.set(0);
                        }
                        xform::inverse_sparse(xform, &coefs, occ, self.cur, off, pitch);
                        if !mb.intra {
                            self.motion(off, blk, mb.mvx, mb.mvy, half_pel, true);
                        }
                    } else if mb.intra {
                        self.fill_intra(off, blk, dc_pred, pred);
                    } else {
                        self.motion(off, blk, mb.mvx, mb.mvy, half_pel, false);
                    }
                }
            }
            // A row of 1-block macroblocks with an odd count gets one more
            // record, an uncoded intra block, which the intra fill writes
            // (not covered by the staged specification).
            if !four && tile.cols % 2 == 1 {
                let bx = tile.x + tile.cols * mb_size;
                let by = tile.y + row * mb_size;
                if bx + blk <= pitch {
                    let off = g.offset + by * pitch + bx;
                    self.fill_intra(off, blk, dc_pred, pred);
                }
            }
        }
        Ok(())
    }

    /// Fill an uncoded intra block with the DC predictor (the flat-block
    /// path of spec/06 §2.3).
    #[inline]
    fn fill_intra(&mut self, off: usize, blk: usize, dc_pred: bool, pred: i32) {
        let v = if dc_pred { ((pred + 1) >> 1) as i16 } else { 0 };
        xform::fill(self.cur, off, self.geom.pitch, blk, v);
    }

    /// Motion compensation of one inter block (spec/07 §5.2 to §5.5):
    /// add the prediction to the residual of a coded block, or write the
    /// prediction into an uncoded one.
    #[inline]
    fn motion(&mut self, off: usize, n: usize, mvx: i32, mvy: i32, half_pel: bool, coded: bool) {
        let pitch = self.geom.pitch;
        let (ix, iy, fx, fy) = if half_pel {
            (mvx >> 1, mvy >> 1, (mvx & 1) as usize, (mvy & 1) as usize)
        } else {
            (mvx, mvy, 0, 0)
        };
        let src = off as isize + iy as isize * pitch as isize + ix as isize;
        let last = src + ((n + fy - 1) * pitch + n + fx) as isize;
        if src >= 0 && last <= self.refp.len() as isize {
            let src = src as usize;
            if n == 8 {
                predict_block::<8>(self.cur, self.refp, off, src, pitch, fx, fy, coded);
            } else {
                predict_block::<4>(self.cur, self.refp, off, src, pitch, fx, fy, coded);
            }
            return;
        }
        // Reads outside the plane area (spec/07 §5.4 has no clipping
        // and assumes vectors stay inside the padding): zeros.
        let refp = self.refp;
        let cur = &mut *self.cur;
        let get = |i: isize| -> i16 {
            if i >= 0 && (i as usize) < refp.len() {
                refp[i as usize]
            } else {
                0
            }
        };
        let pi = pitch as isize;
        for rr in 0..n {
            for c in 0..n {
                let i = src + rr as isize * pi + c as isize;
                let p = match (fx, fy) {
                    (0, 0) => get(i),
                    (1, 0) => get(i).wrapping_add(get(i + 1)) >> 1,
                    (0, 1) => get(i).wrapping_add(get(i + pi)) >> 1,
                    _ => {
                        get(i)
                            .wrapping_add(get(i + 1))
                            .wrapping_add(get(i + pi))
                            .wrapping_add(get(i + pi + 1))
                            >> 2
                    }
                };
                let o = &mut cur[off + rr * pitch + c];
                *o = if coded { o.wrapping_add(p) } else { p };
            }
        }
    }
}

/// One row of `N` reference samples starting at `at`.
#[inline(always)]
fn ref_row<const N: usize>(refp: &[i16], at: usize) -> &[i16; N] {
    refp[at..at + N]
        .first_chunk::<N>()
        .expect("slice of N samples")
}

/// The prediction of an `N` by `N` block read at `src` of `refp`, with
/// the horizontal and vertical half-sample flags `fx` and `fy` (spec/07
/// §5.2: the truncating average of two or four samples in 16-bit
/// arithmetic), added to `cur` at `off` for a coded block or written
/// over it. Every read lies inside `refp`. Row by row over fixed-width
/// arrays, so each row is a handful of vector operations.
#[allow(clippy::too_many_arguments)] // one call site, all plain scalars
#[inline(always)]
fn predict_block<const N: usize>(
    cur: &mut [i16],
    refp: &[i16],
    off: usize,
    src: usize,
    pitch: usize,
    fx: usize,
    fy: usize,
    coded: bool,
) {
    for rr in 0..N {
        let s = src + rr * pitch;
        let mut pred = *ref_row::<N>(refp, s);
        match (fx, fy) {
            (0, 0) => {}
            (1, 0) => {
                let b = ref_row::<N>(refp, s + 1);
                for (p, &b) in pred.iter_mut().zip(b) {
                    *p = p.wrapping_add(b) >> 1;
                }
            }
            (0, 1) => {
                let b = ref_row::<N>(refp, s + pitch);
                for (p, &b) in pred.iter_mut().zip(b) {
                    *p = p.wrapping_add(b) >> 1;
                }
            }
            _ => {
                let b = ref_row::<N>(refp, s + 1);
                let c = ref_row::<N>(refp, s + pitch);
                let d = ref_row::<N>(refp, s + pitch + 1);
                for (i, p) in pred.iter_mut().enumerate() {
                    *p = p.wrapping_add(b[i]).wrapping_add(c[i]).wrapping_add(d[i]) >> 2;
                }
            }
        }
        let d = off + rr * pitch;
        let dst = cur[d..d + N]
            .first_chunk_mut::<N>()
            .expect("slice of N samples");
        if coded {
            for (o, p) in dst.iter_mut().zip(pred) {
                *o = o.wrapping_add(p);
            }
        } else {
            *dst = pred;
        }
    }
}

/// Fill an empty band (`band_flags` bit 0, spec/02 §3.3; the fill
/// values are not covered by the staged specification): bands 0 to 3 of a plane
/// get 0, 1, 3 and 2; later bands get the dword 0x80000000, that is the
/// samples 0 and -32768 in turn. Nothing is written when the band's
/// first dword already has its sign bit set.
pub(crate) fn fill_empty_band(cur: &mut [i16], g: &BandGeom, index: usize) {
    let o = g.offset;
    if cur[o + 1] < 0 {
        return;
    }
    let area = &mut cur[o..o + g.height * g.pitch];
    match index {
        0..=3 => area.fill([0, 1, 3, 2][index]),
        _ => {
            for (i, v) in area.iter_mut().enumerate() {
                *v = if i & 1 == 0 { 0 } else { i16::MIN };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dequant_examples() {
        // s = 1: 1, 2, 3; s = 2: 3, 5, 7; s = 3: 3, 6, 9; s = 4: 6, 10.
        let pos = |s: u32, k: u32| dequant(2 * k - 1, s).unwrap();
        assert_eq!([pos(1, 1), pos(1, 2), pos(1, 3)], [1, 2, 3]);
        assert_eq!([pos(2, 1), pos(2, 2), pos(2, 3)], [3, 5, 7]);
        assert_eq!([pos(3, 1), pos(3, 2), pos(3, 3)], [3, 6, 9]);
        assert_eq!([pos(4, 1), pos(4, 2)], [6, 10]);
        assert_eq!(dequant(2, 4).unwrap(), -6);
        // Past the run for s = 200 (2 * 40 entries): continue in s = 201.
        assert_eq!(dequant(81, 200).unwrap(), 201 + 100 - 1);
    }

    /// The bank equals the staged runtime bank, copied to
    /// `tests/data/iv50-quant-matrices-1007b000.csv` (id, class, q, then
    /// 64 steps; the 4x4 id holds its rows at stride 8).
    #[test]
    fn bank_matches_runtime_dump() {
        let csv = include_str!("../../../tests/data/iv50-quant-matrices-1007b000.csv");
        let bank = build_bank();
        let mut rows = 0;
        for line in csv.lines().skip(1) {
            let f: Vec<&str> = line.split(',').collect();
            let m: usize = f[0].parse().unwrap();
            let cls: usize = f[1].parse().unwrap();
            let q: usize = f[2].parse().unwrap();
            let v: Vec<u8> = f[6..70].iter().map(|x| x.parse().unwrap()).collect();
            for p in 0..64 {
                if m == 5 && (p / 8 >= 4 || p % 8 >= 4) {
                    continue;
                }
                assert_eq!(bank[m][cls][q][p], v[p], "id {m} class {cls} q {q} pos {p}");
            }
            rows += 1;
        }
        assert_eq!(rows, 6 * 2 * 24);
    }

    #[test]
    fn rv_map_ranks() {
        let m = RvMap::new(0, &[]).unwrap();
        // Slot 0: rank[5] = 0 (EOB), rank[2] = 1 (ESC).
        assert_eq!(m.0[5].kind, 1);
        assert_eq!(m.0[2].kind, 2);
        // Run 1 has 40 pairs: ranks 2..41 are codes 80, 78, ..., 2 and
        // ranks 42..81 codes 1, 3, ..., 79. rank[0] = 42.
        assert_eq!((m.0[0].run, m.0[0].code), (1, 1));
        assert_eq!((m.0[1].run, m.0[1].code), (1, 2));
        assert_eq!((m.0[4].run, m.0[4].code), (1, 4));
        let c = RvMap::new(0, &[(0, 1)]).unwrap();
        assert_eq!(c.0[0].code, 2);
        assert_eq!(c.0[1].code, 1);
    }

    #[test]
    fn inherited_vectors_round_halves_away_from_zero() {
        assert_eq!(scale_mv(3, 1), 2);
        assert_eq!(scale_mv(-3, 1), -2);
        assert_eq!(scale_mv(2, 1), 1);
        assert_eq!(scale_mv(-5, 2), -1);
        assert_eq!(scale_mv(-6, 2), -2);
        assert_eq!(scale_mv(7, 0), 7);
    }

    /// Every slot of the run/value tables expands to the staged
    /// per-symbol table (`tables/rv_expanded_100972f4.csv`: slot,
    /// rv_tab_sel, slot address, symbol, rank, run, lidx, level): rank 0
    /// is the end of block, rank 1 the escape, and every other symbol
    /// carries the listed run and level index (spec/05 §2.2).
    #[test]
    fn rv_maps_equal_the_staged_expansion() {
        let csv = include_str!("../../../tests/data/iv50-tables/rv_expanded_100972f4.csv");
        let maps: Vec<RvMap> = (0..9).map(|s| RvMap::new(s, &[]).unwrap()).collect();
        let mut rows = 0;
        for line in csv.lines().skip(1) {
            let f: Vec<i32> = line
                .split(',')
                .enumerate()
                .map(|(i, x)| if i == 2 { 0 } else { x.parse().unwrap() })
                .collect();
            let (slot, sym, rank, run, lidx, level) = (f[0], f[3], f[4], f[5], f[6], f[7]);
            let e = maps[slot as usize].0[sym as usize];
            match rank {
                0 => assert_eq!(e.kind, 1, "slot {slot} symbol {sym}"),
                1 => assert_eq!(e.kind, 2, "slot {slot} symbol {sym}"),
                _ => {
                    assert_eq!(
                        (e.kind, e.run as i32, e.code as i32),
                        (0, run, lidx),
                        "slot {slot} symbol {sym}"
                    );
                    // The level the index stands for at step 1.
                    assert_eq!(dequant(lidx as u32, 1).unwrap(), level);
                }
            }
            rows += 1;
        }
        assert_eq!(rows, 9 * 256);
    }

    /// Swap pairs exchange run and level index of two symbols, in list
    /// order, so a symbol named twice moves twice (spec/05 §2.4; the
    /// pairs are the Y band's of the staged 320x240 keyframe).
    #[test]
    fn swap_pairs_apply_in_order() {
        let pairs = [
            (4, 220),
            (4, 6),
            (8, 61),
            (7, 80),
            (5, 23),
            (12, 59),
            (13, 171),
            (15, 100),
            (18, 103),
        ];
        let plain = RvMap::new(4, &[]).unwrap();
        let swapped = RvMap::new(4, &pairs).unwrap();
        let mut want: Vec<(u8, u8, u16)> =
            plain.0.iter().map(|e| (e.kind, e.run, e.code)).collect();
        for (a, b) in pairs {
            want.swap(a as usize, b as usize);
        }
        let got: Vec<(u8, u8, u16)> = swapped.0.iter().map(|e| (e.kind, e.run, e.code)).collect();
        assert_eq!(got, want);
        // Symbol 4 went to 220 first, then what 220 held went on to 6.
        assert_eq!(
            got[6],
            (plain.0[220].kind, plain.0[220].run, plain.0[220].code)
        );
        assert_eq!(got[220], (plain.0[4].kind, plain.0[4].run, plain.0[4].code));
    }

    /// The reconstruction value of every staged (step, k) pair
    /// (`tables/recon_lut_formula.csv`: step, levels, k, recon): `+recon`
    /// for level index `2k - 1`, `-recon` for `2k` (spec/05 §2.3).
    #[test]
    fn dequant_equals_the_staged_reconstruction_table() {
        let csv = include_str!("../../../tests/data/iv50-tables/recon_lut_formula.csv");
        let mut rows = 0;
        for line in csv.lines().skip(1) {
            let f: Vec<u32> = line.split(',').map(|x| x.parse().unwrap()).collect();
            let (step, levels, k, recon) = (f[0], f[1], f[2], f[3] as i32);
            assert_eq!(levels, 8192 / step);
            assert_eq!(
                dequant(2 * k - 1, step).unwrap(),
                recon,
                "step {step} k {k}"
            );
            assert_eq!(dequant(2 * k, step).unwrap(), -recon, "step {step} k {k}");
            rows += 1;
        }
        assert_eq!(rows, 2040);
        // The last level of each step's table, and the first past it,
        // which is the first of the next step's (spec/06 §5.1).
        for step in 1..255u32 {
            let n = 8192 / step;
            let r = |k: u32, b: u32| (k * b + b / 2 - if b > 1 { b & 1 } else { 0 }) as i32;
            assert_eq!(dequant(2 * n - 1, step).unwrap(), r(n, step));
            assert_eq!(dequant(2 * n + 1, step).unwrap(), r(1, step + 1));
        }
        assert!(dequant(0, 7).is_err());
    }

    /// The step bank is `clamp((base * scale) >> 8, 1, 255)` of the
    /// staged base matrices and scales (spec/06 §5.1).
    #[test]
    fn bank_inputs_equal_the_staged_tables() {
        use super::super::tables::{QUANT_BASE, QUANT_SCALE};
        let base = include_str!("../../../tests/data/iv50-tables/quant_base_1008cf00.csv");
        let mut rows = 0;
        for line in base.lines().skip(1) {
            let f: Vec<&str> = line.split(',').collect();
            let (g, c): (usize, usize) = (f[0].parse().unwrap(), f[1].parse().unwrap());
            let v: Vec<u8> = f[5..69].iter().map(|x| x.parse().unwrap()).collect();
            let n = if g == 5 { 16 } else { 64 };
            assert_eq!(f[2], if g == 5 { "4x4" } else { "8x8" });
            assert_eq!(&QUANT_BASE[g][c][..n], &v[..n], "group {g} class {c}");
            rows += 1;
        }
        assert_eq!(rows, 12);
        let scale = include_str!("../../../tests/data/iv50-tables/quant_scale_1008d200.csv");
        let mut rows = 0;
        for line in scale.lines().skip(1) {
            let f: Vec<&str> = line.split(',').collect();
            let (g, c, q): (usize, usize, usize) = (
                f[0].parse().unwrap(),
                f[1].parse().unwrap(),
                f[2].parse().unwrap(),
            );
            assert_eq!(QUANT_SCALE[g][c][q] as u32, f[5].parse::<u32>().unwrap());
            rows += 1;
        }
        assert_eq!(rows, 6 * 2 * 24);
    }

    /// The four scans equal the staged scan tables; the 4x4 table's
    /// raster index `n` is row `n >> 2`, column `n & 3` (spec/06 §2.5).
    #[test]
    fn scans_equal_the_staged_tables() {
        let csv = include_str!("../../../tests/data/iv50-tables/scan_tables_10098528.csv");
        let mut seen = [0usize; 4];
        for line in csv.lines().skip(1) {
            let f: Vec<&str> = line.split(',').collect();
            let (i, n): (usize, usize) = (f[2].parse().unwrap(), f[3].parse().unwrap());
            let (t, got) = match f[0] {
                "zigzag_8x8" => (0, SCAN_0[i] as usize),
                "column_8x8" => (1, SCAN_1[i] as usize),
                "raster_8x8" => (2, SCAN_2[i] as usize),
                "zigzag_4x4" if i < 16 => (3, SCAN_3[i] as usize),
                "zigzag_4x4" => continue,
                other => panic!("unknown table {other}"),
            };
            let want = if t == 3 { (n >> 2) * 8 + (n & 3) } else { n };
            assert_eq!(got, want, "{} entry {i}", f[0]);
            seen[t] += 1;
        }
        assert_eq!(seen, [64, 64, 64, 16]);
        // Transform types 0 to 4 take zigzag, column-major, raster,
        // raster and the 4x4 zigzag.
        assert_eq!(scan_for(0).unwrap(), &SCAN_0[..]);
        assert_eq!(scan_for(1).unwrap(), &SCAN_1[..]);
        assert_eq!(scan_for(2).unwrap(), &SCAN_2[..]);
        assert_eq!(scan_for(3).unwrap(), &SCAN_2[..]);
        assert_eq!(scan_for(4).unwrap(), &SCAN_3[..]);
        assert!(scan_for(5).is_err());
    }
}
