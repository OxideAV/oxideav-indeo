//! Inverse transforms (`spec/06-slant-inverse-transform.md` §1 and §2;
//! rounding biases in `tables/transform_rounding_10098500`).
//!
//! All arithmetic is on 16-bit values with wrapping. The code computes in
//! `i32` and cuts to 16 bits before every right shift and at the end:
//! additions, subtractions and multiplications commute with the cut, so
//! only the shifts need the 16-bit value.

/// Cut to 16 bits, two's complement.
#[inline(always)]
fn w(v: i32) -> i32 {
    v as i16 as i32
}

/// The eight-point transform of spec/06 §1.2, with `b` added to `x[0]`
/// first (0 for a first pass, 1 for the pass that ends in the halving,
/// §1.3).
#[inline(always)]
fn slant8(x: [i32; 8], b: i32) -> [i32; 8] {
    let x0 = x[0] + b;
    let t0 = w(7 * x[1] - 4 * x[3] + 4) >> 3;
    let t1 = w(4 * x[1] + 7 * x[3] + 4) >> 3;
    let e0 = x0 + t0;
    let e1 = x0 - t0;
    let p = e0 + (x[4] + x[5]);
    let q = e0 - (x[4] + x[5]);
    let s_ = e1 + (x[4] - x[5]);
    let r = e1 - (x[4] - x[5]);
    let u = t1 + x[2];
    let s = t1 - x[2];
    let ww = x[7] - x[6];
    let v = x[7] + x[6];
    let a = w(5 * u + 2 * ww + 2) >> 2;
    let bb = w(2 * u - 5 * ww + 2) >> 2;
    let c = w(5 * s + 2 * v + 2) >> 2;
    let d = w(2 * s - 5 * v + 2) >> 2;
    [p + a, q + bb, q - bb, p - a, s_ + c, r + d, r - d, s_ - c]
}

/// The four-point transform with bias `b`: the `(5, 2)` rotation stage
/// alone (spec/06 §2.3 names the kernel family; its arithmetic is
/// written out here, not there).
#[inline(always)]
fn slant4(x: [i32; 4], b: i32) -> [i32; 4] {
    let x0 = x[0] + b;
    let a = w(5 * x[1] + 2 * x[3] + 2) >> 2;
    let bb = w(2 * x[1] - 5 * x[3] + 2) >> 2;
    let e0 = x0 + x[2];
    let e1 = x0 - x[2];
    [e0 + a, e1 + bb, e1 - bb, e0 - a]
}

/// The final scaling of the last pass: cut to 16 bits and shift right
/// by one (spec/06 §1.3).
#[inline(always)]
fn fin(v: i32) -> i16 {
    (w(v) >> 1) as i16
}

/// Apply transform type `t` (0 to 4) to the coefficient array `c`
/// (8-wide, row-major) and write the block to `out` at `off` with row
/// pitch `pitch`.
pub(crate) fn inverse(t: u32, c: &[i16; 64], out: &mut [i16], off: usize, pitch: usize) {
    match t {
        0 => slant8x8(c, out, off, pitch),
        1 => {
            // Rows only.
            for r in 0..8 {
                let x = std::array::from_fn(|k| c[r * 8 + k] as i32);
                let y = slant8(x, 1);
                let o = &mut out[off + r * pitch..off + r * pitch + 8];
                for k in 0..8 {
                    o[k] = fin(y[k]);
                }
            }
        }
        2 => {
            // Columns only.
            for k in 0..8 {
                let x = std::array::from_fn(|r| c[r * 8 + k] as i32);
                let y = slant8(x, 1);
                for r in 0..8 {
                    out[off + r * pitch + k] = fin(y[r]);
                }
            }
        }
        3 => {
            for r in 0..8 {
                out[off + r * pitch..off + r * pitch + 8].copy_from_slice(&c[r * 8..r * 8 + 8]);
            }
        }
        _ => slant4x4(c, out, off, pitch),
    }
}

fn slant8x8(c: &[i16; 64], out: &mut [i16], off: usize, pitch: usize) {
    // Columns with b = 0, cut to 16 bits.
    let mut m = [0i32; 64];
    for k in 0..8 {
        let x = std::array::from_fn(|r| c[r * 8 + k] as i32);
        let y = slant8(x, 0);
        for r in 0..8 {
            m[r * 8 + k] = w(y[r]);
        }
    }
    // Rows with b = 1, then the final shift.
    for r in 0..8 {
        let x = std::array::from_fn(|k| m[r * 8 + k]);
        let y = slant8(x, 1);
        let o = &mut out[off + r * pitch..off + r * pitch + 8];
        for k in 0..8 {
            o[k] = fin(y[k]);
        }
    }
}

fn slant4x4(c: &[i16; 64], out: &mut [i16], off: usize, pitch: usize) {
    let mut m = [0i32; 16];
    for k in 0..4 {
        let x = std::array::from_fn(|r| c[r * 8 + k] as i32);
        let y = slant4(x, 0);
        for r in 0..4 {
            m[r * 4 + k] = w(y[r]);
        }
    }
    for r in 0..4 {
        let x = std::array::from_fn(|k| m[r * 4 + k]);
        let y = slant4(x, 1);
        let o = &mut out[off + r * pitch..off + r * pitch + 4];
        for k in 0..4 {
            o[k] = fin(y[k]);
        }
    }
}

/// Fill an `n` by `n` block with `v`.
#[inline]
pub(crate) fn fill(out: &mut [i16], off: usize, pitch: usize, n: usize, v: i16) {
    for r in 0..n {
        out[off + r * pitch..off + r * pitch + n].fill(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dc_only_8x8_is_constant() {
        for d in [-300i16, -1, 0, 1, 5, 200, 32767] {
            let mut c = [0i16; 64];
            c[0] = d;
            let mut out = vec![0i16; 64];
            inverse(0, &c, &mut out, 0, 8);
            let want = (d.wrapping_add(1)) >> 1;
            assert!(out.iter().all(|&v| v == want), "d = {d}");
            let mut out4 = vec![0i16; 16];
            inverse(4, &c, &mut out4, 0, 4);
            assert!(out4.iter().all(|&v| v == want), "4x4 d = {d}");
        }
    }

    #[test]
    fn row_transform_touches_only_its_row() {
        let mut c = [0i16; 64];
        c[0] = 10;
        let mut out = vec![0i16; 64];
        inverse(1, &c, &mut out, 0, 8);
        assert!(out[..8].iter().all(|&v| v == 5));
        assert!(out[8..].iter().all(|&v| v == 0));
    }

    #[test]
    fn eight_point_matches_formulas() {
        // x1 = 8: t0 = (56 + 4) >> 3 = 7, t1 = (32 + 4) >> 3 = 4;
        // u = s = 4: a = (20 + 2) >> 2 = 5, bb = (8 + 2) >> 2 = 2,
        // c = 5, d = 2.
        let y = slant8([0, 8, 0, 0, 0, 0, 0, 0], 0);
        assert_eq!(y, [12, 9, 5, 2, -2, -5, -9, -12]);
    }

    /// The recurrence of spec/06 §1.2, written from its text.
    fn spec_slant8(x: [i16; 8], bias: i16) -> [i16; 8] {
        let x0 = x[0].wrapping_add(bias);
        let m = |a: i16, k: i16| a.wrapping_mul(k);
        let a = m(x[1], 4).wrapping_add(m(x[3], 7)).wrapping_add(4) >> 3;
        let b = m(x[1], 7).wrapping_sub(m(x[3], 4)).wrapping_add(4) >> 3;
        let (t, u) = (a.wrapping_add(x[2]), a.wrapping_sub(x[2]));
        let (p, q) = (x0.wrapping_add(b), x0.wrapping_sub(b));
        let (r, s) = (x[4].wrapping_add(x[5]), x[4].wrapping_sub(x[5]));
        let (v, w) = (x[7].wrapping_add(x[6]), x[7].wrapping_sub(x[6]));
        let c = m(t, 5).wrapping_add(m(w, 2)).wrapping_add(2) >> 2;
        let d = m(t, 2).wrapping_sub(m(w, 5)).wrapping_add(2) >> 2;
        let e = m(u, 5).wrapping_add(m(v, 2)).wrapping_add(2) >> 2;
        let f = m(u, 2).wrapping_sub(m(v, 5)).wrapping_add(2) >> 2;
        let (pr, pmr) = (p.wrapping_add(r), p.wrapping_sub(r));
        let (qs, qms) = (q.wrapping_add(s), q.wrapping_sub(s));
        [
            pr.wrapping_add(c),
            pmr.wrapping_add(d),
            pmr.wrapping_sub(d),
            pr.wrapping_sub(c),
            qs.wrapping_add(e),
            qms.wrapping_add(f),
            qms.wrapping_sub(f),
            qs.wrapping_sub(e),
        ]
    }

    /// A small deterministic generator for test vectors.
    fn lcg(state: &mut u32) -> u32 {
        *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        *state >> 8
    }

    /// The eight-point transform equals the 16-bit wrapping recurrence
    /// of spec/06 §1.2 for small and for overflowing inputs (§1.5).
    #[test]
    fn eight_point_equals_the_wrapping_recurrence() {
        let mut st = 12345;
        for round in 0..4000 {
            let range = if round % 2 == 0 { 600 } else { 65536 };
            let x: [i16; 8] = std::array::from_fn(|_| {
                ((lcg(&mut st) % range) as i32 - (range / 2) as i32) as i16
            });
            for bias in [0, 1] {
                let got = slant8(x.map(i32::from), bias).map(|v| v as i16);
                assert_eq!(got, spec_slant8(x, bias as i16), "{x:?}");
            }
        }
    }

    /// Unit coefficients scaled so that every shift is exact reproduce
    /// the staged basis (`tables/slant_basis_8.csv`: index, denominator,
    /// eight samples, sign changes).
    #[test]
    fn eight_point_reproduces_the_staged_basis() {
        let csv = include_str!("../../../tests/data/iv50-tables/slant_basis_8.csv");
        let mut rows = 0;
        for line in csv.lines().skip(1) {
            let f: Vec<i32> = line.split(',').map(|x| x.parse().unwrap()).collect();
            let (k, den) = (f[0] as usize, f[1]);
            let mut x = [0i32; 8];
            x[k] = 32 * den;
            let y = slant8(x, 0);
            let want: Vec<i32> = f[2..10].iter().map(|b| 32 * b).collect();
            assert_eq!(&y[..], &want[..], "coefficient {k}");
            let changes = y.windows(2).filter(|p| (p[0] < 0) != (p[1] < 0)).count() as i32;
            assert_eq!(changes, f[10]);
            rows += 1;
        }
        assert_eq!(rows, 8);
    }

    /// The 2D transform is the vertical pass, then the horizontal pass
    /// with the DC bias and the halving (spec/06 §1.3, §2.2), for every
    /// single coefficient and for dense blocks.
    #[test]
    fn two_dimensional_transform_is_columns_then_rows() {
        let reference = |c: &[i16; 64]| -> Vec<i16> {
            let mut m = [0i16; 64];
            for k in 0..8 {
                let y = spec_slant8(std::array::from_fn(|r| c[r * 8 + k]), 0);
                for r in 0..8 {
                    m[r * 8 + k] = y[r];
                }
            }
            let mut out = vec![0i16; 64];
            for r in 0..8 {
                let y = spec_slant8(std::array::from_fn(|k| m[r * 8 + k]), 1);
                for k in 0..8 {
                    out[r * 8 + k] = y[k] >> 1;
                }
            }
            out
        };
        let mut st = 99;
        let mut blocks: Vec<[i16; 64]> = (0..64)
            .map(|p| {
                let mut c = [0i16; 64];
                c[p] = 64 * (p as i16 + 1);
                c
            })
            .collect();
        for round in 0..200 {
            let range = if round < 150 { 2048 } else { 65536 };
            blocks.push(std::array::from_fn(|_| {
                ((lcg(&mut st) % range) as i32 - (range / 2) as i32) as i16
            }));
        }
        for c in &blocks {
            // A pitch wider than the block: the kernel must stay inside
            // its eight columns.
            let mut out = vec![7i16; 8 * 11];
            inverse(0, c, &mut out, 0, 11);
            let want = reference(c);
            for r in 0..8 {
                assert_eq!(&out[r * 11..r * 11 + 8], &want[r * 8..r * 8 + 8]);
                if r < 7 {
                    assert_eq!(&out[r * 11 + 8..r * 11 + 11], &[7, 7, 7]);
                }
            }
        }
    }

    /// The one-axis transforms are the second-pass form alone (bias and
    /// halving, spec/06 §2.3) along rows (type 1) or columns (type 2),
    /// and type 3 copies the coefficients.
    #[test]
    fn one_axis_transforms_run_the_final_pass_only() {
        let mut st = 7;
        for _ in 0..200 {
            let c: [i16; 64] = std::array::from_fn(|_| (lcg(&mut st) % 4096) as i16 - 2048);
            let mut rows = vec![0i16; 64];
            inverse(1, &c, &mut rows, 0, 8);
            let mut cols = vec![0i16; 64];
            inverse(2, &c, &mut cols, 0, 8);
            let mut copy = vec![0i16; 64];
            inverse(3, &c, &mut copy, 0, 8);
            for i in 0..8 {
                let y = spec_slant8(std::array::from_fn(|k| c[i * 8 + k]), 1);
                let x = spec_slant8(std::array::from_fn(|r| c[r * 8 + i]), 1);
                for j in 0..8 {
                    assert_eq!(rows[i * 8 + j], y[j] >> 1);
                    assert_eq!(cols[j * 8 + i], x[j] >> 1);
                }
            }
            assert_eq!(&copy[..], &c[..]);
        }
    }

    /// The four-point transform is the `(5, 2)` rotation stage alone,
    /// vertical pass first, with the same bias and halving.
    #[test]
    fn four_point_transform_matches_its_formulas() {
        let f4 = |x: [i16; 4], bias: i16| -> [i16; 4] {
            let x0 = x[0].wrapping_add(bias);
            let m = |a: i16, k: i16| a.wrapping_mul(k);
            let a = m(x[1], 5).wrapping_add(m(x[3], 2)).wrapping_add(2) >> 2;
            let b = m(x[1], 2).wrapping_sub(m(x[3], 5)).wrapping_add(2) >> 2;
            let (e0, e1) = (x0.wrapping_add(x[2]), x0.wrapping_sub(x[2]));
            [
                e0.wrapping_add(a),
                e1.wrapping_add(b),
                e1.wrapping_sub(b),
                e0.wrapping_sub(a),
            ]
        };
        let mut st = 5;
        for _ in 0..300 {
            let mut c = [0i16; 64];
            for r in 0..4 {
                for k in 0..4 {
                    c[r * 8 + k] = (lcg(&mut st) % 2048) as i16 - 1024;
                }
            }
            let mut m = [0i16; 16];
            for k in 0..4 {
                let y = f4(std::array::from_fn(|r| c[r * 8 + k]), 0);
                for r in 0..4 {
                    m[r * 4 + k] = y[r];
                }
            }
            let mut out = vec![0i16; 4 * 6];
            inverse(4, &c, &mut out, 0, 6);
            for r in 0..4 {
                let y = f4(std::array::from_fn(|k| m[r * 4 + k]), 1);
                for k in 0..4 {
                    assert_eq!(out[r * 6 + k], y[k] >> 1);
                }
            }
        }
    }
}
