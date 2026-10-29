//! Revocation (docs/revocation.md): signed lists of publisher keys and packages that should no
//! longer be trusted, and owners' signed statements about their own keys.
//!
//! A list arrives as four files, as `Devstribution/trust` publishes them:
//!
//! - `list-key.json` + `.minisig`: the key that signs lists, certified by a root key built into
//!   the launcher, with an end date;
//! - `revoked.json` + `.minisig`: the list, signed by that key, with a serial number that only
//!   goes up (a replayed older list changes nothing) and an expiry date.
//!
//! An owner's statement is one `.dskey` file: the statement's JSON and the signature of the key
//! it is about (and, for a rotation, of the new key too). It needs no list and no root: whoever
//! holds a key may switch it off.
//!
//! Dates are `YYYY-MM-DD` (UTC), compared as text. They say when a list was issued and until when
//! to rely on it; they prove nothing about when a package was signed.

use std::path::Path;

use minisign::PublicKey;
use serde::{Deserialize, Serialize};

use crate::signing::{
    Signature, Signer, parse_public_key, public_key_base64, public_key_id, public_key_sha256,
};
use crate::{Error, Result};

/// The format of lists, certificates and statements this version reads and writes.
pub const FORMAT: u32 = 1;

pub const LIST_FILE: &str = "revoked.json";
pub const CERT_FILE: &str = "list-key.json";
pub const SIG_SUFFIX: &str = ".minisig";
/// The extension of an owner's statement.
pub const STATEMENT_EXT: &str = "dskey";

// The trusted comment of each kind of signature: a signature made for one kind of file cannot
// be used for another.
const CERT_COMMENT: &str = "devstribution list-key";
const LIST_COMMENT: &str = "devstribution revocation-list";
const WITHDRAWAL_COMMENT: &str = "devstribution key-withdrawal";
const ROTATION_COMMENT: &str = "devstribution key-rotation";
const ROTATION_NEW_COMMENT: &str = "devstribution key-rotation new-key";

fn bad(msg: impl Into<String>) -> Error {
    Error::Revocation(msg.into())
}

fn check_date(what: &str, d: &str) -> Result<()> {
    let b = d.as_bytes();
    let ok = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if ok {
        Ok(())
    } else {
        Err(bad(format!("{what} `{d}` is not a date (YYYY-MM-DD)")))
    }
}

fn check_sha256(what: &str, h: &str) -> Result<()> {
    if h.len() == 64 && h.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')) {
        Ok(())
    } else {
        Err(bad(format!(
            "{what} `{h}` is not a SHA-256 (64 lowercase hex digits)"
        )))
    }
}

// -- the list ----------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The key's owner says it is no longer safe.
    Withdrawn,
    /// Something it signed did harm; the entry says what, with evidence.
    Malicious,
    /// Listed by mistake, or cleared after an appeal: kept for the record, changes nothing.
    Cleared,
}

/// One entry of a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub status: Status,
    /// The SHA-256 of the whole public key ([`public_key_sha256`]).
    pub key: String,
    /// Only these packages (`content_sha256`); empty means everything the key signed.
    #[serde(default)]
    pub packages: Vec<String>,
    /// Shown to people; matching uses `key` and `packages` only.
    pub namespace: String,
    pub reason: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub publisher_statement: Option<String>,
    #[serde(default)]
    pub replacement: Option<String>,
    pub listed: String,
    pub updated: String,
}

/// A list (`revoked.json`). Unknown fields are ignored, so a later list can add some.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevocationList {
    pub format: u32,
    pub serial: u64,
    pub issued: String,
    pub expires: String,
    pub entries: Vec<Entry>,
}

impl RevocationList {
    fn check(&self) -> Result<()> {
        if self.format != FORMAT {
            return Err(bad(format!(
                "list format {} (this version reads {FORMAT}): update devstribution",
                self.format
            )));
        }
        check_date("issued", &self.issued)?;
        check_date("expires", &self.expires)?;
        let mut ids = std::collections::BTreeSet::new();
        for e in &self.entries {
            if !ids.insert(&e.id) {
                return Err(bad(format!("entry `{}` appears twice", e.id)));
            }
            check_sha256(&format!("entry `{}`: key", e.id), &e.key)?;
            for p in &e.packages {
                check_sha256(&format!("entry `{}`: package", e.id), p)?;
            }
            check_date(&format!("entry `{}`: listed", e.id), &e.listed)?;
            check_date(&format!("entry `{}`: updated", e.id), &e.updated)?;
        }
        Ok(())
    }

    /// Whether the list is past its expiry date on `today`. An expired list still applies.
    pub fn expired(&self, today: &str) -> bool {
        today > self.expires.as_str()
    }

    /// The entry that applies to a package signed with `key` (base64) whose content is
    /// `content_sha256`: `malicious` before `withdrawn`; `cleared` entries never apply.
    pub fn find(&self, key: &str, content_sha256: &str) -> Option<&Entry> {
        let key = parse_public_key(key).ok().map(|k| public_key_sha256(&k))?;
        let applies = |e: &&Entry| {
            e.status != Status::Cleared
                && e.key == key
                && (e.packages.is_empty() || e.packages.iter().any(|p| p == content_sha256))
        };
        let mut found: Vec<&Entry> = self.entries.iter().filter(applies).collect();
        found.sort_by_key(|e| e.status != Status::Malicious);
        found.into_iter().next()
    }
}

/// The certificate of the key that signs lists (`list-key.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListKeyCert {
    pub format: u32,
    /// The list key, base64.
    pub key: String,
    /// The last day lists signed with it are accepted.
    pub not_after: String,
}

/// The four files of a list, as read from a folder.
#[derive(Debug, Clone)]
pub struct ListFiles {
    pub list: Vec<u8>,
    pub list_sig: String,
    pub cert: Vec<u8>,
    pub cert_sig: String,
}

impl ListFiles {
    /// Reads `revoked.json`, `list-key.json` and their signatures from `dir`.
    pub fn read(dir: &Path) -> Result<ListFiles> {
        let read = |name: String| {
            let path = dir.join(&name);
            std::fs::read(&path).map_err(|e| Error::io(path, e))
        };
        let text = |name: String| {
            read(name.clone())
                .and_then(|b| String::from_utf8(b).map_err(|_| bad(format!("{name} is not text"))))
        };
        Ok(ListFiles {
            list: read(LIST_FILE.into())?,
            list_sig: text(format!("{LIST_FILE}{SIG_SUFFIX}"))?,
            cert: read(CERT_FILE.into())?,
            cert_sig: text(format!("{CERT_FILE}{SIG_SUFFIX}"))?,
        })
    }

    /// Writes the four files into `dir`.
    pub fn write(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        for (name, data) in [
            (LIST_FILE.to_string(), self.list.as_slice()),
            (format!("{LIST_FILE}{SIG_SUFFIX}"), self.list_sig.as_bytes()),
            (CERT_FILE.to_string(), self.cert.as_slice()),
            (format!("{CERT_FILE}{SIG_SUFFIX}"), self.cert_sig.as_bytes()),
        ] {
            let path = dir.join(name);
            std::fs::write(&path, data).map_err(|e| Error::io(path, e))?;
        }
        Ok(())
    }

    /// Checks the chain (a root certified the list key, still within its end date on `today`,
    /// and the list key signed the list) and returns the list. Expiry is not checked here: an
    /// expired list still applies, and the caller says so.
    pub fn verify(&self, roots: &[PublicKey], today: &str) -> Result<RevocationList> {
        if roots.is_empty() {
            return Err(bad(
                "this version of devstribution has no root keys to check lists with yet",
            ));
        }
        let cert_sig = Signature::parse(&self.cert_sig)?;
        if !roots.iter().any(|r| {
            cert_sig
                .verify_document(r, &self.cert, CERT_COMMENT)
                .is_ok()
        }) {
            return Err(bad(
                "the list key is not certified by a devstribution root key",
            ));
        }
        let cert: ListKeyCert =
            serde_json::from_slice(&self.cert).map_err(|e| bad(format!("{CERT_FILE}: {e}")))?;
        if cert.format != FORMAT {
            return Err(bad(format!(
                "{CERT_FILE} format {} (this version reads {FORMAT}): update devstribution",
                cert.format
            )));
        }
        check_date("not_after", &cert.not_after)?;
        if today > cert.not_after.as_str() {
            return Err(bad(format!(
                "the list key was certified until {}: a list signed with it is no longer accepted",
                cert.not_after
            )));
        }
        let list_key = parse_public_key(&cert.key)?;
        Signature::parse(&self.list_sig)?
            .verify_document(&list_key, &self.list, LIST_COMMENT)
            .map_err(|e| bad(format!("{LIST_FILE} is not signed by the list key ({e})")))?;
        let list: RevocationList =
            serde_json::from_slice(&self.list).map_err(|e| bad(format!("{LIST_FILE}: {e}")))?;
        list.check()?;
        Ok(list)
    }
}

/// Makes the files of a list: for `Devstribution/trust` (made offline), and for tests.
pub fn sign_list(
    root: &Signer,
    list_key: &Signer,
    not_after: &str,
    list: &RevocationList,
) -> Result<ListFiles> {
    let cert = serde_json::to_vec_pretty(&ListKeyCert {
        format: FORMAT,
        key: public_key_base64(list_key.public_key()),
        not_after: not_after.into(),
    })
    .expect("serializes");
    let data = serde_json::to_vec_pretty(list).expect("serializes");
    Ok(ListFiles {
        list_sig: list_key.sign_document(&data, LIST_COMMENT)?,
        list: data,
        cert_sig: root.sign_document(&cert, CERT_COMMENT)?,
        cert,
    })
}

// -- owners' statements ------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Lost,
    Stolen,
    Retired,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Lost => "lost",
            Reason::Stolen => "stolen",
            Reason::Retired => "retired",
        }
    }
}

impl std::str::FromStr for Reason {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Reason, String> {
        match s {
            "lost" => Ok(Reason::Lost),
            "stolen" => Ok(Reason::Stolen),
            "retired" => Ok(Reason::Retired),
            _ => Err(format!("`{s}`: the reason is lost, stolen or retired")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementKind {
    /// The key is no longer safe: nothing signed with it is accepted from now on.
    Withdrawal,
    /// The publisher moved to `new_key`: launchers that pinned `key` accept the new one.
    Rotation,
}

/// What an owner states about their key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Statement {
    pub format: u32,
    pub kind: StatementKind,
    /// The namespace the key signs for, shown to people.
    pub namespace: String,
    /// The key the statement is about, base64.
    pub key: String,
    /// Withdrawals only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    /// The owner's new key, base64: required for a rotation, optional for a withdrawal (where
    /// it is only shown: a thief can sign a withdrawal too, so it moves no trust).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_key: Option<String>,
    pub date: String,
}

/// A `.dskey` file: the statement's exact text and its signatures.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatementFile {
    statement: String,
    signature: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    new_key_signature: Option<String>,
}

/// A statement whose signatures were checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedStatement {
    pub statement: Statement,
    /// Identifies the statement: the start of the SHA-256 of its text.
    pub id: String,
    /// [`public_key_sha256`] of `key`, and of `new_key` when there is one.
    pub key_sha256: String,
    pub new_key_sha256: Option<String>,
    /// The fingerprints people see.
    pub key_id: String,
    pub new_key_id: Option<String>,
}

fn statement_file(
    statement: &Statement,
    signer: &Signer,
    comment: &str,
    new: Option<&Signer>,
) -> Result<String> {
    let text = serde_json::to_string_pretty(statement).expect("serializes");
    let file = StatementFile {
        signature: signer.sign_document(text.as_bytes(), comment)?,
        new_key_signature: new
            .map(|n| n.sign_document(text.as_bytes(), ROTATION_NEW_COMMENT))
            .transpose()?,
        statement: text,
    };
    Ok(serde_json::to_string_pretty(&file).expect("serializes") + "\n")
}

/// The text of a withdrawal of `key`'s own key, signed with it.
pub fn withdrawal(
    key: &Signer,
    namespace: &str,
    reason: Reason,
    new_key: Option<&PublicKey>,
    date: &str,
) -> Result<String> {
    check_date("date", date)?;
    let statement = Statement {
        format: FORMAT,
        kind: StatementKind::Withdrawal,
        namespace: namespace.into(),
        key: public_key_base64(key.public_key()),
        reason: Some(reason),
        new_key: new_key.map(public_key_base64),
        date: date.into(),
    };
    statement_file(&statement, key, WITHDRAWAL_COMMENT, None)
}

/// The text of a rotation from `old` to `new`: the old key names the new one, the new key signs
/// back (so nobody can claim someone else's key as their new one).
pub fn rotation(old: &Signer, new: &Signer, namespace: &str, date: &str) -> Result<String> {
    check_date("date", date)?;
    if public_key_base64(old.public_key()) == public_key_base64(new.public_key()) {
        return Err(bad("the new key is the old key"));
    }
    let statement = Statement {
        format: FORMAT,
        kind: StatementKind::Rotation,
        namespace: namespace.into(),
        key: public_key_base64(old.public_key()),
        reason: None,
        new_key: Some(public_key_base64(new.public_key())),
        date: date.into(),
    };
    statement_file(&statement, old, ROTATION_COMMENT, Some(new))
}

/// Reads a `.dskey` file and checks its signatures against the keys it names.
pub fn read_statement(text: &str) -> Result<VerifiedStatement> {
    let file: StatementFile =
        serde_json::from_str(text).map_err(|e| bad(format!("not a key statement: {e}")))?;
    let s: Statement = serde_json::from_str(&file.statement)
        .map_err(|e| bad(format!("not a key statement: {e}")))?;
    if s.format != FORMAT {
        return Err(bad(format!(
            "statement format {} (this version reads {FORMAT}): update devstribution",
            s.format
        )));
    }
    check_date("date", &s.date)?;
    format!("{}/x", s.namespace)
        .parse::<crate::manifest::AppId>()
        .map_err(|_| bad(format!("`{}` is not a valid namespace", s.namespace)))?;
    let key = parse_public_key(&s.key)?;
    let new_key = s.new_key.as_deref().map(parse_public_key).transpose()?;
    let data = file.statement.as_bytes();
    match s.kind {
        StatementKind::Withdrawal => {
            if s.reason.is_none() {
                return Err(bad("a withdrawal gives its reason"));
            }
            Signature::parse(&file.signature)?.verify_document(&key, data, WITHDRAWAL_COMMENT)?;
        }
        StatementKind::Rotation => {
            let (Some(new), Some(new_sig)) = (&new_key, &file.new_key_signature) else {
                return Err(bad("a rotation names the new key and is signed by it"));
            };
            if public_key_base64(new) == public_key_base64(&key) {
                return Err(bad("the new key is the old key"));
            }
            Signature::parse(&file.signature)?.verify_document(&key, data, ROTATION_COMMENT)?;
            Signature::parse(new_sig)?.verify_document(new, data, ROTATION_NEW_COMMENT)?;
        }
    }
    let digest = crate::files::sha256(data);
    Ok(VerifiedStatement {
        id: digest[..8].iter().map(|b| format!("{b:02x}")).collect(),
        key_sha256: public_key_sha256(&key),
        new_key_sha256: new_key.as_ref().map(public_key_sha256),
        key_id: public_key_id(&key),
        new_key_id: new_key.as_ref().map(public_key_id),
        statement: s,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::generate_keys;

    fn signer() -> Signer {
        Signer::from_unencrypted(&generate_keys("acme", None).unwrap().secret_key)
            .unwrap()
            .unwrap()
    }

    fn sha(s: &Signer) -> String {
        public_key_sha256(s.public_key())
    }

    fn b64(s: &Signer) -> String {
        public_key_base64(s.public_key())
    }

    fn entry(id: &str, status: Status, key: String, packages: Vec<String>) -> Entry {
        Entry {
            id: id.into(),
            status,
            key,
            packages,
            namespace: "acme".into(),
            reason: "test".into(),
            evidence: vec![],
            publisher_statement: None,
            replacement: None,
            listed: "2026-10-01".into(),
            updated: "2026-10-01".into(),
        }
    }

    fn list(serial: u64, entries: Vec<Entry>) -> RevocationList {
        RevocationList {
            format: FORMAT,
            serial,
            issued: "2026-10-26".into(),
            expires: "2026-11-25".into(),
            entries,
        }
    }

    #[test]
    fn a_list_verifies_through_the_chain() {
        let (root, list_key) = (signer(), signer());
        let bad_key = signer();
        let l = list(
            3,
            vec![entry("1", Status::Malicious, sha(&bad_key), vec![])],
        );
        let files = sign_list(&root, &list_key, "2027-10-01", &l).unwrap();
        let roots = [root.public_key().clone()];
        assert_eq!(files.verify(&roots, "2026-10-29").unwrap(), l);

        // Written and read back.
        let dir = tempfile::tempdir().unwrap();
        files.write(dir.path()).unwrap();
        assert_eq!(
            ListFiles::read(dir.path())
                .unwrap()
                .verify(&roots, "2026-10-29")
                .unwrap(),
            l
        );

        // Not a root of ours; no roots at all; the list key past its end date.
        assert!(
            files
                .verify(&[signer().public_key().clone()], "2026-10-29")
                .is_err()
        );
        assert!(files.verify(&[], "2026-10-29").is_err());
        let e = files.verify(&roots, "2027-10-02").unwrap_err().to_string();
        assert!(e.contains("certified until 2027-10-01"), "{e}");

        // A list changed after signing; a list signed by the root directly; a certificate
        // signature used as a list signature.
        let mut changed = files.clone();
        changed.list = String::from_utf8(files.list.clone())
            .unwrap()
            .replace("\"serial\": 3", "\"serial\": 4")
            .into_bytes();
        assert!(changed.verify(&roots, "2026-10-29").is_err());
        let mut by_root = files.clone();
        by_root.list_sig = root.sign_document(&files.list, LIST_COMMENT).unwrap();
        assert!(by_root.verify(&roots, "2026-10-29").is_err());
        let mut swapped = files.clone();
        swapped.list_sig = list_key.sign_document(&files.list, CERT_COMMENT).unwrap();
        assert!(swapped.verify(&roots, "2026-10-29").is_err());
    }

    #[test]
    fn bad_lists_are_refused() {
        let (root, list_key) = (signer(), signer());
        let roots = [root.public_key().clone()];
        let k = sha(&signer());
        let mut twice = list(1, vec![entry("1", Status::Withdrawn, k.clone(), vec![])]);
        twice.entries.push(twice.entries[0].clone());
        let mut newer = list(1, vec![]);
        newer.format = 2;
        let mut bad_date = list(1, vec![]);
        bad_date.expires = "25/11/2026".into();
        for l in [
            twice,
            newer,
            bad_date,
            list(1, vec![entry("1", Status::Withdrawn, "ABC".into(), vec![])]),
            list(
                1,
                vec![entry("1", Status::Withdrawn, k, vec!["xyz".into()])],
            ),
        ] {
            let files = sign_list(&root, &list_key, "2027-01-01", &l).unwrap();
            assert!(files.verify(&roots, "2026-10-29").is_err(), "{l:?}");
        }
    }

    #[test]
    fn entries_match_by_key_and_package() {
        let (k1, k2) = (signer(), signer());
        let pkg = "a".repeat(64);
        let l = list(
            1,
            vec![
                entry("w", Status::Withdrawn, sha(&k1), vec![]),
                entry("m", Status::Malicious, sha(&k1), vec![pkg.clone()]),
                entry("c", Status::Cleared, sha(&k2), vec![]),
                entry("p", Status::Malicious, sha(&k2), vec![pkg.clone()]),
            ],
        );
        // Malicious first, then withdrawn; cleared never; a package entry only for that package.
        assert_eq!(l.find(&b64(&k1), &pkg).unwrap().id, "m");
        assert_eq!(l.find(&b64(&k1), &"b".repeat(64)).unwrap().id, "w");
        assert_eq!(l.find(&b64(&k2), &pkg).unwrap().id, "p");
        assert!(l.find(&b64(&k2), &"b".repeat(64)).is_none());
        assert!(l.find(&b64(&signer()), &pkg).is_none());
        assert!(l.expired("2026-11-26") && !l.expired("2026-11-25"));
    }

    #[test]
    fn withdrawals_and_rotations() {
        let (old, new) = (signer(), signer());
        let text = withdrawal(
            &old,
            "acme",
            Reason::Stolen,
            Some(new.public_key()),
            "2026-10-29",
        )
        .unwrap();
        let w = read_statement(&text).unwrap();
        assert_eq!(w.statement.kind, StatementKind::Withdrawal);
        assert_eq!(w.statement.reason, Some(Reason::Stolen));
        assert_eq!(w.key_sha256, sha(&old));
        assert_eq!(w.new_key_sha256, Some(sha(&new)));

        let r = read_statement(&rotation(&old, &new, "acme", "2026-10-29").unwrap()).unwrap();
        assert_eq!(r.statement.kind, StatementKind::Rotation);
        assert_eq!(r.statement.new_key, Some(b64(&new)));
        assert_ne!(r.id, w.id);
        assert!(rotation(&old, &old, "acme", "2026-10-29").is_err());
    }

    #[test]
    fn forged_statements_are_refused() {
        let (old, new, other) = (signer(), signer(), signer());
        let text = withdrawal(&old, "acme", Reason::Lost, None, "2026-10-29").unwrap();
        let mut file: StatementFile = serde_json::from_str(&text).unwrap();

        // Someone else's key named in the statement.
        let forged = file.statement.replace(&b64(&old), &b64(&other));
        let swapped = StatementFile {
            statement: forged,
            signature: file.signature.clone(),
            new_key_signature: None,
        };
        assert!(read_statement(&serde_json::to_string(&swapped).unwrap()).is_err());

        // A withdrawal signature presented as a rotation.
        file.statement = file
            .statement
            .replace("\"withdrawal\"", "\"rotation\"")
            .replace(
                "\"reason\": \"lost\",",
                &format!("\"new_key\": \"{}\",", b64(&new)),
            );
        file.new_key_signature = Some(
            new.sign_document(file.statement.as_bytes(), ROTATION_NEW_COMMENT)
                .unwrap(),
        );
        assert!(read_statement(&serde_json::to_string(&file).unwrap()).is_err());

        // A rotation to a key that did not sign back.
        let text = rotation(&old, &new, "acme", "2026-10-29").unwrap();
        let mut file: StatementFile = serde_json::from_str(&text).unwrap();
        file.new_key_signature = Some(
            other
                .sign_document(file.statement.as_bytes(), ROTATION_NEW_COMMENT)
                .unwrap(),
        );
        assert!(read_statement(&serde_json::to_string(&file).unwrap()).is_err());
        file.new_key_signature = None;
        assert!(read_statement(&serde_json::to_string(&file).unwrap()).is_err());
        assert!(read_statement("not json").is_err());
    }
}
