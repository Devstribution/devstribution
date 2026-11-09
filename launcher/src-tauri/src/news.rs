//! "What's new" after the launcher was updated, and on request: from the changelog built into
//! the launcher, so it needs no connection and also works after an update the user installed by
//! hand (or IT pushed).

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use devstribution_host::paths::Paths;
use serde::{Deserialize, Serialize};

/// The changelog of this very version (`docs/releasing.md`: a release has a dated section).
const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// One version's section of the changelog.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Section {
    pub version: String,
    /// `2026-11-06`, as the changelog writes it.
    pub date: Option<String>,
    /// The section's Markdown, without its heading.
    pub text: String,
}

/// What to show: the versions after `from` up to `version`, newest first.
#[derive(Debug, Clone, Serialize)]
pub struct WhatsNew {
    pub version: String,
    /// The version the user had before, when known.
    pub from: Option<String>,
    pub sections: Vec<Section>,
}

/// The last version the user started, and whether the notice about it still waits.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Seen {
    version: Option<String>,
    /// Set by an update, cleared when the notice is opened or closed: until then it shows again
    /// at every start.
    #[serde(default)]
    pending_from: Option<Option<String>>,
}

fn seen_file(paths: &Paths) -> PathBuf {
    paths.state().join("launcher-version.json")
}

fn load(paths: &Paths) -> Option<Seen> {
    fs::read(seen_file(paths))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

fn save(paths: &Paths, seen: &Seen) -> Result<()> {
    let file = seen_file(paths);
    fs::create_dir_all(file.parent().expect("in state/"))?;
    let tmp = file.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(seen)?)?;
    fs::rename(&tmp, &file).context("saving the launcher's version")
}

/// Called once at start. Returns the notice to show, if this start is the first on a newer
/// version (or one the user has not closed yet). `used_before`: whether this data folder was
/// used by an earlier launcher (apps or runs), for launchers from before this file existed.
pub fn on_start(paths: &Paths, current: &str, used_before: bool) -> Option<WhatsNew> {
    let mut seen = match load(paths) {
        Some(seen) => seen,
        // 0.5.0 and earlier kept no version: an existing user is one who updated.
        None if used_before => Seen {
            version: None,
            pending_from: Some(None),
        },
        None => Seen::default(),
    };
    if seen.version.as_deref() != Some(current) {
        if let Some(old) = &seen.version
            && newer(current, old)
        {
            seen.pending_from = Some(Some(old.clone()));
        }
        seen.version = Some(current.to_string());
        if let Err(e) = save(paths, &seen) {
            devstribution_host::applog::write(&format!("what's new: {e:#}"));
        }
    }
    let from = seen.pending_from?;
    let sections = since(CHANGELOG, from.as_deref(), current);
    (!sections.is_empty()).then(|| WhatsNew {
        version: current.to_string(),
        from,
        sections,
    })
}

/// The notice was opened or closed: it does not come back.
pub fn dismiss(paths: &Paths) -> Result<()> {
    let mut seen = load(paths).unwrap_or_default();
    seen.pending_from = None;
    save(paths, &seen)
}

/// This version's notes, on request.
pub fn current(version: &str) -> WhatsNew {
    let mut sections = since(CHANGELOG, None, version);
    sections.truncate(1);
    WhatsNew {
        version: version.to_string(),
        from: None,
        sections,
    }
}

/// The dated sections after `from` up to `to`, newest first; with no `from`, only `to`'s.
fn since(changelog: &str, from: Option<&str>, to: &str) -> Vec<Section> {
    sections(changelog)
        .into_iter()
        .filter(|s| match from {
            Some(from) => newer(&s.version, from) && !newer(&s.version, to),
            None => s.version == to,
        })
        .collect()
}

/// Every `## [x.y.z] - date` section; `[Unreleased]` is left out.
fn sections(changelog: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut current: Option<Section> = None;
    for line in changelog.lines() {
        if let Some(head) = line.strip_prefix("## ") {
            if let Some(done) = current.take() {
                out.push(done);
            }
            let Some(rest) = head.strip_prefix('[') else {
                continue;
            };
            let Some((version, after)) = rest.split_once(']') else {
                continue;
            };
            if parse(version).is_none() {
                continue;
            }
            let date = after
                .trim()
                .trim_start_matches(['-', '–'])
                .trim()
                .to_string();
            current = Some(Section {
                version: version.to_string(),
                date: (!date.is_empty()).then_some(date),
                text: String::new(),
            });
        } else if line.starts_with("[") && line.contains("]: http") {
            // Link references at the end of the file.
            if let Some(done) = current.take() {
                out.push(done);
            }
        } else if let Some(s) = current.as_mut() {
            s.text.push_str(line);
            s.text.push('\n');
        }
    }
    out.extend(current);
    for s in &mut out {
        s.text = s.text.trim().to_string();
    }
    out.retain(|s| !s.text.is_empty());
    out
}

fn parse(version: &str) -> Option<Vec<u64>> {
    let core = version.split(['-', '+']).next()?;
    core.split('.').map(|n| n.parse().ok()).collect()
}

/// Whether `a` is a later version than `b`; one that cannot be read is never newer.
fn newer(a: &str, b: &str) -> bool {
    match (parse(a), parse(b)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# Changelog\n\nIntro.\n\n## [Unreleased]\n\n- soon\n\n## [0.6.0] - 2026-11-20\n\n### Added\n\n- schedules\n\n## [0.5.0] - 2026-11-06\n\n- faster runs\n\n## [0.4.0] - 2026-11-04\n\n- guard\n\n[0.6.0]: https://example.com\n";

    #[test]
    fn sections_after_the_old_version() {
        let s = since(LOG, Some("0.4.0"), "0.6.0");
        assert_eq!(
            s.iter().map(|s| s.version.as_str()).collect::<Vec<_>>(),
            ["0.6.0", "0.5.0"]
        );
        assert_eq!(s[0].date.as_deref(), Some("2026-11-20"));
        assert_eq!(s[0].text, "### Added\n\n- schedules");
        assert_eq!(s[1].text, "- faster runs");
        assert_eq!(since(LOG, None, "0.5.0")[0].text, "- faster runs");
        assert!(since(LOG, Some("0.6.0"), "0.6.0").is_empty());
    }

    #[test]
    fn the_built_in_changelog_reads() {
        assert!(sections(CHANGELOG).iter().any(|s| s.version == "0.5.0"));
    }

    #[test]
    fn shown_once_after_an_update_until_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths {
            root: tmp.path().to_path_buf(),
        };
        // A new user: nothing to show, now or at the next start.
        assert!(on_start(&paths, "0.5.0", false).is_none());
        assert!(on_start(&paths, "0.5.0", false).is_none());
        // Updated (any later version whose section the built-in changelog has, here 0.5.0
        // pretending the user came from 0.4.0).
        save(
            &paths,
            &Seen {
                version: Some("0.4.0".into()),
                pending_from: None,
            },
        )
        .unwrap();
        let shown = on_start(&paths, "0.5.0", true).unwrap();
        assert_eq!(shown.from.as_deref(), Some("0.4.0"));
        assert_eq!(shown.sections[0].version, "0.5.0");
        // Not closed: shown again.
        assert!(on_start(&paths, "0.5.0", true).is_some());
        dismiss(&paths).unwrap();
        assert!(on_start(&paths, "0.5.0", true).is_none());
        // An older launcher started again (a downgrade by hand) brings no notice.
        assert!(on_start(&paths, "0.4.0", true).is_none());
    }

    #[test]
    fn users_of_launchers_without_the_file_count_as_updated() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths {
            root: tmp.path().to_path_buf(),
        };
        let shown = on_start(&paths, "0.5.0", true).unwrap();
        assert_eq!(shown.from, None);
        assert_eq!(shown.sections.len(), 1);
    }
}
