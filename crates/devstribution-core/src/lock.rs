//! Checks on `uv.lock`: every dependency the app installs must be a wheel from a package index,
//! available for every platform the app declares. This is what makes
//! `uv sync --frozen --no-build` succeed on the user's machine instead of failing there.

use std::collections::{BTreeSet, HashMap, VecDeque};

use serde::Deserialize;

use crate::{Error, Result};

#[derive(Debug, Default)]
pub struct LockReport {
    /// Problems that make the app fail to install somewhere it claims to support.
    pub errors: Vec<String>,
    /// Possible problems the check cannot decide (dependencies behind environment markers).
    pub warnings: Vec<String>,
    /// Number of packages that production installs need.
    pub runtime_packages: usize,
}

#[derive(Deserialize)]
struct RawLock {
    version: Option<u32>,
    #[serde(default)]
    package: Vec<RawPackage>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawPackage {
    name: String,
    version: Option<String>,
    #[serde(default)]
    source: HashMap<String, toml::Value>,
    #[serde(default)]
    dependencies: Vec<RawDep>,
    #[serde(default)]
    optional_dependencies: HashMap<String, Vec<RawDep>>,
    #[serde(default)]
    wheels: Vec<RawWheel>,
}

#[derive(Deserialize)]
struct RawDep {
    name: String,
    marker: Option<String>,
    #[serde(default)]
    extra: Vec<String>,
}

#[derive(Deserialize)]
struct RawWheel {
    url: Option<String>,
    path: Option<String>,
    filename: Option<String>,
    hash: Option<String>,
}

/// The only package index installed apps may use for now. A company policy will be able to
/// name its own mirror (see docs/plan.md, "Policy").
pub const ALLOWED_INDEX: &str = "https://pypi.org/simple";
/// Where that index serves its files.
const ALLOWED_FILES: &str = "https://files.pythonhosted.org/packages/";

/// The install-time gate on `uv.lock`, run on every package before anything is installed:
/// besides the app itself (`source = { virtual = "." }`), every package must come from the
/// allowed index, and every wheel from that index's file host, over HTTPS, with a SHA-256
/// hash. So a package cannot make uv download from anywhere its publisher likes, or install
/// from a path, a URL, git, or a folder.
pub fn check_sources(lock: &str) -> Result<()> {
    let raw: RawLock = toml::from_str(lock).map_err(|e| Error::Lock(e.to_string()))?;
    if raw.version != Some(1) {
        return Err(Error::Lock(format!(
            "lockfile version {:?} is not supported",
            raw.version
        )));
    }
    let mut roots = 0;
    for p in &raw.package {
        let label = format!("{} {}", p.name, p.version.as_deref().unwrap_or(""));
        let label = label.trim_end();
        let source = |key: &str| p.source.get(key).and_then(toml::Value::as_str);
        if p.source.len() == 1 && source("virtual") == Some(".") {
            roots += 1;
            if !p.wheels.is_empty() {
                return Err(Error::Lock(format!(
                    "`{label}` is the app itself but lists wheels"
                )));
            }
            continue;
        }
        let index = source("registry").map(|r| r.trim_end_matches('/'));
        if p.source.len() != 1 || index != Some(ALLOWED_INDEX) {
            return Err(Error::Lock(format!(
                "`{label}` does not come from {ALLOWED_INDEX}; installed apps may only use \
                 packages from that index (lock the app again with `uv lock` against PyPI)"
            )));
        }
        for w in &p.wheels {
            let url_ok = w.url.as_deref().is_some_and(|u| {
                u.starts_with(ALLOWED_FILES) && !u.contains(['?', '#', '\\']) && !u.contains("/../")
            });
            let hash_ok = w.hash.as_deref().is_some_and(|h| {
                h.strip_prefix("sha256:")
                    .is_some_and(|x| x.len() == 64 && x.bytes().all(|b| b.is_ascii_hexdigit()))
            });
            if w.path.is_some() || !url_ok || !hash_ok {
                return Err(Error::Lock(format!(
                    "a wheel of `{label}` is not a file from {ALLOWED_FILES} with a SHA-256 hash"
                )));
            }
        }
    }
    if roots != 1 {
        return Err(Error::Lock(
            "the lockfile must contain exactly one entry for the app itself".into(),
        ));
    }
    Ok(())
}

impl RawWheel {
    fn filename(&self) -> Option<&str> {
        let s = self
            .filename
            .as_deref()
            .or(self.url.as_deref())
            .or(self.path.as_deref())?;
        Some(
            s.rsplit(['/', '\\'])
                .next()
                .unwrap_or(s)
                .split(['?', '#'])
                .next()
                .unwrap_or(s),
        )
    }
}

/// Whether a dependency edge applies on a platform, judged from its marker.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Applies {
    Yes,
    No,
    Maybe,
}

pub fn check_lock(lock: &str, project_name: &str, platforms: &[String]) -> Result<LockReport> {
    let raw: RawLock = toml::from_str(lock).map_err(|e| Error::Lock(e.to_string()))?;
    if raw.version != Some(1) {
        return Err(Error::Lock(format!(
            "lockfile version {:?} is not supported; update uv and run `uv lock`",
            raw.version
        )));
    }
    let roots: Vec<&RawPackage> = raw
        .package
        .iter()
        .filter(|p| p.source.get("virtual").and_then(|v| v.as_str()) == Some("."))
        .collect();
    match roots.as_slice() {
        [root] if root.name == normalize(project_name) => {}
        _ => {
            return Err(Error::Lock(format!(
                "no entry for the project `{project_name}` with `source = {{ virtual = \".\" }}`; \
                 check that `[tool.uv] package = false` is set and run `uv lock`"
            )));
        }
    }
    let mut by_name: HashMap<&str, Vec<&RawPackage>> = HashMap::new();
    for p in &raw.package {
        by_name.entry(p.name.as_str()).or_default().push(p);
    }

    let mut report = LockReport::default();
    let mut needed_anywhere = BTreeSet::new();
    for platform in platforms {
        let definitely = reachable(roots[0], &by_name, platform, false);
        let possibly = reachable(roots[0], &by_name, platform, true);
        for name in &possibly {
            needed_anywhere.insert(*name);
            let sure = definitely.contains(name);
            for p in by_name.get(name).into_iter().flatten() {
                if let Some(problem) = problem(p, platform) {
                    if sure {
                        report.errors.push(problem);
                    } else {
                        report.warnings.push(format!(
                            "{problem} (only needed under an environment marker; check it applies)"
                        ));
                    }
                }
            }
        }
    }
    report.errors.dedup();
    report.warnings.dedup();
    report.runtime_packages = needed_anywhere.len();
    Ok(report)
}

/// Package names needed at runtime on `platform`, following normal (not dev) dependencies and
/// the optional dependencies activated by extras. With `include_maybe`, edges whose marker the
/// check cannot evaluate are followed too.
fn reachable<'a>(
    root: &'a RawPackage,
    by_name: &HashMap<&'a str, Vec<&'a RawPackage>>,
    platform: &str,
    include_maybe: bool,
) -> BTreeSet<&'a str> {
    // Nodes are (package, extra): `None` follows the package's dependencies, `Some(extra)` the
    // optional dependencies of that extra.
    let mut visited: BTreeSet<(*const RawPackage, Option<&str>)> = BTreeSet::new();
    let mut queue: VecDeque<(&RawPackage, Option<&str>)> = VecDeque::from([(root, None)]);
    let mut needed = BTreeSet::new();
    while let Some((pkg, extra)) = queue.pop_front() {
        if !visited.insert((pkg as *const _, extra)) {
            continue;
        }
        if !std::ptr::eq(pkg, root) {
            needed.insert(pkg.name.as_str());
        }
        let deps = match extra {
            None => pkg.dependencies.as_slice(),
            Some(e) => pkg
                .optional_dependencies
                .get(e)
                .map_or(&[][..], Vec::as_slice),
        };
        for dep in deps {
            let applies = dep
                .marker
                .as_deref()
                .map_or(Applies::Yes, |m| applies(m, platform));
            if applies == Applies::No || (applies == Applies::Maybe && !include_maybe) {
                continue;
            }
            for target in by_name.get(dep.name.as_str()).into_iter().flatten() {
                queue.push_back((target, None));
                for e in &dep.extra {
                    queue.push_back((target, Some(e.as_str())));
                }
            }
        }
    }
    needed
}

fn problem(p: &RawPackage, platform: &str) -> Option<String> {
    let label = format!("{} {}", p.name, p.version.as_deref().unwrap_or(""));
    let label = label.trim_end();
    if !p.source.contains_key("registry") {
        let kind = p
            .source
            .keys()
            .next()
            .map_or("an unknown source", String::as_str);
        return Some(format!(
            "`{label}` comes from {kind}; only packages from a package index are allowed"
        ));
    }
    if p.wheels.is_empty() {
        return Some(format!(
            "`{label}` has no wheels (source only), so it would need a build step; \
             pick a version that publishes wheels"
        ));
    }
    let ok = p
        .wheels
        .iter()
        .filter_map(RawWheel::filename)
        .filter_map(wheel_platform_tags)
        .any(|tags| tags.iter().any(|t| tag_supports(t, platform)));
    (!ok).then(|| format!("`{label}` has no wheel for {platform}"))
}

/// Platform tags of a wheel file name: `name-ver(-build)?-py-abi-plat.whl` → `plat` split by `.`.
fn wheel_platform_tags(filename: &str) -> Option<Vec<&str>> {
    let stem = filename.strip_suffix(".whl")?;
    let plat = stem.rsplit('-').next()?;
    Some(plat.split('.').collect())
}

fn tag_supports(tag: &str, platform: &str) -> bool {
    tag == "any"
        || match platform {
            "windows" => tag == "win_amd64",
            "macos" => tag.starts_with("macosx_"),
            "linux" => {
                (tag.starts_with("manylinux")
                    || tag.starts_with("musllinux")
                    || tag.starts_with("linux_"))
                    && tag.ends_with("x86_64")
            }
            _ => false,
        }
}

/// Very small marker evaluation: only clauses naming the operating system are understood.
fn applies(marker: &str, platform: &str) -> Applies {
    let names: &[&str] = match platform {
        "windows" => &["win32", "windows", "nt"],
        "macos" => &["darwin"],
        "linux" => &["linux", "posix"],
        _ => &[],
    };
    // One space between words, whatever the lockfile used (tabs, several spaces), so that
    // `or`/`and` are always seen.
    let m = marker
        .to_ascii_lowercase()
        .replace('"', "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let os_keys = ["sys_platform", "platform_system", "os_name"];
    if !os_keys.iter().any(|k| m.contains(k)) {
        return Applies::Maybe;
    }
    if m.contains(" or ") || m.contains(" and ") {
        return Applies::Maybe;
    }
    let mentions_us = names.iter().any(|n| m.contains(&format!("'{n}'")));
    match (m.contains("=="), m.contains("!="), mentions_us) {
        (true, false, true) => Applies::Yes,
        (true, false, false) => Applies::No,
        (false, true, true) => Applies::No,
        (false, true, false) => Applies::Yes,
        _ => Applies::Maybe,
    }
}

fn normalize(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let c = if matches!(c, '_' | '.') {
            '-'
        } else {
            c.to_ascii_lowercase()
        };
        if !(c == '-' && out.ends_with('-')) {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wheel(name: &str) -> String {
        format!("{{ url = \"https://files.example/{name}\", hash = \"sha256:00\", size = 1 }}")
    }

    fn lock(extra_packages: &str) -> String {
        format!(
            r#"version = 1
revision = 3
requires-python = ">=3.11"

[[package]]
name = "my-app"
version = "0.1.0"
source = {{ virtual = "." }}
dependencies = [
    {{ name = "pure" }},
    {{ name = "native" }},
    {{ name = "pywin32", marker = "sys_platform == 'win32'" }},
    {{ name = "requests", extra = ["socks"] }},
]

[package.dev-dependencies]
dev = [{{ name = "devonly" }}]

[[package]]
name = "pure"
version = "1.0"
source = {{ registry = "https://pypi.org/simple" }}
wheels = [{pure}]

[[package]]
name = "native"
version = "2.0"
source = {{ registry = "https://pypi.org/simple" }}
wheels = [{native_win}, {native_linux}]

[[package]]
name = "pywin32"
version = "311"
source = {{ registry = "https://pypi.org/simple" }}
wheels = [{pywin32}]

[[package]]
name = "requests"
version = "2.0"
source = {{ registry = "https://pypi.org/simple" }}
wheels = [{requests}]

[package.optional-dependencies]
socks = [{{ name = "pysocks" }}]

[[package]]
name = "pysocks"
version = "1.0"
source = {{ registry = "https://pypi.org/simple" }}
wheels = [{pysocks}]

[[package]]
name = "devonly"
version = "1.0"
source = {{ registry = "https://pypi.org/simple" }}
sdist = {{ url = "https://files.example/devonly-1.0.tar.gz" }}
{extra_packages}"#,
            pure = wheel("pure-1.0-py3-none-any.whl"),
            native_win = wheel("native-2.0-cp313-cp313-win_amd64.whl"),
            native_linux =
                wheel("native-2.0-cp313-cp313-manylinux_2_17_x86_64.manylinux2014_x86_64.whl"),
            pywin32 = wheel("pywin32-311-cp313-cp313-win_amd64.whl"),
            requests = wheel("requests-2.0-py3-none-any.whl"),
            pysocks = wheel("PySocks-1.0-py3-none-any.whl"),
        )
    }

    fn platforms(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn accepts_wheels_and_ignores_dev_dependencies() {
        let r = check_lock(&lock(""), "My_App", &platforms(&["windows", "linux"])).unwrap();
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(r.runtime_packages, 5);
    }

    #[test]
    fn refuses_missing_platform_wheels() {
        let r = check_lock(&lock(""), "my-app", &platforms(&["macos"])).unwrap();
        assert_eq!(r.errors, ["`native 2.0` has no wheel for macos"]);
    }

    #[test]
    fn refuses_source_only_dependency_reached_through_an_extra() {
        let text = lock("").replace(
            &format!("wheels = [{}]", wheel("PySocks-1.0-py3-none-any.whl")),
            "sdist = { url = \"https://files.example/PySocks-1.0.tar.gz\" }",
        );
        let r = check_lock(&text, "my-app", &platforms(&["windows"])).unwrap();
        assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
        assert!(r.errors[0].contains("pysocks"));
    }

    #[test]
    fn refuses_git_dependencies_and_stale_locks() {
        let text = lock("").replace(
            "name = \"pure\"\nversion = \"1.0\"\nsource = { registry = \"https://pypi.org/simple\" }",
            "name = \"pure\"\nversion = \"1.0\"\nsource = { git = \"https://github.com/x/pure\" }",
        );
        let r = check_lock(&text, "my-app", &platforms(&["windows"])).unwrap();
        assert!(r.errors[0].contains("comes from git"), "{:?}", r.errors);
        assert!(check_lock(&lock(""), "other-name", &platforms(&["windows"])).is_err());
    }

    #[test]
    fn markers() {
        assert!(applies("sys_platform == 'win32'", "windows") == Applies::Yes);
        assert!(applies("sys_platform == 'win32'", "linux") == Applies::No);
        assert!(applies("platform_system != \"Windows\"", "windows") == Applies::No);
        assert!(applies("python_full_version < '3.12'", "windows") == Applies::Maybe);
        assert!(
            applies(
                "sys_platform == 'linux' or sys_platform == 'darwin'",
                "linux"
            ) == Applies::Maybe
        );
    }

    fn sourced(source: &str, wheel: &str) -> String {
        format!(
            r#"version = 1
[[package]]
name = "my-app"
version = "0.1.0"
source = {{ virtual = "." }}

[[package]]
name = "dep"
version = "1.0"
source = {source}
wheels = [{wheel}]
"#
        )
    }

    const PYPI_WHEEL: &str = "{ url = \"https://files.pythonhosted.org/packages/aa/dep-1.0-py3-none-any.whl\", hash = \"sha256:0000000000000000000000000000000000000000000000000000000000000000\" }";

    #[test]
    fn install_gate_accepts_pypi_wheels_only() {
        check_sources(&sourced(
            "{ registry = \"https://pypi.org/simple\" }",
            PYPI_WHEEL,
        ))
        .unwrap();
        for (source, wheel) in [
            ("{ registry = \"http://evil.example/simple\" }", PYPI_WHEEL),
            (
                "{ registry = \"https://pypi.org/simple\", url = \"x\" }",
                PYPI_WHEEL,
            ),
            (
                "{ url = \"https://evil.example/dep-1.0-py3-none-any.whl\" }",
                PYPI_WHEEL,
            ),
            ("{ path = \"dep-1.0-py3-none-any.whl\" }", PYPI_WHEEL),
            ("{ git = \"https://github.com/x/y\" }", PYPI_WHEEL),
            ("{ virtual = \"../other\" }", PYPI_WHEEL),
            (
                "{ registry = \"https://pypi.org/simple\" }",
                "{ url = \"https://evil.example/packages/dep-1.0-py3-none-any.whl\", hash = \"sha256:0000000000000000000000000000000000000000000000000000000000000000\" }",
            ),
            (
                "{ registry = \"https://pypi.org/simple\" }",
                "{ url = \"https://files.pythonhosted.org/packages/aa/dep-1.0-py3-none-any.whl\" }",
            ),
            (
                "{ registry = \"https://pypi.org/simple\" }",
                "{ path = \"dep-1.0-py3-none-any.whl\", hash = \"sha256:0000000000000000000000000000000000000000000000000000000000000000\" }",
            ),
        ] {
            assert!(
                check_sources(&sourced(source, wheel)).is_err(),
                "accepted {source} {wheel}"
            );
        }
    }

    #[test]
    fn markers_with_tabs_are_still_understood() {
        assert!(
            applies(
                "sys_platform == 'win32'\tor\tsys_platform == 'linux'",
                "windows"
            ) == Applies::Maybe
        );
        assert!(applies("sys_platform  ==  'darwin'", "windows") == Applies::No);
    }
}
