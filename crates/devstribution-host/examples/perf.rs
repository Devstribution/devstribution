//! Times what a user waits for: checking a package, installing it (cold and warm uv cache), the
//! first run and later runs. Section 9 of `docs/roadmap.md` holds the numbers.
//!
//! ```text
//! cargo run --release -p devstribution-host --example perf -- <work-dir> [runs]
//! ```
//!
//! Three generated apps (no dependencies, openpyxl, pandas + openpyxl), each packed with a
//! throwaway key. "Cold" points uv's cache and Python downloads at an empty folder (through
//! `LOCALAPPDATA`/`APPDATA`, which uv keeps), so it includes downloading Python and every wheel;
//! "warm" installs again into a fresh data folder with that cache filled. Needs the network.
//!
//! Set `DEVSTRIBUTION_UV` to the uv the launcher ships (`launcher/src-tauri/binaries/`) to time
//! that one, and `PERF_SPLIT=1` to print, per run, when the process was started, when the app's
//! code began (its first progress message) and when the run ended.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use devstribution_core::package::pack;
use devstribution_core::signing::{Signer, generate_keys};
use devstribution_host::run::{self, RunStatus};
use devstribution_host::store::Store;
use devstribution_host::{Paths, runner};
use serde_json::Map;

const MAIN_PLAIN: &str = r#"
def run(ctx) -> dict:
    ctx.progress(0.5, "started")
    out = ctx.output_dir / "out.txt"
    out.write_text("ok")
    return {"out": out}
"#;

const MAIN_OPENPYXL: &str = r#"
import openpyxl

def run(ctx) -> dict:
    wb = openpyxl.Workbook()
    ws = wb.active
    for i in range(1000):
        ws.append([i, i * 2, f"row {i}"])
    out = ctx.output_dir / "out.xlsx"
    wb.save(out)
    return {"out": out}
"#;

const MAIN_PANDAS: &str = r#"
import pandas as pd

def run(ctx) -> dict:
    df = pd.DataFrame({"a": range(10000), "b": range(10000)})
    df["c"] = df.a * df.b
    out = ctx.output_dir / "out.xlsx"
    df.to_excel(out, index=False)
    return {"out": out}
"#;

fn write_app(dir: &Path, name: &str, deps: &[&str], main: &str) {
    fs::create_dir_all(dir).unwrap();
    let deps = deps
        .iter()
        .map(|d| format!("\"{d}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        dir.join("pyproject.toml"),
        format!(
            r#"[project]
name = "perf-{name}"
version = "1.0.0"
requires-python = ">=3.11"
dependencies = [{deps}]

[tool.uv]
package = false

[tool.devstribution]
schema_version = 1
id = "perf/{name}"
title = "{name}"
entry = "main:run"
platforms = ["windows", "macos", "linux"]

[[tool.devstribution.outputs]]
name = "out"
type = "file"
"#
        ),
    )
    .unwrap();
    fs::write(dir.join("main.py"), main).unwrap();
    let ok = std::process::Command::new(devstribution_host::uv::uv_path())
        .args(["lock", "--quiet"])
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "uv lock failed for {name}");
}

fn secs(d: Duration) -> String {
    format!("{:.2}s", d.as_secs_f64())
}

/// Installs `package` into a fresh data folder; returns the store and per-step times.
fn install(home: &Path, package: &Path) -> (Store, String, Vec<(String, Duration)>) {
    let store = Store::open(Paths {
        root: home.to_path_buf(),
    })
    .unwrap();
    let t = Instant::now();
    let pending = store.inspect(package).unwrap();
    let mut times = vec![("inspect".to_string(), t.elapsed())];
    let mut last = Instant::now();
    let mut last_step = String::from("start");
    store
        .install(&pending.token, &mut |s| {
            times.push((last_step.clone(), last.elapsed()));
            last = Instant::now();
            last_step = format!(
                "{}: {}",
                s.step,
                s.message.chars().take(50).collect::<String>()
            );
        })
        .unwrap();
    (store, pending.id, times)
}

fn time_run(store: &Store, id: &str, runner_dir: &Path) -> Duration {
    let t = Instant::now();
    let (spec, mut record) = store.prepare_run(id, &Map::new(), runner_dir).unwrap();
    let prepared = t.elapsed();
    let first_event: Arc<std::sync::Mutex<Option<Duration>>> = Arc::default();
    let sink = {
        let first_event = first_event.clone();
        Arc::new(move |e: run::RunEvent| {
            if matches!(e, run::RunEvent::Progress { .. }) {
                first_event.lock().unwrap().get_or_insert(t.elapsed());
            }
        })
    };
    let handle = run::start(spec, sink).unwrap();
    let spawned = t.elapsed();
    let result = handle.wait();
    let took = t.elapsed();
    if std::env::var_os("PERF_SPLIT").is_some() {
        println!(
            "    prepare {}, spawned at {}, app code at {:?}, done {} (runner says {} ms)",
            secs(prepared),
            secs(spawned),
            first_event.lock().unwrap().map(secs),
            secs(took),
            result.duration_ms
        );
    }
    assert_eq!(result.status, RunStatus::Ok, "{:?}", result.error);
    store.finish_run(&mut record, &result).unwrap();
    took
}

fn main() {
    let mut args = std::env::args().skip(1);
    let work = std::path::PathBuf::from(args.next().expect("usage: perf <work-dir> [runs]"));
    let runs: usize = args.next().map(|r| r.parse().unwrap()).unwrap_or(5);
    if work.exists() {
        fs::remove_dir_all(&work).unwrap();
    }
    fs::create_dir_all(&work).unwrap();
    let key = Signer::from_unencrypted(&generate_keys("perf", None).unwrap().secret_key)
        .unwrap()
        .unwrap();

    let apps: [(&str, &[&str], &str); 3] = [
        ("plain", &[], MAIN_PLAIN),
        ("openpyxl", &["openpyxl>=3.1"], MAIN_OPENPYXL),
        ("pandas", &["pandas>=2", "openpyxl>=3.1"], MAIN_PANDAS),
    ];
    for (name, deps, main) in apps {
        let src = work.join("src").join(name);
        write_app(&src, name, deps, main);
        let package = work.join(format!("{name}.dsapp"));
        let t = Instant::now();
        let packed = pack(&src, &key, &package).unwrap();
        let pack_time = t.elapsed();
        let size = fs::metadata(&package).unwrap().len();
        println!(
            "\n## {name} ({} files, {size} bytes packed)",
            packed.file_count
        );
        println!("pack: {}", secs(pack_time));

        // Cold: uv's cache and Python downloads in an empty folder of their own.
        let cache_root = work.join("cache").join(name);
        fs::create_dir_all(cache_root.join("local")).unwrap();
        fs::create_dir_all(cache_root.join("roaming")).unwrap();
        // SAFETY: single-threaded here; nothing else reads the environment meanwhile.
        unsafe {
            std::env::set_var("LOCALAPPDATA", cache_root.join("local"));
            std::env::set_var("APPDATA", cache_root.join("roaming"));
        }
        for label in ["cold", "warm"] {
            let home = work.join("home").join(format!("{name}-{label}"));
            let t = Instant::now();
            let (store, id, steps) = install(&home, &package);
            let total = t.elapsed();
            let steps = steps
                .iter()
                .map(|(s, d)| format!("{s} {}", secs(*d)))
                .collect::<Vec<_>>()
                .join(", ");
            println!("install ({label} cache): {} [{steps}]", secs(total));
            let env_size = dir_size(&store.paths.envs());
            println!("  environment on disk: {:.1} MB", env_size as f64 / 1e6);
            let runner_dir = runner::install(&store.paths).unwrap();
            let first = time_run(&store, &id, &runner_dir);
            let later: Vec<Duration> = (0..runs)
                .map(|_| time_run(&store, &id, &runner_dir))
                .collect();
            let mut sorted = later.clone();
            sorted.sort();
            println!(
                "  first run: {}, later runs: median {} (min {}, max {})",
                secs(first),
                secs(sorted[sorted.len() / 2]),
                secs(sorted[0]),
                secs(sorted[sorted.len() - 1])
            );
        }
    }
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}
