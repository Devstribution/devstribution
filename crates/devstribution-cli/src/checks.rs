//! Checks shared by `dev` and `pack`: manifest, lockfile and the app's code.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use devstribution_core::lock::check_lock;
use devstribution_core::manifest::Manifest;
use devstribution_core::package::{PYPROJECT, UV_LOCK};
use serde_json::{Value, json};

pub struct Checked {
    pub manifest: Manifest,
    pub warnings: Vec<String>,
}

/// Runs every check; returns the manifest and warnings, or fails listing all errors.
pub fn check_app(app_dir: &Path, runner_dir: &Path) -> Result<Checked> {
    let pyproject_path = app_dir.join(PYPROJECT);
    let pyproject = fs::read_to_string(&pyproject_path).with_context(|| {
        format!(
            "no {PYPROJECT} in {} (create one with `devstribution init`)",
            app_dir.display()
        )
    })?;
    let manifest = Manifest::parse(&pyproject)?;

    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    match fs::read_to_string(app_dir.join(UV_LOCK)) {
        Err(_) => errors.push(format!(
            "{UV_LOCK} is missing: run `uv lock` in the app folder"
        )),
        Ok(lock) => {
            let report = check_lock(&lock, &manifest.name, &manifest.platforms)?;
            errors.extend(report.errors.into_iter().map(|e| format!("{UV_LOCK}: {e}")));
            warnings.extend(
                report
                    .warnings
                    .into_iter()
                    .map(|w| format!("{UV_LOCK}: {w}")),
            );
        }
    }

    let code = check_code(app_dir, runner_dir, &manifest)?;
    errors.extend(code.0);
    warnings.extend(code.1);

    if !errors.is_empty() {
        print_warnings(&warnings);
        let list: Vec<String> = errors.iter().map(|e| format!("  - {e}")).collect();
        bail!(
            "the app has {} problem(s):\n{}",
            errors.len(),
            list.join("\n")
        );
    }
    Ok(Checked { manifest, warnings })
}

/// Static checks of the Python code (`check_app.py`), run with any Python uv finds.
fn check_code(
    app_dir: &Path,
    runner_dir: &Path,
    manifest: &Manifest,
) -> Result<(Vec<String>, Vec<String>)> {
    let script = runner_dir.join("devstribution_tools").join("check_app.py");
    let spec = json!({
        "kind": manifest.kind.as_str(),
        "entry": manifest.entry.to_string(),
        "inputs": manifest.inputs.iter().map(|i| json!({"name": i.name, "type": i.kind})).collect::<Vec<_>>(),
        "excluded_dirs": [".git", ".venv", "venv", "__pycache__", "node_modules", "dist", ".tox"],
    });
    let mut child = Command::new(crate::env::uv())
        .args(["run", "--no-project", "--quiet", "python", "-I"])
        .arg(&script)
        .arg(app_dir)
        .current_dir(app_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("cannot start uv to check the code; install uv from https://docs.astral.sh/uv/")?;
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(spec.to_string().as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("the code check failed to run");
    }
    let result: Value =
        serde_json::from_slice(&out.stdout).context("the code check returned invalid output")?;
    let list = |key: &str| -> Vec<String> {
        result[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok((list("errors"), list("warnings")))
}

pub fn print_warnings(warnings: &[String]) {
    for w in warnings {
        eprintln!("warning: {w}");
    }
}
