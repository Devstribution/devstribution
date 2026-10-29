//! The checks the launcher's commands make themselves, before anything reaches the host crate:
//! what "open result" may open, when the launcher may update, which files it takes as packages.

use std::fs;

use devstribution_host::policy::{Loader, Policy, Value, Values};
use devstribution_host::run::RunStatus;
use serde_json::json;

use super::*;

fn record(
    output_dir: &Path,
    result_folders: Vec<PathBuf>,
    outputs: serde_json::Value,
) -> RunRecord {
    RunRecord {
        id: "run".into(),
        app_id: "alice/hello".into(),
        app_title: "Hello".into(),
        version: "1.0.0".into(),
        kind: "form".into(),
        started_at: 0,
        finished_at: Some(1),
        status: RunStatus::Ok,
        duration_ms: 1,
        outputs: outputs.as_object().cloned().unwrap_or_default(),
        error: None,
        hint: None,
        guard: Vec::new(),
        output_dir: output_dir.to_path_buf(),
        result_folders,
        output_removed: None,
    }
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// A run's folders: its output folder with `report.xlsx` and `tool.bat` in it, a folder the user
/// chose, and a file outside both.
struct Run {
    _tmp: tempfile::TempDir,
    out: PathBuf,
    chosen: PathBuf,
    report: PathBuf,
    script: PathBuf,
    outside: PathBuf,
}

fn run_folders() -> Run {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("runs").join("run").join("output");
    let chosen = tmp.path().join("chosen");
    fs::create_dir_all(&out).unwrap();
    fs::create_dir_all(&chosen).unwrap();
    let report = out.join("report.xlsx");
    let script = out.join("tool.bat");
    let outside = tmp.path().join("secret.txt");
    for f in [&report, &script, &outside] {
        fs::write(f, "x").unwrap();
    }
    let canon = |p: PathBuf| dunce::canonicalize(p).unwrap();
    Run {
        out: canon(out),
        chosen: canon(chosen),
        report: canon(report),
        script: canon(script),
        outside: canon(outside),
        _tmp: tmp,
    }
}

#[test]
fn open_result_opens_the_runs_results_only() {
    let r = run_folders();
    let rec = record(
        &r.out,
        vec![r.out.clone(), r.chosen.clone()],
        json!({
            "report": path_str(&r.report),
            "tool": path_str(&r.script),
            "leak": path_str(&r.outside),
            "count": 3,
        }),
    );

    // No file: the output folder, opened.
    assert_eq!(
        output_target(&rec, None, false).unwrap(),
        (r.out.clone(), false)
    );
    // A file the run returned, inside its output folder.
    assert_eq!(
        output_target(&rec, Some(&path_str(&r.report)), false).unwrap(),
        (r.report.clone(), false)
    );
    // "Show in folder" is kept.
    assert!(
        output_target(&rec, Some(&path_str(&r.report)), true)
            .unwrap()
            .1
    );
    // A file Windows would run is only shown in its folder.
    assert_eq!(
        output_target(&rec, Some(&path_str(&r.script)), false).unwrap(),
        (r.script.clone(), true)
    );

    // A path the run did not return, even inside its folder.
    let other = r.out.join("other.txt");
    fs::write(&other, "x").unwrap();
    let e = output_target(&rec, Some(&path_str(&other)), false).unwrap_err();
    assert!(e.to_string().contains("not a result of the run"), "{e}");
    // A path the run returned, but outside its folders.
    let e = output_target(&rec, Some(&path_str(&r.outside)), false).unwrap_err();
    assert!(e.to_string().contains("not a result of the run"), "{e}");
    // Getting out with `..` resolves first, so it is outside too.
    let dotdot = path_str(&r.out.join("..").join("..").join("..").join("secret.txt"));
    let rec2 = record(&r.out, vec![r.out.clone()], json!({ "f": dotdot }));
    assert!(output_target(&rec2, Some(&dotdot), false).is_err());
    // A folder is not a file result.
    let rec3 = record(
        &r.out,
        vec![r.out.clone()],
        json!({ "d": path_str(&r.out) }),
    );
    assert!(output_target(&rec3, Some(&path_str(&r.out)), false).is_err());
}

#[test]
fn open_result_with_an_empty_output_folder_opens_the_chosen_one() {
    let r = run_folders();
    fs::remove_file(&r.report).unwrap();
    fs::remove_file(&r.script).unwrap();
    let rec = record(&r.out, vec![r.out.clone(), r.chosen.clone()], json!({}));
    assert_eq!(
        output_target(&rec, None, false).unwrap(),
        (r.chosen.clone(), false)
    );
    // No chosen folder: the empty output folder all the same.
    let rec = record(&r.out, vec![r.out.clone()], json!({}));
    assert_eq!(output_target(&rec, None, false).unwrap().0, r.out);
}

#[test]
fn open_result_after_a_cleanup_says_so() {
    let r = run_folders();
    let mut rec = record(&r.out, vec![r.out.clone()], json!({}));
    rec.output_removed = Some(5);
    let e = output_target(&rec, None, false).unwrap_err();
    assert!(e.to_string().contains("removed by a cleanup"), "{e}");
}

fn policy(values: Vec<(&str, Value)>) -> Policy {
    let values = Values(
        values
            .into_iter()
            .map(|(n, v)| (n.to_string(), v))
            .collect(),
    );
    Policy::from_values("test", &values, &Loader { admin_only: false })
}

#[test]
fn it_can_turn_launcher_updates_off() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = Store::open(Paths {
        root: tmp.path().join("home"),
    })
    .unwrap();
    assert!(updates_allowed(&store).is_ok());

    store.set_policy(Some(policy(vec![(
        "Contact",
        Value::String("the service desk".into()),
    )])));
    assert!(
        updates_allowed(&store).is_ok(),
        "on unless the policy says otherwise"
    );

    store.set_policy(Some(policy(vec![
        ("Contact", Value::String("the service desk".into())),
        ("LauncherUpdates", Value::Dword(0)),
    ])));
    let e = updates_allowed(&store).unwrap_err();
    assert!(e.contains("Your organisation updates devstribution"), "{e}");
    assert!(e.contains("Ask the service desk."), "{e}");
}

#[test]
fn no_update_while_apps_run() {
    assert_eq!(runs_block_update(0), None);
    let one = runs_block_update(1).unwrap();
    assert!(
        one.starts_with("An app is still running") && one.contains("when it has"),
        "{one}"
    );
    let two = runs_block_update(2).unwrap();
    assert!(
        two.starts_with("Some apps are") && two.contains("stop them"),
        "{two}"
    );
}

#[test]
fn packages_among_the_arguments() {
    let cwd = Path::new("work");
    let args = [
        "--flag",
        "a.dsapp",
        "B.DSAPP",
        "renamed.zip",
        "notes.txt",
        "dsapp",
    ];
    let got = dsapp_args(args.map(OsString::from), cwd);
    assert_eq!(
        got,
        ["a.dsapp", "B.DSAPP", "renamed.zip"].map(|f| cwd.join(f).to_string_lossy().into_owned())
    );
    // An absolute path stays as it is.
    let abs = std::env::temp_dir().join("x.dsapp");
    assert_eq!(
        dsapp_args([abs.clone().into_os_string()], cwd),
        [abs.to_string_lossy().into_owned()]
    );
}
