//! The app manifest: `[project]` plus `[tool.devstribution]` in `pyproject.toml`.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::paths::validate_relative_path;
use crate::{Error, Result, SCHEMA_VERSION};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Form,
    Gui,
    Web,
    Cli,
    Background,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Form => "form",
            Kind::Gui => "gui",
            Kind::Web => "web",
            Kind::Cli => "cli",
            Kind::Background => "background",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputType {
    File,
    Folder,
    Text,
    Number,
    Bool,
    Choice,
    Date,
    Secret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputType {
    File,
    Table,
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: InputType,
    pub label: Option<String>,
    pub help: Option<String>,
    #[serde(default)]
    pub accept: Vec<String>,
    #[serde(default)]
    pub choices: Vec<String>,
    pub default: Option<toml::Value>,
    #[serde(default = "yes")]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: OutputType,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub url: String,
    pub sha256: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub sap: bool,
    #[serde(default)]
    pub secrets: Vec<String>,
}

/// The validated manifest of an app.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Manifest {
    /// `[project].name`
    pub name: String,
    /// `[project].version`
    pub version: Version,
    pub requires_python: Option<String>,
    pub schema_version: u32,
    pub id: AppId,
    pub title: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub kind: Kind,
    pub entry: Entry,
    pub platforms: Vec<String>,
    pub requires: Vec<String>,
    pub timeout_seconds: Option<u64>,
    pub resources: Vec<Resource>,
    pub inputs: Vec<Input>,
    pub outputs: Vec<Output>,
    pub permissions: Permissions,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct RawPyproject {
    project: Option<RawProject>,
    tool: Option<RawTool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawProject {
    name: Option<String>,
    version: Option<String>,
    requires_python: Option<String>,
    dynamic: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct RawTool {
    uv: Option<RawUv>,
    devstribution: Option<RawDevstribution>,
}

#[derive(Deserialize)]
struct RawUv {
    package: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDevstribution {
    schema_version: u32,
    id: String,
    title: String,
    description: Option<String>,
    icon: Option<String>,
    #[serde(default = "default_kind")]
    kind: Kind,
    entry: String,
    #[serde(default = "default_platforms")]
    platforms: Vec<String>,
    #[serde(default)]
    requires: Vec<String>,
    timeout_seconds: Option<u64>,
    #[serde(default)]
    resources: Vec<Resource>,
    #[serde(default)]
    inputs: Vec<Input>,
    #[serde(default)]
    outputs: Vec<Output>,
    #[serde(default)]
    permissions: Permissions,
}

fn default_kind() -> Kind {
    Kind::Form
}

fn default_platforms() -> Vec<String> {
    vec!["windows".into()]
}

const PLATFORMS: [&str; 3] = ["windows", "macos", "linux"];

/// Reads only `schema_version`, ignoring everything else, so that a package in a later format
/// (which may have fields this version does not know) is reported as such, not as a bad manifest.
#[derive(Deserialize)]
struct SchemaProbe {
    tool: Option<SchemaProbeTool>,
}

#[derive(Deserialize)]
struct SchemaProbeTool {
    devstribution: Option<SchemaProbeDs>,
}

#[derive(Deserialize)]
struct SchemaProbeDs {
    schema_version: Option<toml::Value>,
}

impl Manifest {
    pub fn parse(pyproject: &str) -> Result<Manifest> {
        let bad = |msg: String| Error::Manifest(msg);
        if let Ok(probe) = toml::from_str::<SchemaProbe>(pyproject) {
            let found = probe
                .tool
                .and_then(|t| t.devstribution)
                .and_then(|d| d.schema_version)
                .and_then(|v| v.as_integer());
            if let Some(found) = found.filter(|&v| v > i64::from(SCHEMA_VERSION)) {
                return Err(Error::NewerFormat {
                    found: u32::try_from(found).unwrap_or(u32::MAX),
                    supported: SCHEMA_VERSION,
                });
            }
        }
        let raw: RawPyproject = toml::from_str(pyproject).map_err(|e| bad(e.to_string()))?;

        let project = raw
            .project
            .ok_or_else(|| bad("missing the [project] table".into()))?;
        if let Some(dynamic) = &project.dynamic {
            if !dynamic.is_empty() {
                return Err(bad(format!(
                    "[project].dynamic = {dynamic:?}: dynamic metadata needs a build step; \
                     write the values directly"
                )));
            }
        }
        let name = project
            .name
            .ok_or_else(|| bad("missing [project].name".into()))?;
        let version: Version = project
            .version
            .ok_or_else(|| bad("missing [project].version".into()))?
            .parse()
            .map_err(|e| bad(format!("[project].version: {e}")))?;

        let tool = raw
            .tool
            .ok_or_else(|| bad("missing [tool.devstribution]".into()))?;
        if tool.uv.and_then(|uv| uv.package) != Some(false) {
            return Err(bad(
                "add `[tool.uv]` with `package = false`: apps are not built or installed as \
                 packages, so there is no build step"
                    .into(),
            ));
        }
        let ds = tool
            .devstribution
            .ok_or_else(|| bad("missing [tool.devstribution]".into()))?;
        if ds.schema_version != SCHEMA_VERSION {
            return Err(bad(format!(
                "[tool.devstribution].schema_version = {}: this version of devstribution \
                 supports only {SCHEMA_VERSION}",
                ds.schema_version
            )));
        }

        let id: AppId = ds
            .id
            .parse()
            .map_err(|e| bad(format!("[tool.devstribution].id: {e}")))?;
        if ds.title.trim().is_empty() {
            return Err(bad("[tool.devstribution].title is empty".into()));
        }
        let entry: Entry = ds
            .entry
            .parse()
            .map_err(|e| bad(format!("[tool.devstribution].entry: {e}")))?;
        if ds.kind == Kind::Form && entry.function.is_none() {
            return Err(bad(format!(
                "[tool.devstribution].entry = \"{}\": form apps need `module:function`, \
                 e.g. \"main:run\"",
                ds.entry
            )));
        }
        if let Some(icon) = &ds.icon {
            validate_relative_path(icon)
                .map_err(|e| bad(format!("[tool.devstribution].icon: {e}")))?;
        }
        if ds.platforms.is_empty() {
            return Err(bad("[tool.devstribution].platforms is empty".into()));
        }
        for p in &ds.platforms {
            if !PLATFORMS.contains(&p.as_str()) {
                return Err(bad(format!(
                    "[tool.devstribution].platforms: unknown platform `{p}` (use {PLATFORMS:?})"
                )));
            }
        }
        if ds.timeout_seconds == Some(0) {
            return Err(bad(
                "[tool.devstribution].timeout_seconds must be > 0".into()
            ));
        }
        for r in &ds.resources {
            validate_resource(r).map_err(bad)?;
        }
        if ds.kind != Kind::Form && (!ds.inputs.is_empty() || !ds.outputs.is_empty()) {
            return Err(bad(format!(
                "inputs and outputs are only used by kind = \"form\" (this app is \"{}\")",
                ds.kind.as_str()
            )));
        }
        validate_inputs(&ds.inputs).map_err(bad)?;
        validate_outputs(&ds.outputs).map_err(bad)?;

        Ok(Manifest {
            name,
            version,
            requires_python: project.requires_python,
            schema_version: ds.schema_version,
            id,
            title: ds.title,
            description: ds.description,
            icon: ds.icon,
            kind: ds.kind,
            entry,
            platforms: ds.platforms,
            requires: ds.requires,
            timeout_seconds: ds.timeout_seconds,
            resources: ds.resources,
            inputs: ds.inputs,
            outputs: ds.outputs,
            permissions: ds.permissions,
        })
    }
}

fn validate_resource(r: &Resource) -> std::result::Result<(), String> {
    if !r.url.starts_with("https://") {
        return Err(format!(
            "resource `{}`: only https:// URLs are allowed",
            r.url
        ));
    }
    if r.sha256.len() != 64
        || !r
            .sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(format!(
            "resource `{}`: sha256 must be 64 lowercase hex digits",
            r.url
        ));
    }
    validate_relative_path(&r.path).map_err(|e| format!("resource `{}`: path {e}", r.url))
}

fn validate_inputs(inputs: &[Input]) -> std::result::Result<(), String> {
    let mut seen = HashSet::new();
    for input in inputs {
        if !is_identifier(&input.name) {
            return Err(format!(
                "input `{}`: name must be a Python identifier",
                input.name
            ));
        }
        if input.name == "ctx" {
            return Err("input `ctx`: the name is reserved for the run context".into());
        }
        if !seen.insert(input.name.as_str()) {
            return Err(format!("input `{}` is declared twice", input.name));
        }
        if input.kind == InputType::Choice && input.choices.is_empty() {
            return Err(format!(
                "input `{}`: type = \"choice\" needs `choices`",
                input.name
            ));
        }
        if input.kind != InputType::Choice && !input.choices.is_empty() {
            return Err(format!(
                "input `{}`: `choices` is only for type = \"choice\"",
                input.name
            ));
        }
        if input.kind != InputType::File && !input.accept.is_empty() {
            return Err(format!(
                "input `{}`: `accept` is only for type = \"file\"",
                input.name
            ));
        }
        if input.kind == InputType::Secret && input.default.is_some() {
            return Err(format!(
                "input `{}`: secrets cannot have a default",
                input.name
            ));
        }
        if let Some(default) = &input.default {
            let ok = match input.kind {
                InputType::Number => default.is_integer() || default.is_float(),
                InputType::Bool => default.is_bool(),
                InputType::Choice => default
                    .as_str()
                    .is_some_and(|d| input.choices.iter().any(|c| c == d)),
                _ => default.is_str(),
            };
            if !ok {
                return Err(format!(
                    "input `{}`: default does not match its type",
                    input.name
                ));
            }
        }
    }
    Ok(())
}

fn validate_outputs(outputs: &[Output]) -> std::result::Result<(), String> {
    let mut seen = HashSet::new();
    for output in outputs {
        if !is_identifier(&output.name) {
            return Err(format!(
                "output `{}`: name must be a Python identifier",
                output.name
            ));
        }
        if !seen.insert(output.name.as_str()) {
            return Err(format!("output `{}` is declared twice", output.name));
        }
    }
    Ok(())
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// `<namespace>/<app>`, lowercase letters, digits and `-`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(into = "String")]
pub struct AppId {
    pub namespace: String,
    pub app: String,
}

impl FromStr for AppId {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let (namespace, app) = s
            .split_once('/')
            .ok_or_else(|| format!("`{s}` must be `<namespace>/<app>`, e.g. \"author/my-app\""))?;
        for (part, max) in [(namespace, 39), (app, 64)] {
            let valid = !part.is_empty()
                && part.len() <= max
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part
                    .bytes()
                    .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'));
            if !valid {
                return Err(format!(
                    "`{s}`: `{part}` must be 1-{max} lowercase letters, digits or `-`, \
                     not starting or ending with `-`"
                ));
            }
        }
        Ok(AppId {
            namespace: namespace.into(),
            app: app.into(),
        })
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.namespace, self.app)
    }
}

impl From<AppId> for String {
    fn from(id: AppId) -> String {
        id.to_string()
    }
}

/// `module` or `module:function`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(into = "String")]
pub struct Entry {
    pub module: String,
    pub function: Option<String>,
}

impl FromStr for Entry {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let (module, function) = match s.split_once(':') {
            Some((m, f)) => (m, Some(f)),
            None => (s, None),
        };
        if module.is_empty() || !module.split('.').all(is_identifier) {
            return Err(format!("`{s}`: `{module}` is not a module name"));
        }
        if let Some(f) = function {
            if !is_identifier(f) {
                return Err(format!("`{s}`: `{f}` is not a function name"));
            }
        }
        Ok(Entry {
            module: module.into(),
            function: function.map(Into::into),
        })
    }
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.function {
            Some(func) => write!(f, "{}:{func}", self.module),
            None => f.write_str(&self.module),
        }
    }
}

impl From<Entry> for String {
    fn from(e: Entry) -> String {
        e.to_string()
    }
}

/// App version: 1 to 4 dot-separated numbers (`1.2.0`). Kept this simple in v1 so that
/// anti-downgrade checks have one unambiguous ordering.
#[derive(Debug, Clone, Serialize)]
#[serde(into = "String")]
pub struct Version {
    parts: Vec<u64>,
    text: String,
}

impl FromStr for Version {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let parts: Option<Vec<u64>> = s
            .split('.')
            .map(|p| {
                let canonical = !p.is_empty() && (p == "0" || !p.starts_with('0'));
                canonical.then(|| p.parse().ok()).flatten()
            })
            .collect();
        match parts {
            Some(parts) if (1..=4).contains(&parts.len()) => Ok(Version {
                parts,
                text: s.into(),
            }),
            _ => Err(format!(
                "`{s}` is not supported: use 1 to 4 numbers separated by dots, e.g. \"1.2.0\""
            )),
        }
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let n = self.parts.len().max(other.parts.len());
        let get = |v: &Version, i| v.parts.get(i).copied().unwrap_or(0);
        (0..n)
            .map(|i| get(self, i).cmp(&get(other, i)))
            .find(|o| o.is_ne())
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl From<Version> for String {
    fn from(v: Version) -> String {
        v.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const EXAMPLE: &str = r#"
[project]
name = "query-orders"
version = "1.2.0"
requires-python = ">=3.11"
dependencies = ["openpyxl>=3.1"]

[tool.uv]
package = false

[tool.devstribution]
schema_version = 1
id = "author/query-orders"
title = "Query orders"
entry = "main:run"

[[tool.devstribution.inputs]]
name = "excel_file"
type = "file"
accept = [".xlsx"]

[[tool.devstribution.inputs]]
name = "mode"
type = "choice"
choices = ["fast", "full"]
default = "fast"

[[tool.devstribution.outputs]]
name = "report"
type = "file"
"#;

    #[test]
    fn parses_the_example() {
        let m = Manifest::parse(EXAMPLE).unwrap();
        assert_eq!(m.id.to_string(), "author/query-orders");
        assert_eq!(m.kind, Kind::Form);
        assert_eq!(m.entry.function.as_deref(), Some("run"));
        assert_eq!(m.platforms, ["windows"]);
        assert_eq!(m.inputs.len(), 2);
        assert!(m.inputs[0].required);
    }

    fn with(from: &str, to: &str) -> String {
        assert!(EXAMPLE.contains(from), "{from}");
        EXAMPLE.replacen(from, to, 1)
    }

    #[test]
    fn later_formats_ask_for_a_newer_launcher() {
        // A format-2 manifest may carry fields format 1 doesn't know: the version still wins.
        let future = with("schema_version = 1", "schema_version = 2\nnew_field = true");
        match Manifest::parse(&future) {
            Err(Error::NewerFormat {
                found: 2,
                supported: 1,
            }) => {}
            other => panic!("expected NewerFormat, got {other:?}"),
        }
        let err = Manifest::parse(&future).unwrap_err().to_string();
        assert!(
            err.contains("needs a newer version of devstribution"),
            "{err}"
        );

        // Older or nonsense numbers stay ordinary manifest errors.
        assert!(matches!(
            Manifest::parse(&with("schema_version = 1", "schema_version = 0")),
            Err(Error::Manifest(_))
        ));
    }

    #[test]
    fn refuses_bad_manifests() {
        let cases = [
            with("package = false", "package = true"),
            with("schema_version = 1", "schema_version = 2"),
            with("author/query-orders", "Author/Query"),
            with("\"main:run\"", "\"main\""),
            with("version = \"1.2.0\"", "version = \"1.2.0rc1\""),
            with(
                "title = \"Query orders\"",
                "title = \"Query orders\"\nkinds = \"gui\"",
            ),
            with("name = \"mode\"", "name = \"excel_file\""),
            with("default = \"fast\"", "default = \"slow\""),
            with("name = \"mode\"", "name = \"ctx\""),
            with(
                "entry = \"main:run\"",
                "entry = \"main:run\"\nkind = \"gui\"",
            ),
        ];
        for case in cases {
            assert!(Manifest::parse(&case).is_err(), "{case}");
        }
    }

    #[test]
    fn version_ordering() {
        let v = |s: &str| s.parse::<Version>().unwrap();
        assert!(v("1.10.0") > v("1.9.3"));
        assert_eq!(v("1.2"), v("1.2.0"));
        assert!(v("2") > v("1.99.99.99"));
        for bad in ["", "1.", "01.2", "1.2.3.4.5", "1.2-beta", "v1"] {
            assert!(bad.parse::<Version>().is_err(), "{bad}");
        }
    }
}
