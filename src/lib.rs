//! Pure-Rust Indeo (IV2/IV3/IV4/IV5) video codec.
//!
//! **Round 2 — Indeo 3 (IV31 / IV32) picture-layer plane preludes.**
//!
//! Round 1 landed the 64-byte combined header
//! ([`indeo3::FrameHeader::parse`], `spec/01`). Round 2 adds
//! [`indeo3::PictureLayer::parse`], a structural parser for the
//! per-plane preludes (`num_vectors` + `mc_vectors[]`) that
//! immediately follow each plane offset, per
//! `docs/video/indeo/indeo3/spec/02-picture-layer.md`.
//!
//! `PictureLayer::parse` does not decode the binary-tree / VQ
//! bitstream payload — that work is the subject of
//! `spec/03-macroblock-layer.md` and is deferred to a later
//! round.
//!
//! Spec coverage in `docs/` at the time of this round:
//!
//! * `docs/video/indeo/indeo3/spec/00-scope.md` — bit/byte
//!   conventions + binary identity.
//! * `docs/video/indeo/indeo3/spec/01-file-header.md` — frame +
//!   bitstream header (round 1).
//! * `docs/video/indeo/indeo3/spec/02-picture-layer.md` — plane
//!   iteration order, plane prelude, half-pel scaling, packed-MV
//!   formula (this round).
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
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NotImplemented => f.write_str(
                "oxideav-indeo: clean-room rebuild in progress — see crates/oxideav-indeo/README.md",
            ),
            Error::Indeo3Header(e) => write!(f, "oxideav-indeo[iv3 header]: {e}"),
            Error::Indeo3PictureLayer(e) => write!(f, "oxideav-indeo[iv3 picture]: {e}"),
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

/// Crate-local Result alias.
pub type Result<T> = core::result::Result<T, Error>;
