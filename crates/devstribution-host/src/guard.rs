//! The launcher's half of the runtime guard.
//!
//! The runner's audit hook (`runner/devstribution_runner/_guard.py`) checks what the app does
//! from inside its process, where the app could get past it by patching memory. The launcher
//! sees every process the run starts, through its Job Object, whatever started it, and checks
//! each against the same rules (`_process_rules.json`): a program malware starts ends the run.

use std::sync::OnceLock;

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

#[cfg(test)]
mod tests {
    use super::*;

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
