//! Whole-decoder tests on streams written by [`super::testutil`]:
//! half-size chroma, motion compensation in its four sample phases,
//! inherited vectors and the quantiser range, each checked against a
//! model computed in the test from the specification's formulas.

use super::band::{build_bank, dequant};
use super::tables::SCAN_0;
use super::testutil::*;
use super::*;

/// The plane byte of band sample `v` (spec/08 §3.0).
fn px(v: i32) -> u8 {
    (v.clamp(-128, 127) + 128) as u8
}

/// The quantiser step of group `g`, class `cls`, level `q` at
/// coefficient position `pos` (spec/06 §5.1).
fn step(g: usize, cls: usize, q: usize, pos: usize) -> u32 {
    build_bank()[g][cls][q][pos] as u32
}

/// A coded macroblock whose blocks each hold a DC level.
fn dc_mb(levels: &[i32]) -> Mb {
    Mb {
        cbp: (1 << levels.len()) - 1,
        blocks: levels.iter().map(|&k| vec![(1, k)]).collect(),
        ..Default::default()
    }
}

/// The constants of DC-only intra blocks in decode order: the DC values
/// accumulate over the tile and a block holds half the sum, rounded up.
fn dc_consts(levels: &[i32], step: u32) -> Vec<i32> {
    let mut sum = 0;
    levels
        .iter()
        .map(|&k| {
            sum += dequant(level_index(k), step).unwrap();
            (sum + 1) >> 1
        })
        .collect()
}

/// The visible rows of a plane.
fn rows(p: Iv50Plane) -> Vec<Vec<u8>> {
    (0..p.height).map(|r| p.row(r).to_vec()).collect()
}

/// Compare every visible sample of `plane` with the plane byte of the
/// band sample `want(x, y)`.
fn assert_samples(plane: Iv50Plane, what: &str, want: impl Fn(usize, usize) -> i32) {
    for (y, row) in rows(plane).iter().enumerate() {
        let expect: Vec<u8> = (0..row.len()).map(|x| px(want(x, y))).collect();
        assert_eq!(row, &expect, "{what} row {y}");
    }
}

/// A band image of `n` by `n` blocks of `blk` samples, block (bx, by)
/// holding `consts[order(bx, by)]`, as band samples in a `pitch`-wide
/// buffer; `mb` is the macroblock size in blocks (1 or 2), which fixes
/// the decode order.
fn block_image(consts: &[i32], n: usize, blk: usize, mb: usize, pitch: usize) -> Vec<i32> {
    let mut img = vec![0; pitch * n * blk];
    let mut k = 0;
    for my in 0..n / mb {
        for mx in 0..n / mb {
            for j in 0..mb * mb {
                let (bx, by) = (mx * mb + j % mb, my * mb + j / mb);
                for y in 0..blk {
                    for x in 0..blk {
                        img[(by * blk + y) * pitch + bx * blk + x] = consts[k];
                    }
                }
                k += 1;
            }
        }
    }
    img
}

/// Band sample `i` of a reference buffer; samples before and after it
/// read zero (the margins around a plane's bands).
fn at(img: &[i32], i: isize) -> i32 {
    if i >= 0 && (i as usize) < img.len() {
        img[i as usize]
    } else {
        0
    }
}

/// The motion-compensated prediction of sample `i` for a vector in half
/// samples (`half`) or whole samples: the truncating average of the two
/// or four neighbours in 16-bit arithmetic (spec/07 §5.2, §5.3).
fn predict(img: &[i32], pitch: usize, i: usize, mvx: i32, mvy: i32, half: bool) -> i32 {
    let (ix, iy, fx, fy) = if half {
        (mvx >> 1, mvy >> 1, mvx & 1, mvy & 1)
    } else {
        (mvx, mvy, 0, 0)
    };
    let p = pitch as isize;
    let s = i as isize + iy as isize * p + ix as isize;
    let w = |v: i32| v as i16 as i32;
    match (fx, fy) {
        (0, 0) => at(img, s),
        (1, 0) => w(at(img, s) + at(img, s + 1)) >> 1,
        (0, 1) => w(at(img, s) + at(img, s + p)) >> 1,
        _ => w(at(img, s) + at(img, s + 1) + at(img, s + p) + at(img, s + p + 1)) >> 2,
    }
}

/// A 16x16 intra frame with half-size chroma: one luma macroblock and
/// one 8x8 macroblock of four 4x4 blocks per chroma plane.
fn half_chroma_intra() -> (Gop, Vec<u8>, [Vec<i32>; 3]) {
    let gop = Gop {
        flags: 2,
        chroma: 4,
        ..Gop::simple(16, 16)
    };
    let levels = [[3, -2, 5, 1], [4, 1, -3, 2], [-1, -1, 6, 2]];
    let bands: Vec<Band> = levels
        .iter()
        .map(|l| Band::new(true, vec![dc_mb(l)]))
        .collect();
    let data = frame(
        0,
        0,
        Some(&gop),
        0,
        &[Some(&bands[0])],
        Some(&bands[1]),
        Some(&bands[2]),
    );
    let consts = [
        dc_consts(&levels[0], step(0, 1, 0, 0)),
        dc_consts(&levels[1], step(5, 1, 0, 0)),
        dc_consts(&levels[2], step(5, 1, 0, 0)),
    ];
    (gop, data, consts)
}

/// Half-size chroma (gop_flags bit 1, spec/02 §1.6): the chroma planes
/// are `(size + 1) >> 1`, decoded like any one-band plane with the 4x4
/// matrix group.
#[test]
fn half_size_chroma_intra_frame() {
    let (_, data, consts) = half_chroma_intra();
    let mut dec = Iv50Decoder::new();
    assert_eq!(dec.decode(&data), Ok(FrameType::Intra));
    assert_eq!(dec.dimensions(), Some((16, 16)));
    assert_eq!(dec.chroma_ratio(), Some(2));
    assert_eq!(dec.yuy2(), None);
    let (y, u, v) = (dec.y().unwrap(), dec.u().unwrap(), dec.v().unwrap());
    assert_eq!((u.width, u.height, v.width, v.height), (8, 8, 8, 8));
    let luma = block_image(&consts[0], 2, 8, 2, 16);
    for (r, row) in rows(y).iter().enumerate() {
        let want: Vec<u8> = (0..16).map(|c| px(luma[r * 16 + c])).collect();
        assert_eq!(row, &want, "luma row {r}");
    }
    // The first chroma band of the stream is V.
    for (plane, c) in [(v, &consts[1]), (u, &consts[2])] {
        let img = block_image(c, 2, 4, 2, 8);
        for (r, row) in rows(plane).iter().enumerate() {
            let want: Vec<u8> = (0..8).map(|x| px(img[r * 8 + x])).collect();
            assert_eq!(row, &want, "chroma row {r}");
        }
    }
    // The planes differ, so the test tells them apart.
    assert_ne!(consts[1], consts[2]);
}

/// Odd luma sizes round the half-size chroma planes up, and the default
/// layout rounds quarter-size ones up.
#[test]
fn chroma_plane_sizes() {
    for (flags, chroma, w, h, cw, ch) in [
        (2, 4, 18, 10, 9, 5),
        (2, 4, 17, 9, 9, 5),
        (0, 6, 18, 10, 5, 3),
        (0, 6, 17, 9, 5, 3),
    ] {
        let gop = Gop {
            flags,
            chroma,
            ..Gop::simple(w, h)
        };
        // Two luma macroblocks, both skipped; empty chroma bands.
        let luma = Band::new(
            true,
            vec![
                Mb {
                    skip: true,
                    ..Default::default()
                };
                2
            ],
        );
        let data = frame(0, 0, Some(&gop), 0, &[Some(&luma)], None, None);
        let mut dec = Iv50Decoder::new();
        assert_eq!(dec.decode(&data), Ok(FrameType::Intra));
        let (u, v) = (dec.u().unwrap(), dec.v().unwrap());
        assert_eq!((u.width, u.height), (cw, ch));
        assert_eq!((v.width, v.height), (cw, ch));
        assert_eq!(dec.chroma_ratio(), Some(if flags == 2 { 2 } else { 4 }));
        // An empty first band of a plane is zero samples.
        assert!(rows(u).iter().chain(&rows(v)).flatten().all(|&s| s == 128));
        assert!(rows(dec.y().unwrap()).iter().flatten().all(|&s| s == 128));
    }
}

/// A chroma band that inherits (band_flags bit 1) takes the vector of
/// the luma macroblock scaled to its macroblock size: halved for 8x8
/// chroma macroblocks under 16x16 luma ones, halves away from zero.
#[test]
fn half_size_chroma_inherits_halved_vectors() {
    for (dy, dx, cy, cx) in [(2, 4, 1, 2), (-3, 3, -2, 2), (5, -1, 3, -1)] {
        let (_, intra, consts) = half_chroma_intra();
        let mut dec = Iv50Decoder::new();
        dec.decode(&intra).unwrap();
        let luma = Band::new(
            true,
            vec![Mb {
                inter: true,
                mv: (dy, dx),
                ..Default::default()
            }],
        );
        let chroma = Band {
            flags: 2,
            ..Band::new(
                true,
                vec![Mb {
                    skip: true,
                    ..Default::default()
                }],
            )
        };
        let data = frame(1, 1, None, 0, &[Some(&luma)], Some(&chroma), Some(&chroma));
        assert_eq!(dec.decode(&data), Ok(FrameType::Inter));
        // Luma: whole-sample vector (dx, dy) over a 16-wide buffer.
        let img = block_image(&consts[0], 2, 8, 2, 16);
        for (r, row) in rows(dec.y().unwrap()).iter().enumerate() {
            let want: Vec<u8> = (0..16)
                .map(|c| px(predict(&img, 16, r * 16 + c, dx, dy, false)))
                .collect();
            assert_eq!(row, &want, "luma row {r} vector ({dx}, {dy})");
        }
        // Chroma: an 8x8 band in a 16-wide buffer, vector (cx, cy).
        for (plane, c) in [
            (dec.v().unwrap(), &consts[1]),
            (dec.u().unwrap(), &consts[2]),
        ] {
            let mut img = vec![0; 16 * 8];
            let small = block_image(c, 2, 4, 2, 8);
            for y in 0..8 {
                img[y * 16..y * 16 + 8].copy_from_slice(&small[y * 8..y * 8 + 8]);
            }
            for (r, row) in rows(plane).iter().enumerate() {
                let want: Vec<u8> = (0..8)
                    .map(|x| px(predict(&img, 16, r * 16 + x, cx, cy, false)))
                    .collect();
                assert_eq!(row, &want, "chroma row {r} vector ({cx}, {cy})");
            }
        }
    }
}

/// DC levels of the 16 luma blocks of the 32x32 reference picture, in
/// decode order.
const REF_LEVELS: [i32; 16] = [5, -3, 9, 2, -7, 4, 11, -6, 3, 8, -10, 1, 6, -2, 7, -9];

/// A 32x32 intra frame with `luma_info` as the luma band info and empty
/// chroma bands, and its luma band image (pitch 32).
fn reference_32(luma_info: u32) -> (Gop, Vec<u8>, Vec<i32>) {
    let gop = Gop {
        luma: vec![luma_info],
        ..Gop::simple(32, 32)
    };
    let mbs: Vec<Mb> = REF_LEVELS.chunks(4).map(dc_mb).collect();
    let data = frame(
        0,
        0,
        Some(&gop),
        0,
        &[Some(&Band::new(true, mbs))],
        None,
        None,
    );
    let consts = dc_consts(&REF_LEVELS, step(0, 1, 0, 0));
    (gop, data, block_image(&consts, 4, 8, 2, 32))
}

/// Motion compensation in whole and half samples (spec/07 §5.2): the
/// four macroblocks of a 32x32 picture take vectors with every phase,
/// coded as deltas against the previous macroblock's vector (spec/07
/// §3.2), and one block adds a residual (spec/07 §5.5).
#[test]
fn motion_compensation_phases() {
    // Vectors (mvx, mvy) in half samples: horizontal half, vertical half
    // with a negative odd x, both halves, whole samples.
    let vectors = [(1, 0), (-3, 1), (3, -5), (-8, -12)];
    let (_, intra, img) = reference_32(1);
    let mut dec = Iv50Decoder::new();
    dec.decode(&intra).unwrap();
    for (r, row) in rows(dec.y().unwrap()).iter().enumerate() {
        let want: Vec<u8> = (0..32).map(|c| px(img[r * 32 + c])).collect();
        assert_eq!(row, &want, "reference row {r}");
    }
    let mut prev = (0, 0);
    let mut mbs = Vec::new();
    for (k, &(mvx, mvy)) in vectors.iter().enumerate() {
        let mut mb = Mb {
            inter: true,
            mv: (mvy - prev.1, mvx - prev.0),
            ..Default::default()
        };
        if k == 1 {
            // Block 0 of the second macroblock: a DC residual.
            mb.cbp = 1;
            mb.blocks = vec![vec![(1, 3)]];
        }
        prev = (mvx, mvy);
        mbs.push(mb);
    }
    let data = frame(1, 1, None, 0, &[Some(&Band::new(true, mbs))], None, None);
    assert_eq!(dec.decode(&data), Ok(FrameType::Inter));
    // An inter block's DC is not predicted: the block holds half of it.
    let residual = (dequant(level_index(3), step(0, 0, 0, 0)).unwrap() + 1) >> 1;
    assert_samples(dec.y().unwrap(), "luma", |x, y| {
        let k = (y / 16) * 2 + x / 16;
        let (mvx, mvy) = vectors[k];
        let want = predict(&img, 32, y * 32 + x, mvx, mvy, true);
        if k == 1 && y < 8 && x < 24 {
            want + residual
        } else {
            want
        }
    });
    assert_ne!(residual, 0);
}

/// The same vectors in a band with whole-sample vectors (`mv_res` 0,
/// spec/02 §1.7) move by twice as far and never interpolate.
#[test]
fn whole_sample_vectors() {
    let vectors = [(1, 0), (-3, 1), (3, -5), (-8, -12)];
    let (_, intra, img) = reference_32(0);
    let mut dec = Iv50Decoder::new();
    dec.decode(&intra).unwrap();
    let mut prev = (0, 0);
    let mbs: Vec<Mb> = vectors
        .iter()
        .map(|&(mvx, mvy)| {
            let mb = Mb {
                inter: true,
                mv: (mvy - prev.1, mvx - prev.0),
                ..Default::default()
            };
            prev = (mvx, mvy);
            mb
        })
        .collect();
    let data = frame(1, 1, None, 0, &[Some(&Band::new(true, mbs))], None, None);
    dec.decode(&data).unwrap();
    assert_samples(dec.y().unwrap(), "luma", |x, y| {
        let (mvx, mvy) = vectors[(y / 16) * 2 + x / 16];
        predict(&img, 32, y * 32 + x, mvx, mvy, false)
    });
}

/// An intra macroblock inside an inter frame (type bit clear, spec/06
/// §5.2) restarts from its own data: coded blocks continue the tile's
/// DC prediction from zero, and a skipped macroblock copies the
/// reference.
#[test]
fn intra_and_skipped_macroblocks_in_an_inter_frame() {
    let (_, intra, img) = reference_32(1);
    let mut dec = Iv50Decoder::new();
    dec.decode(&intra).unwrap();
    let levels = [12, -4, 3, 3];
    let mbs = vec![
        Mb {
            skip: true,
            ..Default::default()
        },
        dc_mb(&levels),
        Mb {
            inter: true,
            ..Default::default()
        },
        Mb {
            skip: true,
            ..Default::default()
        },
    ];
    let data = frame(1, 1, None, 0, &[Some(&Band::new(true, mbs))], None, None);
    assert_eq!(dec.decode(&data), Ok(FrameType::Inter));
    let consts = dc_consts(&levels, step(0, 1, 0, 0));
    assert_samples(dec.y().unwrap(), "luma", |x, y| {
        if y < 16 && x >= 16 {
            consts[(y / 8) * 2 + (x - 16) / 8]
        } else {
            img[y * 32 + x]
        }
    });
}

/// A band quantiser of 24 to 31 (spec/06 §5.1): an inter macroblock
/// takes the intra step table of level `q - 24`; an intra macroblock has
/// no table and its coded blocks are refused.
#[test]
fn band_quantiser_above_23() {
    for gq in [24usize, 27, 31] {
        let (_, intra, img) = reference_32(0);
        let mut dec = Iv50Decoder::new();
        dec.decode(&intra).unwrap();
        // Block 0: a DC level; block 1: one coefficient at scan position
        // 2 (run 3 from the start), which is row 1, column 0.
        let coded = Mb {
            inter: true,
            cbp: 3,
            blocks: vec![vec![(1, 2)], vec![(3, -1)]],
            ..Default::default()
        };
        let rest = Mb {
            skip: true,
            ..Default::default()
        };
        let band = Band {
            gq: gq as u32,
            ..Band::new(true, vec![coded, rest.clone(), rest.clone(), rest.clone()])
        };
        let data = frame(1, 1, None, 0, &[Some(&band)], None, None);
        assert_eq!(dec.decode(&data), Ok(FrameType::Inter));
        let mut residual = vec![0i16; 16 * 8];
        let mut c = [0i16; 64];
        c[0] = dequant(level_index(2), step(0, 1, gq - 24, 0)).unwrap() as i16;
        xform::inverse(0, &c, &mut residual, 0, 16);
        let mut c = [0i16; 64];
        let pos = SCAN_0[2] as usize;
        assert_eq!(pos, 8);
        c[pos] = dequant(level_index(-1), step(0, 1, gq - 24, pos)).unwrap() as i16;
        xform::inverse(0, &c, &mut residual, 8, 16);
        assert_samples(dec.y().unwrap(), "luma", |x, y| {
            if y < 8 && x < 16 {
                img[y * 32 + x] + residual[y * 16 + x] as i32
            } else {
                img[y * 32 + x]
            }
        });
        assert!(residual.iter().any(|&v| v != 0));

        // The same data in an intra macroblock.
        let coded = Mb {
            cbp: 1,
            blocks: vec![vec![(1, 2)]],
            ..Default::default()
        };
        let band = Band {
            gq: gq as u32,
            ..Band::new(true, vec![coded, rest.clone(), rest.clone(), rest.clone()])
        };
        let data = frame(1, 2, None, 0, &[Some(&band)], None, None);
        assert!(matches!(dec.decode(&data), Err(Iv50Error::Unsupported(_))));
        // Uncoded intra blocks need no table.
        let plain = Mb::default();
        let band = Band {
            gq: gq as u32,
            ..Band::new(
                true,
                vec![plain.clone(), plain.clone(), plain.clone(), plain],
            )
        };
        let data = frame(1, 3, None, 0, &[Some(&band)], None, None);
        assert_eq!(dec.decode(&data), Ok(FrameType::Inter));
    }
}

/// Run and escape coding (spec/05 §2.2; spec/04 §5.5): coefficients at
/// chosen scan positions with levels inside and outside the run/value
/// table come out at the scan's raster positions with the level's
/// reconstruction value.
#[test]
fn runs_and_escapes_place_coefficients() {
    let gop = Gop::simple(16, 16);
    // Block 0: positions 0, 1, 5 and 63; block 1: an escaped level and
    // an escaped run; blocks 2 and 3 uncoded.
    let b0 = vec![(1, 4), (1, -2), (4, 1), (58, -1)];
    let b1 = vec![(1, 300), (40, 2)];
    let mb = Mb {
        cbp: 3,
        blocks: vec![b0.clone(), b1.clone()],
        ..Default::default()
    };
    let data = frame(
        0,
        0,
        Some(&gop),
        0,
        &[Some(&Band::new(true, vec![mb]))],
        None,
        None,
    );
    let mut dec = Iv50Decoder::new();
    assert_eq!(dec.decode(&data), Ok(FrameType::Intra));
    let mut band = vec![0i16; 16 * 16];
    let mut sum = 0;
    for (j, block) in [b0, b1].iter().enumerate() {
        let mut c = [0i16; 64];
        let mut pos = -1i32;
        for &(run, level) in block {
            pos += run as i32;
            let p = SCAN_0[pos as usize] as usize;
            c[p] = dequant(level_index(level), step(0, 1, 0, p)).unwrap() as i16;
        }
        // DC prediction over the coded blocks.
        sum += c[0] as i32;
        c[0] = sum as i16;
        xform::inverse(0, &c, &mut band, j * 8, 16);
    }
    // Uncoded intra blocks hold half the running DC sum.
    for y in 8..16 {
        band[y * 16..y * 16 + 16].fill(((sum + 1) >> 1) as i16);
    }
    for (r, row) in rows(dec.y().unwrap()).iter().enumerate() {
        let want: Vec<u8> = (0..16).map(|c| px(band[r * 16 + c] as i32)).collect();
        assert_eq!(row, &want, "row {r}");
    }
}

/// Per-macroblock quantiser deltas (band_flags bit 2, spec/06 §5.2):
/// the step table is the one of `band_glob_quant + delta`.
#[test]
fn macroblock_quantiser_deltas() {
    let gop = Gop::simple(32, 16);
    let deltas = [3, -2];
    let mbs: Vec<Mb> = deltas
        .iter()
        .map(|&delta| Mb {
            cbp: 1,
            delta,
            blocks: vec![vec![(1, 5), (1, 3)]],
            ..Default::default()
        })
        .collect();
    let band = Band {
        flags: 4,
        gq: 10,
        ..Band::new(true, mbs)
    };
    let data = frame(0, 0, Some(&gop), 0, &[Some(&band)], None, None);
    let mut dec = Iv50Decoder::new();
    assert_eq!(dec.decode(&data), Ok(FrameType::Intra));
    let mut img = vec![0i16; 32 * 16];
    let mut sum = 0;
    for (k, &delta) in deltas.iter().enumerate() {
        let q = (10 + delta) as usize;
        let mut c = [0i16; 64];
        c[0] = dequant(level_index(5), step(0, 1, q, 0)).unwrap() as i16;
        c[1] = dequant(level_index(3), step(0, 1, q, 1)).unwrap() as i16;
        sum += c[0] as i32;
        c[0] = sum as i16;
        xform::inverse(0, &c, &mut img, k * 16, 32);
        let fill = ((sum + 1) >> 1) as i16;
        for y in 0..16 {
            for x in 0..16 {
                if y >= 8 || x >= 8 {
                    img[y * 32 + k * 16 + x] = fill;
                }
            }
        }
    }
    for (r, row) in rows(dec.y().unwrap()).iter().enumerate() {
        let want: Vec<u8> = (0..32).map(|c| px(img[r * 32 + c] as i32)).collect();
        assert_eq!(row, &want, "row {r}");
    }
    // The two macroblocks used different steps.
    assert_ne!(step(0, 1, 13, 1), step(0, 1, 8, 1));
}

/// A 64x32 intra frame with four luma bands of 32x16 (two 16x16
/// macroblocks each): band 0 holds the DC levels `dc`, and with
/// `detail` the other three bands carry coefficients through their own
/// transforms (rows, columns, none).
fn four_band_intra(dc: [i32; 8], detail: bool) -> Vec<u8> {
    let gop = Gop {
        decomp: 1,
        luma: vec![0; 4],
        ..Gop::simple(64, 32)
    };
    let low = Band::new(true, dc.chunks(4).map(dc_mb).collect());
    let skipped = Band::new(true, vec![Mb::default(); 2]);
    let busy = |k: i32| {
        let mb = Mb {
            cbp: 0b1010,
            blocks: vec![vec![(1, k), (3, -1), (9, 2)], vec![(2, -k), (60, 1)]],
            ..Default::default()
        };
        Band {
            gq: 4,
            ..Band::new(true, vec![mb.clone(), mb])
        }
    };
    let high: Vec<Band> = (1..4).map(busy).collect();
    let luma: Vec<Option<&Band>> = if detail {
        std::iter::once(Some(&low))
            .chain(high.iter().map(Some))
            .collect()
    } else {
        vec![Some(&low), Some(&skipped), Some(&skipped), Some(&skipped)]
    };
    frame(0, 0, Some(&gop), 0, &luma, None, None)
}

/// Four-band luma with only the low band set: a flat low band of
/// sample `c` gives a flat picture of `(16 * c) >> 6` (the synthesis
/// accumulator holds 64 times the sample, spec/08 §3.0, and the low
/// band enters it with weight 16).
#[test]
fn four_band_flat_low_band() {
    // A first block of level 9 and no further change.
    let mut dec = Iv50Decoder::new();
    let levels = [9, 0, 0, 0, 0, 0, 0, 0];
    // Level 0 is not a coefficient: code only the first block.
    let gop = Gop {
        decomp: 1,
        luma: vec![0; 4],
        ..Gop::simple(64, 32)
    };
    let first = Mb {
        cbp: 1,
        blocks: vec![vec![(1, levels[0])]],
        ..Default::default()
    };
    let low = Band::new(true, vec![first, Mb::default()]);
    let rest = Band::new(true, vec![Mb::default(); 2]);
    let data = frame(
        0,
        0,
        Some(&gop),
        0,
        &[Some(&low), Some(&rest), Some(&rest), Some(&rest)],
        None,
        None,
    );
    assert_eq!(dec.decode(&data), Ok(FrameType::Intra));
    assert_eq!(dec.dimensions(), Some((64, 32)));
    // Group 1 is the low band's matrix group in a four-band plane
    // (spec/06 §5.1).
    let c = dc_consts(&[9], step(1, 1, 0, 0))[0];
    assert_ne!(c, 0);
    assert_samples(dec.y().unwrap(), "luma", |_, _| (16 * c) >> 6);
}

/// The synthetic streams of this module with every bit flipped and at
/// every length: the paths the staged frames do not reach (four bands
/// with one-axis transforms, half-size chroma, inherited vectors,
/// escapes) must return as well.
#[test]
fn corrupted_synthetic_streams_return() {
    let (_, half_intra, _) = half_chroma_intra();
    let inherit = {
        let luma = Band::new(
            true,
            vec![Mb {
                inter: true,
                cbp: 5,
                mv: (-3, 7),
                blocks: vec![vec![(1, 2)], vec![(1, 300), (40, 2)]],
                ..Default::default()
            }],
        );
        let chroma = Band {
            flags: 2,
            ..Band::new(
                true,
                vec![Mb {
                    cbp: 9,
                    blocks: vec![vec![(1, 1)], vec![(2, -1)]],
                    ..Default::default()
                }],
            )
        };
        frame(
            1,
            1,
            None,
            0x80,
            &[Some(&luma)],
            Some(&chroma),
            Some(&chroma),
        )
    };
    let (_, ref32, _) = reference_32(1);
    let moving = {
        let mb = Mb {
            inter: true,
            cbp: 6,
            mv: (3, -5),
            blocks: vec![vec![(1, 4), (1, -2)], vec![(64, 1)]],
            ..Default::default()
        };
        frame(
            3,
            1,
            None,
            0,
            &[Some(&Band::new(true, vec![mb; 4]))],
            None,
            None,
        )
    };
    let four = four_band_intra([5, -3, 9, 2, -7, 4, 11, -6], true);
    let sequences: [Vec<&[u8]>; 3] = [
        vec![&half_intra, &inherit],
        vec![&ref32, &moving],
        vec![&four],
    ];
    for seq in &sequences {
        // The clean sequence decodes.
        let mut dec = Iv50Decoder::new();
        for f in seq {
            dec.decode(f).unwrap();
        }
        for (k, target) in seq.iter().enumerate() {
            for cut in 0..target.len() {
                let mut dec = Iv50Decoder::new().with_max_pixels(1 << 16);
                for f in &seq[..k] {
                    dec.decode(f).unwrap();
                }
                let _ = dec.decode(&target[..cut]);
                let _ = dec.yuy2();
            }
            let mut dec = Iv50Decoder::new().with_max_pixels(1 << 16);
            for bit in 0..target.len() * 8 {
                if bit % 32 == 0 {
                    for f in &seq[..k] {
                        // A corrupted GOP header may have changed the
                        // layout; the clean frames restore it.
                        let _ = dec.decode(f);
                    }
                }
                let mut work = target.to_vec();
                work[bit / 8] ^= 1 << (bit % 8);
                if k > 0 {
                    work[1] = work[1].wrapping_add((bit % 200) as u8 + 1);
                }
                let _ = dec.decode(&work);
                for p in [dec.y(), dec.u(), dec.v()].into_iter().flatten() {
                    let _ = p.row(p.height - 1);
                }
            }
        }
    }
}

/// Decoding is a function of the frames alone: two decoders fed the
/// same sequence hold the same planes, also after an error in between.
#[test]
fn decoding_is_deterministic_across_decoders() {
    let four = four_band_intra([5, -3, 9, 2, -7, 4, 11, -6], true);
    let (_, ref32, _) = reference_32(1);
    let mut a = Iv50Decoder::new();
    let mut b = Iv50Decoder::new();
    a.decode(&four).unwrap();
    // `b` first sees another layout and a broken frame.
    b.decode(&ref32).unwrap();
    assert!(b.decode(&four[..four.len() - 2]).is_err());
    b.decode(&four).unwrap();
    for (pa, pb) in [(a.y(), b.y()), (a.u(), b.u()), (a.v(), b.v())] {
        assert_eq!(rows(pa.unwrap()), rows(pb.unwrap()));
    }
    // The detail bands change the picture.
    let mut flat = Iv50Decoder::new();
    flat.decode(&four_band_intra([5, -3, 9, 2, -7, 4, 11, -6], false))
        .unwrap();
    assert_ne!(rows(a.y().unwrap()), rows(flat.y().unwrap()));
}

/// A half-size chroma band whose macroblocks are not half the luma
/// macroblock size has no one-to-one luma macroblock to inherit from:
/// refused, while the same band decodes when it carries its own data.
#[test]
fn half_size_chroma_inheritance_needs_matching_grids() {
    let gop = Gop {
        flags: 2,
        chroma: 6, // 4x4 macroblocks under 16x16 luma ones
        ..Gop::simple(16, 16)
    };
    let luma = Band::new(true, vec![Mb::default()]);
    let own = Band::new(false, vec![Mb::default(); 4]);
    let intra = frame(0, 0, Some(&gop), 0, &[Some(&luma)], Some(&own), Some(&own));
    let mut dec = Iv50Decoder::new();
    assert_eq!(dec.decode(&intra), Ok(FrameType::Intra));
    let moving = Band::new(
        true,
        vec![Mb {
            inter: true,
            mv: (1, 1),
            ..Default::default()
        }],
    );
    let inherit = Band {
        flags: 2,
        ..Band::new(false, vec![Mb::default(); 4])
    };
    let data = frame(
        1,
        1,
        None,
        0,
        &[Some(&moving)],
        Some(&inherit),
        Some(&inherit),
    );
    assert_eq!(
        dec.decode(&data),
        Err(Iv50Error::Unsupported(
            "half-size chroma inheriting across different macroblock grids"
        ))
    );
    // The failed frame left the picture and the references alone.
    let own_inter = Band::new(
        false,
        vec![
            Mb {
                inter: true,
                ..Default::default()
            };
            4
        ],
    );
    let data = frame(
        1,
        2,
        None,
        0,
        &[Some(&moving)],
        Some(&own_inter),
        Some(&own_inter),
    );
    assert_eq!(dec.decode(&data), Ok(FrameType::Inter));
}

/// The reference of each frame type (spec/07 §1.2, §1.3, §1.5): a type
/// 1 frame predicts from the last type 0 or 1 frame and replaces it; a
/// type 2 frame predicts from the last type 2 frame since then (else
/// from that type 0 or 1 frame) and becomes the reference of later type
/// 2 and 3 frames only; a type 3 frame predicts like a type 2 frame and
/// is never a reference.
#[test]
fn droppable_frames_keep_their_own_reference_chain() {
    let gop = Gop::simple(16, 16);
    let intra = frame(
        0,
        0,
        Some(&gop),
        0,
        &[Some(&Band::new(true, vec![dc_mb(&[40])]))],
        None,
        None,
    );
    let mut dec = Iv50Decoder::new();
    assert_eq!(dec.decode(&intra), Ok(FrameType::Intra));
    // The whole picture is the first block's constant.
    let base = dc_consts(&[40], step(0, 1, 0, 0))[0];
    assert_samples(dec.y().unwrap(), "intra", |_, _| base);

    // The model: the sample of block 0 in the last type 0 or 1 frame,
    // and in the last type 2 frame since.
    let mut main = base;
    let mut scal: Option<i32> = None;
    let types = [1, 2, 3, 3, 2, 3, 1, 3, 2, 2, 3, 1, 1, 3];
    for (n, &ftype) in types.iter().enumerate() {
        // Every frame adds its own residual to block 0 of its reference.
        let level = 2 + n as i32;
        let mb = Mb {
            inter: true,
            cbp: 1,
            blocks: vec![vec![(1, level)]],
            ..Default::default()
        };
        let data = frame(
            ftype,
            n as u32 + 1,
            None,
            0,
            &[Some(&Band::new(true, vec![mb]))],
            None,
            None,
        );
        let want_type = match ftype {
            1 => FrameType::Inter,
            2 => FrameType::DroppableInterScalability,
            _ => FrameType::DroppableInter,
        };
        assert_eq!(dec.decode(&data), Ok(want_type), "frame {n}");
        let residual = (dequant(level_index(level), step(0, 0, 0, 0)).unwrap() + 1) >> 1;
        let reference = if ftype == 1 {
            main
        } else {
            scal.unwrap_or(main)
        };
        let now = reference + residual;
        match ftype {
            1 => {
                main = now;
                scal = None;
            }
            2 => scal = Some(now),
            _ => {}
        }
        // Block 0 holds the sum; the other blocks never change.
        assert_samples(dec.y().unwrap(), "inter", |x, y| {
            if x < 8 && y < 8 { now } else { base }
        });
    }
}

/// With `IV50_WRITE_SEEDS` set to a directory, write this module's
/// synthetic sequences there in the fuzz target's framing (each frame
/// behind its 16-bit little-endian length). The files under
/// `fuzz/seeds/indeo5_decoder/synthetic-*.seq` come from here.
#[test]
fn synthetic_sequences_as_fuzz_seeds() {
    let (_, half_intra, _) = half_chroma_intra();
    let (_, ref32, _) = reference_32(1);
    let mv = |ftype: u32, number: u32, dy: i32, dx: i32| {
        let mb = Mb {
            inter: true,
            cbp: 6,
            mv: (dy, dx),
            blocks: vec![vec![(1, 4), (1, -2)], vec![(64, 1)]],
            ..Default::default()
        };
        frame(
            ftype,
            number,
            None,
            0,
            &[Some(&Band::new(true, vec![mb; 4]))],
            None,
            None,
        )
    };
    let inherit = {
        let luma = Band::new(
            true,
            vec![Mb {
                inter: true,
                mv: (-3, 7),
                ..Default::default()
            }],
        );
        let chroma = Band {
            flags: 2,
            ..Band::new(
                true,
                vec![Mb {
                    cbp: 9,
                    blocks: vec![vec![(1, 1)], vec![(2, -1)]],
                    ..Default::default()
                }],
            )
        };
        frame(
            1,
            1,
            None,
            0x80,
            &[Some(&luma)],
            Some(&chroma),
            Some(&chroma),
        )
    };
    let seeds: [(&str, Vec<Vec<u8>>); 3] = [
        ("half-chroma", vec![half_intra, inherit]),
        (
            "motion",
            vec![
                ref32,
                mv(1, 1, 3, -5),
                mv(2, 2, -1, 1),
                mv(3, 3, 0, 2),
                mv(1, 4, 1, 0),
            ],
        ),
        (
            "four-band",
            vec![four_band_intra([5, -3, 9, 2, -7, 4, 11, -6], true)],
        ),
    ];
    for (name, frames) in &seeds {
        // Every sequence decodes.
        let mut dec = Iv50Decoder::new();
        let mut packed = Vec::new();
        for f in frames {
            dec.decode(f).unwrap();
            packed.extend_from_slice(&(f.len() as u16).to_le_bytes());
            packed.extend_from_slice(f);
        }
        if let Some(dir) = std::env::var_os("IV50_WRITE_SEEDS") {
            let path = std::path::Path::new(&dir).join(format!("synthetic-{name}.seq"));
            std::fs::write(path, &packed).unwrap();
        }
    }
}
