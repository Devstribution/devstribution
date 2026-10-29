//! `keys withdraw`, `keys rotate` (an owner's statements about their key) and `revocations`
//! (what this computer's launcher knows). See docs/revocation.md.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use devstribution_core::revocation::{self, Reason, STATEMENT_EXT};
use devstribution_core::signing::parse_public_key;
use devstribution_host::revocations::Imported;
use devstribution_host::store::Store;
use devstribution_host::time::{date, now_secs};

use crate::env;
use crate::keys::{load_signer, validate_namespace};

fn default_key(namespace: &str) -> Result<PathBuf> {
    Ok(env::keys_dir()?.join(format!("{namespace}.key")))
}

fn write_statement(text: &str, out: Option<PathBuf>, default: String) -> Result<PathBuf> {
    let out = out.unwrap_or_else(|| PathBuf::from(default));
    if out.exists() {
        bail!("{} already exists", out.display());
    }
    fs::write(&out, text).with_context(|| format!("writing {}", out.display()))?;
    Ok(out)
}

pub fn withdraw(
    namespace: &str,
    key: Option<PathBuf>,
    reason: Reason,
    new_key: Option<PathBuf>,
    out: Option<PathBuf>,
) -> Result<()> {
    validate_namespace(namespace)?;
    let signer = load_signer(&key.map_or_else(|| default_key(namespace), Ok)?)?;
    let new_key = new_key
        .map(|p| {
            let text =
                fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
            Ok::<_, anyhow::Error>(parse_public_key(&text)?)
        })
        .transpose()?;
    let text = revocation::withdrawal(
        &signer,
        namespace,
        reason,
        new_key.as_ref(),
        &date(now_secs()),
    )?;
    let out = write_statement(
        &text,
        out,
        format!("{namespace}-withdraw-{}.{STATEMENT_EXT}", signer.key_id()),
    )?;
    println!(
        "Key {} withdrawn ({}): {}",
        signer.key_id(),
        reason.as_str(),
        out.display()
    );
    println!(
        "  Send this file to your users and their IT (a launcher refuses everything signed with \
         the key once it has it), and report it to devstribution so it goes on the public list."
    );
    println!("  Then sign your releases again with a new key.");
    Ok(())
}

pub fn rotate(
    namespace: &str,
    old: Option<PathBuf>,
    new: &Path,
    out: Option<PathBuf>,
) -> Result<()> {
    validate_namespace(namespace)?;
    let old = load_signer(&old.map_or_else(|| default_key(namespace), Ok)?)?;
    let new = load_signer(new)?;
    let text = revocation::rotation(&old, &new, namespace, &date(now_secs()))?;
    let out = write_statement(
        &text,
        out,
        format!("{namespace}-rotate-{}.{STATEMENT_EXT}", new.key_id()),
    )?;
    println!(
        "Key {} hands `{namespace}` over to key {}: {}",
        old.key_id(),
        new.key_id(),
        out.display()
    );
    println!(
        "  Ship this file with your next release signed with the new key: a launcher that has it \
         accepts the new key instead of refusing the package."
    );
    Ok(())
}

pub fn import(file: &Path) -> Result<()> {
    let store = Store::open(env::paths()?)?;
    match store.import_revocations(file)? {
        Imported::List { serial, entries } => {
            println!("Revocation list {serial} imported ({entries} entries)")
        }
        Imported::Statement {
            statement,
            namespace,
            key_id,
        } => println!("Key {statement} for `{namespace}` imported (key {key_id})"),
    }
    Ok(())
}

pub fn status() -> Result<()> {
    let store = Store::open(env::paths()?)?;
    let s = store.revocation_status();
    match (s.serial, &s.expires) {
        (Some(serial), Some(expires)) => println!(
            "Revocation list {serial} ({}), {} entries, issued {}, {} {expires}",
            s.source.as_deref().unwrap_or("?"),
            s.entries,
            s.issued.as_deref().unwrap_or("?"),
            if s.expired { "expired" } else { "expires" },
        ),
        _ if s.can_check_lists => println!("No revocation list yet"),
        _ => println!("No revocation list: this version cannot check lists yet"),
    }
    println!("{} key statement(s) from publishers", s.statements);
    Ok(())
}
