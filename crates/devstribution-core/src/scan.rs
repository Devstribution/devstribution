//! A static scan of an app's code for things worth a second look before it runs.
//!
//! Nothing is executed or imported. Python files are tokenized (comments and strings are told
//! apart from code, so a word in a comment does not count) and matched against rules: modules
//! and calls that need a declared permission (network, SAP), that start programs or load native
//! code, that run code built at run time, and strings and calls typical of malware (starting
//! with Windows, reading browser passwords, switching off the antivirus, injecting into other
//! processes). Files that cannot be read as source (programs, libraries, compiled Python) are
//! flagged by name.
//!
//! The scan raises questions, it does not prove anything: code can hide from any static check
//! (names assembled at run time, data downloaded later), and a flagged line can be harmless.
//! `pack` shows the findings to the publisher, the launcher shows them before an install, and
//! `devstribution scan` prints them for a review.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::manifest::Permissions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// What the app uses, as expected from what it declares.
    Info,
    /// Worth a look: undeclared, or code that cannot be reviewed from the package.
    Warning,
    /// Typical of malware; an ordinary app has no reason to do it.
    Danger,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Danger => "danger",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Network,
    Processes,
    NativeCode,
    DynamicCode,
    Obfuscation,
    Persistence,
    Credentials,
    Surveillance,
    Evasion,
    RuntimeInstall,
    Sap,
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Stable rule name, e.g. `network` or `decode-and-run`.
    pub rule: String,
    pub category: Category,
    pub severity: Severity,
    /// What the code does, in plain words (the same for every file the rule matches).
    pub message: String,
    /// Package path of the file.
    pub path: String,
    /// First matching line, when the match is in source code.
    pub line: Option<u32>,
    /// What matched there, e.g. `import requests` or `eval(...)`.
    pub evidence: String,
    /// How many times the rule matched in this file.
    pub occurrences: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanReport {
    pub files_scanned: usize,
    /// Most severe first, then by file and line.
    pub findings: Vec<Finding>,
}

impl ScanReport {
    pub fn worst(&self) -> Option<Severity> {
        self.findings.iter().map(|f| f.severity).max()
    }

    pub fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == severity)
            .count()
    }
}

struct Rule {
    id: &'static str,
    category: Category,
    severity: Severity,
    message: &'static str,
}

const fn rule(
    id: &'static str,
    category: Category,
    severity: Severity,
    message: &'static str,
) -> Rule {
    Rule {
        id,
        category,
        severity,
        message,
    }
}

use Category as C;
use Severity as S;

// Rules whose severity depends on the manifest are written as `Info` and raised in `finish`.
const NETWORK: Rule = rule(
    "network",
    C::Network,
    S::Info,
    "Connects to other computers",
);
const SAP: Rule = rule("sap", C::Sap, S::Info, "Drives SAP GUI");
const PROCESSES: Rule = rule("processes", C::Processes, S::Info, "Starts other programs");
const NATIVE: Rule = rule(
    "native-code",
    C::NativeCode,
    S::Info,
    "Calls Windows or C functions directly",
);
const DYNAMIC: Rule = rule(
    "dynamic-code",
    C::DynamicCode,
    S::Warning,
    "Runs code that is built while the app runs, which cannot be reviewed in advance",
);
const DYNAMIC_IMPORT: Rule = rule(
    "dynamic-import",
    C::DynamicCode,
    S::Warning,
    "Imports a module whose name is computed while the app runs",
);
const BUILTINS: Rule = rule(
    "builtins",
    C::DynamicCode,
    S::Warning,
    "Reaches into Python's built-in functions, a way to hide calls such as eval",
);
const MARSHAL: Rule = rule(
    "marshal",
    C::DynamicCode,
    S::Warning,
    "Loads compiled Python code from data",
);
const DECODE_AND_RUN: Rule = rule(
    "decode-and-run",
    C::Obfuscation,
    S::Danger,
    "Decodes hidden data and runs it as code",
);
const ENCODED_BLOB: Rule = rule(
    "encoded-blob",
    C::Obfuscation,
    S::Warning,
    "Carries a long block of encoded data",
);
const LONG_LINE: Rule = rule(
    "long-line",
    C::Obfuscation,
    S::Warning,
    "Has lines over 1000 characters long, a common way to hide code",
);
const BIDI: Rule = rule(
    "bidi-text",
    C::Obfuscation,
    S::Danger,
    "Contains invisible text-direction characters, which make code read differently from what runs",
);
const PERSISTENCE: Rule = rule(
    "persistence",
    C::Persistence,
    S::Danger,
    "Makes something start with Windows or on a schedule",
);
const CREDENTIAL_THEFT: Rule = rule(
    "credential-theft",
    C::Credentials,
    S::Danger,
    "Reads saved passwords, browser data or keys of other programs",
);
const CREDENTIAL_STORE: Rule = rule(
    "credential-store",
    C::Credentials,
    S::Warning,
    "Reads the Windows credential store directly, where other apps' secrets are too (apps get theirs through ctx.secrets)",
);
const INJECTION: Rule = rule(
    "process-injection",
    C::Evasion,
    S::Danger,
    "Writes into or runs code inside other processes",
);
const EVASION: Rule = rule(
    "evasion",
    C::Evasion,
    S::Danger,
    "Switches off or works around Windows security features",
);
const DEBUGGER_CHECK: Rule = rule(
    "debugger-check",
    C::Evasion,
    S::Warning,
    "Checks whether it is being watched by a debugger",
);
const SURVEILLANCE: Rule = rule(
    "surveillance",
    C::Surveillance,
    S::Warning,
    "Records the keyboard, the screen, the clipboard or the microphone",
);
const RUNTIME_INSTALL: Rule = rule(
    "runtime-install",
    C::RuntimeInstall,
    S::Warning,
    "Installs Python packages while it runs, outside the signed lockfile",
);
const BINARY: Rule = rule(
    "binary-file",
    C::Binary,
    S::Warning,
    "Ships a program or compiled library, which cannot be reviewed from the package",
);
const SCRIPT_FILE: Rule = rule(
    "script-file",
    C::Binary,
    S::Warning,
    "Ships a Windows script (batch, PowerShell, VBScript), which the code check does not read",
);
const COMPILED_PYTHON: Rule = rule(
    "compiled-python",
    C::Binary,
    S::Warning,
    "Ships compiled Python without its source",
);

/// Modules, matched as the module itself or any submodule (`requests` matches `requests.auth`).
const MODULES: &[(&str, &Rule)] = &[
    ("socket", &NETWORK),
    ("ssl", &NETWORK),
    ("http.client", &NETWORK),
    ("http.server", &NETWORK),
    ("urllib.request", &NETWORK),
    ("urllib3", &NETWORK),
    ("requests", &NETWORK),
    ("httpx", &NETWORK),
    ("aiohttp", &NETWORK),
    ("ftplib", &NETWORK),
    ("smtplib", &NETWORK),
    ("poplib", &NETWORK),
    ("imaplib", &NETWORK),
    ("telnetlib", &NETWORK),
    ("xmlrpc", &NETWORK),
    ("socketserver", &NETWORK),
    ("websocket", &NETWORK),
    ("websockets", &NETWORK),
    ("paramiko", &NETWORK),
    ("asyncssh", &NETWORK),
    ("pycurl", &NETWORK),
    ("grpc", &NETWORK),
    ("zeep", &NETWORK),
    ("smbclient", &NETWORK),
    ("smbprotocol", &NETWORK),
    ("ldap3", &NETWORK),
    ("subprocess", &PROCESSES),
    ("pty", &PROCESSES),
    ("win32process", &PROCESSES),
    ("ctypes", &NATIVE),
    ("_ctypes", &NATIVE),
    ("cffi", &NATIVE),
    ("marshal", &MARSHAL),
    ("builtins", &BUILTINS),
    ("keyring", &CREDENTIAL_STORE),
    ("win32cred", &CREDENTIAL_STORE),
    ("browser_cookie3", &CREDENTIAL_THEFT),
    ("pynput", &SURVEILLANCE),
    ("keyboard", &SURVEILLANCE),
    ("pyHook", &SURVEILLANCE),
    ("PIL.ImageGrab", &SURVEILLANCE),
    ("mss", &SURVEILLANCE),
    ("pyscreenshot", &SURVEILLANCE),
    ("win32clipboard", &SURVEILLANCE),
    ("pyperclip", &SURVEILLANCE),
    ("sounddevice", &SURVEILLANCE),
    ("pyaudio", &SURVEILLANCE),
    ("pip", &RUNTIME_INSTALL),
    ("ensurepip", &RUNTIME_INSTALL),
];

/// Modules with harmless members: importing them counts only once the code uses another
/// member (`socket.gethostname()` only reads the computer's name).
const SOFT_MODULES: &[(&str, &[&str])] = &[("socket", &["gethostname"])];

/// Called functions, by their full name after resolving imports. A trailing `*` matches any
/// name with that start. Bare builtins only match when the file does not define its own.
const CALLS: &[(&str, &Rule)] = &[
    ("eval", &DYNAMIC),
    ("exec", &DYNAMIC),
    ("compile", &DYNAMIC),
    ("builtins.eval", &DYNAMIC),
    ("builtins.exec", &DYNAMIC),
    ("types.FunctionType", &DYNAMIC),
    ("types.CodeType", &DYNAMIC),
    ("os.system", &PROCESSES),
    ("os.popen", &PROCESSES),
    ("os.startfile", &PROCESSES),
    ("os.exec*", &PROCESSES),
    ("os.spawn*", &PROCESSES),
    ("os.posix_spawn*", &PROCESSES),
    ("asyncio.create_subprocess_*", &PROCESSES),
    ("win32api.ShellExecute", &PROCESSES),
];

/// Calls that turn data into bytes or text: together with dynamic code in one statement they
/// are how hidden code is usually unpacked.
const DECODERS: &[&str] = &[
    "base64.*",
    "zlib.decompress",
    "bz2.decompress",
    "lzma.decompress",
    "gzip.decompress",
    "codecs.decode",
    "bytes.fromhex",
    "binascii.*",
    "marshal.loads",
];

/// Names that matter wherever they appear: as an attribute (`kernel32.VirtualAllocEx`) or as a
/// string (`getattr(kernel32, "VirtualAllocEx")`).
const NAMES: &[(&str, &Rule)] = &[
    ("VirtualAllocEx", &INJECTION),
    ("VirtualProtectEx", &INJECTION),
    ("WriteProcessMemory", &INJECTION),
    ("CreateRemoteThread", &INJECTION),
    ("NtUnmapViewOfSection", &INJECTION),
    ("QueueUserAPC", &INJECTION),
    ("SetWindowsHookExA", &SURVEILLANCE),
    ("SetWindowsHookExW", &SURVEILLANCE),
    ("GetAsyncKeyState", &SURVEILLANCE),
    ("CryptUnprotectData", &CREDENTIAL_THEFT),
    ("CredEnumerateW", &CREDENTIAL_STORE),
    ("IsDebuggerPresent", &DEBUGGER_CHECK),
    ("CheckRemoteDebuggerPresent", &DEBUGGER_CHECK),
];

/// Text inside string literals, compared in lower case with `/` read as `\` for paths and runs
/// of `\` collapsed. A string assembled from pieces at run time is not seen.
const STRINGS: &[(&str, &Rule)] = &[
    ("currentversion\\run", &PERSISTENCE),
    ("currentversion\\policies\\explorer\\run", &PERSISTENCE),
    ("\\start menu\\programs\\startup", &PERSISTENCE),
    ("shell:startup", &PERSISTENCE),
    ("schtasks", &PERSISTENCE),
    ("register-scheduledtask", &PERSISTENCE),
    ("schedule.service", &PERSISTENCE),
    ("currentversion\\winlogon", &PERSISTENCE),
    ("image file execution options", &PERSISTENCE),
    ("appinit_dlls", &PERSISTENCE),
    ("\\login data", &CREDENTIAL_THEFT),
    ("\\user data\\default\\", &CREDENTIAL_THEFT),
    ("\\user data\\local state", &CREDENTIAL_THEFT),
    ("\\mozilla\\firefox\\profiles", &CREDENTIAL_THEFT),
    ("key4.db", &CREDENTIAL_THEFT),
    ("logins.json", &CREDENTIAL_THEFT),
    ("wallet.dat", &CREDENTIAL_THEFT),
    ("\\.ssh\\", &CREDENTIAL_THEFT),
    ("id_rsa", &CREDENTIAL_THEFT),
    ("\\microsoft\\credentials", &CREDENTIAL_THEFT),
    ("\\microsoft\\protect\\", &CREDENTIAL_THEFT),
    ("lsass", &CREDENTIAL_THEFT),
    ("sekurlsa", &CREDENTIAL_THEFT),
    ("mimikatz", &CREDENTIAL_THEFT),
    ("set-mppreference", &EVASION),
    ("add-mppreference", &EVASION),
    ("disablerealtimemonitoring", &EVASION),
    ("-encodedcommand", &EVASION),
    ("powershell -enc", &EVASION),
    ("powershell.exe -enc", &EVASION),
    ("-executionpolicy bypass", &EVASION),
    ("-windowstyle hidden", &EVASION),
    ("-w hidden", &EVASION),
    ("vssadmin", &EVASION),
    ("wevtutil cl", &EVASION),
    ("bcdedit", &EVASION),
    ("certutil -urlcache", &EVASION),
    ("certutil.exe -urlcache", &EVASION),
    ("bitsadmin /transfer", &EVASION),
    ("mshta", &EVASION),
    ("amsiscanbuffer", &EVASION),
    ("amsiutils", &EVASION),
    ("netsh advfirewall", &EVASION),
    ("pip install", &RUNTIME_INSTALL),
    ("pip3 install", &RUNTIME_INSTALL),
    ("-m pip", &RUNTIME_INSTALL),
    ("sapgui", &SAP),
];

const BINARY_EXTENSIONS: &[&str] = &[
    "exe", "dll", "pyd", "so", "dylib", "sys", "scr", "com", "msi", "jar", "ocx", "cpl",
];
const SCRIPT_EXTENSIONS: &[&str] = &[
    "bat", "cmd", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "hta", "lnk",
];

const LONG_LINE_CHARS: usize = 1000;
const ENCODED_BLOB_CHARS: usize = 200;

/// Scans the files of an app folder (`files` are package paths under `root`).
pub fn scan<'a>(
    root: &Path,
    files: impl IntoIterator<Item = &'a str>,
    permissions: &Permissions,
) -> ScanReport {
    let mut scan = Scan::default();
    for rel in files {
        scan.file(rel, || fs::read(root.join(rel)).ok());
    }
    scan.finish(permissions)
}

/// The scan of one set of files, before the manifest's declarations are applied.
#[derive(Default)]
pub struct Scan {
    files_scanned: usize,
    findings: BTreeMap<(&'static str, String), Finding>,
}

impl Scan {
    /// Adds one file; `read` is only called for files the scan reads as source.
    pub fn file(&mut self, rel: &str, read: impl FnOnce() -> Option<Vec<u8>>) {
        let ext = rel
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        let ext = ext.as_str();
        if BINARY_EXTENSIONS.contains(&ext) {
            self.add(&BINARY, rel, None, rel);
        } else if SCRIPT_EXTENSIONS.contains(&ext) {
            self.add(&SCRIPT_FILE, rel, None, rel);
        } else if ext == "pyc" || ext == "pyo" {
            self.add(&COMPILED_PYTHON, rel, None, rel);
        } else if (ext == "py" || ext == "pyw")
            && let Some(bytes) = read()
        {
            self.files_scanned += 1;
            self.source(rel, &String::from_utf8_lossy(&bytes));
        }
    }

    /// Scans one Python source file.
    pub fn source(&mut self, rel: &str, src: &str) {
        let mut long_line = false;
        let mut bidi = false;
        for (n, text) in src.lines().enumerate() {
            let line = Some(n as u32 + 1);
            if !long_line && text.chars().count() > LONG_LINE_CHARS {
                long_line = true;
                self.add(&LONG_LINE, rel, line, &format!("line {}", n + 1));
            }
            if !bidi && text.chars().any(is_bidi_control) {
                bidi = true;
                self.add(&BIDI, rel, line, &format!("line {}", n + 1));
            }
        }

        let tokens = tokenize(src, 1);
        let defined: HashSet<&str> = tokens
            .windows(2)
            .filter_map(|w| match (&w[0].tok, &w[1].tok) {
                (Tok::Name(k), Tok::Name(n)) if k == "def" || k == "class" => Some(n.as_str()),
                _ => None,
            })
            .collect();
        let mut aliases: HashMap<String, String> = HashMap::new();
        for stmt in tokens.split(|t| t.tok == Tok::Newline) {
            self.statement(rel, stmt, &defined, &mut aliases);
        }
    }

    fn statement(
        &mut self,
        rel: &str,
        stmt: &[Token],
        defined: &HashSet<&str>,
        aliases: &mut HashMap<String, String>,
    ) {
        let imported = imports(stmt, aliases);
        if !imported.is_empty() {
            for (module, line) in imported {
                self.module(rel, &module, line, &format!("import {module}"));
            }
            return;
        }

        let mut dynamic_line = None;
        let mut decodes = false;
        let mut j = 0;
        while j < stmt.len() {
            match &stmt[j].tok {
                Tok::Name(first) => {
                    let line = stmt[j].line;
                    let after_dot = j > 0 && stmt[j - 1].tok == Tok::Op('.');
                    let after_def = j > 0
                        && matches!(&stmt[j - 1].tok, Tok::Name(k) if k == "def" || k == "class");
                    let mut parts = vec![first.as_str()];
                    let mut k = j + 1;
                    while k + 1 < stmt.len() && stmt[k].tok == Tok::Op('.') {
                        let Tok::Name(next) = &stmt[k + 1].tok else {
                            break;
                        };
                        parts.push(next);
                        k += 2;
                    }
                    for part in &parts {
                        if let Some((_, rule)) = NAMES.iter().find(|(n, _)| n == part) {
                            self.add(rule, rel, Some(line), part);
                        }
                    }
                    let is_call = stmt.get(k).is_some_and(|t| t.tok == Tok::Op('('));
                    let full = resolve(&parts, aliases);
                    let soft = soft_module(&full);
                    if !after_dot
                        && aliases.contains_key(parts[0])
                        && let Some((module, benign)) = soft
                    {
                        let member = full[module.len()..].trim_start_matches('.');
                        let member = member.split('.').next().unwrap_or("");
                        if !benign.contains(&member) {
                            self.add(module_rule(module).unwrap(), rel, Some(line), &full);
                        }
                    }
                    if is_call && !after_dot && !after_def {
                        let literal = matches!(
                            stmt.get(k + 1),
                            Some(Token {
                                tok: Tok::Str(_),
                                ..
                            })
                        ) && stmt.get(k + 2).is_some_and(|t| t.tok == Tok::Op(')'));
                        let builtin_shadowed = parts.len() == 1 && defined.contains(parts[0]);
                        if matches!(full.as_str(), "__import__" | "importlib.import_module") {
                            match (stmt.get(k + 1).map(|t| &t.tok), literal) {
                                (Some(Tok::Str(module)), true) => {
                                    // Not deferred like a plain `import socket`: what it
                                    // returns is not followed.
                                    if let Some(rule) = module_rule(module) {
                                        let evidence = format!("{full}({module:?})");
                                        self.add(rule, rel, Some(line), &evidence);
                                    }
                                }
                                _ => {
                                    self.add(
                                        &DYNAMIC_IMPORT,
                                        rel,
                                        Some(line),
                                        &format!("{full}(...)"),
                                    );
                                    dynamic_line.get_or_insert(line);
                                }
                            }
                        } else if !builtin_shadowed && let Some(rule) = call_rule(&full) {
                            self.add(rule, rel, Some(line), &format!("{full}(...)"));
                            if rule.category == Category::DynamicCode {
                                dynamic_line.get_or_insert(line);
                            }
                        } else if parts.len() > 1
                            && soft.is_none()
                            && let Some(rule) = module_rule(&full)
                        {
                            // `subprocess.run(...)` with the import elsewhere
                            self.add(rule, rel, Some(line), &format!("{full}(...)"));
                        }
                        if matches_any(&full, DECODERS) {
                            decodes = true;
                        }
                    }
                    j = k;
                }
                Tok::Str(text) => {
                    self.string(rel, stmt[j].line, text);
                    j += 1;
                }
                _ => j += 1,
            }
        }
        if let (Some(line), true) = (dynamic_line, decodes) {
            self.add(&DECODE_AND_RUN, rel, Some(line), &format!("line {line}"));
        }
    }

    fn module(&mut self, rel: &str, module: &str, line: u32, evidence: &str) {
        if let Some((soft, benign)) = soft_module(module) {
            // Counted where a member is used, unless the import names one (`from socket import socket`).
            let member = module[soft.len()..].trim_start_matches('.');
            if member.is_empty() || benign.contains(&member) {
                return;
            }
        }
        if let Some(rule) = module_rule(module) {
            self.add(rule, rel, Some(line), evidence);
        } else if let Some(rule) = call_rule(module)
            && rule.category != Category::DynamicCode
        {
            // `from os import system`
            self.add(rule, rel, Some(line), evidence);
        }
    }

    fn string(&mut self, rel: &str, line: u32, text: &str) {
        if let Some((_, rule)) = NAMES.iter().find(|(n, _)| *n == text) {
            self.add(rule, rel, Some(line), &format!("{text:?}"));
        }
        let lower = collapse_backslashes(&text.to_lowercase());
        let as_path = collapse_backslashes(&lower.replace('/', "\\"));
        for (pattern, rule) in STRINGS {
            if lower.contains(pattern) || as_path.contains(pattern) {
                self.add(rule, rel, Some(line), &format!("{:?}", short(text)));
            }
        }
        if looks_encoded(text) {
            let evidence = format!("{} characters", text.chars().count());
            self.add(&ENCODED_BLOB, rel, Some(line), &evidence);
        }
    }

    fn add(&mut self, rule: &'static Rule, rel: &str, line: Option<u32>, evidence: &str) {
        self.findings
            .entry((rule.id, rel.to_string()))
            .and_modify(|f| f.occurrences += 1)
            .or_insert_with(|| Finding {
                rule: rule.id.to_string(),
                category: rule.category,
                severity: rule.severity,
                message: rule.message.to_string(),
                path: rel.to_string(),
                line,
                evidence: evidence.to_string(),
                occurrences: 1,
            });
    }

    /// Applies what the manifest declares and sorts the findings.
    pub fn finish(self, permissions: &Permissions) -> ScanReport {
        let mut findings: Vec<Finding> = self
            .findings
            .into_values()
            .map(|mut f| {
                if f.rule == NETWORK.id && permissions.network.is_empty() {
                    f.severity = Severity::Warning;
                    f.message = format!("{}, but the app declares no network access", f.message);
                }
                if f.rule == SAP.id && !permissions.sap {
                    f.severity = Severity::Warning;
                    f.message = format!("{}, but the app does not declare SAP", f.message);
                }
                f
            })
            .collect();
        findings.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.line.cmp(&b.line))
        });
        ScanReport {
            files_scanned: self.files_scanned,
            findings,
        }
    }
}

fn soft_module(name: &str) -> Option<(&'static str, &'static [&'static str])> {
    SOFT_MODULES.iter().copied().find(|(m, _)| {
        name.strip_prefix(m)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
    })
}

fn module_rule(name: &str) -> Option<&'static Rule> {
    MODULES
        .iter()
        .find(|(m, _)| {
            name.strip_prefix(m)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
        })
        .map(|(_, rule)| *rule)
}

fn call_rule(full: &str) -> Option<&'static Rule> {
    CALLS
        .iter()
        .find(|(pattern, _)| matches_pattern(full, pattern))
        .map(|(_, rule)| *rule)
}

fn matches_pattern(name: &str, pattern: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(start) => name.starts_with(start),
        None => name == pattern,
    }
}

fn matches_any(name: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| matches_pattern(name, p))
}

/// `sp.run` → `subprocess.run` after `import subprocess as sp`.
fn resolve(parts: &[&str], aliases: &HashMap<String, String>) -> String {
    let head = aliases
        .get(parts[0])
        .map(String::as_str)
        .unwrap_or(parts[0]);
    std::iter::once(head)
        .chain(parts[1..].iter().copied())
        .collect::<Vec<_>>()
        .join(".")
}

/// The modules an `import` / `from … import` statement brings in (as full names, so
/// `from os import system` gives `os.system`), recording the names it binds in `aliases`.
fn imports(stmt: &[Token], aliases: &mut HashMap<String, String>) -> Vec<(String, u32)> {
    let Some(at) = stmt
        .iter()
        .position(|t| matches!(&t.tok, Tok::Name(n) if n == "import"))
    else {
        return Vec::new();
    };
    let line = stmt[at].line;
    let mut found = Vec::new();
    let from = stmt[..at]
        .iter()
        .rposition(|t| matches!(&t.tok, Tok::Name(n) if n == "from"));
    let names = split_names(&stmt[at + 1..]);
    match from {
        Some(f) => {
            let base = dotted(&stmt[f + 1..at]);
            if base.is_empty() || base.starts_with('.') {
                return found; // relative import: the app's own code
            }
            found.push((base.clone(), line));
            for (name, alias) in names {
                if name == "*" {
                    continue;
                }
                let full = format!("{base}.{name}");
                aliases.insert(alias.unwrap_or(name), full.clone());
                found.push((full, line));
            }
        }
        None => {
            for (name, alias) in names {
                let bound = alias.unwrap_or_else(|| name.split('.').next().unwrap_or("").into());
                let target = if bound == name.split('.').next().unwrap_or("") {
                    bound.clone()
                } else {
                    name.clone()
                };
                aliases.insert(bound, target);
                found.push((name, line));
            }
        }
    }
    found
}

/// `a.b as c, d` → [("a.b", Some("c")), ("d", None)]; parentheses are ignored.
fn split_names(tokens: &[Token]) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    for group in tokens.split(|t| t.tok == Tok::Op(',')) {
        let group: Vec<&Token> = group
            .iter()
            .filter(|t| !matches!(t.tok, Tok::Op('(') | Tok::Op(')')))
            .collect();
        let as_at = group
            .iter()
            .position(|t| matches!(&t.tok, Tok::Name(n) if n == "as"));
        let (name_part, alias) = match as_at {
            Some(i) => (
                &group[..i],
                match group.get(i + 1).map(|t| &t.tok) {
                    Some(Tok::Name(a)) => Some(a.clone()),
                    _ => None,
                },
            ),
            None => (&group[..], None),
        };
        let name: String = name_part
            .iter()
            .map(|t| match &t.tok {
                Tok::Name(n) => n.as_str(),
                Tok::Op('.') => ".",
                Tok::Op('*') => "*",
                _ => "",
            })
            .collect();
        if !name.is_empty() {
            out.push((name, alias));
        }
    }
    out
}

fn dotted(tokens: &[Token]) -> String {
    tokens
        .iter()
        .map(|t| match &t.tok {
            Tok::Name(n) => n.as_str(),
            Tok::Op('.') => ".",
            _ => "",
        })
        .collect()
}

fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn collapse_backslashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if !(c == '\\' && out.ends_with('\\')) {
            out.push(c);
        }
    }
    out
}

fn short(text: &str) -> String {
    let mut s: String = text.chars().take(80).collect();
    if text.chars().count() > 80 {
        s.push('…');
    }
    s
}

/// A long run of base64 or hex without spaces, mixing letters and digits.
fn looks_encoded(text: &str) -> bool {
    let compact: Vec<char> = text.chars().filter(|c| !matches!(c, '\n' | '\r')).collect();
    compact.len() >= ENCODED_BLOB_CHARS
        && compact
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_'))
        && compact.iter().any(|c| c.is_ascii_digit())
        && compact.iter().any(|c| c.is_ascii_alphabetic())
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Name(String),
    Str(String),
    Op(char),
    /// End of a logical line (or `;`).
    Newline,
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    line: u32,
}

/// A Python tokenizer good enough for the scan: names, strings (with escapes decoded, and the
/// expressions inside f-strings tokenized too), operators and logical line ends. Comments,
/// numbers and indentation are dropped.
fn tokenize(src: &str, first_line: u32) -> Vec<Token> {
    let c: Vec<char> = src.chars().collect();
    let mut out: Vec<Token> = Vec::new();
    let (mut i, mut line, mut depth) = (0usize, first_line, 0i32);
    let end_statement = |out: &mut Vec<Token>, line: u32| {
        if out.last().is_some_and(|t| t.tok != Tok::Newline) {
            out.push(Token {
                tok: Tok::Newline,
                line,
            });
        }
    };
    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            if depth <= 0 {
                end_statement(&mut out, line);
            }
            line += 1;
            i += 1;
        } else if ch == '#' {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '\\' && c.get(i + 1) == Some(&'\n') {
            line += 1;
            i += 2;
        } else if ch == '\\' && c.get(i + 1) == Some(&'\r') && c.get(i + 2) == Some(&'\n') {
            line += 1;
            i += 3;
        } else if ch == '\'' || ch == '"' {
            i = string(&c, i, "", &mut line, &mut out);
        } else if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let word: String = c[start..i].iter().collect();
            let prefix = word.to_ascii_lowercase();
            if i < c.len()
                && (c[i] == '\'' || c[i] == '"')
                && matches!(
                    prefix.as_str(),
                    "r" | "u" | "b" | "f" | "t" | "br" | "rb" | "fr" | "rf" | "tr" | "rt"
                )
            {
                i = string(&c, i, &prefix, &mut line, &mut out);
            } else {
                out.push(Token {
                    tok: Tok::Name(word),
                    line,
                });
            }
        } else if ch.is_ascii_digit() {
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '.' || c[i] == '_') {
                i += 1;
            }
        } else if ch.is_whitespace() {
            i += 1;
        } else {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
            if ch == ';' && depth <= 0 {
                end_statement(&mut out, line);
            } else {
                out.push(Token {
                    tok: Tok::Op(ch),
                    line,
                });
            }
            i += 1;
        }
    }
    out
}

/// Reads the string literal starting at the quote `c[start]`; returns the index after it.
fn string(c: &[char], start: usize, prefix: &str, line: &mut u32, out: &mut Vec<Token>) -> usize {
    let raw = prefix.contains('r');
    let formatted = prefix.contains('f') || prefix.contains('t');
    let first_line = *line;
    let q = c[start];
    let triple = c.get(start + 1) == Some(&q) && c.get(start + 2) == Some(&q);
    let mut i = start + if triple { 3 } else { 1 };
    let mut text = String::new();
    let end = loop {
        let Some(&ch) = c.get(i) else { break i };
        if ch == '\\' && i + 1 < c.len() {
            let next = c[i + 1];
            if next == '\n' {
                *line += 1;
            }
            if raw {
                text.push('\\');
                text.push(next);
            } else {
                match next {
                    'n' => text.push('\n'),
                    't' => text.push('\t'),
                    '\\' | '\'' | '"' => text.push(next),
                    '\n' => {}
                    _ => {
                        text.push('\\');
                        text.push(next);
                    }
                }
            }
            i += 2;
            continue;
        }
        if ch == q {
            if !triple {
                break i + 1;
            }
            if c.get(i + 1) == Some(&q) && c.get(i + 2) == Some(&q) {
                break i + 3;
            }
        }
        if ch == '\n' {
            if !triple {
                break i; // unterminated: stop at the line end
            }
            *line += 1;
        }
        text.push(ch);
        i += 1;
    };
    if formatted {
        // The expressions of an f-string are code: `f"{eval(x)}"`.
        for (offset, expr) in fstring_expressions(&text) {
            out.extend(
                tokenize(&expr, first_line + offset)
                    .into_iter()
                    .filter(|t| t.tok != Tok::Newline),
            );
        }
    }
    out.push(Token {
        tok: Tok::Str(text),
        line: first_line,
    });
    end
}

/// The `{...}` parts of an f-string, each with its line offset in the string.
fn fstring_expressions(text: &str) -> Vec<(u32, String)> {
    let c: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 0;
    while i < c.len() {
        match c[i] {
            '\n' => line += 1,
            '{' if c.get(i + 1) == Some(&'{') => i += 1,
            '{' => {
                let start = i + 1;
                let mut depth = 1;
                let mut j = start;
                while j < c.len() && depth > 0 {
                    match c[j] {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                let expr: String = c[start..j.saturating_sub(1).max(start)].iter().collect();
                out.push((line, expr.clone()));
                line += expr.matches('\n').count() as u32;
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_one(src: &str, permissions: &Permissions) -> ScanReport {
        let mut scan = Scan::default();
        scan.source("main.py", src);
        scan.finish(permissions)
    }

    fn rules(report: &ScanReport) -> Vec<(&str, Severity)> {
        report
            .findings
            .iter()
            .map(|f| (f.rule.as_str(), f.severity))
            .collect()
    }

    #[test]
    fn ordinary_app_is_clean() {
        let report = scan_one(
            r#"
"""Reads a spreadsheet. Uses eval? No. import requests is just text here."""
import pandas as pd
from pathlib import Path
import re

def run(source: Path, ctx):
    # subprocess.run(["calc"])  -- a comment, not code
    df = pd.read_excel(source)
    df.eval("a + b")            # a method, not the builtin
    pattern = re.compile(r"\d+")
    return {"summary": f"{len(df)} rows"}
"#,
            &Permissions::default(),
        );
        assert_eq!(rules(&report), vec![]);
        assert_eq!(report.worst(), None);
    }

    #[test]
    fn network_must_be_declared() {
        let src = "import requests\nrequests.get('https://example.com')\n";
        let report = scan_one(src, &Permissions::default());
        assert_eq!(rules(&report), vec![("network", Severity::Warning)]);
        assert!(report.findings[0].message.contains("declares no network"));
        assert_eq!(report.findings[0].line, Some(1));

        let declared = Permissions {
            network: vec!["example.com".into()],
            ..Permissions::default()
        };
        assert_eq!(
            rules(&scan_one(src, &declared)),
            vec![("network", Severity::Info)]
        );
    }

    #[test]
    fn harmless_socket_use_is_not_network() {
        let benign = "import socket
host = socket.gethostname()
";
        assert_eq!(rules(&scan_one(benign, &Permissions::default())), vec![]);
        let from = "from socket import gethostname
host = gethostname()
";
        assert_eq!(rules(&scan_one(from, &Permissions::default())), vec![]);
        let real = "import socket
s = socket.create_connection((host, 443))
";
        let report = scan_one(real, &Permissions::default());
        assert_eq!(rules(&report), vec![("network", Severity::Warning)]);
        assert_eq!(report.findings[0].evidence, "socket.create_connection");
        assert_eq!(report.findings[0].line, Some(2));
        let class = "from socket import socket
";
        assert_eq!(
            rules(&scan_one(class, &Permissions::default())),
            vec![("network", Severity::Warning)]
        );
    }

    #[test]
    fn aliases_and_from_imports_are_resolved() {
        let report = scan_one(
            "import os as o\nfrom urllib import request as r\no.system('dir')\nr.urlopen(x)\n",
            &Permissions::default(),
        );
        let found = rules(&report);
        assert!(found.contains(&("processes", Severity::Info)), "{found:?}");
        assert!(found.contains(&("network", Severity::Warning)), "{found:?}");
        let processes = report
            .findings
            .iter()
            .find(|f| f.rule == "processes")
            .unwrap();
        assert_eq!(processes.evidence, "os.system(...)");
        assert_eq!(processes.line, Some(3));
    }

    #[test]
    fn decoding_and_running_in_one_statement_is_danger() {
        let report = scan_one(
            "import base64\nexec(base64.b64decode(PAYLOAD))\n",
            &Permissions::default(),
        );
        assert_eq!(report.worst(), Some(Severity::Danger));
        assert!(rules(&report).contains(&("decode-and-run", Severity::Danger)));
        assert!(rules(&report).contains(&("dynamic-code", Severity::Warning)));
    }

    #[test]
    fn eval_in_an_fstring_is_code() {
        let report = scan_one("x = f\"{eval(data)}\"\n", &Permissions::default());
        assert_eq!(rules(&report), vec![("dynamic-code", Severity::Warning)]);
    }

    #[test]
    fn own_function_named_like_a_builtin_is_not_flagged() {
        let report = scan_one(
            "def compile(rows):\n    return rows\n\ncompile([])\n",
            &Permissions::default(),
        );
        assert_eq!(rules(&report), vec![]);
    }

    #[test]
    fn dynamic_imports() {
        let literal = scan_one("m = __import__('socket')\n", &Permissions::default());
        assert_eq!(rules(&literal), vec![("network", Severity::Warning)]);
        let computed = scan_one(
            "import importlib\nm = importlib.import_module(name)\n",
            &Permissions::default(),
        );
        assert_eq!(
            rules(&computed),
            vec![("dynamic-import", Severity::Warning)]
        );
    }

    #[test]
    fn malware_strings_and_names() {
        let report = scan_one(
            r#"
import winreg
key = winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Software\\Microsoft\\Windows\\CurrentVersion\\Run")
db = Path.home() / "AppData/Local/Google/Chrome/User Data/Default/Login Data"
subprocess.run("powershell -ExecutionPolicy Bypass -EncodedCommand AAAA")
kernel32.WriteProcessMemory(h, addr, buf, n, None)
"#,
            &Permissions::default(),
        );
        let found = rules(&report);
        for rule in [
            "persistence",
            "credential-theft",
            "evasion",
            "process-injection",
        ] {
            assert!(
                found.contains(&(rule, Severity::Danger)),
                "{rule}: {found:?}"
            );
        }
        assert_eq!(report.findings[0].severity, Severity::Danger);
    }

    #[test]
    fn hidden_text_and_encoded_blobs() {
        let blob = "QUJD".repeat(80) + "123";
        let src = format!("s = \"{blob}\"\nok = 1 # \u{202E} reversed\n");
        let found = scan_one(&src, &Permissions::default());
        assert!(rules(&found).contains(&("encoded-blob", Severity::Warning)));
        assert!(rules(&found).contains(&("bidi-text", Severity::Danger)));
    }

    #[test]
    fn sap_must_be_declared() {
        let src = "import win32com.client\nsap = win32com.client.GetObject('SAPGUI')\n";
        assert_eq!(
            rules(&scan_one(src, &Permissions::default())),
            vec![("sap", Severity::Warning)]
        );
        let declared = Permissions {
            sap: true,
            ..Permissions::default()
        };
        assert_eq!(
            rules(&scan_one(src, &declared)),
            vec![("sap", Severity::Info)]
        );
    }

    #[test]
    fn files_that_cannot_be_read_are_flagged_by_name() {
        let mut scan = Scan::default();
        for rel in [
            "lib/helper.pyd",
            "tools/setup.bat",
            "cache.pyc",
            "main.py",
            "data.json",
        ] {
            scan.file(rel, || Some(b"x = 1\n".to_vec()));
        }
        let report = scan.finish(&Permissions::default());
        assert_eq!(report.files_scanned, 1);
        let mut found: Vec<_> = report.findings.iter().map(|f| f.rule.as_str()).collect();
        found.sort();
        assert_eq!(found, vec!["binary-file", "compiled-python", "script-file"]);
    }

    #[test]
    fn occurrences_are_counted_per_file() {
        let report = scan_one("eval(a)\neval(b)\n", &Permissions::default());
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].occurrences, 2);
        assert_eq!(report.findings[0].line, Some(1));
    }

    #[test]
    fn strings_and_comments_do_not_break_statements() {
        let src = "x = '''\nimport socket\n'''\ny = \"it's # not a comment\" ; import subprocess\n";
        assert_eq!(
            rules(&scan_one(src, &Permissions::default())),
            vec![("processes", Severity::Info)]
        );
    }
}
