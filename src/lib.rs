//! Pure-Rust **Intel Indeo** video codec family.
//!
//! One crate covers the whole family — Indeo 2 (`RT21` / `IV20`,
//! codec id `indeo2`), Indeo 3 (`IV31` / `IV32`, codec id `indeo3`),
//! Indeo 4 (`IV41` / `IV42`, codec id `indeo4`), and Indeo 5 (`IV50`,
//! codec id `indeo5`) — exposed through one shared registration entry
//! point. Each generation lives in its own `v2` / `v3` / `v4` / `v5`
//! module behind a thin dispatcher so adding the next version is a
//! purely additive change to this file.
//!
//! ## Status
//!
//! - **Indeo 2** — fully decoded. Frame-header parser per
//!   `docs/video/indeo/indeo2/indeo2-trace-reverse-engineering.md`,
//!   little-endian bit reader for the entropy payload, the 143-entry
//!   canonical Huffman codebook (§8.1), the four 256-byte delta /
//!   palette tables (§8.3), and the intra-row-0 / intra-row-≥1 /
//!   inter pair-or-run plane decoder (§§3.6–3.7). Y-plane output is
//!   bit-exact against ffmpeg's reference Indeo 2 decoder for the
//!   in-tree `VPAR0019.AVI` fixture corpus.
//! - **Indeo 3 / 4 / 5** — not yet wired in; their `v3` / `v4` / `v5`
//!   modules and `register()` entries land in subsequent rounds via
//!   this same crate's public API.

#![deny(clippy::needless_range_loop)]

pub mod common;
pub mod v2;

use oxideav_core::{CodecCapabilities, CodecId, CodecInfo, CodecRegistry, CodecTag};

/// Stable codec-id string for Indeo 2 (currently the only fully
/// decoded generation in this crate).
///
/// Matches FFmpeg's lower-case codec name. The AVI demuxer in
/// `oxideav-avi` already maps the `RT21` / `IV20` four-character codes
/// to this id once registered.
pub const CODEC_ID_INDEO2: &str = "indeo2";

/// Stable codec-id string for Indeo 3 — defined here for forward
/// compatibility with the AVI codec map; **not** yet registered by
/// [`register`].
pub const CODEC_ID_INDEO3: &str = "indeo3";

/// Stable codec-id string for Indeo 4 — defined here for forward
/// compatibility with the AVI codec map; **not** yet registered by
/// [`register`].
pub const CODEC_ID_INDEO4: &str = "indeo4";

/// Stable codec-id string for Indeo 5 — defined here for forward
/// compatibility with the AVI codec map; **not** yet registered by
/// [`register`].
pub const CODEC_ID_INDEO5: &str = "indeo5";

/// Register every Indeo decoder this crate currently ships.
///
/// **Currently registers Indeo 2 only.** Round 3 will add an
/// additional `reg.register(...)` for `indeo3`, round 4 for `indeo4`,
/// and round 5 for `indeo5` — all routed through this same function
/// so callers never need to think about which Indeo generation they
/// want.
pub fn register(reg: &mut CodecRegistry) {
    register_indeo2(reg);
    // register_indeo3(reg);  // round 3 — pending docs/video/indeo/indeo3/
    // register_indeo4(reg);  // round 4 — pending docs/video/indeo/indeo4/
    // register_indeo5(reg);  // round 5 — pending docs/video/indeo/indeo5/
}

/// Standalone registration for the Indeo 2 decoder. Exposed publicly
/// so a caller can opt-in to a single Indeo generation if they need
/// to (matches the per-generation registration pattern used elsewhere
/// in the workspace).
pub fn register_indeo2(reg: &mut CodecRegistry) {
    let caps = CodecCapabilities::video("indeo2_sw")
        .with_lossy(true)
        .with_intra_only(false)
        .with_max_size(4096, 4096);
    reg.register(
        CodecInfo::new(CodecId::new(CODEC_ID_INDEO2))
            .capabilities(caps)
            .decoder(v2::make_decoder)
            .tags([CodecTag::fourcc(b"RT21"), CodecTag::fourcc(b"IV20")]),
    );
}
