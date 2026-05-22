//! Pure-Rust Indeo (IV2/IV3/IV4/IV5) video codec.
//!
//! **Round 3 — Indeo 3 (IV31 / IV32) macroblock-layer binary tree.**
//!
//! Round 1 landed the 64-byte combined header
//! ([`indeo3::FrameHeader::parse`], `spec/01`). Round 2 added
//! [`indeo3::PictureLayer::parse`], the per-plane prelude parser
//! (`spec/02`). Round 3 adds [`indeo3::decode_plane_tree`], the
//! binary-tree walk over a plane's bitstream payload (the bytes
//! that begin at the `bitstream_offset` round 2 computed), per
//! `docs/video/indeo/indeo3/spec/03-macroblock-layer.md`. It
//! produces a typed [`indeo3::CellTree`] of INTRA / INTER leaf
//! cells; INTRA cells carry their VQ sub-tree leaves inline.
//!
//! `decode_plane_tree` stops at the per-leaf index-byte fetch (the
//! spec/03 §7 chapter boundary): it records the raw MV-index byte
//! for INTER leaves and the raw codebook-index byte for VQ_DATA
//! leaves, but does not materialise the VQ codebooks
//! (`spec/04-vq-codebooks.md`), perform motion compensation
//! (`spec/05`), or reconstruct pixels (`spec/07`).
//!
//! Spec coverage in `docs/` at the time of this round:
//!
//! * `docs/video/indeo/indeo3/spec/00-scope.md` — bit/byte
//!   conventions + binary identity.
//! * `docs/video/indeo/indeo3/spec/01-file-header.md` — frame +
//!   bitstream header (round 1).
//! * `docs/video/indeo/indeo3/spec/02-picture-layer.md` — plane
//!   iteration order, plane prelude, half-pel scaling, packed-MV
//!   formula (round 2).
//! * `docs/video/indeo/indeo3/spec/03-macroblock-layer.md` —
//!   MSB-first bit reader, 2-bit node codes, MC_TREE / VQ_TREE
//!   walk, INTRA→VQ transition, leaf index-byte fetch (this round).
//!
//! Indeo 2 / 4 / 5 have only a multimedia.cx wiki snapshot under
//! `docs/video/indeo/indeoN/wiki/`, no `spec/`, so they remain at
//! the round-0 scaffold pending docs work.

#![forbid(unsafe_code)]

pub mod indeo3;

/// Crate-local error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Reserved placeholder for codec paths that have not landed
    /// yet (anything beyond the Indeo 3 header + picture layer).
    NotImplemented,
    /// Indeo 3: header errors. Carries the spec section that
    /// pinned down the failing constraint.
    Indeo3Header(indeo3::HeaderError),
    /// Indeo 3: picture-layer (plane prelude) errors per
    /// `docs/video/indeo/indeo3/spec/02-picture-layer.md`.
    Indeo3PictureLayer(indeo3::PictureLayerError),
    /// Indeo 3: macroblock-layer (binary-tree walk) errors per
    /// `docs/video/indeo/indeo3/spec/03-macroblock-layer.md`.
    Indeo3Macroblock(indeo3::MacroblockError),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NotImplemented => f.write_str(
                "oxideav-indeo: clean-room rebuild in progress — see crates/oxideav-indeo/README.md",
            ),
            Error::Indeo3Header(e) => write!(f, "oxideav-indeo[iv3 header]: {e}"),
            Error::Indeo3PictureLayer(e) => write!(f, "oxideav-indeo[iv3 picture]: {e}"),
            Error::Indeo3Macroblock(e) => write!(f, "oxideav-indeo[iv3 macroblock]: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<indeo3::HeaderError> for Error {
    fn from(e: indeo3::HeaderError) -> Self {
        Error::Indeo3Header(e)
    }
}

impl From<indeo3::PictureLayerError> for Error {
    fn from(e: indeo3::PictureLayerError) -> Self {
        Error::Indeo3PictureLayer(e)
    }
}

impl From<indeo3::MacroblockError> for Error {
    fn from(e: indeo3::MacroblockError) -> Self {
        Error::Indeo3Macroblock(e)
    }
}

/// Crate-local Result alias.
pub type Result<T> = core::result::Result<T, Error>;
