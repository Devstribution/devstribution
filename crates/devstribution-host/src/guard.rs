//! The launcher's half of the runtime guard.
//!
//! The runner's audit hook (`runner/devstribution_runner/_guard.py`) checks what the app does
//! from inside its process, where the app could get past it by patching memory. The launcher
//! sees every process the run starts, through its Job Object, whatever started it, and checks
//! each against the same rules (`_process_rules.json`): a program malware starts ends the run.

use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

/// Something the guard did during a run: `stop` (the run was ended), `block` (one call failed),
/// `observe` (reported only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardHit {
    pub action: String,
    pub rule: String,
    pub message: String,
    #[serde(default)]
    pub detail: String,
}

impl GuardHit {
    pub fn is_stop(&self) -> bool {
        self.action == "stop"
    }

    /// The run's error when this stopped it.
    pub fn stop_error(&self) -> String {
        format!("Stopped by devstribution: the app {}", self.message)
    }
}

/// Most guard reports kept per run.
pub const MAX_HITS: usize = 100;

#[derive(Debug, Deserialize)]
struct RulesFile {
    rules: Vec<ProcessRule>,
}

#[derive(Debug, Deserialize)]
pub struct ProcessRule {
    pub id: String,
    pub message: String,
    pub programs: Vec<String>,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub contains: Vec<String>,
}

pub fn process_rules() -> &'static [ProcessRule] {
    static RULES: OnceLock<Vec<ProcessRule>> = OnceLock::new();
    RULES.get_or_init(|| {
        serde_json::from_str::<RulesFile>(crate::runner::PROCESS_RULES)
            .expect("_process_rules.json is valid (checked by the tests)")
            .rules
    })
}

/// Lower case, without the quotes and carets cmd.exe ignores, with typographic dashes as `-`.
fn normalize(command: &str) -> String {
    command
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, '"' | '^'))
        .map(|c| match c {
            '\u{2013}' | '\u{2014}' | '\u{2015}' => '-',
            c => c,
        })
        .collect()
}

/// The first rule a command line matches: one of its words is a rule's program (folder and
/// `.exe` left out) and, after it, a word is one of the rule's flags (`/x` counts as `-x`) or
/// the rest contains one of its phrases. Same algorithm as the runner's `match_process`.
pub fn match_process<'a>(command: &str, rules: &'a [ProcessRule]) -> Option<&'a ProcessRule> {
    let text = normalize(command);
    let words: Vec<&str> = text.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        let name = word.rsplit(['/', '\\']).next().unwrap_or(word);
        let name = name.strip_suffix(".exe").unwrap_or(name);
        for rule in rules {
            if !rule.programs.iter().any(|p| p == name) {
                continue;
            }
            let rest = &words[i + 1..];
            if rule.flags.is_empty() && rule.contains.is_empty() {
                return Some(rule);
            }
            let flag = |w: &&str| match w.strip_prefix(['-', '/']) {
                Some(bare) => rule.flags.iter().any(|f| f.strip_prefix('-') == Some(bare)),
                None => rule.flags.iter().any(|f| f == w),
            };
            if rest.iter().any(flag) {
                return Some(rule);
            }
            let joined = rest.join(" ");
            if rule.contains.iter().any(|c| joined.contains(c.as_str())) {
                return Some(rule);
            }
        }
    }
    None
}

/// Whether a process in a run's Job Object is a Python the runtime guard is not in. The runner
/// starts every Python the app starts through `subprocess` or `multiprocessing` as
/// `python ... <runner>\child.py <run>\guard\config.json ...`, with the guard installed first;
/// one started any other way (a shell, `ctypes`, the `py` launcher) is not.
pub struct PythonCheck {
    bootstrap: String,
    child: String,
    settings: String,
    /// A virtual environment's `python.exe` starts the real interpreter with its own command
    /// line, so the runner's command is seen once; a second one is the app's.
    runner_seen: AtomicBool,
}

impl PythonCheck {
    pub fn new(runner_dir: &Path, settings: &Path) -> PythonCheck {
        let text = |p: &Path| normalize_path(&p.to_string_lossy());
        PythonCheck {
            bootstrap: text(&runner_dir.join("bootstrap.py")),
            child: text(&runner_dir.join("child.py")),
            settings: text(settings),
            runner_seen: AtomicBool::new(false),
        }
    }

    pub fn outside_guard(&self, command: &str) -> bool {
        if !is_python(first_word(command)) {
            return false;
        }
        let text = normalize_path(command);
        if text.contains(&self.child) && text.contains(&self.settings) {
            return false;
        }
        let first_runner =
            text.contains(&self.bootstrap) && !self.runner_seen.swap(true, Ordering::Relaxed);
        !first_runner
    }
}

/// Lower case, backslashes, no quotes: how paths are compared in command lines.
fn normalize_path(text: &str) -> String {
    text.to_lowercase().replace('/', "\\").replace('"', "")
}

/// The program of a Windows command line: up to the closing quote, or the first space.
fn first_word(command: &str) -> &str {
    let command = command.trim_start();
    match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(rest),
        None => command.split_whitespace().next().unwrap_or(""),
    }
}

/// `python`, `pythonw`, `python3.12`, `py`, `pyw`, with or without `.exe` (the runner's
/// `is_python`).
fn is_python(program: &str) -> bool {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    let version = ["pythonw", "python", "pyw", "py"]
        .iter()
        .find_map(|p| name.strip_prefix(p));
    version.is_some_and(|v| {
        v.is_empty()
            || (v.starts_with(|c: char| c.is_ascii_digit())
                && v.split('.')
                    .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pythons_outside_the_guard() {
        let runner =
            Path::new(r"C:\Users\a\AppData\Local\dev.devstribution.launcher\data\runner\abc");
        let settings = Path::new(
            r"C:\Users\a\AppData\Local\dev.devstribution.launcher\data\runs\r1\guard\config.json",
        );
        let check = PythonCheck::new(runner, settings);
        let python = r#""C:\Users\a\AppData\Local\dev.devstribution.launcher\data\envs\x\Scripts\python.exe""#;
        let runner_text = runner.display();
        let settings_text = settings.display();

        // The runner, started again by the environment's python.exe: once.
        let start = format!(r"{python} -I -S -B -X utf8 {runner_text}\bootstrap.py");
        assert!(!check.outside_guard(&start));
        assert!(check.outside_guard(&start));
        // Guarded children, however often.
        let child =
            format!(r#"{python} -I -S -B {runner_text}\CHILD.py "{settings_text}" -c "print(1)""#);
        assert!(!check.outside_guard(&child));
        assert!(!check.outside_guard(&child));
        // Child settings the app wrote elsewhere, a plain Python, the py launcher.
        assert!(check.outside_guard(&format!(
            r"{python} {runner_text}\child.py C:\temp\mine.json -c 1"
        )));
        assert!(check.outside_guard(&format!("{python} -c \"print(1)\"")));
        assert!(check.outside_guard("py -3.12 script.py"));
        assert!(check.outside_guard(r"C:\Python312\pythonw3.12.exe x.py"));
        // Not Python.
        assert!(!check.outside_guard(r#""C:\Windows\System32\cmd.exe" /c python x.py"#));
        assert!(!check.outside_guard(r"C:\tools\pyinstaller.exe x"));
        assert!(!check.outside_guard(r"C:\tools\python-report.exe"));
    }

    /// The runner's matcher is tested on the same cases.
    const CASES: &str = include_str!("../../../runner/tests/process_rule_cases.json");

    #[test]
    fn same_verdicts_as_the_runner() {
        let cases: Vec<(String, Option<String>)> = serde_json::from_str(CASES).unwrap();
        assert!(cases.len() > 10);
        for (command, expected) in cases {
            let got = match_process(&command, process_rules()).map(|r| r.id.clone());
            assert_eq!(got, expected, "{command}");
        }
    }

    #[test]
    fn every_rule_has_a_program_and_a_message() {
        for rule in process_rules() {
            assert!(
                !rule.programs.is_empty() && !rule.message.is_empty(),
                "{}",
                rule.id
            );
            for word in rule
                .programs
                .iter()
                .chain(&rule.flags)
                .chain(&rule.contains)
            {
                assert_eq!(
                    word,
                    &word.to_lowercase(),
                    "{}: rules are lower case",
                    rule.id
                );
            }
        }
    }
}
