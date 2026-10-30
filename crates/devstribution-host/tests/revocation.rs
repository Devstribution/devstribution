//! Revocation end to end (docs/revocation.md): owners' statements, rotations, and signed lists,
//! at inspect, install and before runs. Needs `uv` on PATH (or DEVSTRIBUTION_UV), as flow.rs.

use std::fs;
use std::path::{Path, PathBuf};

use devstribution_core::package::pack;
use devstribution_core::revocation::{
    self, Entry, FORMAT, Reason, RevocationList, Status, sign_list,
};
use devstribution_core::signing::{Signer, generate_keys, public_key_base64, public_key_sha256};
use devstribution_host::policy::{Loader, Policy, Value, Values};
use devstribution_host::revocations::Imported;
use devstribution_host::store::Store;
use devstribution_host::{Paths, runner};
use serde_json::Map;

fn signer() -> Signer {
    Signer::from_unencrypted(&generate_keys("alice", None).unwrap().secret_key)
        .unwrap()
        .unwrap()
}

/// `alice/hello` at `version`, signed with `key`.
fn package(tmp: &Path, version: &str, key: &Signer) -> PathBuf {
    let src = tmp.join(format!("src-{version}-{}", key.key_id()));
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("pyproject.toml"),
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
        src.join("main.py"),
        "def run(ctx) -> dict:\n    return {}\n",
    )
    .unwrap();
    let status = std::process::Command::new(devstribution_host::uv::uv_path())
        .args(["lock", "--quiet"])
        .current_dir(&src)
        .status()
        .unwrap();
    assert!(status.success());
    let pkg = tmp.join(format!("hello-{version}-{}.dsapp", key.key_id()));
    pack(&src, key, &pkg).unwrap();
    pkg
}

fn store(tmp: &Path) -> Store {
    Store::open(Paths {
        root: tmp.join("home"),
    })
    .unwrap()
}

fn install(store: &Store, pkg: &Path) {
    let pending = store.inspect(pkg).unwrap();
    store.install(&pending.token, &mut |_| {}).unwrap();
}

fn write(tmp: &Path, name: &str, text: &str) -> PathBuf {
    let path = tmp.join(name);
    fs::write(&path, text).unwrap();
    path
}

fn can_run(store: &Store) -> anyhow::Result<()> {
    let runner_dir = runner::install(&store.paths)?;
    store.prepare_run("alice/hello", &Map::new(), &runner_dir)?;
    Ok(())
}

/// A withdrawn key installs nothing more; an app it signed runs again only once the user saw
/// why; a rotation hands the pin to the new key, unless the old key was withdrawn.
#[test]
fn owners_statements() {
    let tmp = tempfile::tempdir().unwrap();
    let tmp = tmp.path();
    let store = store(tmp);
    let (old, new, thief) = (signer(), signer(), signer());
    install(&store, &package(tmp, "1.0.0", &old));

    // A new key is refused while nothing hands the namespace over to it.
    let v2_new = package(tmp, "2.0.0", &new);
    assert!(store.inspect(&v2_new).is_err());

    // The owner's rotation: the next version, signed with the new key, installs and the pin
    // moves to it.
    let rotation = revocation::rotation(&old, &new, "alice", "2026-10-29").unwrap();
    let imported = store
        .import_revocations(&write(tmp, "rotate.dskey", &rotation))
        .unwrap();
    assert!(
        matches!(imported, Imported::Statement { ref statement, .. } if statement == "rotation")
    );
    let pending = store.inspect(&v2_new).unwrap();
    assert_eq!(pending.trust, "rotated");
    assert!(
        pending
            .warnings
            .iter()
            .any(|w| w.contains("moved to a new key"))
    );
    store.install(&pending.token, &mut |_| {}).unwrap();
    assert_eq!(
        store.pins().unwrap()["alice"].key,
        public_key_base64(new.public_key())
    );

    // The new key is withdrawn (stolen): its next version is refused, and the installed app
    // runs again only after the user saw why.
    let withdrawal =
        revocation::withdrawal(&new, "alice", Reason::Stolen, None, "2026-10-29").unwrap();
    store
        .import_revocations(&write(tmp, "withdraw.dskey", &withdrawal))
        .unwrap();
    let e = store
        .inspect(&package(tmp, "3.0.0", &new))
        .unwrap_err()
        .to_string();
    assert!(e.contains("no longer safe (stolen)"), "{e}");
    let app = store.app("alice/hello").unwrap();
    let revoked = app.revoked.expect("revoked");
    assert_eq!(revoked.status, "withdrawn");
    let e = can_run(&store).unwrap_err().to_string();
    assert!(e.contains("Hello was revoked"), "{e}");
    let app = store.acknowledge_revocation("alice/hello").unwrap();
    assert_eq!(app.record.revocation_acks, [revoked.id]);
    can_run(&store).unwrap();

    // A thief holding the withdrawn key cannot hand the namespace to a key of their own.
    let theft = revocation::rotation(&new, &thief, "alice", "2026-10-30").unwrap();
    store
        .import_revocations(&write(tmp, "theft.dskey", &theft))
        .unwrap();
    assert!(store.inspect(&package(tmp, "4.0.0", &thief)).is_err());

    // A forged statement is not imported.
    let forged = withdrawal.replace(
        &public_key_base64(new.public_key()),
        &public_key_base64(thief.public_key()),
    );
    assert!(
        store
            .import_revocations(&write(tmp, "forged.dskey", &forged))
            .is_err()
    );
    assert_eq!(store.revocation_status().statements, 3);
}

fn list(serial: u64, entries: Vec<Entry>) -> RevocationList {
    RevocationList {
        format: FORMAT,
        serial,
        issued: "2026-10-29".into(),
        expires: "2099-01-01".into(),
        entries,
    }
}

fn entry(status: Status, key: &Signer) -> Entry {
    Entry {
        id: "2026-0001".into(),
        status,
        key: public_key_sha256(key.public_key()),
        packages: vec![],
        namespace: "alice".into(),
        reason: "Version 1.0.0 of alice/hello uploads browser passwords.".into(),
        evidence: vec!["https://example.org/evidence/2026-0001.html".into()],
        publisher_statement: None,
        replacement: None,
        listed: "2026-10-29".into(),
        updated: "2026-10-29".into(),
    }
}

/// Lists are checked against the root keys and only ever move forward; a `malicious` entry
/// takes the user's "anyway" in free mode and refuses in managed mode; the bundled list counts
/// while it is the newest.
#[test]
fn signed_lists() {
    let tmp = tempfile::tempdir().unwrap();
    let tmp = tmp.path();
    let mut store = store(tmp);
    let (root, list_key, publisher) = (signer(), signer(), signer());
    let files = |serial, entries| {
        sign_list(&root, &list_key, "2099-12-31", &list(serial, entries)).unwrap()
    };
    let folder = |name: &str, serial, entries| {
        let dir = tmp.join(name);
        files(serial, entries).write(&dir).unwrap();
        dir.join(revocation::LIST_FILE)
    };
    let l2 = folder("l2", 2, vec![entry(Status::Malicious, &publisher)]);

    // Without root keys (as in this release) no list verifies.
    let e = store.import_revocations(&l2).unwrap_err().to_string();
    assert!(e.contains("no root keys"), "{e}");
    store.set_revocation_roots(vec![root.public_key().clone()]);

    let imported = store.import_revocations(&l2).unwrap();
    assert!(matches!(
        imported,
        Imported::List {
            serial: 2,
            entries: 1
        }
    ));
    let e = store.import_revocations(&l2).unwrap_err().to_string();
    assert!(e.contains("already has list 2"), "{e}");
    let e = store
        .import_revocations(&folder("l1", 1, vec![]))
        .unwrap_err()
        .to_string();
    assert!(e.contains("older than list 2"), "{e}");
    let status = store.revocation_status();
    assert_eq!(
        (status.serial, status.source.as_deref()),
        (Some(2), Some("imported"))
    );

    // Free mode: shown, and installed only with the user's "anyway" (the install itself).
    let pkg = package(tmp, "1.0.0", &publisher);
    let pending = store.inspect(&pkg).unwrap();
    let revoked = pending.revoked.clone().expect("revoked");
    assert!(revoked.is_malicious() && revoked.message.contains("browser passwords"));
    assert_eq!(revoked.evidence.len(), 1);
    store.install(&pending.token, &mut |_| {}).unwrap();
    assert_eq!(
        store.app("alice/hello").unwrap().record.revocation_acks,
        std::slice::from_ref(&revoked.id)
    );
    can_run(&store).unwrap();

    // Managed mode: refused, at install and before runs, whatever the user says.
    let policy = Policy::from_values(
        "test",
        &Values(vec![(
            "Authors".into(),
            Value::Multi(vec![format!(
                "alice {}",
                public_key_base64(publisher.public_key())
            )]),
        )]),
        &Loader { admin_only: false },
    );
    store.set_policy(Some(policy));
    let blocked = store.inspect(&pkg).unwrap().blocked.expect("blocked");
    assert!(
        blocked
            .reasons
            .iter()
            .any(|r| r.starts_with("it was revoked: version 1.0.0")),
        "{blocked:?}"
    );
    let e = can_run(&store).unwrap_err().to_string();
    assert!(e.contains("it was revoked"), "{e}");
    assert!(store.acknowledge_revocation("alice/hello").is_err());
    store.set_policy(None);

    // A newer bundled list (the entry cleared) wins over the imported one.
    let mut cleared = entry(Status::Cleared, &publisher);
    cleared.reason = "Listed by mistake.".into();
    store.set_bundled_revocations(Some(files(3, vec![cleared])));
    let status = store.revocation_status();
    assert_eq!(
        (status.serial, status.source.as_deref()),
        (Some(3), Some("bundled"))
    );
    assert!(store.app("alice/hello").unwrap().revoked.is_none());
}
