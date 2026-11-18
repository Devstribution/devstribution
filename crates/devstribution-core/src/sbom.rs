//! A software bill of materials for an app: what its `uv.lock` installs, as CycloneDX 1.6 JSON.
//!
//! Every package is named by its PyPI package URL (`pkg:pypi/name@version`), with the wheels
//! the lockfile allows and their SHA-256, and the dependency graph between them. Nothing is
//! downloaded: the lockfile already holds all of it. The app itself is the BOM's subject, with
//! its publisher's namespace, the key that signed it and the hash of its signed content.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::deps::normalize;

/// The installed app a bill of materials is about.
pub struct Subject<'a> {
    /// `namespace/app`.
    pub id: &'a str,
    pub title: &'a str,
    pub version: &'a str,
    pub namespace: &'a str,
    /// The publisher key that signed this version.
    pub key_id: &'a str,
    /// SHA-256 of the signed content (empty for records from before it was kept).
    pub content_sha256: &'a str,
}

/// The program that wrote the BOM, as CycloneDX's `metadata.tools` names it.
pub struct Tool<'a> {
    pub name: &'a str,
    pub version: &'a str,
}

#[derive(Deserialize)]
struct Lock {
    #[serde(default)]
    package: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    version: Option<String>,
    #[serde(default)]
    source: HashMap<String, toml::Value>,
    #[serde(default)]
    dependencies: Vec<Dep>,
    #[serde(default)]
    wheels: Vec<Wheel>,
}

#[derive(Deserialize)]
struct Dep {
    name: String,
}

#[derive(Deserialize)]
struct Wheel {
    url: Option<String>,
    hash: Option<String>,
}

/// The app's own entry in `uv.lock` is a virtual or editable package: it is the subject, not a
/// component.
fn is_app(p: &Package) -> bool {
    p.source.contains_key("virtual") || p.source.contains_key("editable")
}

fn purl(name: &str, version: &str) -> String {
    if version.is_empty() {
        format!("pkg:pypi/{name}")
    } else {
        format!("pkg:pypi/{name}@{version}")
    }
}

/// `sha256:…` from `uv.lock` as a CycloneDX hash; other algorithms are left out.
fn sha256(hash: &str) -> Option<Value> {
    let hex = hash.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| json!({ "alg": "SHA-256", "content": hex.to_ascii_lowercase() }))
}

/// A UUID-shaped serial number derived from what the BOM describes, so the same app version
/// and lockfile always give the same serial (CycloneDX asks for a UUID; this is a version-8
/// layout over SHA-256).
fn serial(subject: &Subject, lock: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for part in [subject.id, subject.version, subject.content_sha256, lock] {
        h.update(part.as_bytes());
        h.update([0]);
    }
    let mut b: [u8; 16] = h.finalize()[..16].try_into().expect("16 bytes");
    b[6] = (b[6] & 0x0f) | 0x80;
    b[8] = (b[8] & 0x3f) | 0x80;
    let x = hex::encode(b);
    format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &x[..8],
        &x[8..12],
        &x[12..16],
        &x[16..20],
        &x[20..]
    )
}

/// The CycloneDX 1.6 document for `subject`, from its `uv.lock`. `timestamp` is RFC 3339 (UTC).
/// A lockfile that does not parse gives a BOM with no components, which says so in a property.
pub fn app_bom(subject: &Subject, lock: &str, tool: &Tool, timestamp: &str) -> Value {
    let parsed = toml::from_str::<Lock>(lock);
    let packages = parsed.as_ref().map(|l| l.package.as_slice()).unwrap_or(&[]);
    let app_ref = format!("app:{}@{}", subject.id, subject.version);

    // Name to bom-ref, to resolve the lockfile's dependencies (which name packages only).
    let refs: BTreeMap<String, String> = packages
        .iter()
        .filter(|p| !is_app(p))
        .map(|p| {
            let name = normalize(&p.name);
            let r = purl(&name, p.version.as_deref().unwrap_or(""));
            (name, r)
        })
        .collect();
    let depends = |p: &Package| -> Vec<String> {
        let mut out: Vec<String> = p
            .dependencies
            .iter()
            .filter_map(|d| refs.get(&normalize(&d.name)).cloned())
            .collect();
        out.sort();
        out.dedup();
        out
    };

    let mut components = Vec::new();
    let mut dependencies = Vec::new();
    for p in packages {
        if is_app(p) {
            dependencies.push(json!({ "ref": app_ref, "dependsOn": depends(p) }));
            continue;
        }
        let name = normalize(&p.name);
        let version = p.version.clone().unwrap_or_default();
        let r = purl(&name, &version);
        let wheels: Vec<Value> = p
            .wheels
            .iter()
            .filter_map(|w| {
                let url = w.url.as_ref()?;
                let mut e = json!({ "type": "distribution", "url": url });
                if let Some(h) = w.hash.as_deref().and_then(sha256) {
                    e["hashes"] = json!([h]);
                }
                Some(e)
            })
            .collect();
        let mut c = json!({
            "type": "library",
            "bom-ref": r,
            "name": name,
            "version": version,
            "purl": r,
        });
        if !wheels.is_empty() {
            c["externalReferences"] = Value::Array(wheels);
        }
        components.push(c);
        dependencies.push(json!({ "ref": r, "dependsOn": depends(p) }));
    }
    components.sort_by(|a, b| a["bom-ref"].as_str().cmp(&b["bom-ref"].as_str()));
    dependencies.sort_by(|a, b| a["ref"].as_str().cmp(&b["ref"].as_str()));
    if !dependencies.iter().any(|d| d["ref"] == app_ref.as_str()) {
        dependencies.insert(0, json!({ "ref": app_ref, "dependsOn": [] }));
    }

    let mut app = json!({
        "type": "application",
        "bom-ref": app_ref,
        "name": subject.id,
        "version": subject.version,
        "description": subject.title,
        "supplier": { "name": subject.namespace },
        "properties": [
            { "name": "devstribution:publisher-key", "value": subject.key_id },
        ],
    });
    if let Some(h) = sha256(&format!("sha256:{}", subject.content_sha256)) {
        app["hashes"] = json!([h]);
    }
    let mut properties = vec![json!({ "name": "devstribution:source", "value": "uv.lock" })];
    if parsed.is_err() {
        properties.push(json!({ "name": "devstribution:lockfile", "value": "could not be read" }));
    }
    json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.6",
        "serialNumber": serial(subject, lock),
        "version": 1,
        "metadata": {
            "timestamp": timestamp,
            "tools": { "components": [{ "type": "application", "name": tool.name, "version": tool.version }] },
            "component": app,
            "properties": properties,
        },
        "components": components,
        "dependencies": dependencies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK: &str = r#"version = 1
requires-python = ">=3.12"

[[package]]
name = "et-xmlfile"
version = "2.0.0"
source = { registry = "https://pypi.org/simple" }
wheels = [
    { url = "https://files.pythonhosted.org/et_xmlfile-2.0.0-py3-none-any.whl", hash = "sha256:7A91720BC756843502C3B7504C77B8FE44217C85C537D85037F0F536151B2CAA", size = 18059 },
]

[[package]]
name = "my-tool"
version = "0.1.0"
source = { virtual = "." }
dependencies = [{ name = "openpyxl" }]

[[package]]
name = "OpenPyXL"
version = "3.1.5"
source = { registry = "https://pypi.org/simple" }
dependencies = [{ name = "et_xmlfile" }]
wheels = [
    { url = "https://files.pythonhosted.org/openpyxl-3.1.5-py2.py3-none-any.whl", hash = "sha256:5282c12b107bffeef825f4617dc029afaf41d0ea60823bbb665ef3079dc79de2", size = 250910 },
]
"#;

    fn subject() -> Subject<'static> {
        Subject {
            id: "acme/my-tool",
            title: "My tool",
            version: "0.1.0",
            namespace: "acme",
            key_id: "ABCDEF0123456789",
            content_sha256: "0000000000000000000000000000000000000000000000000000000000000001",
        }
    }

    const TOOL: Tool = Tool {
        name: "devstribution",
        version: "0.7.0",
    };

    #[test]
    fn lists_packages_hashes_and_graph() {
        let bom = app_bom(&subject(), LOCK, &TOOL, "2026-11-18T10:00:00Z");
        assert_eq!(bom["bomFormat"], "CycloneDX");
        assert_eq!(bom["specVersion"], "1.6");
        let comps = bom["components"].as_array().unwrap();
        let purls: Vec<_> = comps.iter().map(|c| c["purl"].as_str().unwrap()).collect();
        // The app is the subject, not a component; names are normalized.
        assert_eq!(
            purls,
            ["pkg:pypi/et-xmlfile@2.0.0", "pkg:pypi/openpyxl@3.1.5"]
        );
        let hash = &comps[0]["externalReferences"][0]["hashes"][0];
        assert_eq!(hash["alg"], "SHA-256");
        assert_eq!(
            hash["content"],
            "7a91720bc756843502c3b7504c77b8fe44217c85c537d85037f0f536151b2caa"
        );

        let deps = bom["dependencies"].as_array().unwrap();
        let of = |r: &str| deps.iter().find(|d| d["ref"] == r).unwrap()["dependsOn"].clone();
        assert_eq!(
            of("app:acme/my-tool@0.1.0"),
            json!(["pkg:pypi/openpyxl@3.1.5"])
        );
        assert_eq!(
            of("pkg:pypi/openpyxl@3.1.5"),
            json!(["pkg:pypi/et-xmlfile@2.0.0"])
        );

        let app = &bom["metadata"]["component"];
        assert_eq!(app["supplier"]["name"], "acme");
        assert_eq!(app["properties"][0]["value"], "ABCDEF0123456789");
        assert_eq!(app["hashes"][0]["alg"], "SHA-256");
    }

    #[test]
    fn serial_is_stable_and_uuid_shaped() {
        let a = app_bom(&subject(), LOCK, &TOOL, "2026-11-18T10:00:00Z");
        let b = app_bom(&subject(), LOCK, &TOOL, "2027-01-01T00:00:00Z");
        assert_eq!(a["serialNumber"], b["serialNumber"]);
        let s = a["serialNumber"].as_str().unwrap();
        let uuid = s.strip_prefix("urn:uuid:").unwrap();
        let groups: Vec<usize> = uuid.split('-').map(str::len).collect();
        assert_eq!(groups, [8, 4, 4, 4, 12]);
        assert_eq!(&uuid[14..15], "8");
    }

    #[test]
    fn unreadable_lock_gives_an_empty_bom_that_says_so() {
        let mut s = subject();
        s.content_sha256 = "";
        let bom = app_bom(&s, "not toml [[", &TOOL, "2026-11-18T10:00:00Z");
        assert_eq!(bom["components"], json!([]));
        assert!(bom["metadata"]["component"].get("hashes").is_none());
        assert_eq!(
            bom["metadata"]["properties"][1]["value"],
            "could not be read"
        );
        assert_eq!(bom["dependencies"][0]["ref"], "app:acme/my-tool@0.1.0");
    }
}
