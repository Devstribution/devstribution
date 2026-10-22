//! Publisher keys and signatures, in minisign format (interoperable with the `minisign` tool).
//!
//! The signed data is `FILES.sha256`; the trusted comment (covered by the signature) binds it
//! to the app id, version and package schema: `devstribution id=<id> version=<v> schema=<n>`.

use std::io::Cursor;

use minisign::{KeyPair, PublicKey, PublicKeyBox, SecretKey, SecretKeyBox, SignatureBox};

use crate::manifest::{AppId, Version};
use crate::{Error, Result};

const COMMENT_PREFIX: &str = "devstribution";

/// A newly generated key pair, as the text of the `.pub` and `.key` files.
pub struct GeneratedKeys {
    pub public_key: String,
    pub secret_key: String,
    pub key_id: String,
}

/// Generates a key pair. With a password the secret key is encrypted (scrypt), as minisign does.
pub fn generate_keys(namespace: &str, password: Option<String>) -> Result<GeneratedKeys> {
    let KeyPair { pk, sk } = match password {
        Some(password) => KeyPair::generate_encrypted_keypair(Some(password)),
        None => KeyPair::generate_unencrypted_keypair(),
    }
    .map_err(sig_err)?;
    let key_id = fingerprint(&pk);
    let public_key = pk.to_box().map_err(sig_err)?.into_string().replacen(
        "untrusted comment: minisign public key",
        &format!("untrusted comment: devstribution public key for {namespace}:"),
        1,
    );
    let secret_key = sk
        .to_box(Some(&format!(
            "devstribution secret key for {namespace}, keep private"
        )))
        .map_err(sig_err)?
        .into_string();
    Ok(GeneratedKeys {
        public_key,
        secret_key,
        key_id,
    })
}

/// A secret key loaded from a `.key` file.
pub struct Signer {
    sk: SecretKey,
    pk: PublicKey,
}

impl Signer {
    /// Returns `Ok(None)` when the key is encrypted and needs a password.
    pub fn from_unencrypted(secret_key: &str) -> Result<Option<Signer>> {
        let sk_box = SecretKeyBox::from_string(secret_key).map_err(sig_err)?;
        match sk_box.into_unencrypted_secret_key() {
            Ok(sk) if !sk.is_encrypted() => Signer::new(sk).map(Some),
            _ => Ok(None),
        }
    }

    pub fn from_encrypted(secret_key: &str, password: String) -> Result<Signer> {
        let sk_box = SecretKeyBox::from_string(secret_key).map_err(sig_err)?;
        Signer::new(sk_box.into_secret_key(Some(password)).map_err(sig_err)?)
    }

    fn new(sk: SecretKey) -> Result<Signer> {
        let pk = PublicKey::from_secret_key(&sk).map_err(sig_err)?;
        Ok(Signer { sk, pk })
    }

    pub fn key_id(&self) -> String {
        fingerprint(&self.pk)
    }

    pub fn public_key(&self) -> &PublicKey {
        &self.pk
    }

    /// Signs `data` and returns the content of the `.minisig` file.
    pub fn sign(&self, data: &[u8], comment: &TrustedComment) -> Result<String> {
        let sig = minisign::sign(
            Some(&self.pk),
            &self.sk,
            Cursor::new(data),
            Some(&comment.to_string()),
            Some("devstribution package signature"),
        )
        .map_err(sig_err)?;
        Ok(sig.into_string())
    }
}

/// Loads a public key from the text of a `.pub` file (or a bare base64 key).
pub fn parse_public_key(text: &str) -> Result<PublicKey> {
    let text = text.trim();
    let pk = if text.contains('\n') {
        PublicKeyBox::from_string(text)
            .and_then(PublicKeyBox::into_public_key)
            .map_err(sig_err)?
    } else {
        PublicKey::from_base64(text).map_err(sig_err)?
    };
    Ok(pk)
}

/// The key as one base64 line, the form stored when a namespace's key is pinned.
pub fn public_key_base64(pk: &PublicKey) -> String {
    pk.to_base64()
}

/// The text of a `.pub` file for a key.
pub fn public_key_file(pk: &PublicKey, comment: &str) -> String {
    format!("untrusted comment: {comment}\n{}\n", pk.to_base64())
}

/// The key's fingerprint, the identity shown to people: the first 128 bits of the SHA-256 of
/// the whole public key, in groups of four hex digits.
///
/// Not minisign's 8-byte key number: whoever makes a key chooses that number freely, so two
/// different keys can show the same one. Trust decisions compare the full key, never this.
pub fn public_key_id(pk: &PublicKey) -> String {
    fingerprint(pk)
}

fn fingerprint(pk: &PublicKey) -> String {
    let digest = crate::files::sha256(&pk.to_bytes());
    digest[..16]
        .chunks(2)
        .map(|c| format!("{:02X}{:02X}", c[0], c[1]))
        .collect::<Vec<_>>()
        .join("-")
}

/// A signature file parsed but not yet verified.
pub struct Signature {
    sig: SignatureBox,
}

impl Signature {
    pub fn parse(text: &str) -> Result<Signature> {
        let sig = SignatureBox::from_string(text).map_err(sig_err)?;
        if !sig.is_prehashed() {
            return Err(Error::Signature(
                "legacy minisign signatures are not accepted".into(),
            ));
        }
        Ok(Signature { sig })
    }

    /// The trusted comment, which is only trustworthy after [`Signature::verify`].
    pub fn unverified_comment(&self) -> Result<TrustedComment> {
        TrustedComment::parse(&self.sig.trusted_comment().map_err(sig_err)?)
    }

    /// Verifies the signature over `data` and returns the now trusted comment.
    pub fn verify(&self, pk: &PublicKey, data: &[u8]) -> Result<TrustedComment> {
        minisign::verify(pk, &self.sig, Cursor::new(data), true, false, false).map_err(|e| {
            Error::Signature(format!("not made by key {} ({e})", public_key_id(pk)))
        })?;
        self.unverified_comment()
    }
}

/// `devstribution id=<namespace>/<app> version=<version> schema=<n>`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedComment {
    pub id: AppId,
    pub version: Version,
    pub schema: u32,
}

impl TrustedComment {
    pub fn parse(text: &str) -> Result<TrustedComment> {
        let bad = || Error::Signature(format!("unexpected trusted comment `{text}`"));
        let mut words = text.split(' ');
        if words.next() != Some(COMMENT_PREFIX) {
            return Err(bad());
        }
        let mut field = |key: &str| {
            words
                .next()
                .and_then(|w| w.strip_prefix(key))
                .and_then(|w| w.strip_prefix('='))
                .ok_or_else(bad)
        };
        let id = field("id")?.parse().map_err(|_| bad())?;
        let version = field("version")?.parse().map_err(|_| bad())?;
        let schema = field("schema")?.parse().map_err(|_| bad())?;
        if words.next().is_some() {
            return Err(bad());
        }
        let comment = TrustedComment {
            id,
            version,
            schema,
        };
        // Only one textual form is valid, e.g. `version=1.2.0` not `version=01.2.0`.
        if comment.to_string() != text {
            return Err(bad());
        }
        Ok(comment)
    }
}

impl std::fmt::Display for TrustedComment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{COMMENT_PREFIX} id={} version={} schema={}",
            self.id, self.version, self.schema
        )
    }
}

fn sig_err(e: minisign::PError) -> Error {
    Error::Signature(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment() -> TrustedComment {
        TrustedComment::parse("devstribution id=author/app version=1.2.0 schema=1").unwrap()
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let keys = generate_keys("author", None).unwrap();
        let signer = Signer::from_unencrypted(&keys.secret_key).unwrap().unwrap();
        assert_eq!(signer.key_id(), keys.key_id);
        let sig_text = signer.sign(b"data", &comment()).unwrap();

        let pk = parse_public_key(&keys.public_key).unwrap();
        let sig = Signature::parse(&sig_text).unwrap();
        assert_eq!(sig.verify(&pk, b"data").unwrap(), comment());
        assert!(sig.verify(&pk, b"tampered").is_err());

        let other = generate_keys("other", None).unwrap();
        let other_pk = parse_public_key(&other.public_key).unwrap();
        assert!(sig.verify(&other_pk, b"data").is_err());
    }

    #[test]
    fn tampered_trusted_comment_fails() {
        let keys = generate_keys("author", None).unwrap();
        let signer = Signer::from_unencrypted(&keys.secret_key).unwrap().unwrap();
        let sig_text = signer.sign(b"data", &comment()).unwrap();
        let forged = sig_text.replace("version=1.2.0", "version=9.9.9");
        let pk = parse_public_key(&keys.public_key).unwrap();
        assert!(
            Signature::parse(&forged)
                .unwrap()
                .verify(&pk, b"data")
                .is_err()
        );
    }

    #[test]
    fn encrypted_keys_need_the_password() {
        let keys = generate_keys("author", Some("pw".into())).unwrap();
        assert!(
            Signer::from_unencrypted(&keys.secret_key)
                .unwrap()
                .is_none()
        );
        assert!(Signer::from_encrypted(&keys.secret_key, "wrong".into()).is_err());
        let signer = Signer::from_encrypted(&keys.secret_key, "pw".into()).unwrap();
        assert_eq!(signer.key_id(), keys.key_id);
    }

    #[test]
    fn trusted_comment_is_strict() {
        for bad in [
            "",
            "timestamp:123",
            "devstribution id=author/app version=1.2.0",
            "devstribution id=author/app version=01.2.0 schema=1",
            "devstribution id=author/app version=1.2.0 schema=1 extra=1",
            "devstribution version=1.2.0 id=author/app schema=1",
        ] {
            assert!(TrustedComment::parse(bad).is_err(), "{bad}");
        }
    }
}
