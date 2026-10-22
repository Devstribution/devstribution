//! `devstribution`: developer tools for devstribution apps.

mod checks;
mod dev;
mod env;
mod init;
mod keys;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "devstribution",
    version,
    about = "Create, run, sign and check devstribution apps"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new app folder from a template
    Init {
        /// Folder to create (default: the app name)
        dir: Option<PathBuf>,
        /// App id, `<namespace>/<app>` (e.g. `alice/report-tool`)
        #[arg(long)]
        id: String,
        /// `form` (inputs, run(), outputs; the launcher draws the UI) or `gui` (your own UI)
        #[arg(long, default_value = "form")]
        kind: init::TemplateKind,
        /// Title shown to users (default: from the app name)
        #[arg(long)]
        title: Option<String>,
    },
    /// Run an app folder the way the launcher does (unsigned, for development)
    Dev {
        /// App folder (default: current folder)
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Input value, `name=value` (repeatable)
        #[arg(short, long = "input", value_name = "NAME=VALUE")]
        inputs: Vec<String>,
        /// Print the run result as JSON on stdout
        #[arg(long)]
        json: bool,
        /// Arguments passed to `cli` apps
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Check, sign and package an app folder into a .dsapp file
    Pack {
        /// App folder (default: current folder)
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Secret key (default: the key created by `keygen` for the app's namespace)
        #[arg(long)]
        key: Option<PathBuf>,
        /// Output file (default: `<dir>/dist/<app>-<version>.dsapp`)
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// Only show which files would go into the package and which are left out; sign nothing
        #[arg(long)]
        list: bool,
    },
    /// Create the signing key pair for a namespace
    Keygen {
        /// Namespace the key signs for (the part before `/` in app ids)
        #[arg(long)]
        namespace: String,
        /// Folder for the key files (default: the devstribution keys folder)
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Store the secret key unencrypted (for CI with protected secrets only)
        #[arg(long)]
        no_password: bool,
    },
    /// Check a .dsapp file: structure, hashes and (with --key) the publisher's signature
    Verify {
        file: PathBuf,
        /// Publisher's public key (.pub file)
        #[arg(long)]
        key: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Init {
            dir,
            id,
            kind,
            title,
        } => init::init(dir, &id, kind, title).map(|_| 0),
        Command::Dev {
            dir,
            inputs,
            json,
            args,
        } => dev::dev(&dir, &inputs, &args, json),
        Command::Pack {
            dir,
            key: _,
            out: _,
            list: true,
        } => keys::list(&dir).map(|_| 0),
        Command::Pack { dir, key, out, .. } => keys::pack(&dir, key, out).map(|_| 0),
        Command::Keygen {
            namespace,
            dir,
            no_password,
        } => keys::keygen(&namespace, dir, no_password).map(|_| 0),
        Command::Verify { file, key } => keys::verify(&file, key).map(|_| 0),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}
