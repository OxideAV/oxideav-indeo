//! Band recombination into the 8-bit output planes (spec 08).

use super::band::BandGeom;

/// A window in plane samples: first row, first column, height, width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Window {
    pub y: usize,
    pub x: usize,
    pub h: usize,
    pub w: usize,
}

/// An 8-bit output plane with room for the columns and rows the
/// recombination writes past the visible picture (spec 08, 8.3).
#[derive(Debug, Clone)]
pub(crate) struct OutPlane {
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub data: Vec<u8>,
}

impl OutPlane {
    pub fn new(width: usize, height: usize) -> OutPlane {
        let stride = width.next_multiple_of(32) + 32;
        let rows = height.next_multiple_of(2) + 2;
        OutPlane {
            width,
            height,
            stride,
            data: vec![0; stride * rows],
        }
    }
}

/// One band plane (spec 08, 8.5): `pixel = clamp(s, -128, 127) + 128`
/// over the window, columns widened to multiples of 16.
pub(crate) fn one_band(area: &[i16], b: &BandGeom, win: Window, out: &mut OutPlane) {
    let x0 = win.x & !15;
    let w = (15 + win.x + win.w - x0) & !15;
    // Never write past the band's pitch or the output plane.
    let w = w.min(b.pitch - x0).min(out.stride - x0);
    for row in win.y..win.y + win.h {
        if row >= b.height.next_multiple_of(16) || row >= out.data.len() / out.stride {
            break;
        }
        let src = &area[b.offset + row * b.pitch + x0..][..w];
        let dst = &mut out.data[row * out.stride + x0..][..w];
        for (d, &s) in dst.iter_mut().zip(src) {
            *d = (s.clamp(-128, 127) + 128) as u8;
        }
    }
}

/// Four band plane (spec 08, 8.6): the DLL's integer synthesis filter
/// over the window, with rows started on an even row and columns widened
/// to multiples of 32.
pub(crate) fn four_band(area: &[i16], bands: &[BandGeom], win: Window, out: &mut OutPlane) {
    let mut y = win.y;
    let mut h = win.h;
    h += y & 1;
    y &= !1;
    let x0 = win.x & !31;
    let wpp = (31 + win.x + win.w - x0) & !31;
    let pitch = bands[0].pitch;
    let nbr = h.div_ceil(2);
    let nbc = (wpp / 2).min(pitch.saturating_sub(x0 / 2));
    let wpp = wpp.min(out.stride - x0);
    if nbr == 0 || nbc == 0 {
        return;
    }
    let br0 = y / 2;
    let bc0 = x0 / 2;
    let (lo, vo, ho, dof) = (
        bands[0].offset as isize,
        bands[1].offset as isize,
        bands[2].offset as isize,
        bands[3].offset as isize,
    );
    let p = pitch as isize;
    let at = |base: isize, r: isize, c: isize| -> i32 {
        let i = base + r * p + c;
        if i >= 0 && (i as usize) < area.len() {
            area[i as usize] as i32
        } else {
            0
        }
    };
    let out_rows = out.data.len() / out.stride;
    for i in 0..nbr {
        let r = (br0 + i) as isize;
        let last_row = i == nbr - 1;
        let rp = if i == 0 { r } else { r - 1 };
        let rn_c = if last_row { r } else { r + 1 };
        let rn_m = if last_row { r - 1 } else { r + 1 };
        let orow = y + 2 * i;
        if orow + 1 >= out_rows {
            break;
        }
        for j in 0..nbc {
            let c = (bc0 + j) as isize;
            let cl = if j == 0 { c } else { c - 1 };
            let cr = c + 1;
            // Band L.
            let l = at(lo, r, c);
            let lr = at(lo, r, cr);
            let ln = at(lo, rn_c, c);
            let lnr = at(lo, rn_c, cr);
            let mut ee = 16 * l;
            let mut eo = 8 * (l + lr);
            let mut oe = 8 * (l + ln);
            let mut oo = 4 * (l + lr + ln + lnr);
            // Band V (vertical high pass).
            let s1 = |b: isize| at(vo, r, b) + at(vo, rp, b);
            let t1 = |b: isize| at(vo, rp, b) - 6 * at(vo, r, b) + at(vo, rn_m, b);
            let (s1c, s1r, t1c, t1r) = (s1(c), s1(cr), t1(c), t1(cr));
            ee += 8 * s1c;
            eo += 4 * (s1c + s1r);
            oe += 4 * t1c;
            oo += 2 * (t1c + t1r);
            // Band H (horizontal high pass).
            let s2 = |a: isize| at(ho, a, c) + at(ho, a, cl);
            let t2 = |a: isize| at(ho, a, cl) - 6 * at(ho, a, c) + at(ho, a, cr);
            let (s2r, s2n, t2r, t2n) = (s2(r), s2(rn_c), t2(r), t2(rn_c));
            ee += 8 * s2r;
            eo += 4 * t2r;
            oe += 4 * (s2r + s2n);
            oo += 2 * (t2r + t2n);
            // Band D.
            let ds = |b: isize| at(dof, r, b) + at(dof, rp, b);
            let dt = |b: isize| at(dof, rp, b) + at(dof, rn_m, b);
            let de = |b: isize| at(dof, r, b);
            let (scl, sc, scr) = (ds(cl), ds(c), ds(cr));
            let (tcl, tc, tcr) = (dt(cl), dt(c), dt(cr));
            let (ecl, ec, ecr) = (de(cl), de(c), de(cr));
            ee += 4 * (scl + sc);
            eo += 2 * (scl - 6 * sc + scr);
            oe += 2 * (tcl + tc - 6 * (ecl + ec));
            oo += (tcl - 6 * tc + tcr) - 6 * (ecl - 6 * ec + ecr);
            let px = |s: i32| (((s as i16) >> 6) as i32 + 128).clamp(0, 255) as u8;
            let col = x0 + 2 * j;
            if col + 1 >= out.stride {
                break;
            }
            let e = orow * out.stride + col;
            out.data[e] = px(ee);
            out.data[e + 1] = px(eo);
            let o = e + out.stride;
            out.data[o] = px(oe);
            out.data[o + 1] = px(oo);
        }
        // The last written column takes its left neighbour's value.
        let last = x0 + wpp - 1;
        for rr in [orow, orow + 1] {
            let b = rr * out.stride;
            out.data[b + last] = out.data[b + last - 1];
        }
    }
}
