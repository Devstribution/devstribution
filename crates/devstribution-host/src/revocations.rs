//! What this launcher knows of revoked keys (docs/revocation.md): the newest list it holds
//! (bundled with the launcher, or imported) and the owners' statements it was given. Everything
//! is checked locally; nothing is fetched yet.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use devstribution_core::revocation::{
    self, ListFiles, Reason, RevocationList, STATEMENT_EXT, StatementKind, Status,
    VerifiedStatement,
};
use devstribution_core::signing::PublicKey;
use devstribution_core::signing::{parse_public_key, public_key_sha256};
use serde::{Deserialize, Serialize};

use crate::paths::Paths;

/// The root keys lists are checked against: a primary and a backup, made and kept offline
/// (docs/revocation.md, "Keys"). Until they exist no list verifies, and only owners' statements
/// apply.
pub const ROOT_KEYS: &[&str] = &[];

pub fn root_keys() -> Vec<PublicKey> {
    ROOT_KEYS
        .iter()
        .map(|k| parse_public_key(k).expect("a built-in root key parses"))
        .collect()
}

/// Why a package or an installed app should no longer be trusted, as the screens show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revoked {
    /// `withdrawn` (its owner says so) or `malicious` (the list says what it signed did).
    pub status: String,
    /// The list entry or statement: what the user confirmed, so they are asked once.
    pub id: String,
    pub message: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

impl Revoked {
    pub fn is_malicious(&self) -> bool {
        self.status == "malicious"
    }
}

/// What Settings shows.
#[derive(Debug, Clone, Serialize)]
pub struct RevocationStatus {
    /// The list in use, if any.
    pub serial: Option<u64>,
    pub issued: Option<String>,
    pub expires: Option<String>,
    pub expired: bool,
    /// `bundled` (came with the launcher) or `imported`.
    pub source: Option<String>,
    pub entries: usize,
    pub statements: usize,
    /// Whether this launcher can check lists at all (it has root keys).
    pub can_check_lists: bool,
}

/// What an import added.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Imported {
    List {
        serial: u64,
        entries: usize,
    },
    Statement {
        statement: String,
        namespace: String,
        key_id: String,
    },
}

/// Earlier than any end date, for lists that were already accepted.
const ANY_DAY: &str = "";

/// The list and the statements, as read from the data folder.
pub struct Known {
    pub list: Option<(RevocationList, &'static str)>,
    pub statements: Vec<VerifiedStatement>,
}

fn list_dir(paths: &Paths) -> PathBuf {
    paths.state().join("revocations")
}

fn statements_dir(paths: &Paths) -> PathBuf {
    paths.state().join("statements")
}

impl Known {
    /// Reads what the data folder holds and the launcher's bundled list, keeping the list with
    /// the higher serial. A file that does not verify is ignored (and said in the log). The list
    /// key's end date is only checked at import: a list goes on applying when that date passes,
    /// or nobody could rely on what it says about a malicious key once it did.
    pub fn load(paths: &Paths, roots: &[PublicKey], bundled: Option<&ListFiles>) -> Known {
        let held =
            ListFiles::read(&list_dir(paths))
                .ok()
                .and_then(|f| match f.verify(roots, ANY_DAY) {
                    Ok(l) => Some((l, "imported")),
                    Err(e) => {
                        crate::applog::write(&format!("the revocation list held is ignored: {e}"));
                        None
                    }
                });
        let bundled = bundled.and_then(|f| f.verify(roots, ANY_DAY).ok().map(|l| (l, "bundled")));
        let list = match (held, bundled) {
            (Some(h), Some(b)) => Some(if b.0.serial > h.0.serial { b } else { h }),
            (h, b) => h.or(b),
        };
        let mut statements: Vec<VerifiedStatement> = fs::read_dir(statements_dir(paths))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| fs::read_to_string(e.path()).ok())
            .filter_map(|t| revocation::read_statement(&t).ok())
            .collect();
        statements.sort_by(|a, b| a.id.cmp(&b.id));
        Known { list, statements }
    }

    /// Why a package signed with `key` (base64) and holding `content_sha256` should no longer
    /// be trusted, if it should: the list's `malicious` first, then a withdrawal (the owner's
    /// own statement, or the list's entry for it).
    pub fn verdict(&self, key: &str, content_sha256: &str) -> Option<Revoked> {
        let entry = self
            .list
            .as_ref()
            .and_then(|(l, _)| l.find(key, content_sha256));
        if let Some(e) = entry.filter(|e| e.status == Status::Malicious) {
            return Some(Revoked {
                status: "malicious".into(),
                id: format!("list:{}", e.id),
                message: format!(
                    "{} (devstribution revocation list, entry {}, listed {}.)",
                    e.reason.trim_end_matches('.'),
                    e.id,
                    e.listed
                ),
                evidence: e.evidence.clone(),
            });
        }
        let sha = parse_public_key(key).ok().map(|k| public_key_sha256(&k))?;
        if let Some(s) = self
            .statements
            .iter()
            .find(|s| s.statement.kind == StatementKind::Withdrawal && s.key_sha256 == sha)
        {
            let reason = s.statement.reason.map_or("withdrawn", Reason::as_str);
            let mut message = format!(
                "The publisher of `{}` says this key ({}) is no longer safe ({reason}) since {}.",
                s.statement.namespace, s.key_id, s.statement.date
            );
            if let Some(new) = &s.new_key_id {
                message.push_str(&format!(" They name {new} as their new key."));
            }
            return Some(Revoked {
                status: "withdrawn".into(),
                id: format!("statement:{}", s.id),
                message,
                evidence: Vec::new(),
            });
        }
        entry.map(|e| Revoked {
            status: "withdrawn".into(),
            id: format!("list:{}", e.id),
            message: format!(
                "The publisher of `{}` says this key is no longer safe: {} (devstribution \
                 revocation list, entry {}.)",
                e.namespace,
                e.reason.trim_end_matches('.'),
                e.id
            ),
            evidence: e.evidence.clone(),
        })
    }

    /// The owner's rotation from the pinned key `old` to `new` (both base64), when one is held
    /// and neither key is revoked: a withdrawn key cannot hand its trust on, so a thief's
    /// rotation stops counting once the owner withdraws the key.
    pub fn rotation(&self, old: &str, new: &str) -> Option<&VerifiedStatement> {
        let sha = |k: &str| parse_public_key(k).ok().map(|k| public_key_sha256(&k));
        let (old_sha, new_sha) = (sha(old)?, sha(new)?);
        // Any package of the key: a whole-key entry or withdrawal applies to every content.
        let revoked = |k: &str| self.verdict(k, "").is_some();
        if revoked(old) || revoked(new) {
            return None;
        }
        self.statements.iter().find(|s| {
            s.statement.kind == StatementKind::Rotation
                && s.key_sha256 == old_sha
                && s.new_key_sha256.as_deref() == Some(new_sha.as_str())
        })
    }

    pub fn status(&self, can_check_lists: bool, today: &str) -> RevocationStatus {
        let list = self.list.as_ref();
        RevocationStatus {
            serial: list.map(|(l, _)| l.serial),
            issued: list.map(|(l, _)| l.issued.clone()),
            expires: list.map(|(l, _)| l.expires.clone()),
            expired: list.is_some_and(|(l, _)| l.expired(today)),
            source: list.map(|(_, s)| s.to_string()),
            entries: list.map_or(0, |(l, _)| l.entries.len()),
            statements: self.statements.len(),
            can_check_lists,
        }
    }
}

/// Adds a list (`revoked.json`, with `list-key.json` and the signatures beside it) or an
/// owner's statement (`.dskey`) to the data folder, after checking it.
pub fn import(
    paths: &Paths,
    roots: &[PublicKey],
    bundled: Option<&ListFiles>,
    file: &Path,
    today: &str,
) -> Result<Imported> {
    let is_statement = file
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(STATEMENT_EXT));
    if is_statement {
        let text =
            fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
        let s = revocation::read_statement(&text)?;
        let dir = statements_dir(paths);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join(format!("{}.{STATEMENT_EXT}", s.id)), text)?;
        return Ok(Imported::Statement {
            statement: match s.statement.kind {
                StatementKind::Withdrawal => "withdrawal",
                StatementKind::Rotation => "rotation",
            }
            .into(),
            namespace: s.statement.namespace,
            key_id: s.key_id,
        });
    }
    let dir = if file.is_dir() {
        file
    } else if file
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case(revocation::LIST_FILE))
    {
        file.parent().unwrap_or(Path::new("."))
    } else {
        bail!(
            "{} is neither a revocation list (`{}`) nor a key statement (`.{STATEMENT_EXT}`)",
            file.display(),
            revocation::LIST_FILE
        );
    };
    let files = ListFiles::read(dir).map_err(|e| {
        anyhow!(
            "{e}: a list comes with `{}` and both signatures in the same folder",
            revocation::CERT_FILE
        )
    })?;
    let list = files.verify(roots, today)?;
    let current = Known::load(paths, roots, bundled).list;
    if let Some((held, _)) = &current {
        if list.serial == held.serial {
            bail!("this launcher already has list {}", list.serial);
        }
        if list.serial < held.serial {
            bail!(
                "this is list {}, older than list {} which this launcher already has",
                list.serial,
                held.serial
            );
        }
    }
    // Written beside, then moved into place: a half-written list is never read.
    let target = list_dir(paths);
    let tmp = paths.state().join("revocations.new");
    let _ = fs::remove_dir_all(&tmp);
    files.write(&tmp)?;
    let _ = fs::remove_dir_all(&target);
    fs::rename(&tmp, &target)?;
    Ok(Imported::List {
        serial: list.serial,
        entries: list.entries.len(),
    })
}
