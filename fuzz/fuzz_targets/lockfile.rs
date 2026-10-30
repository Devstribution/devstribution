//! The `uv.lock` checks, and the scan's reading of a lockfile.
#![no_main]

use devstribution_core::manifest::Permissions;
use devstribution_core::scan::Scan;
use devstribution_core::{deps, lock};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = lock::check_sources(text);
    let platforms = ["windows".to_string(), "macos".into(), "linux".into()];
    let _ = lock::check_lock(text, "hello-gui", &platforms);
    let _ = deps::check(text);
    let _ = deps::packages(text);

    let mut scan = Scan::default();
    scan.lockfile(text);
    let _ = scan.finish(&Permissions::default());
});
