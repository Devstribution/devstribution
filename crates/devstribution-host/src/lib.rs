//! Installs, prepares and runs devstribution apps. Shared by the launcher and the CLI, so that
//! `devstribution dev` runs apps exactly as the launcher does.

pub mod paths;
pub mod run;
pub mod runner;
pub mod secrets;
pub mod store;
pub mod time;
pub mod uv;
pub mod zone;

pub use paths::Paths;
