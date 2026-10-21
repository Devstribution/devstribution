//! Building `.dsapp` packages and extracting them safely.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{self, Read, Write};
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
        || name == FILES_LIST
        || name == FILES_SIGNATURE
        || name == FILES_PUBLIC_KEY
}

/// The files of an app folder that go into its package.
pub struct AppFiles {
    /// Package path (with `/`) → file on disk, sorted.
    pub files: BTreeMap<String, PathBuf>,
    /// Package paths left out on purpose (caches, virtual environments, `.env`, ...).
    pub excluded: Vec<String>,
}

pub fn collect_app_files(app_dir: &Path) -> Result<AppFiles> {
    let mut files = BTreeMap::new();
    let mut excluded = Vec::new();
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
        if file_type.is_dir() {
            let top = entry.depth() == 1 && EXCLUDED_TOP_DIRS.contains(&name.as_ref());
            if top || EXCLUDED_DIRS.contains(&name.as_ref()) {
                excluded.push(format!("{rel}/"));
                walker.skip_current_dir();
            }
            continue;
        }
        if excluded_file(&name) {
            excluded.push(rel);
            continue;
        }
        if !file_type.is_file() {
            return Err(Error::Package(format!("`{rel}` is not a regular file")));
        }
        validate_relative_path(&rel).map_err(Error::Package)?;
        files.insert(rel, entry.into_path());
    }
    let mut lower = HashSet::new();
    for path in files.keys() {
        if !lower.insert(path.to_lowercase()) {
            return Err(Error::Package(format!(
                "`{path}` differs from another file only by upper/lower case, which breaks on Windows"
            )));
        }
    }
    Ok(AppFiles { files, excluded })
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
    let report = check_lock(&lock, &manifest.name, &manifest.platforms)?;
    if !report.errors.is_empty() {
        return Err(Error::Lock(report.errors.join("; ")));
    }

    let app = collect_app_files(app_dir)?;
    if let Some(icon) = &manifest.icon {
        if !app.files.contains_key(icon) {
            return Err(Error::Manifest(format!(
                "icon `{icon}` is not in the app folder"
            )));
        }
    }
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
    pub content_hash: String,
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
    let file = fs::File::open(package).map_err(|e| Error::io(package, e))?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| refuse(format!("not a valid .dsapp (zip) file: {e}")))?;
    let limits = opts.limits;
    if archive.len() > limits.max_files + 3 {
        return Err(refuse(format!("more than {} files", limits.max_files)));
    }

    let mut seen = HashSet::new();
    let mut hashes: BTreeMap<String, [u8; 32]> = BTreeMap::new();
    let mut list_data = None;
    let mut sig_data = None;
    let mut key_data = None;
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| refuse(format!("corrupt entry: {e}")))?;
        let name = entry.name().to_owned();
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
    if comment.id != manifest.id
        || comment.version != manifest.version
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
    if let Some(icon) = &manifest.icon {
        if list.get(icon).is_none() {
            return Err(Error::Manifest(format!(
                "icon `{icon}` is not in the package"
            )));
        }
    }

    Ok(VerifiedPackage {
        manifest,
        files: list,
        signature: status,
        publisher_key: public_key_base64(&package_key),
        content_hash: content_hash(&list_data),
    })
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
            let name = entry.name().to_owned();
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
