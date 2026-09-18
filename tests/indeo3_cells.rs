//! Indeo 3 real-bitstream pixel tests over the cell decoder (r459).
//!
//! Both vendored `IV32` corpora (`tests/data/README.md`) carry a
//! black-box reference decode (`expected.yuv`, planar `yuv410p`: Y,
//! then U, then V). Every intra frame of both fixtures decodes
//! **pixel-exact on all three planes** through the cell-geometry
//! banks, the binary-tree walk and the mode-byte families A and E.

use oxideav_indeo::indeo3::{
    decode_plane, CodebookSeedArea, DyadDeltaTable, FrameHeader, PictureLayer, PlaneBuffers,
    PlaneContext, PlanePresence, StagingImage, VqArena,
};

const INTRA_BIN: &[u8] = include_bytes!("data/iv32-160x120-all-intra/samples.bin");
const INTRA_IDX: &str = include_str!("data/iv32-160x120-all-intra/samples-index.csv");
const INTRA_YUV: &[u8] = include_bytes!("data/iv32-160x120-all-intra/expected.yuv");
const INTER_BIN: &[u8] = include_bytes!("data/iv32-176x144-4frame-intra-period/samples.bin");
const INTER_IDX: &str = include_str!("data/iv32-176x144-4frame-intra-period/samples-index.csv");
const INTER_YUV: &[u8] = include_bytes!("data/iv32-176x144-4frame-intra-period/expected.yuv");

fn frames<'a>(bin: &'a [u8], idx: &str) -> Vec<&'a [u8]> {
    idx.lines()
        .skip(1)
        .map(|l| {
            let f: Vec<usize> = l.split(',').skip(1).map(|v| v.parse().unwrap()).collect();
            &bin[f[0]..f[0] + f[1]]
        })
        .collect()
}

/// Decode every intra frame of a fixture and return, per frame, the
/// exact-pixel count of each plane `(Y, V, U)` against the reference.
fn intra_frames_exact(
    bin: &[u8],
    idx: &str,
    yuv: &[u8],
    w: u32,
    h: u32,
) -> Vec<Option<[usize; 3]>> {
    let frames = frames(bin, idx);
    let staging = StagingImage::build(&CodebookSeedArea::load());
    let lut = DyadDeltaTable::load();
    let mut arena = VqArena::new();
    let (cw, ch) = (w / 4, h / 4);
    let frame_len = (w * h + 2 * cw * ch) as usize;
    let mut out = Vec::new();
    for (fi, frame) in frames.iter().enumerate() {
        let header = FrameHeader::parse(frame).expect("header");
        let pl = PictureLayer::parse(&header, frame).expect("picture layer");
        let is_intra = pl.planes.iter().all(|p| match p {
            PlanePresence::Present(pr) => pr.num_vectors == 0,
            _ => true,
        });
        if !is_intra {
            out.push(None);
            continue;
        }
        arena
            .apply_alt_quant(
                &staging,
                &header.bitstream.alt_quant,
                header.bitstream.cb_offset,
            )
            .unwrap();
        let exp = &yuv[fi * frame_len..(fi + 1) * frame_len];
        let mut counts = [0usize; 3];
        for (p, plane) in pl.planes.iter().enumerate() {
            let PlanePresence::Present(prelude) = plane else {
                panic!("plane {p} absent")
            };
            let payload = &frame[prelude.bitstream_offset..];
            let mut bufs = if p == 0 {
                PlaneBuffers::luma(w, h)
            } else {
                PlaneBuffers::chroma(w, h)
            };
            let ctx = PlaneContext {
                staging: &staging,
                arena: &arena,
                cb_offset: header.bitstream.cb_offset,
                lut: &lut,
                mvs: &prelude.motion_vectors,
                reference: None,
            };
            let stats = decode_plane(payload, &mut bufs, &ctx)
                .unwrap_or_else(|e| panic!("frame {fi} plane {p}: {e}"));
            assert!(stats.cells > 0);
            let (ow, oh) = if p == 0 { (w, h) } else { (cw, ch) };
            let ours = bufs.to_pixels(ow, oh);
            // yuv410p plane order is Y, U, V; the coded chroma order is
            // V (plane 1) then U (plane 2).
            let e: &[u8] = match p {
                0 => &exp[..(w * h) as usize],
                1 => &exp[(w * h + cw * ch) as usize..],
                _ => &exp[(w * h) as usize..(w * h + cw * ch) as usize],
            };
            counts[p] = ours.iter().zip(e).filter(|(a, b)| a == b).count();
        }
        out.push(Some(counts));
    }
    out
}

#[test]
fn all_intra_160x120_every_frame_pixel_exact() {
    let r = intra_frames_exact(INTRA_BIN, INTRA_IDX, INTRA_YUV, 160, 120);
    assert_eq!(r.len(), 8);
    for (i, f) in r.iter().enumerate() {
        assert_eq!(*f, Some([160 * 120, 40 * 30, 40 * 30]), "frame {i}");
    }
}

#[test]
fn intra_period_176x144_intra_frames_pixel_exact() {
    // Frames 0 and 4 are the intra frames (two luma strips: 160 + 16,
    // exercising the last-strip geometry bank); the inter frames are
    // the motion-compensation round's target.
    let r = intra_frames_exact(INTER_BIN, INTER_IDX, INTER_YUV, 176, 144);
    assert_eq!(r.len(), 8);
    for i in [0usize, 4] {
        assert_eq!(r[i], Some([176 * 144, 44 * 36, 44 * 36]), "frame {i}");
    }
    for i in [1usize, 2, 3, 5, 6, 7] {
        assert_eq!(r[i], None, "frame {i} is inter");
    }
}
