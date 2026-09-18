//! Cross-check of the regenerated quantiser matrix bank against the
//! staged runtime image (`docs/video/indeo/indeo5/tables/
//! quant_matrices_1007b000.csv`, vendored as
//! `tests/data/iv50-quant-matrices-1007b000.csv`): every one of the
//! 288 `(group, class, quant)` matrices the crate derives from the two
//! on-disk tables through the `ICOpen`-time fill arithmetic must equal
//! the live bank the Validator round measured (`spec/06 §5.4`).

use oxideav_indeo::indeo5::{quant_matrix, QUANT_GROUPS, QUANT_LEVELS};

const CSV: &str = include_str!("data/iv50-quant-matrices-1007b000.csv");

#[test]
fn regenerated_bank_matches_staged_runtime_image() {
    let mut seen = 0;
    for line in CSV.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        assert!(cols.len() >= 70, "short row: {line}");
        let g: usize = cols[0].parse().unwrap();
        let c: usize = cols[1].parse().unwrap();
        let q: usize = cols[2].parse().unwrap();
        let matrix_index: usize = cols[3].parse().unwrap();
        assert_eq!(matrix_index, q + 24 * (c + 2 * g));
        let expected: Vec<u8> = cols[6..70].iter().map(|v| v.parse().unwrap()).collect();
        let ours = quant_matrix(g, c, q);
        assert_eq!(&ours[..], &expected[..], "matrix (g={g}, c={c}, q={q})");
        seen += 1;
    }
    assert_eq!(seen, QUANT_GROUPS * 2 * QUANT_LEVELS);
}
