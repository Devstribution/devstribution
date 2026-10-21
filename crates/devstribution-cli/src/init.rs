//! `devstribution init`: a new app folder that already passes `dev` and `pack`.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use devstribution_core::manifest::AppId;

use crate::env;

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum TemplateKind {
    Form,
    Gui,
}

const AGENTS_MD: &str = include_str!("../templates/AGENTS.md");
const GITIGNORE: &str = "__pycache__/\n.venv/\ndist/\n*.dsapp\n*.key\n.env\n";

pub fn init(
    dir: Option<PathBuf>,
    id: &str,
    kind: TemplateKind,
    title: Option<String>,
) -> Result<()> {
    let id: AppId = id
        .parse()
        .map_err(|e: String| anyhow::anyhow!("--id: {e}"))?;
    let dir = dir.unwrap_or_else(|| PathBuf::from(&id.app));
    if dir.exists() && fs::read_dir(&dir)?.next().is_some() {
        bail!("{} is not empty", dir.display());
    }
    let title = title.unwrap_or_else(|| title_case(&id.app));
    let (manifest_tail, main_py) = match kind {
        TemplateKind::Form => (FORM_MANIFEST, FORM_MAIN),
        TemplateKind::Gui => (GUI_MANIFEST, GUI_MAIN),
    };
    let pyproject = format!(
        "[project]\n\
         name = \"{app}\"\n\
         version = \"0.1.0\"\n\
         description = \"{title}\"\n\
         requires-python = \">=3.11\"\n\
         dependencies = []\n\
         \n\
         [tool.uv]\n\
         package = false                  # apps are not built or installed as packages\n\
         \n\
         [tool.devstribution]\n\
         schema_version = 1\n\
         id = \"{id}\"\n\
         title = \"{title}\"\n\
         {manifest_tail}",
        app = id.app,
        title = title.replace('"', "'"),
    );
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("pyproject.toml"), pyproject)?;
    fs::write(dir.join("main.py"), main_py)?;
    fs::write(dir.join("AGENTS.md"), AGENTS_MD)?;
    fs::write(dir.join(".gitignore"), GITIGNORE)?;
    env::run_uv(&["lock", "--quiet"], &dir).context("creating uv.lock")?;

    println!("Created {} in {}", id, dir.display());
    println!("  run it:      devstribution dev {}", dir.display());
    println!(
        "  sign keys:   devstribution keygen --namespace {}   (once)",
        id.namespace
    );
    println!("  package it:  devstribution pack {}", dir.display());
    Ok(())
}

fn title_case(app: &str) -> String {
    let words: Vec<String> = app
        .split('-')
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect();
    let mut title = words.join(" ");
    if let Some(first) = title.get(..1) {
        title = first.to_uppercase() + &title[1..];
    }
    title
}

const FORM_MANIFEST: &str = r#"kind = "form"                    # the launcher draws the form from inputs/outputs
entry = "main:run"

[[tool.devstribution.inputs]]
name = "name"
type = "text"                    # file | folder | text | number | bool | choice | date | secret
label = "Your name"
default = "world"

[[tool.devstribution.outputs]]
name = "greeting"
type = "file"                    # file | table | text
label = "Greeting"
"#;

const FORM_MAIN: &str = r#"from pathlib import Path


def run(name: str, ctx) -> dict:
    """Called by the launcher with the form's inputs; returns the outputs."""
    ctx.progress(0.5, "Writing the greeting")
    out: Path = ctx.output_dir / "greeting.txt"
    out.write_text(f"Hello, {name}!\n", encoding="utf-8")
    return {"greeting": out}
"#;

const GUI_MANIFEST: &str = r#"kind = "gui"                     # the app brings its own window
entry = "main"                   # run as `python -m main`
"#;

const GUI_MAIN: &str = r#"import tkinter as tk

import devstribution_runner


def main() -> None:
    ctx = devstribution_runner.ctx  # logs, secrets and output folder; None outside devstribution
    root = tk.Tk()
    root.title("Hello")
    label = tk.Label(root, text="Hello from devstribution", padx=40, pady=20)
    label.pack()

    def clicked() -> None:
        label.config(text="Clicked!")
        if ctx is not None:
            ctx.log("button clicked")

    tk.Button(root, text="Click me", command=clicked).pack(pady=(0, 20))
    root.mainloop()


if __name__ == "__main__":
    main()
"#;
