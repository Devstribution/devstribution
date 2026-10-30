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

/// Deletes every secret kept for the app, whatever its current manifest declares. Windows only:
/// the other stores cannot be listed, and their secrets go by name ([`delete`]).
pub fn delete_all(app_id: &str) -> Result<()> {
    let prefix = format!("{app_id}/");
    for user in stored_users() {
        if let Some(name) = user.strip_prefix(&prefix) {
            delete(app_id, name)?;
        }
    }
    Ok(())
}

/// Deletes every devstribution secret of this user, for "delete app data" (Windows only).
pub fn delete_every_app() -> Result<()> {
    for user in stored_users() {
        if let Some((app_id, name)) = user.rsplit_once('/') {
            delete(app_id, name)?;
        }
    }
    Ok(())
}

/// The `<app id>/<name>` of every secret in the credential store.
#[cfg(windows)]
fn stored_users() -> Vec<String> {
    use windows_sys::Win32::Security::Credentials::{CREDENTIALW, CredEnumerateW, CredFree};
    let suffix = format!(".{SERVICE}");
    let mut users = Vec::new();
    let mut count = 0;
    let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: Windows fills `count` and `list`, which stay valid until `CredFree`; each target
    // name is a nul-terminated string.
    unsafe {
        if CredEnumerateW(std::ptr::null(), 0, &mut count, &mut list) == 0 {
            return users;
        }
        for i in 0..count as usize {
            let target = (**list.add(i)).TargetName;
            if target.is_null() {
                continue;
            }
            let len = (0..).take_while(|&n| *target.add(n) != 0).count();
            let target = String::from_utf16_lossy(std::slice::from_raw_parts(target, len));
            if let Some(user) = target.strip_suffix(&suffix) {
                users.push(user.to_string());
            }
        }
        CredFree(list.cast());
    }
    users
}

#[cfg(not(windows))]
fn stored_users() -> Vec<String> {
    Vec::new()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn delete_all_removes_what_the_manifest_no_longer_names() {
        let (app, other) = ("b10-test/app", "b10-test/app2");
        set(app, "dropped_by_an_update", "x").unwrap();
        set(other, "kept", "y").unwrap();
        delete_all(app).unwrap();
        assert_eq!(get(app, "dropped_by_an_update").unwrap(), None);
        assert_eq!(get(other, "kept").unwrap().as_deref(), Some("y"));
        delete(other, "kept").unwrap();
    }
}
