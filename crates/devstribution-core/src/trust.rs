//! `.trust` files: an author IT can approve in managed mode.
//!
//! A `.trust` file names one namespace and the public keys that sign for it. The publisher
//! writes it (`devstribution trust export`), or a launcher that refused a package writes one
//! for the user to forward ("ask your IT to trust X"). IT lists it in the policy; from then on
//! that namespace's packages install when they are signed by one of its keys.
//!
//! ```toml
//! # devstribution trust file
//! namespace = "acme"
//! name = "Acme tools team"          # optional, shown to people
//! contact = "tools@acme.example"    # optional
//!
//! [[keys]]
//! key = "RWQ…"                      # the full public key, base64
//! fingerprint = "1A2B-…"            # optional; checked against the key when present
//! ```
//!
//! The key is what counts: trust decisions compare the full key, never the fingerprint.

use serde::{Deserialize, Serialize};

use crate::manifest::AppId;
use crate::signing::{parse_public_key, public_key_base64, public_key_id};

/// An author: a namespace and the keys allowed to sign for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedAuthor {
    pub namespace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
    pub keys: Vec<TrustedKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedKey {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

impl TrustedAuthor {
    /// An author with one key, its fingerprint filled in.
    pub fn new(namespace: &str, key: &str) -> Result<TrustedAuthor, String> {
        let pk = parse_public_key(key).map_err(|e| format!("not a public key: {e}"))?;
        let author = TrustedAuthor {
            namespace: namespace.to_string(),
            name: None,
            contact: None,
            keys: vec![TrustedKey {
                key: public_key_base64(&pk),
                fingerprint: Some(public_key_id(&pk)),
            }],
        };
        author.check()?;
        Ok(author)
    }

    /// Reads a `.trust` file's text.
    pub fn parse(text: &str) -> Result<TrustedAuthor, String> {
        let author: TrustedAuthor = toml::from_str(text).map_err(|e| e.to_string())?;
        author.check()?;
        Ok(author)
    }

    /// The namespace is valid, there is a key, every key parses, and every fingerprint given
    /// matches its key (a hand-edited file that mixes up keys is refused, not half-trusted).
    pub fn check(&self) -> Result<(), String> {
        format!("{}/x", self.namespace)
            .parse::<AppId>()
            .map_err(|_| format!("`{}` is not a valid namespace", self.namespace))?;
        if self.keys.is_empty() {
            return Err(format!("no key for `{}`", self.namespace));
        }
        for k in &self.keys {
            let pk = parse_public_key(&k.key)
                .map_err(|e| format!("`{}`: not a public key: {e}", self.namespace))?;
            if let Some(fp) = &k.fingerprint
                && !fp.eq_ignore_ascii_case(&public_key_id(&pk))
            {
                return Err(format!(
                    "`{}`: the fingerprint {fp} does not belong to the key next to it",
                    self.namespace
                ));
            }
        }
        Ok(())
    }

    /// Whether `key` (base64 or `.pub` text) is one of this author's keys, compared in full.
    pub fn has_key(&self, key: &str) -> bool {
        let Ok(key) = parse_public_key(key).map(|k| public_key_base64(&k)) else {
            return false;
        };
        self.keys
            .iter()
            .any(|k| parse_public_key(&k.key).is_ok_and(|pk| public_key_base64(&pk) == key))
    }

    pub fn fingerprints(&self) -> Vec<String> {
        self.keys
            .iter()
            .filter_map(|k| parse_public_key(&k.key).ok())
            .map(|pk| public_key_id(&pk))
            .collect()
    }

    /// The text of a `.trust` file, with a header saying what it is.
    pub fn to_file(&self) -> String {
        let body = toml::to_string(self).expect("a trust file serializes");
        format!(
            "# devstribution trust file: the keys that sign apps for `{}`.\n\
             # IT approves the author by listing this file in the devstribution policy.\n\
             # Check the fingerprint with the author through another channel first.\n\n{body}",
            self.namespace
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::generate_keys;

    fn key() -> String {
        generate_keys("acme", None).unwrap().public_key
    }

    #[test]
    fn round_trip() {
        let pubfile = key();
        let mut author = TrustedAuthor::new("acme", &pubfile).unwrap();
        author.name = Some("Acme tools".into());
        let text = author.to_file();
        assert!(text.starts_with("# devstribution trust file"));
        let back = TrustedAuthor::parse(&text).unwrap();
        assert_eq!(back, author);
        assert!(back.has_key(&pubfile));
        assert!(!back.has_key(&key()));
    }

    #[test]
    fn refuses_bad_files() {
        let k = TrustedAuthor::new("acme", &key()).unwrap().keys[0]
            .key
            .clone();
        let other_fp = TrustedAuthor::new("acme", &key()).unwrap().fingerprints()[0].clone();
        for text in [
            format!("namespace = \"Acme!\"\n[[keys]]\nkey = \"{k}\"\n"),
            "namespace = \"acme\"\nkeys = []\n".to_string(),
            "namespace = \"acme\"\n[[keys]]\nkey = \"not a key\"\n".to_string(),
            format!(
                "namespace = \"acme\"\n[[keys]]\nkey = \"{k}\"\nfingerprint = \"{other_fp}\"\n"
            ),
            format!("namespace = \"acme\"\nextra = 1\n[[keys]]\nkey = \"{k}\"\n"),
        ] {
            assert!(TrustedAuthor::parse(&text).is_err(), "{text}");
        }
    }
}
