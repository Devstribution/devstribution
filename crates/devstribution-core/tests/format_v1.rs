//! Package format 1 is frozen: a package made when it was frozen must keep verifying, with the same
//! manifest and the same content hash, in every later version. See `docs/format.md`.

use std::path::Path;

use devstribution_core::manifest::{InputType, Kind, OutputType};
use devstribution_core::package::{Limits, SignatureStatus, VerifyOptions, extract_and_verify};
use devstribution_core::signing::parse_public_key;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/format-1");

#[test]
fn the_frozen_format_1_package_still_verifies() {
    let dir = Path::new(FIXTURE);
    let key =
        parse_public_key(&std::fs::read_to_string(dir.join("publisher.pub")).unwrap()).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let verified = extract_and_verify(
        &dir.join("hello.dsapp"),
        &tmp.path().join("app"),
        &VerifyOptions {
            trusted_key: Some(&key),
            limits: Limits::default(),
        },
    )
    .expect("format 1 must stay readable");

    assert_eq!(
        verified.signature,
        SignatureStatus::Trusted {
            key_id: "953FA77D699AFE8C".into()
        }
    );
    // Names the install folder: it must not change for an unchanged package.
    assert_eq!(verified.content_hash, "b28483c9670bd4b5");
    assert_eq!(
        verified.files.paths().collect::<Vec<_>>(),
        [
            "AGENTS.md",
            "icon.png",
            "pkg/__init__.py",
            "pkg/main.py",
            "pyproject.toml",
            "templates/report.xlsx",
            "uv.lock",
        ]
    );

    let m = &verified.manifest;
    assert_eq!(m.schema_version, 1);
    assert_eq!(m.id.to_string(), "format-test/hello");
    assert_eq!(m.version.to_string(), "1.0.0");
    assert_eq!(m.kind, Kind::Form);
    assert_eq!(m.entry.to_string(), "pkg.main:run");
    assert_eq!(m.icon.as_deref(), Some("icon.png"));
    assert_eq!(m.platforms, ["windows", "macos", "linux"]);
    assert_eq!(m.timeout_seconds, Some(600));
    assert_eq!(
        m.inputs
            .iter()
            .map(|i| (i.name.as_str(), i.kind))
            .collect::<Vec<_>>(),
        [
            ("source", InputType::File),
            ("target", InputType::Folder),
            ("title", InputType::Text),
            ("limit", InputType::Number),
            ("verbose", InputType::Bool),
            ("mode", InputType::Choice),
            ("day", InputType::Date),
            ("token", InputType::Secret),
        ]
    );
    assert!(!m.inputs[1].required);
    assert_eq!(m.inputs[0].accept, [".xlsx", ".csv"]);
    assert_eq!(m.inputs[5].choices, ["fast", "full"]);
    assert_eq!(
        m.outputs
            .iter()
            .map(|o| (o.name.as_str(), o.kind))
            .collect::<Vec<_>>(),
        [
            ("report", OutputType::File),
            ("rows", OutputType::Table),
            ("summary", OutputType::Text),
        ]
    );
    assert_eq!(m.permissions.network, ["api.example.com"]);
    assert_eq!(m.permissions.folders, ["user-selected"]);
    assert_eq!(m.permissions.secrets, ["token"]);
}
