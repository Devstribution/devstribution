//! `diff`: what changed between two versions of an app, for a review before an update.

use std::path::Path;

use anyhow::{Context, Result};
use devstribution_core::diff::{self, AppDiff, AppFolder, FileStatus};
use devstribution_core::manifest::Manifest;
use devstribution_core::package::{self, Limits, PYPROJECT, VerifyOptions};
use devstribution_core::scan::Severity;

/// Prints the comparison of `old` and `new` (app folders or `.dsapp` files); with `code`, the
/// line-by-line changes of every text file too. The exit code is 1 when the new version adds
/// something that looks like malware, so CI can stop on it.
pub fn diff(old: &Path, new: &Path, code: bool, json: bool) -> Result<u8> {
    let tmp = tempfile::tempdir()?;
    let a = open(old, &tmp.path().join("old"))?;
    let b = open(new, &tmp.path().join("new"))?;
    let d = diff::diff(&a, &b)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&d)?);
    } else {
        print_diff(&a, &b, &d, code);
    }
    let danger = d.highlights.iter().any(|h| h.severity == Severity::Danger);
    Ok(u8::from(danger))
}

/// An app folder as `pack` would package it, or a `.dsapp` verified and extracted to `dest`.
fn open(target: &Path, dest: &Path) -> Result<AppFolder> {
    if target.is_dir() {
        Manifest::parse(
            &std::fs::read_to_string(target.join(PYPROJECT))
                .with_context(|| format!("no {PYPROJECT} in {}", target.display()))?,
        )?;
        let app = package::collect_app_files(target)?;
        return Ok(AppFolder::new(target, app.files.keys().map(String::as_str)));
    }
    let opts = VerifyOptions {
        trusted_key: None,
        limits: Limits::default(),
    };
    let verified = package::extract_and_verify(target, dest, &opts)
        .with_context(|| format!("{}", target.display()))?;
    Ok(AppFolder::new(dest, verified.files.paths()))
}

fn print_diff(a: &AppFolder, b: &AppFolder, d: &AppDiff, code: bool) {
    println!("{} → {}", d.old_version, d.new_version);
    if d.is_empty() {
        println!("  no changes");
        return;
    }

    if !d.highlights.is_empty() {
        println!("\nWorth a look");
        for h in &d.highlights {
            println!("  {:<7}  {}", h.severity.as_str(), h.message);
        }
    }

    let p = &d.permissions;
    if !p.is_empty() {
        println!("\nPermissions");
        let rows = |what: &str, added: &[String], removed: &[String]| {
            for v in added {
                println!("  + {what} {v}");
            }
            for v in removed {
                println!("  - {what} {v}");
            }
        };
        rows("network", &p.network_added, &p.network_removed);
        rows("folders", &p.folders_added, &p.folders_removed);
        rows("secret", &p.secrets_added, &p.secrets_removed);
        match p.sap {
            Some(true) => println!("  + sap"),
            Some(false) => println!("  - sap"),
            None => {}
        }
    }

    if !d.declared.is_empty() {
        println!("\nDeclared");
        for c in &d.declared {
            println!("  {c}");
        }
    }

    if !d.packages.is_empty() {
        println!("\nPackages (uv.lock)");
        for c in &d.packages {
            match (&c.old, &c.new) {
                (None, Some(n)) => println!("  + {} {n}", c.name),
                (Some(o), None) => println!("  - {} {o}", c.name),
                (Some(o), Some(n)) => println!(
                    "    {} {o} → {n}{}",
                    c.name,
                    if c.downgraded() { "  (older)" } else { "" }
                ),
                (None, None) => {}
            }
        }
    }

    if !d.findings_added.is_empty() || !d.findings_removed.is_empty() {
        println!("\nCode scan");
        for f in &d.findings_added {
            println!("  + {:<7}  {}", f.severity.as_str(), f.message);
            println!("             {}", crate::scan::location(f));
        }
        for f in &d.findings_removed {
            println!("  - {:<7}  {} ({})", f.severity.as_str(), f.message, f.path);
        }
    }

    println!("\nFiles");
    for f in &d.files {
        let mark = match f.status {
            FileStatus::Added => '+',
            FileStatus::Removed => '-',
            FileStatus::Changed => '~',
        };
        let detail = if f.binary {
            "  (cannot be read: program, library or compiled file)".to_string()
        } else {
            format!("  (+{} -{})", f.lines_added, f.lines_removed)
        };
        println!("  {mark} {}{detail}", f.path);
    }

    if code {
        for f in d.files.iter().filter(|f| !f.binary) {
            if let Some(text) = diff::file_text_diff(a, b, &f.path) {
                println!("\n{text}");
            }
        }
    } else if d.files.iter().any(|f| !f.binary) {
        println!("\nAdd --code to see the changed lines.");
    }

    let worth = d
        .highlights
        .iter()
        .filter(|h| h.severity != Severity::Info)
        .count();
    if worth > 0 {
        println!(
            "\n{worth} change(s) worth a look. Packages are compared by version only: what changed \
             inside them is not shown."
        );
    }
}
