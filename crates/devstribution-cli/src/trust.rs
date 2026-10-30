//! `trust export` and `trust show`: the `.trust` files IT approves authors with.

use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use devstribution_core::package::{self, Limits, VerifyOptions};
use devstribution_core::trust::TrustedAuthor;

use crate::env;

/// Writes the `.trust` file for a namespace: from its public key (by default the one `keygen`
/// made), or from the key a package carries.
pub fn export(
    namespace: Option<String>,
    key: Option<PathBuf>,
    from_package: Option<PathBuf>,
    name: Option<String>,
    contact: Option<String>,
    out: Option<PathBuf>,
    force: bool,
) -> Result<()> {
    let (namespace, key_text) = match (from_package, namespace) {
        (Some(pkg), ns) => {
            let tmp = tempfile::tempdir()?;
            let opts = VerifyOptions {
                trusted_key: None,
                limits: Limits::default(),
            };
            let verified = package::extract_and_verify(&pkg, &tmp.path().join("app"), &opts)?;
            let pkg_ns = verified.manifest.id.namespace.clone();
            if ns.as_ref().is_some_and(|n| *n != pkg_ns) {
                bail!(
                    "the package is from `{pkg_ns}`, not `{}`",
                    ns.unwrap_or_default()
                );
            }
            (pkg_ns, verified.publisher_key)
        }
        (None, Some(ns)) => {
            let path = match key {
                Some(k) => k,
                None => env::keys_dir()?.join(format!("{ns}.pub")),
            };
            let text = fs::read_to_string(&path)
                .with_context(|| format!("reading the public key {}", path.display()))?;
            (ns, text)
        }
        (None, None) => bail!("give --namespace (with your key) or --from-package"),
    };
    let mut author = TrustedAuthor::new(&namespace, &key_text).map_err(|e| anyhow!(e))?;
    author.name = name;
    author.contact = contact;
    let out = out.unwrap_or_else(|| PathBuf::from(format!("{namespace}.trust")));
    if !force && out.exists() && !confirm_overwrite(&out)? {
        bail!("{} was left as it is", out.display());
    }
    fs::write(&out, author.to_file()).with_context(|| format!("writing {}", out.display()))?;
    say!("Trust file for `{namespace}` written to {}", out.display());
    for fp in author.fingerprints() {
        say!("  key fingerprint {fp}");
    }
    say!(
        "IT approves the author by listing this file in the devstribution policy. Tell them the \
         fingerprint through another channel, so they can check the file is really yours."
    );
    Ok(())
}

/// Asks before replacing a file IT may already have been given. Without a terminal to ask on
/// (CI, a pipe) the answer is no, and `--force` is the way to say yes.
fn confirm_overwrite(path: &Path) -> Result<bool> {
    if !io::stdin().is_terminal() {
        bail!(
            "{} already exists; use --force to replace it",
            path.display()
        );
    }
    eprint!("{} already exists. Replace it? [y/N] ", path.display());
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}

/// Prints what a `.trust` file approves, for IT to check before listing it.
pub fn show(file: &Path) -> Result<()> {
    let text = fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let author = TrustedAuthor::parse(&text).map_err(|e| anyhow!("{}: {e}", file.display()))?;
    say!("Author `{}`", author.namespace);
    if let Some(name) = &author.name {
        say!("  name: {name}");
    }
    if let Some(contact) = &author.contact {
        say!("  contact: {contact}");
    }
    for fp in author.fingerprints() {
        say!("  key fingerprint {fp}");
    }
    say!(
        "Approving it lets every app `{}/…` signed with these keys install.",
        author.namespace
    );
    Ok(())
}
