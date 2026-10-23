//! `scan`: the code scan of an app folder or a `.dsapp`, as the launcher shows it before an install.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use devstribution_core::manifest::Manifest;
use devstribution_core::package::{self, Limits, PYPROJECT, VerifyOptions};
use devstribution_core::scan::{self, Finding, ScanReport, Severity};

/// Prints the scan; the exit code is 1 when something looks like malware, so CI can stop on it.
pub fn scan(target: &Path, json: bool) -> Result<u8> {
    let (title, report) = if target.is_dir() {
        let manifest = Manifest::parse(
            &fs::read_to_string(target.join(PYPROJECT))
                .with_context(|| format!("no {PYPROJECT} in {}", target.display()))?,
        )?;
        let app = package::collect_app_files(target)?;
        let report = scan::scan(
            target,
            app.files.keys().map(String::as_str),
            &manifest.permissions,
        );
        (format!("{} {}", manifest.id, manifest.version), report)
    } else {
        // Verified first: the scan describes the files that would be installed.
        let tmp = tempfile::tempdir()?;
        let dest = tmp.path().join("app");
        let opts = VerifyOptions {
            trusted_key: None,
            limits: Limits::default(),
        };
        let verified = package::extract_and_verify(target, &dest, &opts)?;
        let m = &verified.manifest;
        let report = scan::scan(&dest, verified.files.paths(), &m.permissions);
        (format!("{} {}", m.id, m.version), report)
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&title, &report);
    }
    Ok(u8::from(report.worst() == Some(Severity::Danger)))
}

fn print_report(title: &str, report: &ScanReport) {
    println!("{title}: {} Python file(s) scanned", report.files_scanned);
    if let Some(deps) = &report.dependencies {
        println!(
            "  {} package(s) in uv.lock checked against PyPI's malicious-package reports up to {}",
            deps.packages, deps.list_date
        );
    }
    if report.findings.is_empty() {
        println!("  nothing flagged");
        return;
    }
    for f in &report.findings {
        println!("  {:<7}  {}", f.severity.as_str(), f.message);
        println!("           {}", location(f));
    }
    println!(
        "{} danger, {} warning, {} info. The scan reads the code without running it: it can miss \
         what is hidden well, and a flagged line can be harmless.",
        report.count(Severity::Danger),
        report.count(Severity::Warning),
        report.count(Severity::Info)
    );
}

/// `main.py:12: eval(...)`, plus how many more times it matched.
pub fn location(f: &Finding) -> String {
    let mut s = match f.line {
        Some(line) => format!("{}:{line}: {}", f.path, f.evidence),
        None => f.path.clone(),
    };
    if f.occurrences > 1 {
        s.push_str(&format!(" (and {} more)", f.occurrences - 1));
    }
    s
}
