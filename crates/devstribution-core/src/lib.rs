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
pub mod scan;
pub mod signing;

pub use error::{Error, Result};

/// Package format version written by this crate. Format 1 is frozen (docs/format.md): later
/// versions of this crate must keep accepting it, and a package with a higher number is refused
/// with [`Error::NewerFormat`] rather than a parse error.
pub const SCHEMA_VERSION: u32 = 1;

/// Name of the file holding the hashes of every other file in the package.
pub const FILES_LIST: &str = "FILES.sha256";

/// Name of the publisher's signature over [`FILES_LIST`].
pub const FILES_SIGNATURE: &str = "FILES.sha256.minisig";

/// Name of the publisher's public key, carried in the package so that the signature can always
/// be checked, and so that the first install can pin the key for its namespace (TOFU).
pub const FILES_PUBLIC_KEY: &str = "FILES.sha256.pub";
