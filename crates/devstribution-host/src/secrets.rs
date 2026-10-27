//! App secrets in the system credential store (Windows Credential Manager, macOS Keychain,
//! Linux kernel keyring), one entry per app and secret name.

use anyhow::{Context, Result};

const SERVICE: &str = "devstribution";

fn entry(app_id: &str, name: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, &format!("{app_id}/{name}"))
        .context("cannot open the system credential store (Credential Manager on Windows), where secrets are kept")
}

pub fn get(app_id: &str, name: &str) -> Result<Option<String>> {
    match entry(app_id, name)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("reading a secret from the credential store"),
    }
}

pub fn set(app_id: &str, name: &str, value: &str) -> Result<()> {
    entry(app_id, name)?
        .set_password(value)
        .context("saving a secret in the credential store")
}

pub fn delete(app_id: &str, name: &str) -> Result<()> {
    match entry(app_id, name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).context("deleting a secret from the credential store"),
    }
}
