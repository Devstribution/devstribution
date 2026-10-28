//! Managed mode: the policy IT sets for the launcher, with no server involved.
//!
//! The launcher is in managed mode when `HKLM\SOFTWARE\Policies\devstribution` exists (pushed
//! by GPO or Intune; a user cannot write under HKLM). Otherwise it is in free mode, as before.
//! The key's values, all optional:
//!
//! | Value | Type | |
//! |---|---|---|
//! | `PolicyFile` | REG_SZ | A TOML policy file (below), on a local disk or a network share |
//! | `TrustFolder` | REG_SZ | A folder of `.trust` files: every author in it is approved |
//! | `Authors` | REG_MULTI_SZ | Approved authors inline, one `<namespace> <public key>` per line |
//! | `Contact` | REG_SZ | Who users should ask, shown when something is refused |
//! | `DryRun` | REG_DWORD | 1: only log what would be refused, refuse nothing |
//! | `ScanDanger` | REG_SZ | `refuse` (default) or `ask`: a package the code scan flags as malware-like |
//! | `UsersMayAllowGuardRules` | REG_DWORD | 1: a user may let a stopped app run past the guard rule that stopped it |
//! | `GuardReportOnly` | REG_MULTI_SZ | Guard rules that only report, for every app |
//! | `MinLauncherVersion` | REG_SZ | Older launchers install and run nothing |
//!
//! The policy file has the same settings in `snake_case`, plus `trust_files` and `[[authors]]`
//! tables in the `.trust` format; a registry value wins over the file. The file, the trust
//! folder and the `.trust` files must be files the user cannot change: the launcher refuses
//! them otherwise. A policy that cannot be read is not ignored: the launcher stays in managed
//! mode, approves no one, and says why (it fails closed).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use devstribution_core::manifest::Version;
use devstribution_core::trust::TrustedAuthor;
use serde::{Deserialize, Serialize};

use crate::time::now_secs;

/// The registry key that turns managed mode on.
pub const POLICY_KEY: &str = r"SOFTWARE\Policies\devstribution";

/// What happens to a package the code scan or the dependency check flags as malware-like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ScanDanger {
    /// Not installed.
    #[default]
    Refuse,
    /// The user decides, as in free mode.
    Ask,
}

#[derive(Debug, Clone, Serialize)]
pub struct Policy {
    /// Where the policy came from, as shown in Settings.
    pub source: String,
    pub authors: Vec<TrustedAuthor>,
    pub contact: Option<String>,
    pub dry_run: bool,
    pub scan_danger: ScanDanger,
    pub users_may_allow_guard_rules: bool,
    pub guard_report_only: Vec<String>,
    pub min_launcher_version: Option<String>,
    /// Whether the launcher may look for its own updates and install them. IT that deploys the
    /// launcher itself turns this off.
    pub launcher_updates: bool,
    /// What could not be read (a policy file, a `.trust` file). Authors that could not be read
    /// are not approved.
    pub problems: Vec<String>,
}

/// The TOML policy file. Unknown fields are refused: a misspelt setting must not be silently
/// ignored.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyFile {
    contact: Option<String>,
    dry_run: Option<bool>,
    scan_danger: Option<ScanDanger>,
    users_may_allow_guard_rules: Option<bool>,
    #[serde(default)]
    guard_report_only: Vec<String>,
    min_launcher_version: Option<String>,
    launcher_updates: Option<bool>,
    trust_folder: Option<PathBuf>,
    #[serde(default)]
    trust_files: Vec<PathBuf>,
    #[serde(default)]
    authors: Vec<TrustedAuthor>,
}

/// The registry values under [`POLICY_KEY`], or a stand-in for them in tests.
pub trait PolicyValues {
    fn string(&self, name: &str) -> Option<String>;
    fn dword(&self, name: &str) -> Option<u32>;
    fn multi(&self, name: &str) -> Option<Vec<String>>;
}

/// How files named by the policy are read.
pub struct Loader {
    /// Refuse files and folders the current user can write (always on for the real policy).
    pub admin_only: bool,
}

impl Policy {
    /// The policy of this computer: `None` in free mode.
    pub fn from_system() -> Option<Policy> {
        let values = system::open()?;
        Some(Policy::from_values(
            &format!(r"HKLM\{POLICY_KEY}"),
            &values,
            &Loader { admin_only: true },
        ))
    }

    /// Builds the policy from registry values (and the files they name). Never fails: what
    /// cannot be read goes to `problems`, and whoever it would have approved is not approved.
    pub fn from_values(source: &str, values: &dyn PolicyValues, loader: &Loader) -> Policy {
        let mut problems = Vec::new();
        let mut source = source.to_string();
        let file = match values.string("PolicyFile").filter(|s| !s.trim().is_empty()) {
            Some(path) => {
                source = format!("{source} · {path}");
                match loader.read_policy_file(Path::new(path.trim())) {
                    Ok(file) => file,
                    Err(e) => {
                        problems.push(format!("{e:#}"));
                        PolicyFile::default()
                    }
                }
            }
            None => PolicyFile::default(),
        };

        let mut authors = file.authors;
        for line in values.multi("Authors").unwrap_or_default() {
            match inline_author(&line) {
                Ok(a) => authors.push(a),
                Err(e) => problems.push(format!("registry value Authors: {e}")),
            }
        }
        let mut trust_files = file.trust_files;
        let folder = values
            .string("TrustFolder")
            .filter(|s| !s.trim().is_empty())
            .map(|s| PathBuf::from(s.trim()))
            .or(file.trust_folder);
        if let Some(folder) = folder {
            match loader.list_trust_folder(&folder) {
                Ok(files) => trust_files.extend(files),
                Err(e) => problems.push(format!("{e:#}")),
            }
        }
        for path in trust_files {
            match loader.read_trust_file(&path) {
                Ok(a) => authors.push(a),
                Err(e) => problems.push(format!("{e:#}")),
            }
        }

        let min_launcher_version = values
            .string("MinLauncherVersion")
            .or(file.min_launcher_version)
            .filter(|v| !v.trim().is_empty());
        if let Some(v) = &min_launcher_version
            && v.trim().parse::<Version>().is_err()
        {
            problems.push(format!(
                "MinLauncherVersion `{v}` is not a version: no launcher is recent enough"
            ));
        }
        let scan_danger = match values.string("ScanDanger") {
            Some(s) if s.trim().eq_ignore_ascii_case("ask") => ScanDanger::Ask,
            Some(s) if s.trim().eq_ignore_ascii_case("refuse") => ScanDanger::Refuse,
            Some(s) => {
                problems.push(format!(
                    "ScanDanger `{s}` is neither `refuse` nor `ask`: refusing"
                ));
                ScanDanger::Refuse
            }
            None => file.scan_danger.unwrap_or_default(),
        };
        let mut guard_report_only = file.guard_report_only;
        guard_report_only.extend(values.multi("GuardReportOnly").unwrap_or_default());
        guard_report_only.retain(|r| !r.trim().is_empty());

        Policy {
            source,
            authors: merge_authors(authors),
            contact: values
                .string("Contact")
                .or(file.contact)
                .filter(|c| !c.trim().is_empty()),
            dry_run: values
                .dword("DryRun")
                .map(|v| v != 0)
                .or(file.dry_run)
                .unwrap_or(false),
            scan_danger,
            users_may_allow_guard_rules: values
                .dword("UsersMayAllowGuardRules")
                .map(|v| v != 0)
                .or(file.users_may_allow_guard_rules)
                .unwrap_or(false),
            guard_report_only,
            min_launcher_version,
            launcher_updates: values
                .dword("LauncherUpdates")
                .map(|v| v != 0)
                .or(file.launcher_updates)
                .unwrap_or(true),
            problems,
        }
    }

    /// The approved author for `namespace`, if `key` is one of its keys (compared in full).
    pub fn approves(&self, namespace: &str, key: &str) -> bool {
        self.authors
            .iter()
            .any(|a| a.namespace == namespace && a.has_key(key))
    }

    /// Whether any author is approved for the namespace (with whatever key).
    pub fn knows(&self, namespace: &str) -> bool {
        self.authors.iter().any(|a| a.namespace == namespace)
    }

    /// Why this launcher may not install or run anything, if it is older than the policy asks.
    pub fn launcher_refusal(&self, launcher_version: &str) -> Option<String> {
        let min = self.min_launcher_version.as_deref()?.trim();
        let ok = match (min.parse::<Version>(), launcher_version.parse::<Version>()) {
            (Ok(min), Ok(have)) => have >= min,
            _ => false,
        };
        (!ok).then(|| {
            format!(
                "your organisation requires devstribution {min} or newer, and this is \
                 {launcher_version}: ask for the launcher to be updated"
            )
        })
    }

    /// Who to ask, as one sentence for messages.
    pub fn ask_whom(&self) -> String {
        match &self.contact {
            Some(c) => format!("Ask {c}."),
            None => "Ask your IT department.".into(),
        }
    }
}

/// One author per namespace: keys of the same namespace from several sources are combined.
fn merge_authors(authors: Vec<TrustedAuthor>) -> Vec<TrustedAuthor> {
    let mut out: Vec<TrustedAuthor> = Vec::new();
    for a in authors {
        match out.iter_mut().find(|o| o.namespace == a.namespace) {
            Some(o) => {
                for k in a.keys {
                    if !o.has_key(&k.key) {
                        o.keys.push(k);
                    }
                }
                o.name = o.name.take().or(a.name);
                o.contact = o.contact.take().or(a.contact);
            }
            None => out.push(a),
        }
    }
    out.sort_by(|a, b| a.namespace.cmp(&b.namespace));
    out
}

/// `<namespace> <public key>` from the `Authors` registry value.
fn inline_author(line: &str) -> Result<TrustedAuthor, String> {
    let mut parts = line.split_whitespace();
    let (Some(namespace), Some(key), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(format!("`{line}` is not `<namespace> <public key>`"));
    };
    TrustedAuthor::new(namespace, key)
}

impl Loader {
    fn read_policy_file(&self, path: &Path) -> Result<PolicyFile> {
        let text = self.read_protected(path)?;
        toml::from_str(&text).with_context(|| format!("the policy file {}", path.display()))
    }

    fn read_trust_file(&self, path: &Path) -> Result<TrustedAuthor> {
        let text = self.read_protected(path)?;
        TrustedAuthor::parse(&text).map_err(|e| anyhow!("the trust file {}: {e}", path.display()))
    }

    fn list_trust_folder(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        if !dir.is_absolute() {
            bail!("the trust folder {} is not a full path", dir.display());
        }
        if self.admin_only && user_can_write(dir) {
            bail!(
                "the trust folder {} can be changed by users, so its authors are not approved",
                dir.display()
            );
        }
        let mut files: Vec<PathBuf> = fs::read_dir(dir)
            .with_context(|| format!("cannot read the trust folder {}", dir.display()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("trust"))
            })
            .collect();
        files.sort();
        Ok(files)
    }

    /// Reads a file the policy names, if only administrators can change it (and its folder).
    fn read_protected(&self, path: &Path) -> Result<String> {
        if !path.is_absolute() {
            bail!("{} is not a full path", path.display());
        }
        let text =
            fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
        if self.admin_only {
            let folder = path.parent().unwrap_or(path);
            if user_can_write(path) || user_can_write(folder) {
                bail!(
                    "{} can be changed by users, so the launcher does not use it: only \
                     administrators may be able to change it",
                    path.display()
                );
            }
        }
        Ok(text)
    }
}

/// Whether the current user could change `path`: write a file, or add files to a folder. Asks
/// Windows by opening it for writing (nothing is written), which accounts for every group the
/// user is in and for share permissions on a network folder.
fn user_can_write(path: &Path) -> bool {
    #[cfg(windows)]
    {
        // `std` opens folders too (it always asks for backup semantics); for a folder, write
        // access is the right to add files and folders to it.
        fs::OpenOptions::new().write(true).open(path).is_ok()
    }
    #[cfg(not(windows))]
    {
        if path.is_dir() {
            fs::metadata(path).is_ok_and(|m| !m.permissions().readonly())
        } else {
            fs::OpenOptions::new().write(true).open(path).is_ok()
        }
    }
}

/// One line of the policy log (`state/policy.log`, JSON lines), which IT can collect: what
/// the policy refused, or in a dry run would have refused.
#[derive(Debug, Serialize)]
pub struct LogEntry<'a> {
    pub at: u64,
    /// `refused` or `would_refuse` (dry run).
    pub decision: &'a str,
    /// `install` or `run`.
    pub action: &'a str,
    pub app: &'a str,
    pub version: &'a str,
    pub key: &'a str,
    pub reasons: &'a [String],
}

pub fn log(file: &Path, entry: &LogEntry) {
    let Ok(mut line) = serde_json::to_string(entry) else {
        return;
    };
    line.push('\n');
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

pub fn log_now<'a>(
    decision: &'a str,
    action: &'a str,
    app: &'a str,
    version: &'a str,
    key: &'a str,
    reasons: &'a [String],
) -> LogEntry<'a> {
    LogEntry {
        at: now_secs(),
        decision,
        action,
        app,
        version,
        key,
        reasons,
    }
}

/// A `.trust` file asking IT to approve the author of a refused package.
pub fn trust_request(namespace: &str, key: &str) -> Result<String> {
    let author = TrustedAuthor::new(namespace, key).map_err(|e| anyhow!(e))?;
    Ok(author.to_file())
}

/// Registry values from a list, for tests and for checking a policy before rolling it out.
#[derive(Debug, Default)]
pub struct Values(pub Vec<(String, Value)>);

#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Dword(u32),
    Multi(Vec<String>),
}

impl PolicyValues for Values {
    fn string(&self, name: &str) -> Option<String> {
        self.0.iter().find_map(|(n, v)| match v {
            Value::String(s) if n.eq_ignore_ascii_case(name) => Some(s.clone()),
            _ => None,
        })
    }
    fn dword(&self, name: &str) -> Option<u32> {
        self.0.iter().find_map(|(n, v)| match v {
            Value::Dword(d) if n.eq_ignore_ascii_case(name) => Some(*d),
            _ => None,
        })
    }
    fn multi(&self, name: &str) -> Option<Vec<String>> {
        self.0.iter().find_map(|(n, v)| match v {
            Value::Multi(m) if n.eq_ignore_ascii_case(name) => Some(m.clone()),
            _ => None,
        })
    }
}

#[cfg(windows)]
mod system {
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, RRF_RT_REG_DWORD, RRF_RT_REG_MULTI_SZ,
        RRF_RT_REG_SZ, RegCloseKey, RegGetValueW, RegOpenKeyExW,
    };

    use super::{POLICY_KEY, PolicyValues};

    pub struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The policy key, or `None` when it does not exist (free mode).
    pub fn open() -> Option<Key> {
        let mut key: HKEY = null_mut();
        let path = wide(POLICY_KEY);
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                path.as_ptr(),
                0,
                KEY_READ | KEY_WOW64_64KEY,
                &mut key,
            )
        };
        (status == ERROR_SUCCESS).then_some(Key(key))
    }

    impl Key {
        /// The raw value, as UTF-16 units or bytes, of the given type.
        fn raw(&self, name: &str, flags: u32) -> Option<Vec<u8>> {
            let name = wide(name);
            let mut size: u32 = 0;
            let status = unsafe {
                RegGetValueW(
                    self.0,
                    std::ptr::null(),
                    name.as_ptr(),
                    flags,
                    null_mut(),
                    null_mut(),
                    &mut size,
                )
            };
            if status != ERROR_SUCCESS {
                return None;
            }
            // The value can grow between the two calls (a policy refresh): leave room.
            let mut buf = vec![0u8; size as usize + 64];
            let mut size = buf.len() as u32;
            let status = unsafe {
                RegGetValueW(
                    self.0,
                    std::ptr::null(),
                    name.as_ptr(),
                    flags,
                    null_mut(),
                    buf.as_mut_ptr().cast(),
                    &mut size,
                )
            };
            if status != ERROR_SUCCESS {
                return None;
            }
            buf.truncate(size as usize);
            Some(buf)
        }

        fn utf16(&self, name: &str, flags: u32) -> Option<Vec<u16>> {
            let bytes = self.raw(name, flags)?;
            Some(
                bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&c| u16::from_le_bytes(c))
                    .collect(),
            )
        }
    }

    impl PolicyValues for Key {
        fn string(&self, name: &str) -> Option<String> {
            let units = self.utf16(name, RRF_RT_REG_SZ)?;
            let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
            Some(String::from_utf16_lossy(&units[..end]))
        }

        fn dword(&self, name: &str) -> Option<u32> {
            let bytes = self.raw(name, RRF_RT_REG_DWORD)?;
            Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
        }

        fn multi(&self, name: &str) -> Option<Vec<String>> {
            let units = self.utf16(name, RRF_RT_REG_MULTI_SZ)?;
            Some(
                units
                    .split(|&u| u == 0)
                    .filter(|s| !s.is_empty())
                    .map(String::from_utf16_lossy)
                    .collect(),
            )
        }
    }
}

#[cfg(not(windows))]
mod system {
    use super::Values;

    /// Managed mode is Windows only for now.
    pub fn open() -> Option<Values> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devstribution_core::signing::generate_keys;

    fn key() -> String {
        generate_keys("x", None).unwrap().public_key
    }

    fn loader() -> Loader {
        Loader { admin_only: false }
    }

    fn values(v: Vec<(&str, Value)>) -> Values {
        Values(v.into_iter().map(|(n, v)| (n.to_string(), v)).collect())
    }

    #[test]
    fn an_empty_key_approves_no_one() {
        let p = Policy::from_values("test", &Values::default(), &loader());
        assert!(p.authors.is_empty() && p.problems.is_empty());
        assert_eq!(p.scan_danger, ScanDanger::Refuse);
        assert!(!p.dry_run && !p.users_may_allow_guard_rules);
        assert!(
            p.launcher_updates,
            "updates stay on unless IT turns them off"
        );
        assert!(!p.approves("acme", &key()));
    }

    #[test]
    fn authors_from_every_source() {
        let dir = tempfile::tempdir().unwrap();
        let (k1, k2, k3, k4) = (key(), key(), key(), key());
        let trust = dir.path().join("trust");
        fs::create_dir_all(&trust).unwrap();
        fs::write(
            trust.join("acme.trust"),
            TrustedAuthor::new("acme", &k1).unwrap().to_file(),
        )
        .unwrap();
        fs::write(trust.join("notes.txt"), "not a trust file").unwrap();
        fs::write(trust.join("broken.trust"), "namespace = 1").unwrap();
        let file = dir.path().join("policy.toml");
        fs::write(
            &file,
            format!(
                "contact = \"the service desk\"\nscan_danger = \"ask\"\n\
                 trust_folder = '{}'\n\n[[authors]]\nnamespace = \"acme\"\n\
                 [[authors.keys]]\nkey = \"{}\"\n",
                trust.display(),
                TrustedAuthor::new("acme", &k2).unwrap().keys[0].key
            ),
        )
        .unwrap();
        let k3_line = format!(
            "tools {}",
            TrustedAuthor::new("tools", &k3).unwrap().keys[0].key
        );
        let p = Policy::from_values(
            "test",
            &values(vec![
                ("PolicyFile", Value::String(file.display().to_string())),
                (
                    "Authors",
                    Value::Multi(vec![k3_line, "only-one-part".into()]),
                ),
                ("DryRun", Value::Dword(1)),
            ]),
            &loader(),
        );
        assert!(p.approves("acme", &k1), "from the trust folder");
        assert!(p.approves("acme", &k2), "from the policy file");
        assert!(p.approves("tools", &k3), "inline in the registry");
        assert!(!p.approves("acme", &k3) && !p.approves("acme", &k4));
        assert!(!p.approves("other", &k1));
        assert_eq!(p.authors.len(), 2, "acme's keys are merged");
        assert_eq!(p.problems.len(), 2, "{:?}", p.problems);
        assert_eq!(p.contact.as_deref(), Some("the service desk"));
        assert_eq!(p.scan_danger, ScanDanger::Ask);
        assert!(p.dry_run);
    }

    #[test]
    fn registry_values_win_over_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("policy.toml");
        fs::write(
            &file,
            "dry_run = true\ncontact = \"file\"\nlauncher_updates = true\n",
        )
        .unwrap();
        let p = Policy::from_values(
            "test",
            &values(vec![
                ("PolicyFile", Value::String(file.display().to_string())),
                ("DryRun", Value::Dword(0)),
                ("LauncherUpdates", Value::Dword(0)),
                ("Contact", Value::String("registry".into())),
                ("ScanDanger", Value::String("maybe".into())),
            ]),
            &loader(),
        );
        assert!(!p.dry_run);
        assert!(!p.launcher_updates);
        assert_eq!(p.contact.as_deref(), Some("registry"));
        assert_eq!(p.scan_danger, ScanDanger::Refuse);
        assert_eq!(p.problems.len(), 1);
    }

    #[test]
    fn an_unreadable_policy_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("policy.toml");
        fs::write(&bad, "allowed_authors = [\"acme\"]\n").unwrap();
        for path in [
            bad.display().to_string(),
            "relative.toml".into(),
            "/nope/x.toml".into(),
        ] {
            let p = Policy::from_values(
                "test",
                &values(vec![("PolicyFile", Value::String(path.clone()))]),
                &loader(),
            );
            assert!(p.authors.is_empty(), "{path}");
            assert_eq!(p.problems.len(), 1, "{path}: {:?}", p.problems);
        }
    }

    #[cfg(unix)]
    #[test]
    fn files_users_can_change_are_refused() {
        // Running as root, every file is writable: that is the case to refuse.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("policy.toml");
        fs::write(&file, "dry_run = true\n").unwrap();
        let p = Policy::from_values(
            "test",
            &values(vec![(
                "PolicyFile",
                Value::String(file.display().to_string()),
            )]),
            &Loader { admin_only: true },
        );
        assert!(!p.dry_run);
        assert!(
            p.problems[0].contains("can be changed by users"),
            "{:?}",
            p.problems
        );
    }

    #[test]
    fn minimum_launcher_version() {
        let p = Policy::from_values(
            "test",
            &values(vec![("MinLauncherVersion", Value::String("0.3".into()))]),
            &loader(),
        );
        assert!(p.launcher_refusal("0.2.9").is_some());
        assert!(p.launcher_refusal("0.3.0").is_none());
        let p = Policy::from_values(
            "test",
            &values(vec![("MinLauncherVersion", Value::String("soon".into()))]),
            &loader(),
        );
        assert!(p.launcher_refusal("9.9.9").is_some());
    }
}
