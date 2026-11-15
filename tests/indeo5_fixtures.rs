//! The staged IV50 fixtures of `docs/video/indeo/indeo5/fixtures/`
//! decoded with [`Iv50Decoder`] and compared with the vendor driver's
//! packed `YUY2` output: byte for byte for the two intra keyframes, by
//! SHA-256 for the eight frames of the vendor-encoded inter sequence
//! (the digests are those of the fixture's `expected.yuv`, one 50 688
//! byte frame each; its `notes.md` lists their first and last digits).

use oxideav_indeo::indeo5::{FrameType, Iv50Decoder};

mod sha256 {
    //! SHA-256 (FIPS 180-4), enough to pin plane digests in tests.

    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    fn block(h: &mut [u32; 8], chunk: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (slot, word) in w.iter_mut().zip(chunk.as_chunks::<4>().0) {
            *slot = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = *h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }

    /// The digest of `data` as 64 lower-case hex digits.
    pub fn hex(data: &[u8]) -> String {
        let mut h = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];
        let mut msg = data.to_vec();
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());
        for chunk in msg.as_chunks::<64>().0 {
            block(&mut h, chunk);
        }
        h.iter().map(|w| format!("{w:08x}")).collect()
    }

    #[test]
    fn known_digests() {
        assert_eq!(
            hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}

const EDUC: &[u8] = include_bytes!("data/intra-240x180-educ.iv50");
const EDUC_YUY2: &[u8] = include_bytes!("data/intra-240x180-educ.expected.yuy2");
const INDEO5: &[u8] = include_bytes!("data/intra-320x240-indeo5.iv50");
const INDEO5_YUY2: &[u8] = include_bytes!("data/intra-320x240-indeo5.expected.yuy2");
const INTER: &[u8] = include_bytes!("data/iv50-inter-176x144-8frame-vendor-enc/samples.bin");
const INTER_INDEX: &str =
    include_str!("data/iv50-inter-176x144-8frame-vendor-enc/samples-index.csv");

/// SHA-256 of each 176x144 `YUY2` frame of the inter fixture's
/// `expected.yuv`.
const INTER_YUY2_SHA256: [&str; 8] = [
    "10453b0b98c9bd05c9762c01c2ffee2f5f126dc2bdee41e139d714f5534ee2bb",
    "cf32dac7fc655cf3ac4d9e29c8d6a8cc552ea0e346597d9bdb747f2e00e6d429",
    "a0dc99bb906606e4ef421b6ee7a2e8a1c3551e75e615956faf98570306edc835",
    "14147652f4d306d2d4aab9f3bf632c7837fcbd93a5ca805be5d311e2138de2c8",
    "1d24fab0bd60007fc7788a750ae8c77cb92b260920ce67d30433194e60a92e41",
    "413d9c1f1e11b238c9dd0ab7aa28c3cfef39d55bf5fc1347acd1eb43d048d7d7",
    "441362cda0bca6beff020475ebd75b6f3c8e30b4eafaf82a63f55e745d454da7",
    "50ebe32cba3dfec956eea512ed34e6d79196271cb159a72ee426af87059b2a16",
];

/// The (offset, size, frame type, frame number) rows of the index.
fn inter_frames() -> Vec<(usize, usize, u32, u32)> {
    INTER_INDEX
        .lines()
        .skip(1)
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            (
                f[1].parse().unwrap(),
                f[2].parse().unwrap(),
                f[3].parse().unwrap(),
                f[4].parse().unwrap(),
            )
        })
        .collect()
}

/// The visible samples of the three planes, Y then U then V.
fn planes(dec: &Iv50Decoder) -> [Vec<u8>; 3] {
    [dec.y(), dec.u(), dec.v()].map(|p| {
        let p = p.unwrap();
        (0..p.height).flat_map(|r| p.row(r).to_vec()).collect()
    })
}

#[test]
fn intra_keyframes_equal_the_vendor_yuy2_output() {
    for (frame, want, w, h) in [(EDUC, EDUC_YUY2, 240, 180), (INDEO5, INDEO5_YUY2, 320, 240)] {
        let mut dec = Iv50Decoder::new();
        assert_eq!(dec.decode(frame), Ok(FrameType::Intra));
        assert_eq!(dec.dimensions(), Some((w, h)));
        let got = dec.yuy2().unwrap();
        assert_eq!(got.len(), want.len());
        let first = got.iter().zip(want).position(|(a, b)| a != b);
        assert_eq!(first, None, "{w}x{h}: first differing byte");
    }
}

#[test]
fn intra_keyframe_planes_are_pinned() {
    let mut dec = Iv50Decoder::new();
    dec.decode(INDEO5).unwrap();
    let [y, u, v] = planes(&dec);
    assert_eq!((y.len(), u.len(), v.len()), (320 * 240, 80 * 60, 80 * 60));
    assert_eq!(
        [sha256::hex(&y), sha256::hex(&u), sha256::hex(&v)],
        [
            "209998c91b4124393690b9cd3453bb1bd84ba4dd65d0b472a079dc22f44dbea9",
            "6ae6dc69b6187caf5193db33e93e0db484b9ba95a813594931bc9bf3662d790a",
            "75adb2ea5f8a1c9522e85effcfab8bb6c38c5408daa76dfcde318813584e6241",
        ]
    );
    // The 240x180 frame is black: luma 16, chroma 128 everywhere.
    let mut dec = Iv50Decoder::new();
    dec.decode(EDUC).unwrap();
    let [y, u, v] = planes(&dec);
    assert_eq!((y.len(), u.len(), v.len()), (240 * 180, 60 * 45, 60 * 45));
    assert!(y.iter().all(|&s| s == 16));
    assert!(u.iter().chain(&v).all(|&s| s == 128));
}

#[test]
fn inter_sequence_equals_the_vendor_yuy2_output() {
    let frames = inter_frames();
    assert_eq!(frames.len(), 8);
    let mut dec = Iv50Decoder::new();
    for (i, &(off, size, ftype, number)) in frames.iter().enumerate() {
        let data = &INTER[off..off + size];
        assert_eq!(u32::from(data[0] >> 5), ftype, "frame {i} type");
        assert_eq!(u32::from(data[1]), number, "frame {i} number");
        let want = if ftype == 0 {
            FrameType::Intra
        } else {
            FrameType::Inter
        };
        assert_eq!(dec.decode(data), Ok(want), "frame {i}");
        assert_eq!(dec.dimensions(), Some((176, 144)));
        let yuy2 = dec.yuy2().unwrap();
        assert_eq!(yuy2.len(), 176 * 144 * 2);
        assert_eq!(sha256::hex(&yuy2), INTER_YUY2_SHA256[i], "frame {i}");
    }
}

/// Digests of the decoder's own 8-bit planes for the inter sequence
/// (Y 176x144, U and V 44x36). The `YUY2` test above ties them to the
/// vendor output; these catch a change in one plane directly.
const INTER_PLANE_SHA256: [[&str; 3]; 8] = [
    [
        "8fe9288911f7abb43b8ad1fe945e4b9c9bed5960f611d134c16be4977be12165",
        "031c200b0cc1eaf5cf55d1e5e5c3fab738279cff631ef3376855244f626621c2",
        "d0a03096998cb17e1355210899f092ca1626c17bc52388f189b8a3ce054dc9ca",
    ],
    [
        "33413d182356605bfb67f04706acb9ffa2cf6bd7824c7add79efdb3688213064",
        "bc6fa41eb96b55ab2a99c33d39f530ef107aa3b9a27e779d784d1291f14dafe8",
        "334167c01fea79f7bbcfea9be6dc78a81013d235b5d4a11fcad88e817516f668",
    ],
    [
        "7c9874776a379bde4f106e0e9959845f1a5cbf65f728351b1324fe67423ea6a6",
        "b07de092c78d4954ca4a4f645eeaa4137891e9adf8e20cb78f4f4638841c85e9",
        "6c5231be0007a234eb60ed840e88a0efbbf8fc83299aabc6c4e10a7e1461b588",
    ],
    [
        "ab366df92ae8f65c3b2325d317909461cb1b973ee1203fe65651ac126b0f4f0a",
        "a171230a55acf9ef880b2060a841d16ae5f9f76714381b70b865f3a962eb15d5",
        "4f03fafff0c6b5cefcfcb693cf1f66609ebb5bc86393837ffad2ffa618b8629c",
    ],
    [
        "cc9ede687854c6e7b84303ca8fadce34d31db915cf082d466adaf04ffe52ea7a",
        "cfd9cf6c4fc02d70a58b1c4557d150025170eff9c2ec881c7f6377f08d25f55b",
        "7003ef963ee51123845741723ffaa304228b1cb1c7b24ff9139dd3b5f4bf84b1",
    ],
    [
        "97996be935c912caeedda7dda324c0329741ba58c9ef1e4409cfcadc6e180bf0",
        "122a098dc304e6040d5af09e591a916c841762ce8a5f6f9bf2666d40d21bee3c",
        "5eda160bf519b05a495b6d338967deee2ef69db37363b5b36ae54eb676664239",
    ],
    [
        "3126e7e58bc3f9c56bc168f61a773fa9752c16ef71356e16f9293907065a1b1b",
        "3c6ddc2e2b3f9612cb606df94d172076d9fbff56bee3ba43055a1b0c36b1347e",
        "97fed0dec242aa70ebff4e91c12fe54338d832a1554c35e0b9cf9f4faf8f0121",
    ],
    [
        "14bd5c2276ed85d8f8d4f8848c127fd3697649aaac220356a399c193922b86b8",
        "f4b111d4020da66d258d6be50c8e2d407578cc059988eede3a926af7e9ab6b7a",
        "14f2a5714aca69c535a5d9e5cb61c39d458ce2aa4c8917e84d3ef1e42ddf950d",
    ],
];

#[test]
fn inter_sequence_planes_are_pinned() {
    let mut dec = Iv50Decoder::new();
    for (i, &(off, size, ..)) in inter_frames().iter().enumerate() {
        dec.decode(&INTER[off..off + size]).unwrap();
        let [y, u, v] = planes(&dec);
        assert_eq!((y.len(), u.len(), v.len()), (176 * 144, 44 * 36, 44 * 36));
        let got = [sha256::hex(&y), sha256::hex(&u), sha256::hex(&v)];
        assert_eq!(got, INTER_PLANE_SHA256[i].map(String::from), "frame {i}");
    }
}

/// Decoding the sequence twice on one decoder gives the same pictures:
/// the intra frame resets every reference.
#[test]
fn inter_sequence_restarts_cleanly_at_the_intra_frame() {
    let frames = inter_frames();
    let mut dec = Iv50Decoder::new();
    for _ in 0..2 {
        for (i, &(off, size, ..)) in frames.iter().enumerate() {
            dec.decode(&INTER[off..off + size]).unwrap();
            assert_eq!(sha256::hex(&dec.yuy2().unwrap()), INTER_YUY2_SHA256[i]);
        }
    }
}

/// An inter frame decoded without its predecessor still decodes (the
/// reference is whatever the last frame left), and a fresh decoder
/// refuses it.
#[test]
fn inter_frame_needs_a_reference() {
    let frames = inter_frames();
    let (off, size, ..) = frames[1];
    let mut dec = Iv50Decoder::new();
    assert!(dec.decode(&INTER[off..off + size]).is_err());
    assert!(dec.y().is_none());
}
