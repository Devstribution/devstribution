//! Any bytes offered as a `.dsapp`: the zip reader and the checks before the signature.
#![no_main]

use devstribution_fuzz::extract_and_check;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let work = tempfile::tempdir().unwrap();
    let package = work.path().join("in.dsapp");
    std::fs::write(&package, data).unwrap();
    extract_and_check(work.path(), &package);
});
