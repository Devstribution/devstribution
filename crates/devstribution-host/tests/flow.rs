//! Install → run → history with real uv and Python. Needs `uv` on PATH (or DEVSTRIBUTION_UV).

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use devstribution_core::package::pack;
use devstribution_core::signing::{Signer, generate_keys};
use devstribution_host::run::{self, RunEvent, RunStatus};
use devstribution_host::store::Store;
use devstribution_host::{Paths, runner};
use serde_json::{Map, json};

const MAIN: &str = r#"
def run(name: str, ctx) -> dict:
    ctx.progress(0.5, "greeting")
    out = ctx.output_dir / "greeting.txt"
    out.write_text(f"Hello, {name}!")
    return {"greeting": out}
"#;

fn write_app(dir: &Path, version: &str) {
    fs::create_dir_all(dir).unwrap();
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

[[tool.devstribution.inputs]]
name = "name"
type = "text"
default = "world"

[[tool.devstribution.outputs]]
name = "greeting"
type = "file"
"#
        ),
    )
    .unwrap();
    fs::write(dir.join("main.py"), MAIN).unwrap();
    let status = std::process::Command::new(devstribution_host::uv::uv_path())
        .args(["lock", "--quiet"])
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success());
}

fn signer() -> Signer {
    Signer::from_unencrypted(&generate_keys("alice", None).unwrap().secret_key)
        .unwrap()
        .unwrap()
}

#[test]
fn install_run_update_and_refusals() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Paths {
        root: tmp.path().join("home"),
    })
    .unwrap();
    let key = signer();

    let v1 = tmp.path().join("v1.dsapp");
    write_app(&tmp.path().join("src1"), "1.0.0");
    pack(&tmp.path().join("src1"), &key, &v1).unwrap();

    let pending = store.inspect(&v1).unwrap();
    assert_eq!(pending.action, "install");
    assert_eq!(pending.trust, "new_publisher");
    let mut steps = Vec::new();
    let app = store
        .install(&pending.token, &mut |s| steps.push(s.step))
        .unwrap();
    assert!(steps.contains(&"environment") && steps.last() == Some(&"ready"));
    assert_eq!(app.record.version, "1.0.0");

    // Run it.
    let runner_dir = runner::install(&store.paths).unwrap();
    let mut inputs = Map::new();
    inputs.insert("name".into(), json!("Ada"));
    let (spec, mut record) = store
        .prepare_run("alice/hello", &inputs, &runner_dir)
        .unwrap();
    let events: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink = {
        let events = events.clone();
        Arc::new(move |e: RunEvent| {
            if let RunEvent::Progress { message, .. } = e {
                events.lock().unwrap().push(message);
            }
        })
    };
    let result = run::start(spec, sink).unwrap().wait();
    assert_eq!(result.status, RunStatus::Ok, "{:?}", result.error);
    let out = result.outputs["greeting"].as_str().unwrap();
    assert_eq!(fs::read_to_string(out).unwrap(), "Hello, Ada!");
    assert_eq!(*events.lock().unwrap(), ["greeting"]);
    store.finish_run(&mut record, &result).unwrap();
    assert_eq!(store.runs(None, 10).unwrap()[0].status, RunStatus::Ok);
    let stats = store.stats(14).unwrap();
    assert_eq!((stats.runs, stats.ok, stats.per_day.len()), (1, 1, 14));
    assert_eq!(
        store.app("alice/hello").unwrap().record.last_inputs["name"],
        json!("Ada")
    );

    // Same namespace, another key: refused.
    let intruder = tmp.path().join("intruder.dsapp");
    write_app(&tmp.path().join("src2"), "2.0.0");
    pack(&tmp.path().join("src2"), &signer(), &intruder).unwrap();
    let err = store.inspect(&intruder).unwrap_err().to_string();
    assert!(
        err.contains("may not come from the same publisher"),
        "{err}"
    );

    // Update with the pinned key.
    let v2 = tmp.path().join("v2.dsapp");
    pack(&tmp.path().join("src2"), &key, &v2).unwrap();
    let pending = store.inspect(&v2).unwrap();
    assert_eq!(
        (pending.action.as_str(), pending.trust.as_str()),
        ("update", "first_use")
    );
    store.install(&pending.token, &mut |_| {}).unwrap();
    assert_eq!(store.app("alice/hello").unwrap().record.version, "2.0.0");

    // Downgrade: refused.
    let err = store.inspect(&v1).unwrap_err().to_string();
    assert!(err.contains("older"), "{err}");

    store.uninstall("alice/hello").unwrap();
    assert!(store.installed().unwrap().is_empty());
    // The pin survives uninstall.
    assert!(store.inspect(&intruder).is_err());
}
