//! End-to-end: init → dev → keygen → pack → verify, with an isolated DEVSTRIBUTION_HOME.
//! Needs `uv` on PATH (or DEVSTRIBUTION_UV).

use std::fs;
use std::path::Path;

use assert_cmd::Command;

fn cli(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("devstribution").unwrap();
    cmd.env("DEVSTRIBUTION_HOME", home)
        .env_remove("DEVSTRIBUTION_KEY_PASSWORD");
    cmd
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn init_dev_pack_verify() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let app = tmp.path().join("hello");

    cli(&home)
        .args(["init", "--id", "alice/hello"])
        .arg(&app)
        .assert()
        .success();
    assert!(app.join("uv.lock").is_file());

    let run = cli(&home)
        .arg("dev")
        .arg(&app)
        .args(["-i", "name=Ada", "--json"])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let result: serde_json::Value = serde_json::from_str(stdout(&run).trim()).unwrap();
    let greeting = result["outputs"]["greeting"].as_str().unwrap();
    assert_eq!(fs::read_to_string(greeting).unwrap(), "Hello, Ada!\n");
    assert!(
        !app.join("__pycache__").exists(),
        "dev must not write into the app folder"
    );

    // No key yet: pack explains how to create one.
    let no_key = cli(&home).arg("pack").arg(&app).output().unwrap();
    assert!(!no_key.status.success());
    assert!(String::from_utf8_lossy(&no_key.stderr).contains("keygen --namespace alice"));

    cli(&home)
        .args(["keygen", "--namespace", "alice", "--no-password"])
        .assert()
        .success();
    // A namespace keeps its key.
    cli(&home)
        .args(["keygen", "--namespace", "alice", "--no-password"])
        .assert()
        .failure();

    let pkg = tmp.path().join("hello.dsapp");
    cli(&home)
        .arg("pack")
        .arg(&app)
        .arg("-o")
        .arg(&pkg)
        .assert()
        .success();

    let verified = cli(&home)
        .arg("verify")
        .arg(&pkg)
        .arg("--key")
        .arg(home.join("keys/alice.pub"))
        .output()
        .unwrap();
    assert!(verified.status.success());
    assert!(stdout(&verified).contains("signature OK"));

    // A key from another namespace does not verify it.
    cli(&home)
        .args(["keygen", "--namespace", "mallory", "--no-password"])
        .assert()
        .success();
    cli(&home)
        .arg("verify")
        .arg(&pkg)
        .arg("--key")
        .arg(home.join("keys/mallory.pub"))
        .assert()
        .failure();
}

#[test]
fn dev_reports_signature_mismatch_before_running() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let app = tmp.path().join("app");
    cli(&home)
        .args(["init", "--id", "alice/app"])
        .arg(&app)
        .assert()
        .success();
    fs::write(
        app.join("main.py"),
        "def run(nme: str, ctx):\n    return {}\n",
    )
    .unwrap();
    let out = cli(&home).arg("dev").arg(&app).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("missing the parameter `name`"), "{err}");
    assert!(err.contains("`nme`, which is not declared"), "{err}");
}
