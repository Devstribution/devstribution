//! `keygen`, `pack` and `verify`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use devstribution_core::manifest::AppId;
use devstribution_core::package::{self, Limits, SignatureStatus, VerifyOptions};
use devstribution_core::signing::{self, Signer};

use crate::checks;
use crate::env;

const PASSWORD_ENV: &str = "DEVSTRIBUTION_KEY_PASSWORD";

pub(crate) fn validate_namespace(namespace: &str) -> Result<()> {
    format!("{namespace}/x")
        .parse::<AppId>()
        .map(|_| ())
        .map_err(|_| {
            anyhow::anyhow!(
                "`{namespace}` is not a valid namespace: use lowercase letters, digits and `-`"
            )
        })
}

pub fn keygen(namespace: &str, dir: Option<PathBuf>, no_password: bool) -> Result<()> {
    validate_namespace(namespace)?;
    let dir = match dir {
        Some(d) => d,
        None => env::keys_dir()?,
    };
    let secret_path = dir.join(format!("{namespace}.key"));
    let public_path = dir.join(format!("{namespace}.pub"));
    for path in [&secret_path, &public_path] {
        if path.exists() {
            bail!(
                "{} already exists. A namespace keeps its key: replacing it would make your \
                 users' launchers refuse your next versions. Move the old files away only if \
                 you really want a new identity.",
                path.display()
            );
        }
    }
    let password = if no_password {
        eprintln!("warning: the secret key will be stored without a password");
        None
    } else if let Ok(pw) = std::env::var(PASSWORD_ENV) {
        Some(pw)
    } else {
        let pw = rpassword::prompt_password("Password for the new key: ")?;
        if pw.is_empty() {
            bail!("empty password (use --no-password to store the key unencrypted)");
        }
        if rpassword::prompt_password("Repeat the password: ")? != pw {
            bail!("the passwords do not match");
        }
        Some(pw)
    };
    let keys = signing::generate_keys(namespace, password)?;
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    write_private(&secret_path, keys.secret_key.as_bytes())?;
    fs::write(&public_path, &keys.public_key)?;
    say!("Key {} created for namespace `{namespace}`", keys.key_id);
    say!(
        "  secret key: {}  (keep it private and backed up)",
        secret_path.display()
    );
    say!(
        "  public key: {}  (publish it; launchers use it to verify you)",
        public_path.display()
    );
    Ok(())
}

fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    let mut file = options
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.write_all(data)?;
    Ok(())
}

pub(crate) fn load_signer(path: &Path) -> Result<Signer> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    if let Some(signer) = Signer::from_unencrypted(&text)? {
        return Ok(signer);
    }
    let password = match std::env::var(PASSWORD_ENV) {
        Ok(pw) => pw,
        Err(_) => rpassword::prompt_password(format!("Password for {}: ", path.display()))?,
    };
    Ok(Signer::from_encrypted(&text, password)?)
}

pub fn pack(dir: &Path, key: Option<PathBuf>, out: Option<PathBuf>) -> Result<()> {
    let app_dir = env::absolute(dir)?;
    let runner_dir = devstribution_host::runner::install(&env::paths()?)?;

    if !env::uv_succeeds(&["lock", "--check", "--quiet"], &app_dir)? {
        bail!("uv.lock does not match pyproject.toml: run `uv lock` and try again");
    }
    let checked = checks::check_app(&app_dir, &runner_dir)?;
    checks::print_warnings(&checked.warnings);
    let manifest = &checked.manifest;

    let key_path = match key {
        Some(k) => k,
        None => {
            let path = env::keys_dir()?.join(format!("{}.key", manifest.id.namespace));
            if !path.exists() {
                bail!(
                    "no signing key for namespace `{ns}` ({}); create one with \
                     `devstribution keygen --namespace {ns}` or pass --key",
                    path.display(),
                    ns = manifest.id.namespace
                );
            }
            path
        }
    };
    let signer = load_signer(&key_path)?;
    let out = out.unwrap_or_else(|| {
        app_dir
            .join("dist")
            .join(format!("{}-{}.dsapp", manifest.id.app, manifest.version))
    });
    let result = package::pack(&app_dir, &signer, &out)?;
    checks::print_warnings(&result.lock_warnings);
    say!(
        "Packed {} {} ({} files, {} KB), signed with key {}",
        result.manifest.id,
        result.manifest.version,
        result.file_count,
        result.package_bytes.div_ceil(1024),
        result.key_id
    );
    if !result.excluded.is_empty() {
        say!("  left out: {}", result.excluded.join(", "));
    }
    say!("  {}", out.display());
    Ok(())
}

/// `pack --list`: what would go into the package, with no checks of the code and no signing.
pub fn list(dir: &Path) -> Result<()> {
    let app_dir = env::absolute(dir)?;
    let app = package::collect_app_files(&app_dir)?;
    say!("Would pack {} file(s):", app.files.len());
    for rel in app.files.keys() {
        let mark = if app.unconfirmed_data.contains(rel) {
            "  ! "
        } else {
            "    "
        };
        say!("{mark}{rel}");
    }
    if !app.excluded.is_empty() {
        say!("Left out:");
        for rel in &app.excluded {
            say!("    {rel}");
        }
    }
    if !app.unconfirmed_data.is_empty() {
        say!(
            "! {} file(s) look like data: `pack` refuses them until {} leaves them out, or names \
             them with `!path` if the app ships them on purpose",
            app.unconfirmed_data.len(),
            package::IGNORE_FILE
        );
    }
    Ok(())
}

pub fn verify(file: &Path, key: Option<PathBuf>) -> Result<()> {
    let public_key = match &key {
        Some(path) => {
            let text =
                fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
            Some(signing::parse_public_key(&text)?)
        }
        None => None,
    };
    let tmp = tempfile::tempdir()?;
    let opts = VerifyOptions {
        trusted_key: public_key.as_ref(),
        limits: Limits::default(),
    };
    let verified = package::extract_and_verify(file, &tmp.path().join("app"), &opts)?;
    let m = &verified.manifest;
    say!(
        "{} {}: \"{}\" ({})",
        m.id,
        m.version,
        m.title,
        m.kind.as_str()
    );
    say!(
        "  {} files, hashes OK, content hash {}",
        verified.files.len(),
        verified.content_hash
    );
    match verified.signature {
        SignatureStatus::Trusted { key_id } => {
            say!("  signature OK: made by the given key {key_id}")
        }
        SignatureStatus::NewKey { key_id } => say!(
            "  signature valid, made by key {key_id} carried in the package; \
             pass --key <publisher>.pub to check it is the publisher you expect"
        ),
    }
    Ok(())
}
