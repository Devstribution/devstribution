//! The code scan, over any file an app can hold. The scan runs before install, on code nobody has
//! vetted, so it must neither fall over nor take long (libFuzzer's timeout catches the second).
#![no_main]

use devstribution_core::manifest::Permissions;
use devstribution_core::scan::Scan;
use libfuzzer_sys::fuzz_target;

const NAMES: [&str; 7] = [
    "main.py", "app.pyw", "uv.lock", "run.ps1", "x.pyc", "lib.dll", "a",
];

fuzz_target!(|data: &[u8]| {
    let Some((&pick, bytes)) = data.split_first() else {
        return;
    };
    let mut scan = Scan::default();
    scan.file(NAMES[pick as usize % NAMES.len()], || Some(bytes.to_vec()));
    let report = scan.finish(&Permissions::default());
    // A report ends up on the install screen and in `scan --json`.
    serde_json::to_string(&report).unwrap();
});
