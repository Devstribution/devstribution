const COMMANDS: &[&str] = &[
    "overview",
    "get_app",
    "inspect_package",
    "confirm_install",
    "discard_install",
    "uninstall_app",
    "repair_app",
    "release_quarantine",
    "start_run",
    "cancel_run",
    "list_runs",
    "run_log",
    "open_output",
    "set_secret",
    "clear_secret",
    "settings",
    "uninstall_launcher",
    "take_pending_files",
    "save_trust_request",
];

fn main() {
    // Windows: the DLLs the launcher imports are loaded from System32 only, never from its
    // own (user-writable) folder, where a planted `version.dll` would run inside the signed
    // launcher.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    }
    // The launcher's own commands go through the same permission system as Tauri's: the
    // window can call only those listed in its capability.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri build");
}
