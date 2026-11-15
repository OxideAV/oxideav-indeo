//! Packed `YUY2` output from the 8-bit planes of a quarter-size-chroma
//! picture (`spec/08-output-reconstruction.md` §3.4a).
//!
//! The luma plane is copied. Each chroma plane is widened by two in
//! each direction with a truncating two-tap average, horizontally
//! first, and every resulting chroma row serves two output rows. A
//! chroma sample sits on luma column `4i`, row `4k`; the last chroma
//! column and row repeat instead of averaging with anything outside
//! the plane.

use super::Iv50Plane;

/// `⌊(a + b) / 2⌋` of two bytes.
#[inline(always)]
fn avg(a: u8, b: u8) -> u8 {
    ((a as u16 + b as u16) >> 1) as u8
}

/// Chroma row `k` of `p` widened to `pairs` samples: sample `m` is
/// chroma column `m / 2` for even `m` and the average of columns
/// `(m - 1) / 2` and `(m + 1) / 2` for odd `m`, columns clamped to the
/// plane (§3.4a, `h(m, k)`).
fn widen_row(p: &Iv50Plane, k: usize, pairs: usize, out: &mut [u8]) {
    let row = p.row(k.min(p.height - 1));
    let last = p.width - 1;
    for (m, o) in out[..pairs].iter_mut().enumerate() {
        let a = row[(m / 2).min(last)];
        *o = if m & 1 == 0 {
            a
        } else {
            avg(a, row[(m / 2 + 1).min(last)])
        };
    }
}

/// Pack the planes as `Y0 U Y1 V`, top row first, `2 * ceil(w / 2)`
/// bytes per luma pair row. `u` and `v` must be the quarter-size planes
/// of `y`.
pub(crate) fn pack(y: &Iv50Plane, u: &Iv50Plane, v: &Iv50Plane) -> Vec<u8> {
    let (w, h) = (y.width, y.height);
    let pairs = w.div_ceil(2);
    let mut out = Vec::with_capacity(pairs * 4 * h);
    // Widened chroma rows k and k + 1 of each plane.
    let mut rows = vec![0u8; 4 * pairs];
    let (u0, rest) = rows.split_at_mut(pairs);
    let (u1, rest) = rest.split_at_mut(pairs);
    let (v0, v1) = rest.split_at_mut(pairs);
    let mut loaded = usize::MAX;
    for (r, line) in (0..h).map(|r| (r, y.row(r))) {
        let j = r >> 1;
        let k = j >> 1;
        if k != loaded {
            widen_row(u, k, pairs, u0);
            widen_row(u, k + 1, pairs, u1);
            widen_row(v, k, pairs, v0);
            widen_row(v, k + 1, pairs, v1);
            loaded = k;
        }
        let between = j & 1 == 1;
        for m in 0..pairs {
            let y0 = line[2 * m];
            let y1 = line[(2 * m + 1).min(w - 1)];
            let (cu, cv) = if between {
                (avg(u0[m], u1[m]), avg(v0[m], v1[m]))
            } else {
                (u0[m], v0[m])
            };
            out.extend_from_slice(&[y0, cu, y1, cv]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(data: &[u8], width: usize, height: usize) -> Iv50Plane<'_> {
        Iv50Plane {
            width,
            height,
            stride: width,
            data,
        }
    }

    /// An 8 by 8 picture with 2 by 2 chroma: columns 0, avg, 1, 1 and
    /// rows k, k, avg, avg, then the last chroma row repeated.
    #[test]
    fn chroma_is_interpolated_horizontally_then_vertically() {
        let luma: Vec<u8> = (0..64).collect();
        let u = [10u8, 21, 40, 81];
        let v = [200u8, 100, 50, 25];
        let out = pack(&plane(&luma, 8, 8), &plane(&u, 2, 2), &plane(&v, 2, 2));
        assert_eq!(out.len(), 8 * 8 * 2);
        let at = |r: usize, m: usize| (out[(r * 4 + m) * 4 + 1], out[(r * 4 + m) * 4 + 3]);
        // Rows 0 and 1: chroma row 0 widened: 10, (10 + 21) >> 1, 21, 21.
        for r in [0, 1] {
            assert_eq!(
                [at(r, 0).0, at(r, 1).0, at(r, 2).0, at(r, 3).0],
                [10, 15, 21, 21]
            );
            assert_eq!(
                [at(r, 0).1, at(r, 1).1, at(r, 2).1, at(r, 3).1],
                [200, 150, 100, 100]
            );
        }
        // Rows 2 and 3: average of the widened rows 0 and 1; the
        // horizontal average comes first: (15 + ((40 + 81) >> 1)) >> 1.
        for r in [2, 3] {
            assert_eq!([at(r, 0).0, at(r, 1).0, at(r, 2).0], [25, 37, 51]);
        }
        // Rows 4 and 5: chroma row 1; rows 6 and 7: row 1 averaged with
        // its repeat.
        for r in [4, 5, 6, 7] {
            assert_eq!(
                [at(r, 0).0, at(r, 1).0, at(r, 2).0, at(r, 3).0],
                [40, 60, 81, 81]
            );
        }
        // Luma is copied.
        for r in 0..8 {
            for c in 0..8 {
                assert_eq!(out[(r * 8 + c) * 2], (r * 8 + c) as u8);
            }
        }
    }

    #[test]
    fn odd_width_repeats_the_last_luma_sample() {
        let luma = [1u8, 2, 3, 4, 5, 6];
        let c = [7u8];
        let out = pack(&plane(&luma, 3, 2), &plane(&c, 1, 1), &plane(&c, 1, 1));
        assert_eq!(out, [1, 7, 2, 7, 3, 7, 3, 7, 4, 7, 5, 7, 6, 7, 6, 7]);
    }
}
