//! Validation of the relative paths stored in packages and manifests.
//!
//! The rules are stricter than any single filesystem needs, so that a package that extracts
//! cleanly on Linux also extracts identically on Windows.

/// Checks a package-relative path such as `src/app/main.py`.
///
/// Refused: empty paths, absolute paths, `\`, `:` (drive letters, NTFS streams), `.` and `..`
/// components, empty components, control characters, the characters Windows forbids in names
/// (`<>"|?*`), invisible Unicode formatting characters (right-to-left overrides, zero-width
/// spaces: they make a name look like another), Windows reserved device names, 8.3 short-name
/// aliases (`FOO~1`) and components ending with a dot or a space (Windows silently strips them).
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
    if let Some(c) = path
        .chars()
        .find(|c| matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return Err(format!(
            "`{path}` contains `{c}`, which Windows does not allow in names"
        ));
    }
    if path.chars().any(is_invisible_format_char) {
        return Err(format!(
            "`{}` contains an invisible formatting character",
            path.escape_default()
        ));
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
        if is_short_name_alias(component) {
            return Err(format!(
                "`{path}`: `{component}` looks like a Windows short name (`~` and a digit), which \
                 can stand for another file"
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
        .trim_end_matches(' ')
        .to_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    // COM0-COM9, LPT0-LPT9, and the superscript digits Windows also treats as devices.
    let mut chars = stem.chars();
    let prefix: String = chars.by_ref().take(3).collect();
    let rest: Vec<char> = chars.collect();
    (prefix == "COM" || prefix == "LPT")
        && rest.len() == 1
        && (rest[0].is_ascii_digit() || matches!(rest[0], '¹' | '²' | '³'))
}

fn is_short_name_alias(component: &str) -> bool {
    let b = component.as_bytes();
    b.windows(2).any(|w| w[0] == b'~' && w[1].is_ascii_digit())
}

/// Unicode format characters that change how a name displays without being visible:
/// bidirectional overrides and isolates, zero-width spaces and joiners, the byte-order mark.
fn is_invisible_format_char(c: char) -> bool {
    matches!(c as u32,
        0x00AD | 0x061C | 0x180E | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F
        | 0xFEFF | 0xFFF9..=0xFFFB)
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
            "COM\u{b9}",
            "lpt\u{b2}.txt",
            "CONIN$",
            "conout$.log",
            "a<b",
            "q?.py",
            "star*.py",
            "pipe|.py",
            "quote\".py",
            "evil\u{202e}yp.exe",
            "zero\u{200b}width.py",
            "PROGRA~1/x.py",
            "main~2.py",
        ] {
            assert!(check(p).is_err(), "{p:?} should be refused");
        }
    }
}
