//! Finding newer versions of installed apps in update folders. Needs `uv` on PATH (or
//! DEVSTRIBUTION_UV), like `flow.rs`.

use std::fs;
use std::path::Path;

use devstribution_core::package::pack;
use devstribution_core::signing::{Signer, generate_keys, public_key_base64};
use devstribution_host::Paths;
use devstribution_host::policy::{Loader, Policy, Value, Values};
use devstribution_host::store::Store;

fn signer() -> Signer {
    Signer::from_unencrypted(&generate_keys("alice", None).unwrap().secret_key)
        .unwrap()
        .unwrap()
}

/// Packs version `version` of `alice/hello` to `out`.
fn package(work: &Path, version: &str, key: &Signer, out: &Path) {
    let dir = work.join(format!("src-{version}"));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("pyproject.toml"),
        format!(
            r#"[project]
name = "hello"
version = "{version}"
requires-python = ">=3.11"
dependencies = []

[tool.uv]
package = false

[tool.devstribution]
schema_version = 1
id = "alice/hello"
title = "Hello"
entry = "main:run"
platforms = ["windows", "macos", "linux"]
"#
        ),
    )
    .unwrap();
    fs::write(
        dir.join("main.py"),
        format!("VERSION = '{version}'\ndef run(ctx):\n    return {{}}\n"),
    )
    .unwrap();
    let status = std::process::Command::new(devstribution_host::uv::uv_path())
        .args(["lock", "--quiet"])
        .current_dir(&dir)
        .status()
        .unwrap();
    assert!(status.success());
    pack(&dir, key, out).unwrap();
}

fn install(store: &Store, path: &Path) {
    let pending = store.inspect(path).unwrap();
    store.install(&pending.token, &mut |_| {}).unwrap();
}

fn found(store: &Store) -> Vec<String> {
    store
        .check_app_updates()
        .unwrap()
        .updates
        .into_iter()
        .map(|u| format!("{} {}", u.id, u.version))
        .collect()
}

#[test]
fn only_real_newer_versions_are_offered() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path();
    let store = Store::open(Paths {
        root: work.join("home"),
    })
    .unwrap();
    let key = signer();
    let folder = work.join("updates");
    fs::create_dir(&folder).unwrap();

    package(work, "1.1.0", &key, &work.join("v1.1.0.dsapp"));
    install(&store, &work.join("v1.1.0.dsapp"));

    // Nothing is looked at until a folder is added.
    package(work, "1.2.0", &key, &folder.join("hello-1.2.0.dsapp"));
    assert!(found(&store).is_empty());
    store.add_update_folder(&folder).unwrap();
    store.add_update_folder(&folder).unwrap();
    assert_eq!(store.update_folders().unwrap().len(), 1);
    assert_eq!(found(&store), ["alice/hello 1.2.0"]);

    // The highest of several wins; older and equal versions are no updates.
    package(work, "1.3.0", &key, &folder.join("hello-1.3.0.dsapp"));
    package(work, "1.0.0", &key, &folder.join("hello-1.0.0.dsapp"));
    fs::copy(work.join("v1.1.0.dsapp"), folder.join("same.dsapp")).unwrap();
    assert_eq!(found(&store), ["alice/hello 1.3.0"]);

    // A higher version from another key, or not a package at all, is not offered.
    fs::remove_file(folder.join("hello-1.2.0.dsapp")).unwrap();
    fs::remove_file(folder.join("hello-1.3.0.dsapp")).unwrap();
    package(work, "9.0.0", &signer(), &folder.join("impostor.dsapp"));
    fs::write(folder.join("garbage.dsapp"), b"not a zip").unwrap();
    assert!(found(&store).is_empty());

    // What is offered installs through the usual flow, and is then no longer offered.
    package(work, "2.0.0", &key, &folder.join("hello-2.0.0.dsapp"));
    let check = store.check_app_updates().unwrap();
    assert_eq!(check.updates[0].installed_version, "1.1.0");
    install(&store, &check.updates[0].path);
    assert!(found(&store).is_empty());

    // A folder that went away is said, not fatal.
    let gone = work.join("gone");
    fs::create_dir(&gone).unwrap();
    store.add_update_folder(&gone).unwrap();
    fs::remove_dir(&gone).unwrap();
    assert_eq!(store.check_app_updates().unwrap().problems.len(), 1);
    store.remove_update_folder(&gone).unwrap();
    assert!(store.check_app_updates().unwrap().problems.is_empty());
}

#[test]
fn managed_mode_offers_only_approved_authors_and_reads_its_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path();
    let key = signer();
    let folder = work.join("it-share");
    fs::create_dir(&folder).unwrap();

    let policy = |author: &Signer| {
        Policy::from_values(
            "test",
            &Values(vec![
                (
                    "Authors".to_string(),
                    Value::Multi(vec![format!(
                        "alice {}",
                        public_key_base64(author.public_key())
                    )]),
                ),
                (
                    "UpdateFolder".to_string(),
                    Value::String(folder.display().to_string()),
                ),
            ]),
            &Loader { admin_only: false },
        )
    };
    let mut store = Store::open(Paths {
        root: work.join("home"),
    })
    .unwrap();
    store.set_policy(Some(policy(&key)));
    package(work, "1.0.0", &key, &work.join("v1.dsapp"));
    install(&store, &work.join("v1.dsapp"));

    // Signed by the approved key: offered, from IT's folder alone.
    package(work, "2.0.0", &key, &folder.join("v2.dsapp"));
    assert_eq!(found(&store), ["alice/hello 2.0.0"]);

    // The same package once the author is no longer the approved one.
    store.set_policy(Some(policy(&signer())));
    assert!(found(&store).is_empty());
}
