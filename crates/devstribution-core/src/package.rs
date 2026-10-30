//! Building `.dsapp` packages and extracting them safely.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use minisign::PublicKey;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use crate::files::{FilesList, sha256};
use crate::lock::check_lock;
use crate::manifest::Manifest;
use crate::paths::validate_relative_path;
use crate::signing::{
    Signature, Signer, TrustedComment, parse_public_key, public_key_base64, public_key_file,
    public_key_id,
};
use crate::{Error, FILES_LIST, FILES_PUBLIC_KEY, FILES_SIGNATURE, Result};

pub const PYPROJECT: &str = "pyproject.toml";
pub const UV_LOCK: &str = "uv.lock";
/// What `pack` leaves out, in `.gitignore` syntax, next to `pyproject.toml`. Read after the app
/// folder's own `.gitignore`, so its lines win; `!pattern` also marks a data file as meant to ship.
pub const IGNORE_FILE: &str = ".dsappignore";
const GITIGNORE: &str = ".gitignore";

/// Size limits applied when extracting, against zip bombs and runaway packages.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_files: 20_000,
            max_file_bytes: 512 << 20,
            max_total_bytes: 2 << 30,
        }
    }
}

/// Folders never packed, wherever they are in the app folder.
const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".venv",
    "venv",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
    "node_modules",
    ".idea",
    ".vscode",
];

/// Folders never packed when they are at the top of the app folder.
const EXCLUDED_TOP_DIRS: &[&str] = &["dist"];

fn excluded_file(name: &str) -> bool {
    name.ends_with(".pyc")
        || name.ends_with(".pyo")
        || name.ends_with(".dsapp")
        || name == ".DS_Store"
        || name == "Thumbs.db"
        || name == ".env"
        || name == IGNORE_FILE
        || name == GITIGNORE
        || name == FILES_LIST
        || name == FILES_SIGNATURE
        || name == FILES_PUBLIC_KEY
}

/// Extensions of files that usually hold data (a client's spreadsheets, a database, exported
/// archives, logs) rather than app code. `pack` refuses them unless `.dsappignore` names them with
/// `!pattern`: a package is signed and handed out, so data must never get in by accident.
const DATA_EXTENSIONS: &[&str] = &[
    "xlsx", "xlsm", "xlsb", "xls", "ods", "csv", "tsv", "db", "sqlite", "sqlite3", "accdb", "mdb",
    "parquet", "feather", "pkl", "pickle", "h5", "hdf5", "npy", "npz", "zip", "7z", "rar", "tar",
    "gz", "tgz", "bz2", "xz", "log", "bak", "pdf", "docx", "doc", "pptx", "msg", "eml",
];

fn looks_like_data(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| DATA_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Python source, which stays packable even in a folder that holds data.
fn is_code(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| matches!(ext.to_ascii_lowercase().as_str(), "py" | "pyi"))
}

/// The folders of `path`, innermost first, without the app folder itself: `a/b/c.json` →
/// `a/b`, `a`.
fn parent_dirs(path: &str) -> impl Iterator<Item = &str> {
    let mut rest = path;
    std::iter::from_fn(move || {
        let (dir, _) = rest.rsplit_once('/')?;
        rest = dir;
        Some(dir)
    })
}

/// The files of an app folder that go into its package.
pub struct AppFiles {
    /// Package path (with `/`) → file on disk, sorted.
    pub files: BTreeMap<String, PathBuf>,
    /// Package paths left out: built-in rules (caches, virtual environments, `.env`, ...) and the
    /// app's `.gitignore` / `.dsappignore`. Folders end with `/`.
    pub excluded: Vec<String>,
    /// Data-like files (see `DATA_EXTENSIONS`) that are neither left out nor listed with
    /// `!pattern`, and the other files in their folders (an app's last result in `output/` is
    /// often a `.json` next to its spreadsheets), apart from Python code. `pack` refuses to sign
    /// while this is not empty.
    pub unconfirmed_data: Vec<String>,
}

/// The app's `.gitignore` and `.dsappignore` rules, in that order (later lines win).
fn ignore_rules(app_dir: &Path) -> Result<ignore::gitignore::Gitignore> {
    let mut builder = ignore::gitignore::GitignoreBuilder::new(app_dir);
    for name in [GITIGNORE, IGNORE_FILE] {
        let path = app_dir.join(name);
        if path.is_file()
            && let Some(e) = builder.add(&path)
        {
            return Err(Error::Package(format!("{name}: {e}")));
        }
    }
    builder
        .build()
        .map_err(|e| Error::Package(format!("{IGNORE_FILE}: {e}")))
}

pub fn collect_app_files(app_dir: &Path) -> Result<AppFiles> {
    let rules = ignore_rules(app_dir)?;
    let mut files = BTreeMap::new();
    let mut excluded = Vec::new();
    let mut unconfirmed_data = Vec::new();
    // Files that are not data themselves, but would be refused next to data: (path, name).
    let mut beside_data = Vec::new();
    let mut walker = walkdir::WalkDir::new(app_dir)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter();
    while let Some(entry) = walker.next() {
        let entry = entry.map_err(|e| {
            let path = e.path().unwrap_or(app_dir).to_owned();
            Error::io(path, e.into())
        })?;
        if entry.depth() == 0 {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(app_dir)
            .expect("walkdir stays inside app_dir");
        let rel = rel
            .components()
            .map(|c| c.as_os_str().to_str())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| Error::Package(format!("`{}` is not a UTF-8 file name", rel.display())))?
            .join("/");
        let name = entry.file_name().to_string_lossy();
        let file_type = entry.file_type();
        if file_type.is_symlink() {
            return Err(Error::Package(format!(
                "`{rel}` is a symbolic link; packages can only contain regular files"
            )));
        }
        let rule = rules.matched(Path::new(&rel), file_type.is_dir());
        if file_type.is_dir() {
            let top = entry.depth() == 1 && EXCLUDED_TOP_DIRS.contains(&name.as_ref());
            if top || EXCLUDED_DIRS.contains(&name.as_ref()) || rule.is_ignore() {
                excluded.push(format!("{rel}/"));
                walker.skip_current_dir();
            }
            continue;
        }
        if excluded_file(&name) || rule.is_ignore() {
            excluded.push(rel);
            continue;
        }
        if !rule.is_whitelist() {
            if looks_like_data(&name) {
                unconfirmed_data.push(rel.clone());
            } else if !is_code(&name) {
                beside_data.push(rel.clone());
            }
        }
        if !file_type.is_file() {
            return Err(Error::Package(format!("`{rel}` is not a regular file")));
        }
        validate_relative_path(&rel).map_err(Error::Package)?;
        files.insert(rel, entry.into_path());
    }
    // A folder (below the app folder) holding unconfirmed data is a data folder: what else it
    // holds is refused too, unless it is code or listed with `!pattern`.
    let data_dirs: HashSet<&str> = unconfirmed_data
        .iter()
        .filter_map(|p| parent_dirs(p).next())
        .collect();
    let mut beside: Vec<String> = beside_data
        .into_iter()
        .filter(|p| parent_dirs(p).any(|d| data_dirs.contains(d)))
        .collect();
    unconfirmed_data.append(&mut beside);
    unconfirmed_data.sort();
    let mut lower = HashSet::new();
    for path in files.keys() {
        if !lower.insert(path.to_lowercase()) {
            return Err(Error::Package(format!(
                "`{path}` differs from another file only by upper/lower case, which breaks on Windows"
            )));
        }
    }
    Ok(AppFiles {
        files,
        excluded,
        unconfirmed_data,
    })
}

/// Package path of the entry module: `pkg.main` → `pkg/main.py` or `pkg/main/__init__.py`.
fn entry_files(module: &str) -> [String; 2] {
    let base = module.replace('.', "/");
    [format!("{base}.py"), format!("{base}/__init__.py")]
}

/// The checks on the collected files that `pack` needs before signing anything.
pub fn check_app_files(app: &AppFiles, manifest: &Manifest) -> Result<()> {
    if !app.unconfirmed_data.is_empty() {
        let shown: Vec<&str> = app
            .unconfirmed_data
            .iter()
            .take(8)
            .map(String::as_str)
            .collect();
        let more = app.unconfirmed_data.len() - shown.len();
        return Err(Error::Package(format!(
            "{} file(s) look like data, or sit in a folder with data, not app code: {}{}. A \
             package is signed and handed out, so \
             data must not get in by accident. Leave them out with a line in {IGNORE_FILE} (e.g. \
             `output/`), or, if the app needs them, list them there as `!path` (e.g. \
             `!templates/report.xlsx`)",
            app.unconfirmed_data.len(),
            shown.join(", "),
            if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            }
        )));
    }
    for required in [PYPROJECT, UV_LOCK] {
        if !app.files.contains_key(required) {
            return Err(Error::Package(format!(
                "`{required}` is left out by .gitignore or {IGNORE_FILE}, but every package needs it"
            )));
        }
    }
    if !entry_files(&manifest.entry.module)
        .iter()
        .any(|f| app.files.contains_key(f))
    {
        return Err(Error::Package(format!(
            "the entry module `{}` is not in the package: no `{}` (missing, or left out by \
             .gitignore or {IGNORE_FILE})",
            manifest.entry.module,
            entry_files(&manifest.entry.module)[0]
        )));
    }
    if let Some(icon) = &manifest.icon
        && !app.files.contains_key(icon)
    {
        return Err(Error::Manifest(format!(
            "icon `{icon}` is not in the package (missing, or left out by .gitignore or \
                 {IGNORE_FILE})"
        )));
    }
    Ok(())
}

pub struct PackResult {
    pub manifest: Manifest,
    pub file_count: usize,
    pub package_bytes: u64,
    pub excluded: Vec<String>,
    pub lock_warnings: Vec<String>,
    pub key_id: String,
    pub content_hash: String,
}

/// Builds and signs `<out>` from an app folder. All checks that do not need Python run here.
pub fn pack(app_dir: &Path, signer: &Signer, out: &Path) -> Result<PackResult> {
    let read = |name: &str| {
        let path = app_dir.join(name);
        fs::read_to_string(&path).map_err(|e| Error::io(path, e))
    };
    let manifest = Manifest::parse(&read(PYPROJECT)?)?;
    let lock = read(UV_LOCK).map_err(|_| {
        Error::Lock("missing; run `uv lock` in the app folder (the lockfile is required)".into())
    })?;
    // The same gate the launcher applies at install, so a package that would be refused is
    // never signed.
    crate::lock::check_sources(&lock)?;
    let report = check_lock(&lock, &manifest.name, &manifest.platforms)?;
    if !report.errors.is_empty() {
        return Err(Error::Lock(report.errors.join("; ")));
    }

    let app = collect_app_files(app_dir)?;
    check_app_files(&app, &manifest)?;
    let mut list = FilesList::new();
    let mut contents = Vec::with_capacity(app.files.len());
    for (rel, path) in &app.files {
        let data = fs::read(path).map_err(|e| Error::io(path, e))?;
        if looks_like_secret_key(&data) {
            return Err(Error::Package(format!(
                "`{rel}` looks like a secret signing key; move it out of the app folder"
            )));
        }
        list.insert(rel, &data)?;
        contents.push((rel.as_str(), data));
    }
    let list_text = list.to_canonical_string();
    let comment = TrustedComment {
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        schema: manifest.schema_version,
    };
    let signature = signer.sign(list_text.as_bytes(), &comment)?;

    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    let tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| Error::io(parent, e))?;
    let mut zip = ZipWriter::new(tmp.as_file());
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    let mut add = |name: &str, data: &[u8]| -> Result<()> {
        let zip_err = |e: zip::result::ZipError| Error::io(out, io::Error::other(e));
        zip.start_file(name, options).map_err(zip_err)?;
        zip.write_all(data).map_err(|e| Error::io(out, e))
    };
    add(FILES_LIST, list_text.as_bytes())?;
    add(FILES_SIGNATURE, signature.as_bytes())?;
    let key_comment = format!("devstribution public key for {}", manifest.id.namespace);
    add(
        FILES_PUBLIC_KEY,
        public_key_file(signer.public_key(), &key_comment).as_bytes(),
    )?;
    for (rel, data) in &contents {
        add(rel, data)?;
    }
    zip.finish()
        .map_err(|e| Error::io(out, io::Error::other(e)))?;
    tmp.persist(out).map_err(|e| Error::io(out, e.error))?;
    let package_bytes = fs::metadata(out).map_err(|e| Error::io(out, e))?.len();

    Ok(PackResult {
        manifest,
        file_count: list.len(),
        package_bytes,
        excluded: app.excluded,
        lock_warnings: report.warnings,
        key_id: signer.key_id(),
        content_hash: content_hash(list_text.as_bytes()),
    })
}

fn looks_like_secret_key(data: &[u8]) -> bool {
    data.starts_with(b"untrusted comment:")
        && data.len() < 4096
        && String::from_utf8_lossy(data)
            .to_lowercase()
            .contains("secret key")
}

/// Short hash naming an installed version's folder (`apps\<id>\<version-hash>\`).
pub fn content_hash(files_list: &[u8]) -> String {
    hex::encode(&sha256(files_list)[..8])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureStatus {
    /// Made by the key the caller trusts for this namespace.
    Trusted { key_id: String },
    /// Valid signature by the key carried in the package, but no key was trusted for this
    /// namespace yet: the files are intact, the publisher's identity is not established.
    /// Installing it pins this key (trust on first use).
    NewKey { key_id: String },
}

pub struct VerifyOptions<'a> {
    pub trusted_key: Option<&'a PublicKey>,
    pub limits: Limits,
}

pub struct VerifiedPackage {
    pub manifest: Manifest,
    pub files: FilesList,
    pub signature: SignatureStatus,
    /// The publisher's key, as one base64 line (what gets pinned for the namespace).
    pub publisher_key: String,
    /// Short form of the content hash, for folder names only.
    pub content_hash: String,
    /// SHA-256 of `FILES.sha256`, in full: what identifies the content when comparing versions.
    pub content_sha256: String,
}

/// The largest zip index (central directory) a package can have.
const MAX_INDEX_BYTES: u64 = 64 << 20;

/// Opens a package's zip, refusing what is too large before the zip's index is read: the index
/// of a crafted file alone could take gigabytes of memory, and update folders are read at start.
fn open_archive(package: &Path, limits: Limits) -> Result<ZipArchive<fs::File>> {
    let refuse = |msg: String| Error::Package(msg);
    let mut file = fs::File::open(package).map_err(|e| Error::io(package, e))?;
    let file_len = file.metadata().map_err(|e| Error::io(package, e))?.len();
    if file_len > limits.max_total_bytes.saturating_add(64 << 20) {
        return Err(refuse("the file is larger than any package can be".into()));
    }
    if !index_within_limits(&mut file, file_len, limits).map_err(|e| Error::io(package, e))? {
        return Err(refuse(format!("more than {} files", limits.max_files)));
    }
    let archive =
        ZipArchive::new(file).map_err(|e| refuse(format!("not a valid .dsapp (zip) file: {e}")))?;
    if archive.len() > limits.max_files + 3 {
        return Err(refuse(format!("more than {} files", limits.max_files)));
    }
    Ok(archive)
}

/// Whether every end-of-index record at the end of the file (each one that could be the real one,
/// as a zip's comment may hold look-alikes) claims an index within the limits.
fn index_within_limits(file: &mut fs::File, len: u64, limits: Limits) -> io::Result<bool> {
    const RECORD: usize = 22;
    let tail_len = len.min(RECORD as u64 + 65_535);
    let tail_start = len - tail_len;
    file.seek(SeekFrom::Start(tail_start))?;
    let mut tail = vec![0; tail_len as usize];
    file.read_exact(&mut tail)?;
    let u16_at = |b: &[u8], at: usize| u64::from(u16::from_le_bytes([b[at], b[at + 1]]));
    let u32_at = |b: &[u8], at: usize| {
        u64::from(u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]))
    };
    let u64_at = |b: &[u8], at: usize| u32_at(b, at) | (u32_at(b, at + 4) << 32);
    for at in 0..tail.len().saturating_sub(RECORD - 1) {
        if &tail[at..at + 4] != b"PK\x05\x06" {
            continue;
        }
        let mut entries = u16_at(&tail, at + 10);
        let mut size = u32_at(&tail, at + 12);
        // Zip64: these fields are full, and the real numbers are in a record the locator names.
        let record = tail_start + at as u64;
        if (entries == 0xFFFF || size == 0xFFFF_FFFF) && record >= 20 {
            let mut locator = [0; 20];
            file.seek(SeekFrom::Start(record - 20))?;
            file.read_exact(&mut locator)?;
            if &locator[..4] == b"PK\x06\x07" {
                let mut zip64 = [0; 56];
                file.seek(SeekFrom::Start(u64_at(&locator, 8)))?;
                if file.read_exact(&mut zip64).is_ok() && &zip64[..4] == b"PK\x06\x06" {
                    entries = u64_at(&zip64, 32);
                    size = u64_at(&zip64, 40);
                }
            }
        }
        if entries > limits.max_files as u64 + 3 || size > MAX_INDEX_BYTES {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Extracts `package` into the new folder `dest` and verifies it. On any error `dest` is removed.
///
/// Checks: path rules, no links or special files, no duplicate names (ignoring case), size
/// limits, a canonical `FILES.sha256` listing exactly the extracted files with matching hashes,
/// a valid signature by the key carried in the package (which must be `trusted_key` when set),
/// whose trusted comment matches the manifest,
/// and a valid manifest with a lockfile.
pub fn extract_and_verify(
    package: &Path,
    dest: &Path,
    opts: &VerifyOptions,
) -> Result<VerifiedPackage> {
    fs::create_dir(dest).map_err(|e| Error::io(dest, e))?;
    let result = extract_inner(package, dest, opts);
    if result.is_err() {
        let _ = fs::remove_dir_all(dest);
    }
    result
}

fn extract_inner(package: &Path, dest: &Path, opts: &VerifyOptions) -> Result<VerifiedPackage> {
    let refuse = |msg: String| Error::Package(msg);
    let limits = opts.limits;
    let mut archive = open_archive(package, limits)?;
    // Any zip can be offered (a downloaded .dsapp often arrives renamed to .zip): one without
    // the signed file list is said to be no app before anything is extracted.
    if archive.index_for_name(FILES_LIST).is_none() {
        return Err(refuse(format!(
            "this is not a devstribution app: a zip file without {FILES_LIST}"
        )));
    }

    let mut seen = HashSet::new();
    // Every folder prefix as first spelled: `A/x.py` next to `a/y.py` would end up in one
    // folder on Windows but two on Linux.
    let mut folders: HashMap<String, String> = HashMap::new();
    let mut hashes: BTreeMap<String, [u8; 32]> = BTreeMap::new();
    let mut list_data = None;
    let mut sig_data = None;
    let mut key_data = None;
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| refuse(format!("corrupt entry: {e}")))?;
        let name = entry
            .name()
            .map_err(|e| refuse(format!("an entry name cannot be read: {e}")))?
            .into_owned();
        let is_dir = name.ends_with('/');
        let path = name.strip_suffix('/').unwrap_or(&name);
        validate_relative_path(path).map_err(refuse)?;
        if entry.encrypted() {
            return Err(refuse(format!("`{name}` is encrypted")));
        }
        if entry.is_symlink() {
            return Err(refuse(format!("`{name}` is a symbolic link")));
        }
        if let Some(mode) = entry.unix_mode() {
            let kind = mode & 0o170000;
            let expected = if is_dir { 0o040000 } else { 0o100000 };
            if kind != 0 && kind != expected {
                return Err(refuse(format!("`{name}` is not a regular file")));
            }
        }
        if !seen.insert(path.to_lowercase()) {
            return Err(refuse(format!(
                "`{path}` appears twice (names are compared ignoring case)"
            )));
        }
        let parts: Vec<&str> = path.split('/').collect();
        let folder_count = if is_dir { parts.len() } else { parts.len() - 1 };
        for n in 1..=folder_count {
            let prefix = parts[..n].join("/");
            let first = folders
                .entry(prefix.to_lowercase())
                .or_insert_with(|| prefix.clone());
            if *first != prefix {
                return Err(refuse(format!(
                    "the folders `{first}` and `{prefix}` differ only in case"
                )));
            }
        }
        if is_dir {
            continue;
        }

        let budget = limits.max_file_bytes.min(limits.max_total_bytes - total);
        let mut limited = (&mut entry).take(budget + 1);
        if path == FILES_LIST || path == FILES_SIGNATURE || path == FILES_PUBLIC_KEY {
            let mut data = Vec::new();
            limited
                .read_to_end(&mut data)
                .map_err(|e| refuse(format!("`{name}`: {e}")))?;
            total += data.len() as u64;
            if data.len() as u64 > budget || data.len() > 16 << 20 {
                return Err(refuse(format!("`{name}` is too large")));
            }
            match path {
                FILES_LIST => list_data = Some(data),
                FILES_SIGNATURE => sig_data = Some(data),
                _ => key_data = Some(data),
            }
            continue;
        }

        let target = dest.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let mut out = fs::File::create_new(&target).map_err(|e| Error::io(&target, e))?;
        let mut hasher = HashingWriter {
            inner: &mut out,
            hasher: Sha256::new(),
        };
        let written =
            io::copy(&mut limited, &mut hasher).map_err(|e| refuse(format!("`{name}`: {e}")))?;
        if written > budget {
            return Err(refuse(if written > limits.max_file_bytes {
                format!("`{path}` is larger than {} bytes", limits.max_file_bytes)
            } else {
                format!(
                    "the package is larger than {} bytes uncompressed",
                    limits.max_total_bytes
                )
            }));
        }
        total += written;
        hashes.insert(path.to_owned(), hasher.hasher.finalize().into());
    }

    let list_data = list_data.ok_or_else(|| refuse(format!("missing {FILES_LIST}")))?;
    let sig_data =
        sig_data.ok_or_else(|| refuse(format!("missing {FILES_SIGNATURE} (unsigned package)")))?;
    let list = FilesList::parse(&list_data)?;
    let missing: Vec<&str> = list.paths().filter(|p| !hashes.contains_key(*p)).collect();
    if !missing.is_empty() {
        return Err(refuse(format!(
            "files listed but missing: {}",
            preview(&missing)
        )));
    }
    let extra: Vec<&str> = hashes
        .keys()
        .map(String::as_str)
        .filter(|p| list.get(p).is_none())
        .collect();
    if !extra.is_empty() {
        return Err(refuse(format!(
            "files not listed in {FILES_LIST}: {}",
            preview(&extra)
        )));
    }
    let modified: Vec<&str> = hashes
        .iter()
        .filter(|(p, h)| list.get(p) != Some(h))
        .map(|(p, _)| p.as_str())
        .collect();
    if !modified.is_empty() {
        return Err(refuse(format!(
            "files modified after signing: {}",
            preview(&modified)
        )));
    }

    let sig_text = String::from_utf8(sig_data)
        .map_err(|_| refuse(format!("{FILES_SIGNATURE} is not UTF-8")))?;
    let signature = Signature::parse(&sig_text)?;
    let key_data = key_data.ok_or_else(|| refuse(format!("missing {FILES_PUBLIC_KEY}")))?;
    let key_text = String::from_utf8(key_data)
        .map_err(|_| refuse(format!("{FILES_PUBLIC_KEY} is not UTF-8")))?;
    let package_key = parse_public_key(&key_text)?;
    let comment = signature.verify(&package_key, &list_data)?;
    let key_id = public_key_id(&package_key);
    let status = match opts.trusted_key {
        Some(trusted) if public_key_base64(trusted) == public_key_base64(&package_key) => {
            SignatureStatus::Trusted { key_id }
        }
        Some(trusted) => {
            return Err(Error::Signature(format!(
                "signed with key {key_id}, but this publisher's key is {}: \
                 it was not published by the same publisher",
                public_key_id(trusted)
            )));
        }
        None => SignatureStatus::NewKey { key_id },
    };

    for required in [PYPROJECT, UV_LOCK] {
        if list.get(required).is_none() {
            return Err(refuse(format!("missing {required}")));
        }
    }
    let pyproject =
        fs::read_to_string(dest.join(PYPROJECT)).map_err(|e| Error::io(dest.join(PYPROJECT), e))?;
    let manifest = Manifest::parse(&pyproject)?;
    let lock =
        fs::read_to_string(dest.join(UV_LOCK)).map_err(|e| Error::io(dest.join(UV_LOCK), e))?;
    crate::lock::check_sources(&lock)?;
    // The rest of what `pack` checks in the lockfile, for packages not made by our `pack`: a
    // wheel for every declared platform, no package that would have to be built from source.
    let report = check_lock(&lock, &manifest.name, &manifest.platforms)?;
    if !report.errors.is_empty() {
        return Err(Error::Lock(report.errors.join("; ")));
    }
    check_signed_for(&comment, &manifest)?;
    if !entry_files(&manifest.entry.module)
        .iter()
        .any(|f| list.get(f).is_some())
    {
        return Err(Error::Manifest(format!(
            "the entry module `{}` is not in the package",
            manifest.entry.module
        )));
    }
    if let Some(icon) = &manifest.icon
        && list.get(icon).is_none()
    {
        return Err(Error::Manifest(format!(
            "icon `{icon}` is not in the package"
        )));
    }

    Ok(VerifiedPackage {
        manifest,
        files: list,
        signature: status,
        publisher_key: public_key_base64(&package_key),
        content_hash: content_hash(&list_data),
        content_sha256: hex::encode(sha256(&list_data)),
    })
}

/// What a package says about itself, read without extracting it.
pub struct PackageHeader {
    pub manifest: Manifest,
    pub publisher_key: String,
    pub content_hash: String,
    pub content_sha256: String,
}

/// The signed manifest of `package`: enough to tell which app and version it is, and who signed
/// it. Only the signed list, the signature, the key and the manifest are read, so the other files
/// are **not** checked: this is for noticing a package, and installing one still goes through
/// [`extract_and_verify`].
pub fn read_header(package: &Path, limits: Limits) -> Result<PackageHeader> {
    let refuse = |msg: String| Error::Package(msg);
    let mut archive = open_archive(package, limits)?;
    let mut read = |name: &str| -> Result<Vec<u8>> {
        let mut entry = archive
            .by_name(name)
            .map_err(|e| refuse(format!("`{name}`: {e}")))?;
        let mut data = Vec::new();
        (&mut entry)
            .take((16 << 20) + 1)
            .read_to_end(&mut data)
            .map_err(|e| refuse(format!("`{name}`: {e}")))?;
        if data.len() > 16 << 20 {
            return Err(refuse(format!("`{name}` is too large")));
        }
        Ok(data)
    };
    let list_data = read(FILES_LIST)?;
    let sig_text = String::from_utf8(read(FILES_SIGNATURE)?)
        .map_err(|_| refuse(format!("{FILES_SIGNATURE} is not UTF-8")))?;
    let key_text = String::from_utf8(read(FILES_PUBLIC_KEY)?)
        .map_err(|_| refuse(format!("{FILES_PUBLIC_KEY} is not UTF-8")))?;
    let pyproject = read(PYPROJECT)?;

    let package_key = parse_public_key(&key_text)?;
    let comment = Signature::parse(&sig_text)?.verify(&package_key, &list_data)?;
    let list = FilesList::parse(&list_data)?;
    if list.get(PYPROJECT) != Some(&sha256(&pyproject)) {
        return Err(refuse(format!("{PYPROJECT} was modified after signing")));
    }
    let pyproject =
        String::from_utf8(pyproject).map_err(|_| refuse(format!("{PYPROJECT} is not UTF-8")))?;
    let manifest = Manifest::parse(&pyproject)?;
    check_signed_for(&comment, &manifest)?;
    Ok(PackageHeader {
        manifest,
        publisher_key: public_key_base64(&package_key),
        content_hash: content_hash(&list_data),
        content_sha256: hex::encode(sha256(&list_data)),
    })
}

/// The signature's trusted comment must name the manifest's app, version and schema.
fn check_signed_for(comment: &TrustedComment, manifest: &Manifest) -> Result<()> {
    // The version as written, not only its value: `1.2` and `1.2.0` are not the same text.
    if comment.id != manifest.id
        || comment.version.to_string() != manifest.version.to_string()
        || comment.schema != manifest.schema_version
    {
        return Err(Error::Signature(format!(
            "signed for {} {} (schema {}) but the manifest says {} {} (schema {})",
            comment.id,
            comment.version,
            comment.schema,
            manifest.id,
            manifest.version,
            manifest.schema_version
        )));
    }
    Ok(())
}

/// Checks that `dir`, extracted and verified earlier, still holds exactly the files of `list`
/// with their content: nothing changed, added or removed since, and no links.
pub fn verify_extracted(dir: &Path, list: &FilesList) -> Result<()> {
    let changed = |what: String| {
        Error::Package(format!(
            "the app's files changed after they were checked: {what}"
        ))
    };
    let mut found = 0usize;
    for entry in walkdir::WalkDir::new(dir).follow_links(false) {
        let entry = entry.map_err(|e| Error::io(e.path().unwrap_or(dir).to_owned(), e.into()))?;
        if entry.depth() == 0 || entry.file_type().is_dir() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(dir)
            .expect("walkdir stays inside dir")
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if !entry.file_type().is_file() {
            return Err(changed(format!("`{rel}` is not a regular file")));
        }
        let expected = list
            .get(&rel)
            .ok_or_else(|| changed(format!("`{rel}` was added")))?;
        let mut sink = io::sink();
        let mut hasher = HashingWriter {
            inner: &mut sink,
            hasher: Sha256::new(),
        };
        let mut file = fs::File::open(entry.path()).map_err(|e| Error::io(entry.path(), e))?;
        io::copy(&mut file, &mut hasher).map_err(|e| Error::io(entry.path(), e))?;
        if <[u8; 32]>::from(hasher.hasher.finalize()) != *expected {
            return Err(changed(format!("`{rel}` was modified")));
        }
        found += 1;
    }
    if found != list.len() {
        return Err(changed("files were removed".into()));
    }
    Ok(())
}

fn preview(paths: &[&str]) -> String {
    let shown: Vec<String> = paths.iter().take(5).map(|p| format!("`{p}`")).collect();
    let more = paths.len().saturating_sub(5);
    if more > 0 {
        format!("{} and {more} more", shown.join(", "))
    } else {
        shown.join(", ")
    }
}

struct HashingWriter<'a, W: Write> {
    inner: &'a mut W,
    hasher: Sha256,
}

impl<W: Write> Write for HashingWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;

    /// The end of a zip's index, as the file's last bytes.
    fn end_record(entries: u16, size: u32) -> Vec<u8> {
        let mut r = b"PK\x05\x06".to_vec();
        r.extend([0; 6]);
        r.extend(entries.to_le_bytes());
        r.extend(size.to_le_bytes());
        r.extend([0; 6]);
        r
    }

    #[test]
    fn a_huge_index_is_refused_before_it_is_read() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x.dsapp");
        for bytes in [
            end_record(30_000, 0),
            end_record(1, 200 << 20),
            // Hidden behind a harmless record, as a comment could be.
            [end_record(30_000, 0), end_record(0, 0)].concat(),
        ] {
            fs::write(&path, bytes).unwrap();
            let err = read_header(&path, Limits::default())
                .err()
                .unwrap()
                .to_string();
            assert!(err.contains("more than"), "{err}");
        }
        // Within the limits, it goes on to the zip reader.
        fs::write(&path, end_record(0, 0)).unwrap();
        let err = read_header(&path, Limits::default())
            .err()
            .unwrap()
            .to_string();
        assert!(!err.contains("more than"), "{err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::{generate_keys, parse_public_key};

    const PYPROJECT_TEXT: &str = r#"[project]
name = "hello"
version = "1.0.0"
requires-python = ">=3.11"
dependencies = []

[tool.uv]
package = false

[tool.devstribution]
schema_version = 1
id = "author/hello"
title = "Hello"
entry = "main:run"
"#;

    const LOCK_TEXT: &str = r#"version = 1
revision = 3
requires-python = ">=3.11"

[[package]]
name = "hello"
version = "1.0.0"
source = { virtual = "." }
"#;

    struct Fixture {
        dir: tempfile::TempDir,
        signer: Signer,
        public_key: PublicKey,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        fs::create_dir_all(app.join("pkg/__pycache__")).unwrap();
        fs::write(app.join(PYPROJECT), PYPROJECT_TEXT).unwrap();
        fs::write(app.join(UV_LOCK), LOCK_TEXT).unwrap();
        fs::write(app.join("main.py"), "def run(ctx):\n    return {}\n").unwrap();
        fs::write(app.join("pkg/util.py"), "X = 1\n").unwrap();
        fs::write(app.join("pkg/__pycache__/util.cpython-313.pyc"), "junk").unwrap();
        fs::write(app.join(".env"), "TOKEN=secret").unwrap();
        let keys = generate_keys("author", None).unwrap();
        Fixture {
            signer: Signer::from_unencrypted(&keys.secret_key).unwrap().unwrap(),
            public_key: parse_public_key(&keys.public_key).unwrap(),
            dir,
        }
    }

    impl Fixture {
        fn app(&self) -> PathBuf {
            self.dir.path().join("app")
        }
        fn pack(&self) -> PathBuf {
            let out = self.dir.path().join("hello.dsapp");
            pack(&self.app(), &self.signer, &out).unwrap();
            out
        }
        fn verify(&self, pkg: &Path, key: Option<&PublicKey>) -> Result<VerifiedPackage> {
            let dest = self.dir.path().join(format!("x{}", rand_suffix()));
            extract_and_verify(
                pkg,
                &dest,
                &VerifyOptions {
                    trusted_key: key,
                    limits: Limits::default(),
                },
            )
        }
    }

    fn rand_suffix() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    }

    type Edit = Box<dyn Fn(&str, Vec<u8>) -> Vec<(String, Vec<u8>)>>;

    /// Rewrites a package entry by entry, letting the test change names or contents.
    fn rewrite(
        src: &Path,
        dst: &Path,
        mut edit: impl FnMut(&str, Vec<u8>) -> Vec<(String, Vec<u8>)>,
    ) {
        let mut archive = ZipArchive::new(fs::File::open(src).unwrap()).unwrap();
        let mut zip = ZipWriter::new(fs::File::create(dst).unwrap());
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            let name = entry.name().unwrap().into_owned();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            for (n, d) in edit(&name, data) {
                zip.start_file(n, SimpleFileOptions::default()).unwrap();
                zip.write_all(&d).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    #[test]
    fn pack_then_verify() {
        let f = fixture();
        let pkg = f.pack();
        let v = f.verify(&pkg, Some(&f.public_key)).unwrap();
        assert_eq!(v.manifest.id.to_string(), "author/hello");
        assert_eq!(
            v.files.paths().collect::<Vec<_>>(),
            ["main.py", "pkg/util.py", "pyproject.toml", "uv.lock"]
        );
        assert!(matches!(v.signature, SignatureStatus::Trusted { .. }));
        let unchecked = f.verify(&pkg, None).unwrap();
        assert!(matches!(
            unchecked.signature,
            SignatureStatus::NewKey { .. }
        ));
    }

    #[test]
    fn extracted_files_are_checked_again() {
        let f = fixture();
        let pkg = f.pack();
        let dest = f.dir.path().join("checked");
        let v = extract_and_verify(
            &pkg,
            &dest,
            &VerifyOptions {
                trusted_key: None,
                limits: Limits::default(),
            },
        )
        .unwrap();
        verify_extracted(&dest, &v.files).unwrap();
        let original = fs::read(dest.join("main.py")).unwrap();

        fs::write(dest.join("main.py"), "print('changed')").unwrap();
        let err = verify_extracted(&dest, &v.files).unwrap_err().to_string();
        assert!(err.contains("`main.py` was modified"), "{err}");
        fs::remove_file(dest.join("main.py")).unwrap();
        let err = verify_extracted(&dest, &v.files).unwrap_err().to_string();
        assert!(err.contains("removed"), "{err}");
        fs::write(dest.join("main.py"), original).unwrap();
        fs::write(dest.join("pkg").join("extra.py"), "").unwrap();
        let err = verify_extracted(&dest, &v.files).unwrap_err().to_string();
        assert!(err.contains("`pkg/extra.py` was added"), "{err}");
    }

    fn packed_paths(f: &Fixture) -> Vec<String> {
        let v = f.verify(&f.pack(), Some(&f.public_key)).unwrap();
        v.files.paths().map(String::from).collect()
    }

    fn pack_error(f: &Fixture) -> String {
        let out = f.dir.path().join("refused.dsapp");
        let err = pack(&f.app(), &f.signer, &out)
            .err()
            .expect("pack should refuse");
        assert!(!out.exists(), "nothing is written when pack refuses");
        err.to_string()
    }

    #[test]
    fn dsappignore_and_gitignore_leave_files_out() {
        let f = fixture();
        let app = f.app();
        fs::create_dir_all(app.join("output/2026")).unwrap();
        fs::write(app.join("output/2026/notes.txt"), "client notes").unwrap();
        fs::write(app.join("scratch.py"), "").unwrap();
        fs::write(app.join("debug.txt"), "").unwrap();
        fs::write(app.join(".gitignore"), "scratch.py\n").unwrap();
        fs::write(app.join(IGNORE_FILE), "# client data\noutput/\n*.txt\n").unwrap();

        assert_eq!(
            packed_paths(&f),
            ["main.py", "pkg/util.py", "pyproject.toml", "uv.lock"]
        );
        let collected = collect_app_files(&app).unwrap();
        for left_out in [
            "output/",
            "scratch.py",
            "debug.txt",
            ".gitignore",
            IGNORE_FILE,
        ] {
            assert!(
                collected.excluded.iter().any(|e| e == left_out),
                "{left_out} in {:?}",
                collected.excluded
            );
        }
    }

    #[test]
    fn data_files_are_refused_unless_confirmed() {
        let f = fixture();
        let app = f.app();
        fs::create_dir_all(app.join("output")).unwrap();
        fs::create_dir_all(app.join("templates")).unwrap();
        fs::write(app.join("output/Clienti.XLSX"), "client data").unwrap();
        fs::write(app.join("output/archive.db"), "client data").unwrap();
        fs::write(app.join("templates/report.xlsx"), "template").unwrap();

        let err = pack_error(&f);
        assert!(err.contains("output/Clienti.XLSX"), "{err}");
        assert!(err.contains("output/archive.db"), "{err}");
        assert!(err.contains(IGNORE_FILE), "{err}");

        // Leaving the folder out and confirming the template is enough.
        fs::write(app.join(IGNORE_FILE), "output/\n!templates/report.xlsx\n").unwrap();
        assert!(packed_paths(&f).contains(&"templates/report.xlsx".to_string()));

        // A `!` line in .dsappignore wins over the app's .gitignore.
        fs::write(app.join(".gitignore"), "*.xlsx\n").unwrap();
        assert!(packed_paths(&f).contains(&"templates/report.xlsx".to_string()));
    }

    #[test]
    fn files_next_to_data_are_refused_too() {
        let f = fixture();
        let app = f.app();
        fs::create_dir_all(app.join("output/2026")).unwrap();
        fs::create_dir_all(app.join("config")).unwrap();
        fs::create_dir_all(app.join("templates")).unwrap();
        fs::write(app.join("output/Clienti.xlsx"), "client data").unwrap();
        fs::write(app.join("output/ultimo_esito.json"), "client figures").unwrap();
        fs::write(app.join("output/2026/note.txt"), "client notes").unwrap();
        fs::write(app.join("output/helpers.py"), "").unwrap();
        fs::write(app.join("config/settings.json"), "{}").unwrap();
        fs::write(app.join("templates/report.xlsx"), "template").unwrap();
        fs::write(app.join("templates/layout.json"), "{}").unwrap();

        let collected = collect_app_files(&app).unwrap();
        assert_eq!(
            collected.unconfirmed_data,
            [
                "output/2026/note.txt",
                "output/Clienti.xlsx",
                "output/ultimo_esito.json",
                "templates/layout.json",
                "templates/report.xlsx",
            ]
        );
        assert!(pack_error(&f).contains("output/ultimo_esito.json"));

        // Confirming the template clears its folder; the JSON next to it needs no line of its own.
        fs::write(app.join(IGNORE_FILE), "output/\n!templates/report.xlsx\n").unwrap();
        let packed = packed_paths(&f);
        for path in [
            "config/settings.json",
            "templates/layout.json",
            "templates/report.xlsx",
        ] {
            assert!(packed.contains(&path.to_string()), "{path} in {packed:?}");
        }
    }

    #[test]
    fn required_files_cannot_be_left_out() {
        let f = fixture();
        fs::write(f.app().join(IGNORE_FILE), "uv.lock\n").unwrap();
        assert!(pack_error(&f).contains("`uv.lock` is left out"));

        fs::write(f.app().join(IGNORE_FILE), "main.py\n").unwrap();
        assert!(pack_error(&f).contains("entry module `main`"));
    }

    #[test]
    fn entry_module_can_be_a_package() {
        let f = fixture();
        let app = f.app();
        let text = fs::read_to_string(app.join(PYPROJECT)).unwrap();
        fs::write(app.join(PYPROJECT), text.replace("main:run", "pkg:run")).unwrap();
        fs::write(
            app.join("pkg/__init__.py"),
            "def run(ctx):\n    return {}\n",
        )
        .unwrap();
        assert!(packed_paths(&f).contains(&"pkg/__init__.py".to_string()));
    }

    #[test]
    fn the_header_says_what_verifying_says() {
        let f = fixture();
        let pkg = f.pack();
        let header = read_header(&pkg, Limits::default()).unwrap();
        let v = f.verify(&pkg, None).unwrap();
        assert_eq!(header.manifest.id, v.manifest.id);
        assert_eq!(header.publisher_key, v.publisher_key);
        assert_eq!(header.content_sha256, v.content_sha256);
    }

    #[test]
    fn the_header_of_a_changed_or_unsigned_package_is_refused() {
        let f = fixture();
        let pkg = f.pack();
        let out = f.dir.path().join("changed.dsapp");

        // A higher version written over the signed manifest must not look like an update.
        rewrite(&pkg, &out, |name, data| {
            let data = if name == PYPROJECT {
                String::from_utf8(data)
                    .unwrap()
                    .replace("1.0.0", "9.0.0")
                    .into_bytes()
            } else {
                data
            };
            vec![(name.to_string(), data)]
        });
        assert!(read_header(&out, Limits::default()).is_err());

        for missing in [FILES_LIST, FILES_SIGNATURE, FILES_PUBLIC_KEY, PYPROJECT] {
            rewrite(&pkg, &out, |name, data| {
                if name == missing {
                    vec![]
                } else {
                    vec![(name.to_string(), data)]
                }
            });
            assert!(read_header(&out, Limits::default()).is_err(), "{missing}");
        }

        fs::write(&out, b"not a zip").unwrap();
        assert!(read_header(&out, Limits::default()).is_err());
    }

    #[test]
    fn packing_is_deterministic_apart_from_the_signature() {
        let f = fixture();
        let a = f.pack();
        let v1 = f.verify(&a, None).unwrap();
        let second_path = f.dir.path().join("again.dsapp");
        pack(&f.app(), &f.signer, &second_path).unwrap();
        let v2 = f.verify(&second_path, None).unwrap();
        assert_eq!(v1.content_hash, v2.content_hash);
    }

    #[test]
    fn a_package_renamed_to_zip_still_installs_and_other_zips_are_no_app() {
        let f = fixture();
        let pkg = f.pack();
        // How browsers and mail often deliver a .dsapp: same bytes, another name.
        let renamed = f.dir.path().join("hello.zip");
        fs::copy(&pkg, &renamed).unwrap();
        f.verify(&renamed, Some(&f.public_key)).unwrap();

        // An ordinary zip (here: the package without its signed list) is refused before
        // anything is extracted, with a message that says what it is.
        let other = f.dir.path().join("photos.zip");
        rewrite(&pkg, &other, |n, d| {
            if n == FILES_LIST {
                vec![]
            } else {
                vec![(n.into(), d)]
            }
        });
        let dest = f.dir.path().join("never");
        let err = extract_and_verify(
            &other,
            &dest,
            &VerifyOptions {
                trusted_key: None,
                limits: Limits::default(),
            },
        )
        .err()
        .expect("refused")
        .to_string();
        assert!(err.contains("not a devstribution app"), "{err}");
        assert!(!dest.exists());
    }

    #[test]
    fn refuses_wrong_key() {
        let f = fixture();
        let pkg = f.pack();
        let other = parse_public_key(&generate_keys("x", None).unwrap().public_key).unwrap();
        assert!(matches!(
            f.verify(&pkg, Some(&other)),
            Err(Error::Signature(_))
        ));
    }

    #[test]
    fn refuses_a_swapped_public_key() {
        let f = fixture();
        let pkg = f.pack();
        let other = generate_keys("x", None).unwrap().public_key;
        let bad = f.dir.path().join("swapped.dsapp");
        rewrite(&pkg, &bad, |n, d| {
            let d = if n == FILES_PUBLIC_KEY {
                other.clone().into_bytes()
            } else {
                d
            };
            vec![(n.into(), d)]
        });
        assert!(matches!(f.verify(&bad, None), Err(Error::Signature(_))));
    }

    #[test]
    fn refuses_tampering() {
        let f = fixture();
        let pkg = f.pack();
        let bad = f.dir.path().join("bad.dsapp");
        let cases: Vec<Edit> = vec![
            // modified file
            Box::new(|n, d| {
                vec![(
                    n.into(),
                    if n == "main.py" {
                        b"import os".to_vec()
                    } else {
                        d
                    },
                )]
            }),
            // extra file
            Box::new(|n, d| {
                let mut v = vec![(n.to_string(), d)];
                if n == "main.py" {
                    v.push(("evil.py".into(), b"x".to_vec()))
                }
                v
            }),
            // missing file
            Box::new(|n, d| {
                if n == "pkg/util.py" {
                    vec![]
                } else {
                    vec![(n.into(), d)]
                }
            }),
            // path traversal
            Box::new(|n, d| {
                vec![(
                    if n == "main.py" {
                        "../main.py".into()
                    } else {
                        n.into()
                    },
                    d,
                )]
            }),
            // case-insensitive duplicate
            Box::new(|n, d| {
                let mut v = vec![(n.to_string(), d.clone())];
                if n == "main.py" {
                    v.push(("MAIN.py".into(), d))
                }
                v
            }),
            // folders differing only in case
            Box::new(|n, d| {
                let mut v = vec![(n.to_string(), d.clone())];
                if n == "pkg/util.py" {
                    v.push(("PKG/other.py".into(), d))
                }
                v
            }),
            // unsigned
            Box::new(|n, d| {
                if n == FILES_SIGNATURE {
                    vec![]
                } else {
                    vec![(n.into(), d)]
                }
            }),
        ];
        for (i, case) in cases.iter().enumerate() {
            rewrite(&pkg, &bad, |n, d| case(n, d));
            assert!(
                f.verify(&bad, Some(&f.public_key)).is_err(),
                "case {i} accepted"
            );
        }
    }

    #[test]
    fn refuses_manifest_that_does_not_match_the_signature() {
        let f = fixture();
        let pkg = f.pack();
        // Re-sign a list that includes a pyproject with another version, using the trusted
        // comment of the original: the comment no longer matches the manifest.
        fs::write(
            f.app().join(PYPROJECT),
            PYPROJECT_TEXT.replace("1.0.0", "2.0.0"),
        )
        .unwrap();
        fs::write(f.app().join(UV_LOCK), LOCK_TEXT.replace("1.0.0", "2.0.0")).unwrap();
        let newer = f.dir.path().join("newer.dsapp");
        pack(&f.app(), &f.signer, &newer).unwrap();
        let mut old_sig = Vec::new();
        rewrite(&pkg, &f.dir.path().join("scratch.dsapp"), |n, d| {
            if n == FILES_SIGNATURE {
                old_sig = d.clone()
            }
            vec![(n.into(), d)]
        });
        let bad = f.dir.path().join("bad.dsapp");
        rewrite(&newer, &bad, |n, d| {
            vec![(
                n.into(),
                if n == FILES_SIGNATURE {
                    old_sig.clone()
                } else {
                    d
                },
            )]
        });
        assert!(f.verify(&bad, Some(&f.public_key)).is_err());
    }

    #[test]
    fn refuses_oversized_files() {
        let f = fixture();
        let pkg = f.pack();
        let dest = f.dir.path().join("small");
        let limits = Limits {
            max_file_bytes: 10,
            ..Limits::default()
        };
        let r = extract_and_verify(
            &pkg,
            &dest,
            &VerifyOptions {
                trusted_key: None,
                limits,
            },
        );
        assert!(r.is_err());
        assert!(!dest.exists(), "partial extraction must be removed");
    }

    #[test]
    fn excludes_caches_and_refuses_secret_keys() {
        let f = fixture();
        let files = collect_app_files(&f.app()).unwrap();
        assert!(files.excluded.contains(&".env".to_string()));
        assert!(files.excluded.contains(&"pkg/__pycache__/".to_string()));
        let keys = generate_keys("author", None).unwrap();
        fs::write(f.app().join("author.key"), keys.secret_key).unwrap();
        let out = f.dir.path().join("x.dsapp");
        assert!(pack(&f.app(), &f.signer, &out).is_err());
    }
}
