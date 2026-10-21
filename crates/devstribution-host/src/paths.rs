//! The local folders (`%LOCALAPPDATA%\devstribution\` on Windows).

use std::path::PathBuf;

use anyhow::{Context, Result};
use devstribution_core::manifest::AppId;

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    /// `DEVSTRIBUTION_HOME` if set (tests, CI, portable use), else the user's local data folder.
    pub fn from_env() -> Result<Paths> {
        let root = match std::env::var_os("DEVSTRIBUTION_HOME") {
            Some(home) => PathBuf::from(home),
            None => dirs::data_local_dir()
                .context("cannot find the local data folder")?
                .join("devstribution"),
        };
        Ok(Paths { root })
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
