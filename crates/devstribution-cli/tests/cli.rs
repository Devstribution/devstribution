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
    // Text mode writes CRLF on Windows.
    assert_eq!(
        fs::read_to_string(greeting).unwrap().replace("\r\n", "\n"),
        "Hello, Ada!\n"
    );
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

    let scanned = cli(&home).arg("scan").arg(&pkg).output().unwrap();
    assert!(scanned.status.success());
    assert!(
        stdout(&scanned).contains("nothing flagged"),
        "{}",
        stdout(&scanned)
    );

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

#[test]
fn scan_flags_malware_like_code_and_pack_warns() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let app = tmp.path().join("hello");
    cli(&home)
        .args(["init", "--id", "alice/hello"])
        .arg(&app)
        .assert()
        .success();
    let main = app.join("main.py");
    let code = fs::read_to_string(&main).unwrap()
        + "
import base64

def _later():
    exec(base64.b64decode(\"cHJpbnQoMSk=\"))
";
    fs::write(&main, code).unwrap();

    let scanned = cli(&home)
        .arg("scan")
        .arg(&app)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        scanned.status.code(),
        Some(1),
        "danger findings fail the scan"
    );
    let report: serde_json::Value = serde_json::from_str(&stdout(&scanned)).unwrap();
    let rules: Vec<&str> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["rule"].as_str().unwrap())
        .collect();
    assert_eq!(rules, ["decode-and-run", "dynamic-code"]);

    // The publisher sees the same findings when packing; signing still works.
    cli(&home)
        .args(["keygen", "--namespace", "alice", "--no-password"])
        .assert()
        .success();
    let packed = cli(&home)
        .arg("pack")
        .arg(&app)
        .arg("-o")
        .arg(tmp.path().join("hello.dsapp"))
        .output()
        .unwrap();
    assert!(packed.status.success());
    let stderr = String::from_utf8_lossy(&packed.stderr);
    assert!(stderr.contains("code scan (danger): main.py:"), "{stderr}");
}

#[test]
fn scan_flags_malicious_and_look_alike_dependencies() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let app = tmp.path().join("hello");
    cli(&home)
        .args(["init", "--id", "alice/hello"])
        .arg(&app)
        .assert()
        .success();
    let package = |name: &str, version: &str| {
        format!(
            "\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n\
             source = {{ registry = \"https://pypi.org/simple\" }}\n"
        )
    };
    let lock = "version = 1\n\n[[package]]\nname = \"hello\"\nversion = \"0.1.0\"\n\
                source = { virtual = \".\" }\n"
        .to_string()
        + &package("requests", "2.32.3")
        // Reported as malicious on PyPI (OSV MAL-2022-…).
        + &package("0requests", "0.0.1")
        + &package("requezts", "1.0.0");
    fs::write(app.join("uv.lock"), lock).unwrap();

    let scanned = cli(&home)
        .arg("scan")
        .arg(&app)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        scanned.status.code(),
        Some(1),
        "a malicious package fails the scan"
    );
    let report: serde_json::Value = serde_json::from_str(&stdout(&scanned)).unwrap();
    assert_eq!(report["dependencies"]["packages"], 3);
    let flagged: Vec<(&str, &str)> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["rule"].as_str().unwrap(), f["evidence"].as_str().unwrap()))
        .collect();
    assert!(
        flagged.contains(&("malicious-package", "0requests 0.0.1")),
        "{flagged:?}"
    );
    assert!(
        flagged.contains(&("typosquat", "requezts 1.0.0 (looks like `requests`)")),
        "{flagged:?}"
    );
}
