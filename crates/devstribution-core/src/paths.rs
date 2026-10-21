//! Validation of the relative paths stored in packages and manifests.
//!
//! The rules are stricter than any single filesystem needs, so that a package that extracts
//! cleanly on Linux also extracts identically on Windows.

/// Checks a package-relative path such as `src/app/main.py`.
///
/// Refused: empty paths, absolute paths, `\`, `:` (drive letters, NTFS streams), `.` and `..`
/// components, empty components, control characters, Windows reserved device names and
/// components ending with a dot or a space (Windows silently strips them).
pub fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("empty path".into());
    }
    if path.starts_with('/') {
        return Err(format!("`{path}` is an absolute path"));
    }
    if path.contains('\\') {
        return Err(format!("`{path}` contains `\\`; use `/` as separator"));
    }
    if path.contains(':') {
        return Err(format!("`{path}` contains `:`"));
    }
    if path.chars().any(|c| c.is_control()) {
        return Err(format!("`{path}` contains a control character"));
    }
    for component in path.split('/') {
        match component {
            "" => return Err(format!("`{path}` has an empty path component")),
            "." | ".." => return Err(format!("`{path}` contains `{component}`")),
            _ => {}
        }
        if component.ends_with('.') || component.ends_with(' ') {
            return Err(format!(
                "`{path}`: `{component}` ends with a dot or a space"
            ));
        }
        if is_reserved_windows_name(component) {
            return Err(format!(
                "`{path}`: `{component}` is a reserved name on Windows"
            ));
        }
    }
    Ok(())
}

fn is_reserved_windows_name(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::validate_relative_path as check;

    #[test]
    fn accepts_normal_paths() {
        for p in [
            "main.py",
            "pkg/mod.py",
            "assets/icon.png",
            ".gitignore",
            "a b/c.txt",
        ] {
            assert!(check(p).is_ok(), "{p}");
        }
    }

    #[test]
    fn refuses_dangerous_paths() {
        for p in [
            "",
            "/etc/passwd",
            "../x",
            "a/../b",
            "./a",
            "a//b",
            "a\\b",
            "C:/x",
            "a:stream",
            "con",
            "NUL.txt",
            "dir/COM1",
            "lpt9.py",
            "trailing.",
            "space ",
            "a/\u{0}b",
            "a/",
        ] {
            assert!(check(p).is_err(), "{p:?} should be refused");
        }
    }
}
