//! Install → run → history with real uv and Python. Needs `uv` on PATH (or DEVSTRIBUTION_UV).

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use devstribution_core::package::pack;
use devstribution_core::signing::{Signer, generate_keys, public_key_base64};
use devstribution_host::policy::{Loader, Policy, Value, Values};
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
    write_app_with(dir, version, MAIN);
}

fn write_app_with(dir: &Path, version: &str, main: &str) {
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
    fs::write(dir.join("main.py"), main).unwrap();
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
    assert!(pending.changes.is_none());
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
    // The run left the runner's folder as installed (no `__pycache__`), so the check before
    // the next run does not rewrite it under another run that is still using it.
    assert!(
        !runner_dir
            .join("devstribution_runner")
            .join("__pycache__")
            .exists()
    );
    let stats = store.stats(14).unwrap();
    assert_eq!((stats.runs, stats.ok, stats.per_day.len()), (1, 1, 14));
    assert_eq!(stats.usage.len(), 1);
    assert_eq!(
        (stats.usage[0].app_id.as_str(), stats.usage[0].runs),
        ("alice/hello", 1)
    );
    assert_eq!(stats.usage[0].last_run_at, record.started_at);
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
    // Compared with the installed version.
    let changes = pending
        .changes
        .as_ref()
        .expect("an update says what changed");
    assert_eq!(
        (changes.old_version.as_str(), changes.new_version.as_str()),
        ("1.0.0", "2.0.0")
    );
    assert!(changes.files.iter().any(|f| f.path == "pyproject.toml"));
    store.install(&pending.token, &mut |_| {}).unwrap();
    assert_eq!(store.app("alice/hello").unwrap().record.version, "2.0.0");
    // What was typed in the form is still there for the next run.
    assert_eq!(
        store.app("alice/hello").unwrap().record.last_inputs["name"],
        json!("Ada")
    );

    // Downgrade: refused.
    let err = store.inspect(&v1).unwrap_err().to_string();
    assert!(err.contains("older"), "{err}");

    store.uninstall("alice/hello").unwrap();
    assert!(store.installed().unwrap().is_empty());
    // The pin survives uninstall.
    assert!(store.inspect(&intruder).is_err());
}

/// Confirmation screens can be open side by side: what one install changes (the pinned key, the
/// highest version) must be checked again when the other is confirmed.
#[test]
fn stale_confirmations_are_checked_again() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Paths {
        root: tmp.path().join("home"),
    })
    .unwrap();
    let (key, other_key) = (signer(), signer());
    write_app(&tmp.path().join("src1"), "1.0.0");
    write_app(&tmp.path().join("src2"), "2.0.0");
    let v1 = tmp.path().join("v1.dsapp");
    let v2 = tmp.path().join("v2.dsapp");
    let v2_other = tmp.path().join("v2-other.dsapp");
    pack(&tmp.path().join("src1"), &key, &v1).unwrap();
    pack(&tmp.path().join("src2"), &key, &v2).unwrap();
    pack(&tmp.path().join("src2"), &other_key, &v2_other).unwrap();

    // Two first installs of the same namespace, signed by different keys: the second must
    // not get in once the first has pinned its key.
    let first = store.inspect(&v1).unwrap();
    let second = store.inspect(&v2_other).unwrap();
    store.install(&first.token, &mut |_| {}).unwrap();
    let err = format!(
        "{:#}",
        store.install(&second.token, &mut |_| {}).unwrap_err()
    );
    assert!(err.contains("same publisher"), "{err}");
    assert_eq!(store.app("alice/hello").unwrap().record.version, "1.0.0");

    // An older version confirmed after a newer one was installed: refused.
    let older = store.inspect(&v1).unwrap();
    let newer = store.inspect(&v2).unwrap();
    store.install(&newer.token, &mut |_| {}).unwrap();
    let err = format!(
        "{:#}",
        store.install(&older.token, &mut |_| {}).unwrap_err()
    );
    assert!(err.contains("older"), "{err}");
    assert_eq!(store.app("alice/hello").unwrap().record.version, "2.0.0");
}

/// What the confirmation screen showed is what gets installed: staged files and the pending
/// record changed in between (anything the user runs can write the data folder) are refused.
#[test]
fn staged_installs_changed_after_confirmation_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Paths {
        root: tmp.path().join("home"),
    })
    .unwrap();
    write_app(&tmp.path().join("src"), "1.0.0");
    let pkg = tmp.path().join("app.dsapp");
    pack(&tmp.path().join("src"), &signer(), &pkg).unwrap();
    let staging = store.paths.staging();

    let pending = store.inspect(&pkg).unwrap();
    fs::write(
        staging.join(&pending.token).join("app").join("main.py"),
        "evil = 1",
    )
    .unwrap();
    let err = format!(
        "{:#}",
        store.install(&pending.token, &mut |_| {}).unwrap_err()
    );
    assert!(err.contains("`main.py` was modified"), "{err}");

    let pending = store.inspect(&pkg).unwrap();
    let record = staging.join(&pending.token).join("pending.json");
    // Any change: here only a space, which still parses.
    let text = fs::read_to_string(&record).unwrap();
    fs::write(&record, format!("{text} ")).unwrap();
    let err = format!(
        "{:#}",
        store.install(&pending.token, &mut |_| {}).unwrap_err()
    );
    assert!(
        err.contains("changed after the confirmation screen"),
        "{err}"
    );
    assert!(store.installed().unwrap().is_empty());

    let pending = store.inspect(&pkg).unwrap();
    store.install(&pending.token, &mut |_| {}).unwrap();
}

/// A byte that is not UTF-8 or a huge line in the app's output must not end the run, and a
/// runner changed on disk is put back before the run. (Forged results are checked by the
/// unit tests of `run`.)
#[test]
fn app_output_is_handled_defensively() {
    const ROGUE: &str = r#"
import sys

def run(name: str, ctx) -> dict:
    sys.stderr.buffer.write(b"caf\xe9 " + b"x" * 100000 + b"\n")
    sys.stderr.flush()
    print("still running")
    out = ctx.output_dir / "greeting.txt"
    out.write_text("hi")
    return {"greeting": out}
"#;
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Paths {
        root: tmp.path().join("home"),
    })
    .unwrap();
    write_app_with(&tmp.path().join("src"), "1.0.0", ROGUE);
    let pkg = tmp.path().join("rogue.dsapp");
    pack(&tmp.path().join("src"), &signer(), &pkg).unwrap();
    let pending = store.inspect(&pkg).unwrap();
    store.install(&pending.token, &mut |_| {}).unwrap();

    let runner_dir = runner::install(&store.paths).unwrap();
    fs::write(runner_dir.join("json.py"), "raise SystemExit('shadowed')").unwrap();
    let runner_dir = runner::install(&store.paths).unwrap();
    assert!(!runner_dir.join("json.py").exists());

    let (spec, _) = store
        .prepare_run("alice/hello", &Map::new(), &runner_dir)
        .unwrap();
    let lines: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink = {
        let lines = lines.clone();
        Arc::new(move |e: RunEvent| match e {
            RunEvent::Output { line } => lines.lock().unwrap().push(line),
            RunEvent::Log { message, .. } => lines.lock().unwrap().push(message),
            _ => {}
        })
    };
    let result = run::start(spec, sink).unwrap().wait();
    assert_eq!(result.status, RunStatus::Ok, "{:?}", result.error);
    assert_eq!(result.outputs.len(), 1);
    let lines = lines.lock().unwrap();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("caf\u{fffd} ") && l.len() < 20_000)
    );
    assert!(lines.iter().any(|l| l == "still running"));
}

/// A package of `alice/hello` (or `id`) at `version`, signed with `key`.
fn package(
    tmp: &Path,
    name: &str,
    id: &str,
    version: &str,
    main: &str,
    key: &Signer,
) -> std::path::PathBuf {
    let src = tmp.join(format!("src-{name}"));
    write_app_with(&src, version, main);
    let pyproject = src.join("pyproject.toml");
    let text = fs::read_to_string(&pyproject).unwrap();
    fs::write(
        &pyproject,
        text.replace("\"alice/hello\"", &format!("\"{id}\"")),
    )
    .unwrap();
    let pkg = tmp.join(format!("{name}.dsapp"));
    pack(&src, key, &pkg).unwrap();
    pkg
}

fn managed(values: Vec<(&str, Value)>) -> Policy {
    let values = Values(
        values
            .into_iter()
            .map(|(n, v)| (n.to_string(), v))
            .collect(),
    );
    Policy::from_values("test", &values, &Loader { admin_only: false })
}

fn approve(namespace: &str, key: &Signer) -> String {
    format!("{namespace} {}", public_key_base64(key.public_key()))
}

/// Managed mode: only authors the policy approves install and run; a refusal says why and who
/// to ask, and is logged; a dry run only logs; unreadable approvals keep installed apps running.
#[test]
fn managed_mode_installs_only_approved_authors() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = Store::open(Paths {
        root: tmp.path().join("home"),
    })
    .unwrap();
    let alice = signer();
    let policy = |extra: Vec<(&str, Value)>| {
        let mut v = vec![
            ("Authors", Value::Multi(vec![approve("alice", &alice)])),
            ("Contact", Value::String("the IT service desk".into())),
        ];
        v.extend(extra);
        managed(v)
    };
    store.set_policy(Some(policy(vec![])));

    // An author IT has not approved: shown, but blocked, and the install is refused.
    let stranger = package(tmp.path(), "bob", "bob/hello", "1.0.0", MAIN, &signer());
    let pending = store.inspect(&stranger).unwrap();
    assert_eq!(pending.trust, "not_approved");
    let blocked = pending.blocked.clone().expect("blocked");
    assert!(
        blocked.reasons[0].contains("has not approved the author `bob`"),
        "{blocked:?}"
    );
    assert_eq!(blocked.contact.as_deref(), Some("the IT service desk"));
    assert!(blocked.author_not_approved);
    let err = store
        .install(&pending.token, &mut |_| {})
        .unwrap_err()
        .to_string();
    assert!(err.contains("does not allow"), "{err}");

    // An approved namespace signed with another key.
    let other_key = package(
        tmp.path(),
        "other-key",
        "alice/hello",
        "1.0.0",
        MAIN,
        &signer(),
    );
    let blocked = store.inspect(&other_key).unwrap().blocked.expect("blocked");
    assert!(
        blocked.reasons[0].contains("but not the key"),
        "{blocked:?}"
    );

    // Malware-like code from an approved author: refused by default.
    let danger =
        format!("{MAIN}\ndef _unused():\n    import base64\n    exec(base64.b64decode('eD0x'))\n");
    let flagged = package(
        tmp.path(),
        "flagged",
        "alice/hello",
        "1.0.0",
        &danger,
        &alice,
    );
    let blocked = store.inspect(&flagged).unwrap().blocked.expect("blocked");
    assert!(blocked.reasons[0].contains("malware"), "{blocked:?}");

    // The approved author installs and runs.
    let good = package(tmp.path(), "good", "alice/hello", "1.0.0", MAIN, &alice);
    let pending = store.inspect(&good).unwrap();
    assert_eq!(pending.trust, "approved");
    assert!(
        pending.blocked.is_none() && pending.warnings.is_empty(),
        "{:?}",
        pending.warnings
    );
    let app = store.install(&pending.token, &mut |_| {}).unwrap();
    assert!(app.record.policy_approved);
    assert_eq!(store.pins().unwrap()["alice"].source, "policy");
    let result = run_recorded(&store).unwrap();
    assert_eq!(result.status, RunStatus::Ok, "{:?}", result.error);

    let log = fs::read_to_string(store.policy_log()).unwrap();
    assert_eq!(log.lines().count(), 3, "{log}");
    assert!(log.lines().all(|l| l.contains("\"decision\":\"refused\"")));

    // IT approves another key for alice instead: the installed app no longer runs.
    store.set_policy(Some(managed(vec![(
        "Authors",
        Value::Multi(vec![approve("alice", &signer())]),
    )])));
    assert_eq!(
        store.app("alice/hello").unwrap().trust,
        devstribution_host::store::Trust::NotApproved
    );
    let err = run_recorded(&store).unwrap_err().to_string();
    assert!(
        err.contains("cannot run") && err.contains("Ask your IT department"),
        "{err}"
    );

    // The approvals cannot be read (a share out of reach): what was approved keeps running,
    // nothing new installs.
    store.set_policy(Some(managed(vec![(
        "PolicyFile",
        Value::String(
            tmp.path()
                .join("unreachable")
                .join("policy.toml")
                .display()
                .to_string(),
        ),
    )])));
    assert_eq!(store.policy().unwrap().problems.len(), 1);
    assert_eq!(run_recorded(&store).unwrap().status, RunStatus::Ok);
    assert!(store.inspect(&stranger).unwrap().blocked.is_some());

    // A dry run refuses nothing, warns, and logs what it would have refused.
    store.set_policy(Some(policy(vec![("DryRun", Value::Dword(1))])));
    let pending = store.inspect(&stranger).unwrap();
    assert!(pending.blocked.is_none());
    assert!(
        pending.warnings.iter().any(|w| w.contains("would refuse")),
        "{:?}",
        pending.warnings
    );
    store.install(&pending.token, &mut |_| {}).unwrap();
    let log = fs::read_to_string(store.policy_log()).unwrap();
    assert!(
        log.lines()
            .last()
            .unwrap()
            .contains("\"decision\":\"would_refuse\""),
        "{log}"
    );

    // A launcher older than the policy asks for installs and runs nothing.
    store.set_policy(Some(policy(vec![(
        "MinLauncherVersion",
        Value::String("999.0".into()),
    )])));
    let err = run_recorded(&store).unwrap_err().to_string();
    assert!(
        err.contains("999.0 or newer") && err.contains("Ask the IT service desk"),
        "{err}"
    );

    // Free mode again: trust on first use, as before.
    store.set_policy(None);
    assert_eq!(store.inspect(&good).unwrap().trust, "first_use");
}

/// In managed mode a user cannot let an app run past the guard rule that stopped it, unless
/// the policy allows it; rules the policy makes report-only do not stop anyone.
#[test]
fn managed_mode_keeps_guard_decisions_for_it() {
    const HIDDEN: &str = r#"
import marshal

def run(name: str, ctx) -> dict:
    exec(marshal.loads(marshal.dumps(compile("x = 1", "hidden", "exec"))))
    out = ctx.output_dir / "greeting.txt"
    out.write_text("hi")
    return {"greeting": out}
"#;
    let tmp = tempfile::tempdir().unwrap();
    let mut store = Store::open(Paths {
        root: tmp.path().join("home"),
    })
    .unwrap();
    let alice = signer();
    let authors = || ("Authors", Value::Multi(vec![approve("alice", &alice)]));
    store.set_policy(Some(managed(vec![
        authors(),
        ("ScanDanger", Value::String("ask".into())),
    ])));
    let pkg = package(tmp.path(), "hidden", "alice/hello", "1.0.0", HIDDEN, &alice);
    let pending = store.inspect(&pkg).unwrap();
    assert!(pending.blocked.is_none(), "{:?}", pending.blocked);
    store.install(&pending.token, &mut |_| {}).unwrap();

    assert_eq!(run_recorded(&store).unwrap().status, RunStatus::Error);
    let err = store
        .release_quarantine("alice/hello", true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("does not let users"), "{err}");

    store.set_policy(Some(managed(vec![
        authors(),
        ("GuardReportOnly", Value::Multi(vec!["hidden-code".into()])),
    ])));
    store.release_quarantine("alice/hello", false).unwrap();
    let result = run_recorded(&store).unwrap();
    assert_eq!(result.status, RunStatus::Ok, "{:?}", result.error);
}

fn install_app(store: &Store, tmp: &Path, main: &str) {
    write_app_with(&tmp.join("src"), "1.0.0", main);
    let pkg = tmp.join("app.dsapp");
    pack(&tmp.join("src"), &signer(), &pkg).unwrap();
    let pending = store.inspect(&pkg).unwrap();
    store.install(&pending.token, &mut |_| {}).unwrap();
}

/// Runs the installed app to the end and records it, as the launcher does.
fn run_recorded(store: &Store) -> anyhow::Result<run::RunResult> {
    let runner_dir = runner::install(&store.paths).unwrap();
    let (spec, mut record) = store.prepare_run("alice/hello", &Map::new(), &runner_dir)?;
    let result = run::start(spec, Arc::new(|_| {})).unwrap().wait();
    store.finish_run(&mut record, &result).unwrap();
    Ok(result)
}

/// A failed run says what the user can try: the runner's hint for errors that come from the PC,
/// the "send it to the author" hint for a bug, and nothing extra for an `AppError`; the hint is
/// kept in the run's record.
#[test]
fn failed_runs_say_what_to_try() {
    for (main, hint) in [
        (
            "def run(name: str, ctx):
    raise PermissionError(13, 'denied', 'C:/x.xlsx')
",
            Some("close it and try again"),
        ),
        (
            "def run(name: str, ctx):
    return 1 / 0
",
            Some(run::BUG_HINT),
        ),
        (
            "from devstribution_runner import AppError

def run(name: str, ctx):
    raise AppError('Choose the export')
",
            None,
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(Paths {
            root: tmp.path().join("home"),
        })
        .unwrap();
        install_app(&store, tmp.path(), main);
        let result = run_recorded(&store).unwrap();
        assert_eq!(result.status, RunStatus::Error);
        match hint {
            Some(h) => assert!(
                result.hint.as_deref().unwrap().contains(h),
                "{:?}",
                result.hint
            ),
            None => {
                assert_eq!(result.error.as_deref(), Some("Choose the export"));
                assert_eq!(result.hint, None);
            }
        }
        assert_eq!(store.runs(None, 1).unwrap()[0].hint, result.hint);
        assert!(
            !result.hint.unwrap_or_default().contains("  "),
            "a broken line continuation"
        );
    }
}

/// An app the runtime guard stops: it loads hidden compiled code.
const HIDDEN: &str = r#"
import marshal

def run(name: str, ctx) -> dict:
    exec(marshal.loads(marshal.dumps(compile("x = 1", "hidden", "exec"))))
    out = ctx.output_dir / "greeting.txt"
    out.write_text("hi")
    return {"greeting": out}
"#;

/// The runtime guard stops an app that loads hidden code, the app is quarantined until the
/// user decides, and allowing the rule lets it run (still reported).
#[test]
fn guard_stop_quarantines_until_allowed() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Paths {
        root: tmp.path().join("home"),
    })
    .unwrap();
    install_app(&store, tmp.path(), HIDDEN);

    let result = run_recorded(&store).unwrap();
    assert_eq!(result.status, RunStatus::Error);
    assert_eq!(result.guard_stop().unwrap().rule, "hidden-code");
    assert!(
        result
            .error
            .unwrap()
            .starts_with("Stopped by devstribution")
    );
    assert!(result.outputs.is_empty());
    let app = store.app("alice/hello").unwrap();
    assert_eq!(app.record.quarantine.unwrap().hit.rule, "hidden-code");
    assert_eq!(store.runs(None, 1).unwrap()[0].guard[0].rule, "hidden-code");

    let refused = run_recorded(&store).unwrap_err().to_string();
    assert!(refused.contains("is blocked"), "{refused}");

    let app = store.release_quarantine("alice/hello", true).unwrap();
    assert!(app.record.quarantine.is_none());
    assert_eq!(app.record.guard_allow, ["hidden-code"]);
    let result = run_recorded(&store).unwrap();
    assert_eq!(result.status, RunStatus::Ok, "{:?}", result.error);
    assert!(result.guard.iter().any(|h| h.rule == "allowed"));
}

/// Another Python the app starts runs under the guard too: what it does stops the whole run and
/// quarantines the app, as if the app had done it.
#[test]
fn child_pythons_run_under_the_guard() {
    const CHILD: &str = r#"
import subprocess, sys

def run(name: str, ctx) -> dict:
    key = ctx.output_dir.parent / "home" / ".ssh" / "id_ed25519"
    subprocess.run([sys.executable, "-c", "import sys; open(sys.argv[1], 'rb').read()", str(key)])
    import time
    time.sleep(20)
    out = ctx.output_dir / "greeting.txt"
    out.write_text("hi")
    return {"greeting": out}
"#;
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Paths {
        root: tmp.path().join("home"),
    })
    .unwrap();
    install_app(&store, tmp.path(), CHILD);

    let runner_dir = runner::install(&store.paths).unwrap();
    let (spec, mut record) = store
        .prepare_run("alice/hello", &Map::new(), &runner_dir)
        .unwrap();
    // A saved key, where the app looks for it (made here: the app itself may not write it).
    let ssh = spec.run_dir.join("home").join(".ssh");
    fs::create_dir_all(&ssh).unwrap();
    fs::write(ssh.join("id_ed25519"), "key").unwrap();
    let started = std::time::Instant::now();
    let result = run::start(spec, Arc::new(|_| {})).unwrap().wait();
    store.finish_run(&mut record, &result).unwrap();
    let stop = result.guard_stop().expect("the child's stop ends the run");
    assert_eq!(stop.rule, "saved-secrets", "{:?}", result.guard);
    assert!(started.elapsed() < std::time::Duration::from_secs(15));
    let app = store.app("alice/hello").unwrap();
    assert_eq!(app.record.quarantine.unwrap().hit.rule, "saved-secrets");
}

/// A Python started behind the runner's back (straight through the Windows API, so not
/// rewritten to start guarded) is found in the Job Object and ends the run.
#[cfg(windows)]
#[test]
fn job_object_stops_pythons_outside_the_guard() {
    const SNEAKY: &str = r#"
import ctypes, sys, time

def run(name: str, ctx) -> dict:
    command = f'"{sys.executable}" -c "import time; time.sleep(30)"'
    ctypes.windll.kernel32.WinExec(command.encode("mbcs"), 0)
    time.sleep(20)
    out = ctx.output_dir / "greeting.txt"
    out.write_text("hi")
    return {"greeting": out}
"#;
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Paths {
        root: tmp.path().join("home"),
    })
    .unwrap();
    install_app(&store, tmp.path(), SNEAKY);
    let started = std::time::Instant::now();
    let result = run_recorded(&store).unwrap();
    let stop = result
        .guard_stop()
        .expect("stopped by the Job Object watch");
    assert_eq!(stop.rule, "unguarded-python", "{:?}", result.guard);
    assert!(started.elapsed() < std::time::Duration::from_secs(15));
}

/// A stop that `state.json` cannot record (here a folder where its temporary file goes) still
/// blocks the app, is saved as soon as the state can be written, and the run is recorded.
#[test]
fn guard_stop_holds_when_the_state_cannot_be_saved() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths {
        root: tmp.path().join("home"),
    };
    let store = Store::open(paths.clone()).unwrap();
    install_app(&store, tmp.path(), HIDDEN);

    let runner_dir = runner::install(&store.paths).unwrap();
    let (spec, mut record) = store
        .prepare_run("alice/hello", &Map::new(), &runner_dir)
        .unwrap();
    let result = run::start(spec, Arc::new(|_| {})).unwrap().wait();
    let blocker = store.paths.state().join("state.json.tmp");
    fs::create_dir(&blocker).unwrap();
    let err = format!("{:#}", store.finish_run(&mut record, &result).unwrap_err());
    assert!(err.contains("could not record"), "{err}");
    assert_eq!(store.runs(None, 1).unwrap()[0].guard[0].rule, "hidden-code");
    let app = store.app("alice/hello").unwrap();
    assert_eq!(app.record.quarantine.unwrap().hit.rule, "hidden-code");
    let refused = run_recorded(&store).unwrap_err().to_string();
    assert!(refused.contains("is blocked"), "{refused}");

    fs::remove_dir(&blocker).unwrap();
    let refused = run_recorded(&store).unwrap_err().to_string();
    assert!(refused.contains("is blocked"), "{refused}");
    let reopened = Store::open(paths).unwrap();
    let app = reopened.app("alice/hello").unwrap();
    assert_eq!(app.record.quarantine.unwrap().hit.rule, "hidden-code");
}

/// A program started behind the runner's back (straight through the Windows API, which no
/// audit hook sees) is still caught: the launcher checks every process in the Job Object.
#[cfg(windows)]
#[test]
fn job_object_catches_programs_the_hook_cannot_see() {
    const SNEAKY: &str = r#"
import ctypes, time

def run(name: str, ctx) -> dict:
    # Harmless: `rem` does nothing, ping only waits. The command line is what matters.
    ctypes.windll.kernel32.WinExec(b"cmd /c rem vssadmin delete shadows /all & ping -n 6 127.0.0.1", 0)
    time.sleep(20)
    out = ctx.output_dir / "greeting.txt"
    out.write_text("hi")
    return {"greeting": out}
"#;
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(Paths {
        root: tmp.path().join("home"),
    })
    .unwrap();
    install_app(&store, tmp.path(), SNEAKY);
    let started = std::time::Instant::now();
    let result = run_recorded(&store).unwrap();
    let stop = result
        .guard_stop()
        .expect("stopped by the Job Object watch");
    assert_eq!(stop.rule, "delete-backups");
    assert!(stop.detail.contains("vssadmin"), "{}", stop.detail);
    assert!(started.elapsed() < std::time::Duration::from_secs(15));
    assert!(
        store
            .app("alice/hello")
            .unwrap()
            .record
            .quarantine
            .is_some()
    );
}
