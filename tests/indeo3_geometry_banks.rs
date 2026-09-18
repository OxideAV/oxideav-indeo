//! Cross-check of the regenerated Indeo 3 cell-geometry banks against
//! the staged populator output (`docs/video/indeo/indeo3/tables/
//! 04-cell-geometry-banks.csv`, vendored as
//! `tests/data/iv32-cell-geometry-banks.csv`): four geometries
//! (160×120, 176×144, 320×240, 400×300) × four banks (chroma / luma,
//! full-strip / last-strip) × heap indices 1..255, all five sub-tables.
//! The 176×144 and 160×120 banks were measured byte-for-byte equal to
//! the vendor's after `ICDecompressBegin` (`spec/04 §5.3`).

use oxideav_indeo::indeo3::{GeometryBank, PlaneBanks};

const CSV: &str = include_str!("data/iv32-cell-geometry-banks.csv");

fn bank<'a>(
    luma: &'a PlaneBanks,
    chroma: &'a PlaneBanks,
    plane: &str,
    which: &str,
) -> &'a GeometryBank {
    let p = if plane == "luma" { luma } else { chroma };
    if which == "full" {
        &p.full
    } else {
        &p.last
    }
}

#[test]
fn regenerated_banks_match_staged_populator_output() {
    let mut rows = 0;
    let mut cache: Vec<((u32, u32), PlaneBanks, PlaneBanks)> = Vec::new();
    for line in CSV.lines().skip(1) {
        let c: Vec<&str> = line.split(',').collect();
        assert_eq!(c.len(), 11, "row: {line}");
        let w: u32 = c[0].parse().unwrap();
        let h: u32 = c[1].parse().unwrap();
        if !cache.iter().any(|(g, _, _)| *g == (w, h)) {
            cache.push(((w, h), PlaneBanks::luma(w, h), PlaneBanks::chroma(w, h)));
        }
        let (_, luma, chroma) = cache.iter().find(|(g, _, _)| *g == (w, h)).unwrap();
        let b = bank(luma, chroma, c[2], c[3]);
        let index: usize = c[5].parse().unwrap();
        let expected: (u8, u8, u8, u32, u32) = (
            c[6].parse().unwrap(),
            c[7].parse().unwrap(),
            c[8].parse().unwrap(),
            c[9].parse().unwrap(),
            c[10].parse().unwrap(),
        );
        let got = (
            b.h4[index],
            b.w4[index],
            b.strip[index],
            b.ypos[index],
            b.xpos[index],
        );
        assert_eq!(
            got, expected,
            "{w}x{h} {} {} index {index}: (h4, w4, strip, ypos, xpos)",
            c[2], c[3]
        );
        rows += 1;
    }
    assert_eq!(rows, 4 * 4 * 255);
}
