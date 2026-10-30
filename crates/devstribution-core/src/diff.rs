//! What changed between two versions of an app: what the update screen summarises and what an
//! IT reviewer reads (`devstribution diff`).
//!
//! An update from a publisher people already trust is the realistic way to slip something in,
//! so the comparison leads with what gives the app more reach: new permissions, new things the
//! code scan finds, new or older packages, programs and compiled files. The files themselves
//! come last, with the lines added and removed in each.
//!
//! Like the scan, nothing is run. Packages are compared by name and locked version; what changed
//! inside a package's own code is not shown.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

use crate::manifest::{Manifest, Permissions};
use crate::package::{PYPROJECT, UV_LOCK};
use crate::scan::{self, BINARY_EXTENSIONS, Finding, Severity};
use crate::{Error, Result, deps};

/// Files larger than this are compared by content only, without counting lines.
const MAX_TEXT_BYTES: usize = 4 << 20;

/// The files of one version: a folder and the package paths in it.
#[derive(Debug, Clone)]
pub struct AppFolder {
    pub root: PathBuf,
    pub files: Vec<String>,
}

impl AppFolder {
    pub fn new<'a>(root: &Path, files: impl IntoIterator<Item = &'a str>) -> AppFolder {
        AppFolder {
            root: root.to_path_buf(),
            files: files.into_iter().map(str::to_string).collect(),
        }
    }

    /// Every regular file under `root`, as installed app folders hold exactly a package's files.
    pub fn all_files(root: &Path) -> Result<AppFolder> {
        let mut files = Vec::new();
        for entry in walkdir::WalkDir::new(root).follow_links(false) {
            let entry = entry.map_err(|e| {
                Error::io(
                    root,
                    e.into_io_error()
                        .unwrap_or_else(|| std::io::Error::other("walk")),
                )
            })?;
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
            let parts: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            files.push(parts.join("/"));
        }
        files.sort();
        Ok(AppFolder {
            root: root.to_path_buf(),
            files,
        })
    }

    fn read(&self, rel: &str) -> Option<Vec<u8>> {
        fs::read(self.root.join(rel)).ok()
    }

    fn manifest(&self) -> Result<Manifest> {
        let path = self.root.join(PYPROJECT);
        Manifest::parse(&fs::read_to_string(&path).map_err(|e| Error::io(path, e))?)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDiff {
    pub old_version: String,
    pub new_version: String,
    /// What deserves a look first, most important first, in plain words.
    pub highlights: Vec<Highlight>,
    pub permissions: PermissionChanges,
    /// Other changes to what the app declares (entry point, required programs, platforms…).
    pub declared: Vec<String>,
    pub packages: Vec<PackageChange>,
    pub files: Vec<FileChange>,
    /// Findings of the code scan that the old version did not have (same rule, same file), or
    /// had fewer of: a second `exec` in a file that had one is new code to look at.
    pub findings_added: Vec<Finding>,
    /// Findings the old version had and the new one does not.
    pub findings_removed: Vec<Finding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Highlight {
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionChanges {
    pub network_added: Vec<String>,
    pub network_removed: Vec<String>,
    pub folders_added: Vec<String>,
    pub folders_removed: Vec<String>,
    pub secrets_added: Vec<String>,
    pub secrets_removed: Vec<String>,
    /// The new value, when it changed.
    pub sap: Option<bool>,
}

impl PermissionChanges {
    /// Whether the new version may do something the old one could not.
    pub fn widened(&self) -> bool {
        !self.network_added.is_empty()
            || !self.folders_added.is_empty()
            || !self.secrets_added.is_empty()
            || self.sap == Some(true)
    }

    pub fn is_empty(&self) -> bool {
        *self == PermissionChanges::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageChange {
    pub name: String,
    /// `None`: added.
    pub old: Option<String>,
    /// `None`: removed.
    pub new: Option<String>,
}

impl PackageChange {
    /// The locked version went down (false when a version is not one PEP 440 describes).
    pub fn downgraded(&self) -> bool {
        match (&self.old, &self.new) {
            (Some(o), Some(n)) => compare_versions(n, o) == Some(Ordering::Less),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub status: FileStatus,
    /// A program, library or compiled file, or anything that is not text: it cannot be read.
    pub binary: bool,
    pub lines_added: usize,
    pub lines_removed: usize,
}

impl AppDiff {
    pub fn is_empty(&self) -> bool {
        self.permissions.is_empty()
            && self.declared.is_empty()
            && self.packages.is_empty()
            && self.files.is_empty()
    }
}

/// Compares two versions of an app. Both are scanned, each against its own declarations.
pub fn diff(old: &AppFolder, new: &AppFolder) -> Result<AppDiff> {
    let (om, nm) = (old.manifest()?, new.manifest()?);
    let mut d = AppDiff {
        old_version: om.version.to_string(),
        new_version: nm.version.to_string(),
        permissions: permission_changes(&om.permissions, &nm.permissions),
        declared: declared_changes(&om, &nm),
        ..AppDiff::default()
    };

    let lock = |f: &AppFolder| {
        f.read(UV_LOCK)
            .map(|b| deps::packages(&String::from_utf8_lossy(&b)))
            .unwrap_or_default()
    };
    d.packages = package_changes(&lock(old), &lock(new));
    d.files = file_changes(old, new);

    let old_scan = scan::scan(
        &old.root,
        old.files.iter().map(String::as_str),
        &om.permissions,
    );
    let new_scan = scan::scan(
        &new.root,
        new.files.iter().map(String::as_str),
        &nm.permissions,
    );
    let key = |f: &Finding| (f.rule.clone(), f.path.clone());
    let old_counts: HashMap<_, _> = old_scan
        .findings
        .iter()
        .map(|f| (key(f), f.occurrences))
        .collect();
    let new_keys: HashSet<_> = new_scan.findings.iter().map(key).collect();
    d.findings_added = new_scan
        .findings
        .into_iter()
        .filter(|f| old_counts.get(&key(f)).is_none_or(|n| f.occurrences > *n))
        .collect();
    d.findings_removed = old_scan
        .findings
        .into_iter()
        .filter(|f| !new_keys.contains(&key(f)))
        .collect();

    d.highlights = highlights(&d);
    Ok(d)
}

/// A unified diff of one text file between the two versions, for reviewers. `None` when the
/// file is the same, binary, or too large.
pub fn file_text_diff(old: &AppFolder, new: &AppFolder, path: &str) -> Option<String> {
    let a = old.read(path).unwrap_or_default();
    let b = new.read(path).unwrap_or_default();
    if a == b {
        return None;
    }
    let (a, b) = (text(path, &a)?, text(path, &b)?);
    let diff = TextDiff::from_lines(a, b);
    Some(
        diff.unified_diff()
            .context_radius(3)
            .header(&format!("a/{path}"), &format!("b/{path}"))
            .to_string(),
    )
}

fn permission_changes(old: &Permissions, new: &Permissions) -> PermissionChanges {
    let sets = |a: &[String], b: &[String]| -> (Vec<String>, Vec<String>) {
        let (a, b): (BTreeSet<_>, BTreeSet<_>) = (a.iter().collect(), b.iter().collect());
        (
            b.difference(&a).map(|s| s.to_string()).collect(),
            a.difference(&b).map(|s| s.to_string()).collect(),
        )
    };
    let (network_added, network_removed) = sets(&old.network, &new.network);
    let (folders_added, folders_removed) = sets(&old.folders, &new.folders);
    let (secrets_added, secrets_removed) = sets(&old.secrets, &new.secrets);
    PermissionChanges {
        network_added,
        network_removed,
        folders_added,
        folders_removed,
        secrets_added,
        secrets_removed,
        sap: (old.sap != new.sap).then_some(new.sap),
    }
}

fn declared_changes(old: &Manifest, new: &Manifest) -> Vec<String> {
    let mut out = Vec::new();
    let mut changed = |what: &str, a: String, b: String| {
        if a != b {
            out.push(format!("{what}: {} → {}", or_none(&a), or_none(&b)));
        }
    };
    changed("kind", old.kind.as_str().into(), new.kind.as_str().into());
    changed("starts with", old.entry.to_string(), new.entry.to_string());
    changed(
        "needs installed",
        old.requires.join(", "),
        new.requires.join(", "),
    );
    changed(
        "runs on",
        old.platforms.join(", "),
        new.platforms.join(", "),
    );
    changed(
        "Python",
        old.requires_python.clone().unwrap_or_default(),
        new.requires_python.clone().unwrap_or_default(),
    );
    changed(
        "time limit",
        old.timeout_seconds
            .map(|s| format!("{s} s"))
            .unwrap_or_default(),
        new.timeout_seconds
            .map(|s| format!("{s} s"))
            .unwrap_or_default(),
    );
    let names = |m: &Manifest| -> (String, String) {
        let inputs: Vec<_> = m.inputs.iter().map(|i| i.name.as_str()).collect();
        let outputs: Vec<_> = m.outputs.iter().map(|o| o.name.as_str()).collect();
        (inputs.join(", "), outputs.join(", "))
    };
    let ((oi, oo), (ni, no)) = (names(old), names(new));
    changed("inputs", oi, ni);
    changed("outputs", oo, no);
    out
}

fn or_none(s: &str) -> &str {
    if s.is_empty() { "none" } else { s }
}

fn package_changes(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
) -> Vec<PackageChange> {
    let names: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    names
        .into_iter()
        .filter(|n| old.get(*n) != new.get(*n))
        .map(|n| PackageChange {
            name: n.clone(),
            old: old.get(n).cloned(),
            new: new.get(n).cloned(),
        })
        .collect()
}

fn file_changes(old: &AppFolder, new: &AppFolder) -> Vec<FileChange> {
    let olds: BTreeSet<&str> = old.files.iter().map(String::as_str).collect();
    let news: BTreeSet<&str> = new.files.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for path in olds.union(&news) {
        let a = olds
            .contains(path)
            .then(|| old.read(path).unwrap_or_default());
        let b = news
            .contains(path)
            .then(|| new.read(path).unwrap_or_default());
        let status = match (&a, &b) {
            (Some(a), Some(b)) if a == b => continue,
            (Some(_), Some(_)) => FileStatus::Changed,
            (None, Some(_)) => FileStatus::Added,
            _ => FileStatus::Removed,
        };
        let (a, b) = (a.unwrap_or_default(), b.unwrap_or_default());
        let (ta, tb) = (text(path, &a), text(path, &b));
        let mut change = FileChange {
            path: path.to_string(),
            status,
            binary: ta.is_none() || tb.is_none(),
            lines_added: 0,
            lines_removed: 0,
        };
        if let (Some(ta), Some(tb)) = (ta, tb) {
            for c in TextDiff::from_lines(ta, tb).iter_all_changes() {
                match c.tag() {
                    ChangeTag::Insert => change.lines_added += 1,
                    ChangeTag::Delete => change.lines_removed += 1,
                    ChangeTag::Equal => {}
                }
            }
        }
        out.push(change);
    }
    out
}

/// The file as text, or `None` for programs, libraries, compiled code and anything else that
/// is not UTF-8 text (an empty side, for an added or removed file, is text).
fn text<'a>(path: &str, data: &'a [u8]) -> Option<&'a str> {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    if ext.is_some_and(|e| BINARY_EXTENSIONS.contains(&e.as_str()) || e == "pyc")
        || data.len() > MAX_TEXT_BYTES
        || data.contains(&0)
    {
        return None;
    }
    std::str::from_utf8(data).ok()
}

fn highlights(d: &AppDiff) -> Vec<Highlight> {
    let mut out = Vec::new();
    let mut add = |severity, message: String| out.push(Highlight { severity, message });
    let p = &d.permissions;
    if !p.network_added.is_empty() {
        add(
            Severity::Warning,
            format!("Now connects to {}", p.network_added.join(", ")),
        );
    }
    if !p.folders_added.is_empty() {
        add(
            Severity::Warning,
            format!("Now reads and writes {}", p.folders_added.join(", ")),
        );
    }
    if !p.secrets_added.is_empty() {
        add(
            Severity::Warning,
            format!("Now uses the secrets {}", p.secrets_added.join(", ")),
        );
    }
    if p.sap == Some(true) {
        add(Severity::Warning, "Now uses SAP".into());
    }
    for f in &d.findings_added {
        if f.severity != Severity::Info && f.category != scan::Category::Binary {
            add(
                f.severity,
                format!("New in the code: {} ({})", f.message, f.path),
            );
        }
    }
    let binaries: Vec<&str> = d
        .files
        .iter()
        .filter(|f| f.binary && f.status != FileStatus::Removed)
        .map(|f| f.path.as_str())
        .collect();
    if !binaries.is_empty() {
        add(
            Severity::Warning,
            format!(
                "{} that cannot be reviewed: {}",
                count(
                    binaries.len(),
                    "new or changed program or compiled file",
                    "new or changed programs or compiled files"
                ),
                list(&binaries)
            ),
        );
    }
    let added: Vec<&str> = d
        .packages
        .iter()
        .filter(|c| c.old.is_none())
        .map(|c| c.name.as_str())
        .collect();
    if !added.is_empty() {
        add(
            Severity::Warning,
            format!(
                "{}: {}",
                count(added.len(), "new package", "new packages"),
                list(&added)
            ),
        );
    }
    let older: Vec<String> = d
        .packages
        .iter()
        .filter(|c| c.downgraded())
        .map(|c| {
            format!(
                "{} {} → {}",
                c.name,
                c.old.as_deref().unwrap_or(""),
                c.new.as_deref().unwrap_or("")
            )
        })
        .collect();
    if !older.is_empty() {
        add(
            Severity::Warning,
            format!("Goes back to older package versions: {}", older.join(", ")),
        );
    }
    for f in &d.findings_added {
        if f.severity == Severity::Info && f.category != scan::Category::Binary {
            add(
                f.severity,
                format!("New in the code: {} ({})", f.message, f.path),
            );
        }
    }
    let upgraded = d
        .packages
        .iter()
        .filter(|c| c.old.is_some() && c.new.is_some() && !c.downgraded())
        .count();
    if upgraded > 0 {
        add(
            Severity::Info,
            format!("{} updated", count(upgraded, "package", "packages")),
        );
    }
    let removed = p.network_removed.len()
        + p.folders_removed.len()
        + p.secrets_removed.len()
        + usize::from(p.sap == Some(false));
    if removed > 0 {
        add(Severity::Info, "Asks for less than before".into());
    }
    out.sort_by_key(|h| std::cmp::Reverse(h.severity));
    out
}

fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn list(items: &[&str]) -> String {
    const SHOWN: usize = 4;
    let mut s = items[..items.len().min(SHOWN)].join(", ");
    if items.len() > SHOWN {
        s.push_str(&format!(" and {} more", items.len() - SHOWN));
    }
    s
}

/// Compares release numbers part by part (`2.10` > `2.9`); `None` when a part is not a plain
/// number in both, where only PEP 440 could say (`2.0rc1`).
fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    Some(Pep440::parse(a)?.cmp(&Pep440::parse(b)?))
}

/// A PyPI version as PEP 440 orders it: `1.0.dev1 < 1.0a1 < 1.0b2 < 1.0rc1 < 1.0 < 1.0.post1`.
/// A local label (`+cpu`) is ignored: it never decides whether a package went down.
#[derive(Debug, PartialEq, Eq)]
struct Pep440 {
    epoch: u64,
    release: Vec<u64>,
    /// `a` = 0, `b` = 1, `rc` = 2, and its number.
    pre: Option<(u8, u64)>,
    post: Option<u64>,
    dev: Option<u64>,
}

impl Pep440 {
    fn parse(text: &str) -> Option<Pep440> {
        let lower = text.trim().to_ascii_lowercase();
        let v = lower.strip_prefix('v').unwrap_or(&lower);
        let v = v.split('+').next()?;
        let (epoch, v) = match v.split_once('!') {
            Some((e, rest)) => (e.parse().ok()?, rest),
            None => (0, v),
        };
        let end = v
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(v.len());
        let (nums, mut rest) = v.split_at(end);
        let nums = nums.strip_suffix('.').unwrap_or(nums);
        let release: Vec<u64> = nums
            .split('.')
            .map(|p| p.parse().ok())
            .collect::<Option<_>>()?;
        let mut version = Pep440 {
            epoch,
            release,
            pre: None,
            post: None,
            dev: None,
        };
        while !rest.is_empty() {
            rest = rest.trim_start_matches(['.', '-', '_']);
            if rest.is_empty() {
                return None;
            }
            // `1.0-1` is the old spelling of `1.0.post1`.
            if rest.as_bytes()[0].is_ascii_digit() {
                let (n, tail) = number(rest)?;
                version.post.replace(n).is_none().then_some(())?;
                rest = tail;
                continue;
            }
            let word_end = rest
                .find(|c: char| !c.is_ascii_alphabetic())
                .unwrap_or(rest.len());
            let (word, tail) = rest.split_at(word_end);
            let tail = tail.trim_start_matches(['.', '-', '_']);
            let (n, tail) = number(tail).unwrap_or((0, tail));
            rest = tail;
            match word {
                "a" | "alpha" => version.pre = Some((0, n)),
                "b" | "beta" => version.pre = Some((1, n)),
                "c" | "rc" | "pre" | "preview" => version.pre = Some((2, n)),
                "post" | "rev" | "r" => version.post = Some(n),
                "dev" => version.dev = Some(n),
                _ => return None,
            }
        }
        while version.release.len() > 1 && version.release.last() == Some(&0) {
            version.release.pop();
        }
        Some(version)
    }

    /// The part after the release, ordered as PEP 440 says: a bare `.dev` sorts before the
    /// pre-releases, a final release after them, `.post` after the final release.
    fn suffix_key(&self) -> (i8, (u8, u64), i8, u64, i8, u64) {
        let pre = match (self.pre, self.post, self.dev) {
            (None, None, Some(_)) => (-1, (0, 0)),
            (None, ..) => (1, (0, 0)),
            (Some(p), ..) => (0, p),
        };
        let post = self.post.map_or((-1, 0), |n| (0, n));
        let dev = self.dev.map_or((1, 0), |n| (0, n));
        (pre.0, pre.1, post.0, post.1, dev.0, dev.1)
    }
}

fn number(s: &str) -> Option<(u64, &str)> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (n, rest) = s.split_at(end);
    Some((n.parse().ok()?, rest))
}

impl Ord for Pep440 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.epoch
            .cmp(&other.epoch)
            .then_with(|| self.release.cmp(&other.release))
            .then_with(|| self.suffix_key().cmp(&other.suffix_key()))
    }
}

impl PartialOrd for Pep440 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order_as_pep_440() {
        let ascending = [
            "1.0.dev1",
            "1.0a1",
            "1.0a2.dev1",
            "1.0a2",
            "1.0b1",
            "1.0rc1",
            "1.0rc2",
            "1.0",
            "1.0.post1.dev1",
            "1.0.post1",
            "1.0.1",
            "1.1",
            "1.10",
            "2!0.1",
        ];
        for pair in ascending.windows(2) {
            assert_eq!(
                compare_versions(pair[0], pair[1]),
                Some(Ordering::Less),
                "{} < {}",
                pair[0],
                pair[1]
            );
            assert_eq!(compare_versions(pair[1], pair[0]), Some(Ordering::Greater));
        }
        // Spellings and padding that mean the same version.
        for (a, b) in [
            ("1.0", "1.0.0"),
            ("1.0rc1", "1.0.RC1"),
            ("1.0-rc.1", "1.0c1"),
            ("1.0-1", "1.0.post1"),
            ("v1.0beta", "1.0b0"),
            ("1.0+cpu", "1.0"),
        ] {
            assert_eq!(compare_versions(a, b), Some(Ordering::Equal), "{a} = {b}");
        }
        assert_eq!(compare_versions("1.0", "not a version"), None);
        assert_eq!(compare_versions("1.0rc", "1.0rc0"), Some(Ordering::Equal));
    }

    #[test]
    fn a_release_candidate_after_the_release_is_a_downgrade() {
        let change = |old: &str, new: &str| PackageChange {
            name: "x".into(),
            old: Some(old.into()),
            new: Some(new.into()),
        };
        assert!(change("1.0", "1.0rc1").downgraded());
        assert!(change("2.0.post1", "2.0").downgraded());
        assert!(!change("1.0rc1", "1.0").downgraded());
        assert!(!change("1.0rc1", "1.0rc2").downgraded());
    }

    const PYPROJECT_TEMPLATE: &str = r#"[project]
name = "report"
version = "VERSION"
requires-python = ">=3.12"

[tool.uv]
package = false

[tool.devstribution]
schema_version = 1
id = "alice/report"
title = "Report"
entry = "main:run"
PERMISSIONS
"#;

    fn lock(packages: &[(&str, &str)]) -> String {
        let mut s = String::from(
            "version = 1\n\n[[package]]\nname = \"report\"\nversion = \"0.1.0\"\nsource = { virtual = \".\" }\n",
        );
        for (name, version) in packages {
            s.push_str(&format!(
                "\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\nsource = {{ registry = \"https://pypi.org/simple\" }}\n"
            ));
        }
        s
    }

    fn app(
        dir: &Path,
        version: &str,
        permissions: &str,
        packages: &[(&str, &str)],
        files: &[(&str, &[u8])],
    ) -> AppFolder {
        fs::create_dir_all(dir).unwrap();
        let pyproject = PYPROJECT_TEMPLATE
            .replace("VERSION", version)
            .replace("PERMISSIONS", permissions);
        fs::write(dir.join(PYPROJECT), pyproject).unwrap();
        fs::write(dir.join(UV_LOCK), lock(packages)).unwrap();
        for (path, data) in files {
            let p = dir.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, data).unwrap();
        }
        AppFolder::all_files(dir).unwrap()
    }

    const MAIN: &[u8] = b"def run(inputs):\n    return {}\n";

    #[test]
    fn the_same_app_has_no_changes() {
        let t = tempfile::tempdir().unwrap();
        let a = app(
            &t.path().join("a"),
            "1.0.0",
            "",
            &[("openpyxl", "3.1.5")],
            &[("main.py", MAIN)],
        );
        let b = app(
            &t.path().join("b"),
            "1.0.0",
            "",
            &[("openpyxl", "3.1.5")],
            &[("main.py", MAIN)],
        );
        let d = diff(&a, &b).unwrap();
        assert!(d.is_empty(), "{d:?}");
        assert!(d.highlights.is_empty(), "{:?}", d.highlights);
    }

    #[test]
    fn new_permissions_code_and_packages_come_first() {
        let t = tempfile::tempdir().unwrap();
        let a = app(
            &t.path().join("a"),
            "1.0.0",
            "",
            &[("openpyxl", "3.1.5"), ("pandas", "2.2.3")],
            &[("main.py", MAIN), ("old.py", b"x = 1\n")],
        );
        let b = app(
            &t.path().join("b"),
            "1.1.0",
            "[tool.devstribution.permissions]\nnetwork = [\"api.example.com\"]\n",
            &[("openpyxl", "3.1.2"), ("pandas", "2.2.4"), ("httpx", "0.28.1")],
            &[
                ("main.py", b"import subprocess\nimport httpx\n\ndef run(inputs):\n    subprocess.run(['cmd'])\n    return {}\n"),
                ("lib/fast.pyd", b"MZ\x00\x01"),
            ],
        );
        let d = diff(&a, &b).unwrap();
        assert_eq!(
            (d.old_version.as_str(), d.new_version.as_str()),
            ("1.0.0", "1.1.0")
        );
        assert_eq!(d.permissions.network_added, ["api.example.com"]);
        assert!(d.permissions.widened());

        let names: Vec<_> = d.packages.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["httpx", "openpyxl", "pandas"]);
        assert!(d.packages[1].downgraded());
        assert!(!d.packages[2].downgraded());

        let file = |p: &str| d.files.iter().find(|f| f.path == p).unwrap();
        assert_eq!(file("old.py").status, FileStatus::Removed);
        assert_eq!(file("lib/fast.pyd").status, FileStatus::Added);
        assert!(file("lib/fast.pyd").binary);
        let main = file("main.py");
        assert_eq!(main.status, FileStatus::Changed);
        assert_eq!((main.lines_added, main.lines_removed), (4, 0));

        assert!(
            d.findings_added.iter().any(|f| f.rule == "processes"),
            "{:?}",
            d.findings_added
        );

        let text: Vec<_> = d.highlights.iter().map(|h| h.message.as_str()).collect();
        assert!(
            text[0].starts_with("Now connects to api.example.com"),
            "{text:?}"
        );
        assert!(
            text.iter()
                .any(|m| m.contains("cannot be reviewed: lib/fast.pyd")),
            "{text:?}"
        );
        assert!(text.contains(&"1 new package: httpx"), "{text:?}");
        assert!(
            text.iter().any(|m| m.contains("openpyxl 3.1.5 → 3.1.2")),
            "{text:?}"
        );
        assert!(text.contains(&"1 package updated"), "{text:?}");
        // Most severe first.
        assert!(
            d.highlights
                .windows(2)
                .all(|w| w[0].severity >= w[1].severity)
        );
    }

    #[test]
    fn findings_that_only_moved_are_not_new() {
        let t = tempfile::tempdir().unwrap();
        let a = app(
            &t.path().join("a"),
            "1.0.0",
            "",
            &[],
            &[("main.py", b"import subprocess\nsubprocess.run(['x'])\n")],
        );
        let b = app(
            &t.path().join("b"),
            "1.0.1",
            "",
            &[],
            &[(
                "main.py",
                b"# moved down\n\nimport subprocess\nsubprocess.run(['x'])\n",
            )],
        );
        let d = diff(&a, &b).unwrap();
        assert!(d.findings_added.is_empty(), "{:?}", d.findings_added);
        assert!(d.findings_removed.is_empty());
        assert!(d.highlights.is_empty(), "{:?}", d.highlights);
    }

    #[test]
    fn fewer_permissions_and_declared_changes() {
        let t = tempfile::tempdir().unwrap();
        let a = app(
            &t.path().join("a"),
            "1.0.0",
            "[tool.devstribution.permissions]\nnetwork = [\"api.example.com\"]\nsap = true\n",
            &[],
            &[("main.py", MAIN)],
        );
        let b = app(&t.path().join("b"), "2.0.0", "", &[], &[("main.py", MAIN)]);
        let d = diff(&a, &b).unwrap();
        assert!(!d.permissions.widened());
        assert_eq!(d.permissions.network_removed, ["api.example.com"]);
        assert_eq!(d.permissions.sap, Some(false));
        assert_eq!(d.highlights.len(), 1);
        assert_eq!(d.highlights[0].message, "Asks for less than before");
    }

    #[test]
    fn a_text_diff_for_reviewers() {
        let t = tempfile::tempdir().unwrap();
        let a = app(&t.path().join("a"), "1.0.0", "", &[], &[("main.py", MAIN)]);
        let b = app(
            &t.path().join("b"),
            "1.0.1",
            "",
            &[],
            &[("main.py", b"def run(inputs):\n    return {'n': 1}\n")],
        );
        let text = file_text_diff(&a, &b, "main.py").unwrap();
        assert!(text.contains("--- a/main.py"), "{text}");
        assert!(text.contains("-    return {}"), "{text}");
        assert!(text.contains("+    return {'n': 1}"), "{text}");
        assert_eq!(
            file_text_diff(&a, &b, "pyproject.toml").map(|_| ()),
            Some(())
        );
        assert!(file_text_diff(&a, &a, "main.py").is_none());
    }

    #[test]
    fn versions_compare_by_number() {
        assert_eq!(compare_versions("2.10", "2.9"), Some(Ordering::Greater));
        assert_eq!(compare_versions("1.0", "1.0.0"), Some(Ordering::Equal));
        assert_eq!(compare_versions("2.0rc1", "1.0"), Some(Ordering::Greater));
        assert_eq!(compare_versions("2.0-unknown", "1.0"), None);
    }
}
