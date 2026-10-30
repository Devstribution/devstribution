//! What the extraction targets share: small limits, and what must hold after every extraction.

use std::fs;
use std::path::Path;

use devstribution_core::package::{
    Limits, VerifyOptions, extract_and_verify, read_header, verify_extracted,
};

/// Small enough that a zip bomb costs a moment, not the run.
pub const LIMITS: Limits = Limits {
    max_files: 64,
    max_file_bytes: 1 << 20,
    max_total_bytes: 4 << 20,
};

/// Reads and extracts `package` (inside `work`, a folder of its own), then checks what must hold
/// whatever the package holds.
pub fn extract_and_check(work: &Path, package: &Path) {
    let _ = read_header(package, LIMITS);

    let dest = work.join("dest");
    let opts = VerifyOptions {
        trusted_key: None,
        limits: LIMITS,
    };
    let result = extract_and_verify(package, &dest, &opts);

    // Nothing lands beside the folder it was extracted to.
    for entry in fs::read_dir(work).unwrap() {
        let path = entry.unwrap().path();
        assert!(path == package || path == dest, "written outside: {path:?}");
    }
    match result {
        // A refused package leaves nothing behind.
        Err(_) => assert!(!dest.exists(), "left behind after a refusal"),
        // An accepted one holds exactly its signed list, regular files only.
        Ok(verified) => {
            verify_extracted(&dest, &verified.files).expect("an accepted package matches its list");
            let mut files = 0;
            for entry in walkdir::WalkDir::new(&dest) {
                let entry = entry.unwrap();
                let kind = entry.file_type();
                assert!(!kind.is_symlink(), "a link: {:?}", entry.path());
                if kind.is_file() {
                    files += 1;
                }
            }
            assert_eq!(files, verified.files.len());
            assert!(verified.files.len() <= LIMITS.max_files);
        }
    }
}
