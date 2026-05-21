//! Pure-Rust Indeo (IV2/IV3/IV4/IV5) video codec.
//!
//! **Round 1 — Indeo 3 (IV31 / IV32) frame-header parser.** This
//! round lands a structural parser for the 16-byte frame header
//! plus the 48-byte bitstream header that precede every Indeo 3
//! codec frame, per `docs/video/indeo/indeo3/spec/01-file-header.md`.
//! No macroblock / VQ / motion-compensation decode yet — that work
//! sits on `spec/02-picture-layer.md` and later, none of which are
//! yet in `docs/`.
//!
//! Spec coverage in `docs/` at the time of this round:
//!
//! * `docs/video/indeo/indeo3/spec/00-scope.md` — bit/byte
//!   conventions + binary identity.
//! * `docs/video/indeo/indeo3/spec/01-file-header.md` — the entire
//!   header layout this module parses.
//!
//! Indeo 2 / 4 / 5 have only a multimedia.cx wiki snapshot under
//! `docs/video/indeo/indeoN/wiki/`, no `spec/`, so this round picks
//! Indeo 3 (the version with the deepest documented coverage).

#![forbid(unsafe_code)]

pub mod indeo3;

/// Crate-local error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Reserved placeholder for codec paths that have not landed
    /// yet (anything beyond the Indeo 3 header).
    NotImplemented,
    /// Indeo 3: header errors. Carries the spec section that
    /// pinned down the failing constraint.
    Indeo3Header(indeo3::HeaderError),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NotImplemented => f.write_str(
                "oxideav-indeo: clean-room rebuild in progress — see crates/oxideav-indeo/README.md",
            ),
            Error::Indeo3Header(e) => write!(f, "oxideav-indeo[iv3 header]: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<indeo3::HeaderError> for Error {
    fn from(e: indeo3::HeaderError) -> Self {
        Error::Indeo3Header(e)
    }
}

/// Crate-local Result alias.
pub type Result<T> = core::result::Result<T, Error>;
