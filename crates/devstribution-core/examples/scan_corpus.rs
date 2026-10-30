//! How often the code scan flags ordinary code (roadmap B8). Reads, never runs, what it is given.
//!
//! `cargo run --release -p devstribution-core --example scan_corpus -- [--each-file] [--show=<rule>] <folder>...`
//!
//! Every entry directly in a folder counts as one app: a package folder or a single file. With
//! `--each-file`, every Python file below the folders counts as one app, which is closer to a
//! script someone delivers. Nothing is declared, so undeclared network and SAP use show as they
//! would for an app that declares nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use devstribution_core::manifest::Permissions;
use devstribution_core::scan::{Finding, Scan, Severity};

#[derive(Default)]
struct Tally {
    /// Apps with this rule, then findings (one per file).
    apps: usize,
    files: usize,
    severity: Option<Severity>,
}

fn is_python(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("py") || e.eq_ignore_ascii_case("pyw"))
}

fn files_below(root: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|p| !p.components().any(|c| c.as_os_str() == "__pycache__"))
        .collect()
}

/// The findings of one app; with `only_python`, binaries beside the code do not count.
fn scan_app(base: &Path, files: &[PathBuf], only_python: bool) -> (usize, Vec<Finding>) {
    let mut scan = Scan::default();
    for path in files {
        if only_python && !is_python(path) {
            continue;
        }
        let rel = path.strip_prefix(base).unwrap_or(path);
        let rel = rel.to_string_lossy().replace('\\', "/");
        scan.file(&rel, || std::fs::read(path).ok());
    }
    let report = scan.finish(&Permissions::default());
    (report.files_scanned, report.findings)
}

fn main() {
    let mut each_file = false;
    let mut show = Vec::new();
    let mut folders = Vec::new();
    for arg in std::env::args().skip(1) {
        if arg == "--each-file" {
            each_file = true;
        } else if let Some(rule) = arg.strip_prefix("--show=") {
            show.push(rule.to_string());
        } else {
            folders.push(PathBuf::from(arg));
        }
    }

    // (name, base the paths are relative to, files)
    let mut apps: Vec<(String, PathBuf, Vec<PathBuf>)> = Vec::new();
    for folder in &folders {
        if each_file {
            for file in files_below(folder).into_iter().filter(|p| is_python(p)) {
                let base = file.parent().unwrap().to_path_buf();
                apps.push((file.display().to_string(), base, vec![file]));
            }
            continue;
        }
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".dist-info") || name == "__pycache__" {
                continue;
            }
            if path.is_dir() {
                apps.push((name, path.clone(), files_below(&path)));
            } else if is_python(&path) {
                apps.push((name, folder.clone(), vec![path]));
            }
        }
    }
    apps.sort_by(|a, b| a.0.cmp(&b.0));

    let mut rules: BTreeMap<String, Tally> = BTreeMap::new();
    let mut scanned_apps = 0;
    let mut scanned_files = 0;
    let (mut with_danger, mut with_warning, mut clean) = (0, 0, 0);
    let mut dangers = Vec::new();
    let mut reviewable_warning = 0;
    for (name, base, files) in &apps {
        let (count, findings) = scan_app(base, files, false);
        if count == 0 {
            continue;
        }
        scanned_apps += 1;
        scanned_files += count;
        let worst = findings.iter().map(|f| f.severity).max();
        match worst {
            Some(Severity::Danger) => with_danger += 1,
            Some(Severity::Warning) => with_warning += 1,
            _ => clean += 1,
        }
        // Warnings that remain once an app declares its network use and ships no binaries.
        if findings.iter().any(|f| {
            f.severity >= Severity::Warning
                && !matches!(f.rule.as_str(), "network" | "binary-file" | "script-file")
        }) {
            reviewable_warning += 1;
        }
        let mut seen: Vec<&str> = Vec::new();
        for f in &findings {
            let tally = rules.entry(f.rule.clone()).or_default();
            tally.files += 1;
            tally.severity = tally.severity.max(Some(f.severity));
            if !seen.contains(&f.rule.as_str()) {
                seen.push(&f.rule);
                tally.apps += 1;
            }
            if f.severity == Severity::Danger || show.contains(&f.rule) {
                dangers.push(format!(
                    "{name} | {} | {}:{} | {}",
                    f.rule,
                    f.path,
                    f.line.unwrap_or(0),
                    f.evidence
                ));
            }
        }
    }

    let percent = |n: usize| 100.0 * n as f64 / scanned_apps.max(1) as f64;
    println!("{scanned_apps} apps, {scanned_files} Python files");
    println!("danger:  {with_danger} apps ({:.1}%)", percent(with_danger));
    println!(
        "warning: {with_warning} apps ({:.1}%)",
        percent(with_warning)
    );
    println!("neither: {clean} apps ({:.1}%)", percent(clean));
    println!(
        "a warning or danger beyond undeclared network and shipped binaries: {reviewable_warning} apps ({:.1}%)",
        percent(reviewable_warning)
    );
    println!("\nrule, severity, apps, files");
    let mut sorted: Vec<_> = rules.iter().collect();
    sorted.sort_by_key(|(_, t)| (std::cmp::Reverse(t.severity), std::cmp::Reverse(t.apps)));
    for (rule, t) in sorted {
        let severity = t.severity.map_or("", Severity::as_str);
        println!("{rule:<20} {severity:<8} {:>6} {:>6}", t.apps, t.files);
    }
    println!("\nevery danger finding, and the rules asked for with --show=<rule>:");
    for line in &dangers {
        println!("{line}");
    }
}
