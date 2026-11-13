//! Installing an app's packages from a company mirror instead of PyPI, with real uv and Python.
//! Needs `uv` on PATH (or DEVSTRIBUTION_UV).

use std::fs;
use std::io::{Cursor, Read, Write};
use std::net::TcpListener;
use std::path::Path;

use devstribution_host::uv::{self, LockMode, Sources};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const WHEEL: &str = "mirror_demo-1.0-py3-none-any.whl";

/// A wheel of `mirror_demo` 1.0 whose module holds `value`.
fn wheel(value: &str) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let files = [
        ("mirror_demo/__init__.py", format!("VALUE = {value:?}\n")),
        (
            "mirror_demo-1.0.dist-info/METADATA",
            "Metadata-Version: 2.1\nName: mirror-demo\nVersion: 1.0\n".into(),
        ),
        (
            "mirror_demo-1.0.dist-info/WHEEL",
            "Wheel-Version: 1.0\nGenerator: test\nRoot-Is-Purelib: true\nTag: py3-none-any\n"
                .into(),
        ),
        (
            "mirror_demo-1.0.dist-info/RECORD",
            "mirror_demo/__init__.py,,\nmirror_demo-1.0.dist-info/METADATA,,\n\
             mirror_demo-1.0.dist-info/WHEEL,,\nmirror_demo-1.0.dist-info/RECORD,,\n"
                .into(),
        ),
    ];
    for (name, text) in files {
        zip.start_file(name, options).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// An app that depends on `mirror_demo`, locked against PyPI with `wheel`'s hash.
fn write_app(dir: &Path, wheel: &[u8]) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("pyproject.toml"),
        r#"[project]
name = "hello"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = ["mirror-demo==1.0"]

[tool.uv]
package = false

[tool.devstribution]
schema_version = 1
id = "alice/hello"
title = "Hello"
entry = "main:run"
platforms = ["windows", "macos", "linux"]
"#,
    )
    .unwrap();
    let hash = hex::encode(Sha256::digest(wheel));
    fs::write(
        dir.join("uv.lock"),
        format!(
            r#"version = 1
revision = 3
requires-python = ">=3.11"

[[package]]
name = "hello"
version = "0.1.0"
source = {{ virtual = "." }}
dependencies = [
    {{ name = "mirror-demo" }},
]

[package.metadata]
requires-dist = [{{ name = "mirror-demo", specifier = "==1.0" }}]

[[package]]
name = "mirror-demo"
version = "1.0"
source = {{ registry = "https://pypi.org/simple" }}
wheels = [
    {{ url = "https://files.pythonhosted.org/packages/00/00/{WHEEL}", hash = "sha256:{hash}", size = {} }},
]
"#,
            wheel.len()
        ),
    )
    .unwrap();
}

/// A PyPI-style simple index on 127.0.0.1 that serves `wheel` as `mirror_demo` 1.0; returns its
/// address.
fn serve(wheel: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&buf[..n]),
                }
            }
            let request = String::from_utf8_lossy(&request);
            let path = request.split_whitespace().nth(1).unwrap_or_default();
            let (status, kind, body) = if path.starts_with("/simple/mirror-demo") {
                let page =
                    format!("<html><body><a href=\"/files/{WHEEL}\">{WHEEL}</a></body></html>");
                ("200 OK", "text/html", page.into_bytes())
            } else if path == format!("/files/{WHEEL}") {
                ("200 OK", "application/octet-stream", wheel.clone())
            } else {
                ("404 Not Found", "text/plain", b"not found".to_vec())
            };
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{port}/simple")
}

fn install(app: &Path, env: &Path, index: String) -> anyhow::Result<()> {
    let sources = Sources {
        package_index: Some(index),
        ..Default::default()
    };
    uv::sync(app, env, LockMode::Frozen, &sources, &mut |_| {}, &|| false)
}

/// The signed lockfile names PyPI, but with a mirror set every package comes from the mirror,
/// and a file whose hash differs from the lockfile's is refused.
#[test]
fn packages_come_from_the_mirror_with_the_locked_hashes() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app");
    let good = wheel("from the mirror");
    write_app(&app, &good);

    let env = tmp.path().join("env");
    install(&app, &env, serve(good)).unwrap();
    assert!(env.join(uv::MIRROR_REQUIREMENTS).is_file());
    let out = std::process::Command::new(uv::python_in(&env, false))
        .args(["-c", "import mirror_demo; print(mirror_demo.VALUE)"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "from the mirror"
    );

    // The same name and version, other bytes: the mirror cannot change what is installed.
    let env = tmp.path().join("env-tampered");
    let err = install(&app, &env, serve(wheel("tampered"))).unwrap_err();
    let shown = format!("{err:#}");
    assert!(shown.to_lowercase().contains("hash"), "{shown}");
}
