//! Package format, signing and verification for devstribution apps.
//!
//! A `.dsapp` package is a zip holding an app folder plus `FILES.sha256` (the hash of every file,
//! in canonical form) and `FILES.sha256.minisig` (the publisher's minisign signature over it).
//! See `docs/plan.md`, "App package standard (v1)".

pub mod error;
pub mod files;
pub mod lock;
pub mod manifest;
pub mod package;
pub mod paths;
pub mod signing;

pub use error::{Error, Result};

/// Package format version written by this crate and the only one it accepts.
pub const SCHEMA_VERSION: u32 = 1;

/// Name of the file holding the hashes of every other file in the package.
pub const FILES_LIST: &str = "FILES.sha256";

/// Name of the publisher's signature over [`FILES_LIST`].
pub const FILES_SIGNATURE: &str = "FILES.sha256.minisig";
