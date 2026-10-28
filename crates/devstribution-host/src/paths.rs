//! The local folders (`%LOCALAPPDATA%\dev.devstribution.launcher\data\` on Windows).

use std::path::PathBuf;

use anyhow::{Context, Result};
use devstribution_core::manifest::AppId;

/// The launcher's bundle identifier (`tauri.conf.json`).
pub const BUNDLE_ID: &str = "dev.devstribution.launcher";

/// The folders the data folder holds, as the first launchers created them.
const OLD_FOLDERS: &[&str] = &[
    "apps", "envs", "runs", "staging", "state", "cache", "runner", "dev-envs",
];

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    /// `DEVSTRIBUTION_HOME` if set (tests, CI, portable use), else `data` in the launcher's
    /// own local data folder (named after its bundle id).
    ///
    /// Not `%LOCALAPPDATA%\devstribution`: that is where the per-user installer puts the
    /// launcher's program files, and data must not sit next to the signed executables. The
    /// bundle-id folder is also the one the uninstaller removes when asked to delete app data.
    pub fn from_env() -> Result<Paths> {
        let root = match std::env::var_os("DEVSTRIBUTION_HOME") {
            Some(home) => PathBuf::from(home),
            None => dirs::data_local_dir()
                .context("cannot find the local application data folder (%LOCALAPPDATA%): ask your IT department, the user profile may be damaged")?
                .join(BUNDLE_ID)
                .join("data"),
        };
        Ok(Paths { root })
    }

    /// Moves the data of launchers before 0.2 (`%LOCALAPPDATA%\devstribution\…`) to `self`,
    /// once, when `self` has none yet.
    pub fn migrate_old_location(&self) -> Result<()> {
        let Some(old) = dirs::data_local_dir().map(|d| d.join("devstribution")) else {
            return Ok(());
        };
        if old == self.root
            || !old.join("state").join("state.json").is_file()
            || self.root.join("state").join("state.json").exists()
        {
            return Ok(());
        }
        std::fs::create_dir_all(&self.root)?;
        for name in OLD_FOLDERS {
            let from = old.join(name);
            if from.is_dir() {
                let to = self.root.join(name);
                if to.exists() {
                    std::fs::remove_dir_all(&to)?;
                }
                std::fs::rename(&from, &to).with_context(|| {
                    format!(
                        "cannot move devstribution's data from {} to {}: close every app \
                             started from devstribution and open it again",
                        from.display(),
                        to.display()
                    )
                })?;
            }
        }
        Ok(())
    }

    /// Extracted, verified app code: `apps\<namespace>\<app>\<content-hash>\`.
    pub fn apps(&self) -> PathBuf {
        self.root.join("apps")
    }

    /// Python environments, one per installed version: `envs\<namespace>\<app>\<content-hash>\`.
    pub fn envs(&self) -> PathBuf {
        self.root.join("envs")
    }

    /// One folder per run, with `output\`, the run log and `run.json`.
    pub fn runs(&self) -> PathBuf {
        self.root.join("runs")
    }

    /// Packages being reviewed on the confirmation screen.
    pub fn staging(&self) -> PathBuf {
        self.root.join("staging")
    }

    /// Installed apps, pinned publisher keys, highest installed versions.
    pub fn state(&self) -> PathBuf {
        self.root.join("state")
    }

    /// The launcher's own log (`applog`).
    pub fn launcher_log(&self) -> PathBuf {
        self.root.join("logs").join("launcher.log")
    }

    /// What the launcher did to the run history (`journal`).
    pub fn journal(&self) -> PathBuf {
        self.state().join("journal.jsonl")
    }

    /// Bytecode of app code, kept out of the verified app folders.
    pub fn pycache(&self) -> PathBuf {
        self.root.join("cache").join("pycache")
    }

    /// Environments of `devstribution dev`.
    pub fn dev_envs(&self) -> PathBuf {
        self.root.join("dev-envs")
    }

    pub fn app_dir(&self, id: &AppId, hash: &str) -> PathBuf {
        self.apps().join(&id.namespace).join(&id.app).join(hash)
    }

    pub fn env_dir(&self, id: &AppId, hash: &str) -> PathBuf {
        self.envs().join(&id.namespace).join(&id.app).join(hash)
    }
}
