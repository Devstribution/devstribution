//! Evidence for IT in the Windows Event Log: installs, runs, what the runtime guard did, and
//! quarantines, in the Application log under the source `devstribution`, as the user who ran
//! the launcher.
//!
//! Each event has a fixed id (below, and in `docs/it.md`) and one text: what happened, the app,
//! its version and publisher key, the run. Never what a run was given (inputs, secrets, the
//! files the user chose) or what an app's error said: an event says what devstribution did, not
//! what the user worked on.
//!
//! Off until [`init`] is called (the launcher does; tests and the CLI do not), and a no-op away
//! from Windows. An event that cannot be written is not an error worth another one.
//!
//! Registering the source needs an administrator, which the per-user install is not. Without
//! it the events are written all the same, and Event Viewer puts "The description for Event ID
//! … cannot be found" above the text; IT registers it once per machine (`docs/it.md`).

/// The source name, in the Application log.
pub const SOURCE: &str = "devstribution";

/// Event ids. Kept forever once released: IT filters and alerts on them.
pub mod id {
    pub const INSTALLED: u32 = 100;
    pub const UPDATED: u32 = 101;
    pub const UNINSTALLED: u32 = 102;
    pub const REFUSED: u32 = 110;
    pub const RUN_OK: u32 = 200;
    pub const RUN_FAILED: u32 = 201;
    pub const RUN_CANCELLED: u32 = 202;
    pub const GUARD_STOP: u32 = 300;
    pub const GUARD_BLOCK: u32 = 301;
    pub const GUARD_REPORT: u32 = 302;
    pub const QUARANTINE_RELEASED: u32 = 310;
    pub const QUARANTINE_ALLOWED: u32 = 311;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

/// The longest text an event gets: Event Viewer shows more, but a line of evidence does not
/// need it, and an app's guard detail could be long.
const MAX_TEXT: usize = 4000;

/// One event's text: a sentence, then `Name: value` lines (empty values left out).
pub fn text(summary: &str, fields: &[(&str, &str)]) -> String {
    let mut out = summary.trim().to_string();
    for (name, value) in fields {
        let value = value.trim();
        if !value.is_empty() {
            out.push_str("\r\n");
            out.push_str(name);
            out.push_str(": ");
            out.push_str(value);
        }
    }
    if out.len() > MAX_TEXT {
        let mut end = MAX_TEXT;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
        out.push('…');
    }
    out
}

#[cfg(windows)]
mod imp {
    use super::Level;
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::EventLog::{
        EVENTLOG_ERROR_TYPE, EVENTLOG_INFORMATION_TYPE, EVENTLOG_WARNING_TYPE,
        RegisterEventSourceW, ReportEventW,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct Source {
        handle: usize,
        /// The `TOKEN_USER` of this process, so each event names the user it was written as.
        user: Vec<u64>,
    }

    static SOURCE: OnceLock<Option<Source>> = OnceLock::new();

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn token_user() -> Vec<u64> {
        // SAFETY: plain Win32 calls on this process's own token; the buffer outlives the call
        // and is sized from the first one.
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Vec::new();
            }
            let mut len = 0u32;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
            // u64s, so the buffer is aligned for TOKEN_USER.
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len);
            CloseHandle(token);
            if ok == 0 { Vec::new() } else { buf }
        }
    }

    pub fn init() {
        SOURCE.get_or_init(|| {
            let name = wide(super::SOURCE);
            // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call.
            let handle = unsafe { RegisterEventSourceW(std::ptr::null(), name.as_ptr()) };
            (!handle.is_null()).then(|| Source {
                handle: handle as usize,
                user: token_user(),
            })
        });
    }

    pub fn write(level: Level, id: u32, text: &str) {
        let Some(Some(source)) = SOURCE.get() else {
            return;
        };
        let kind = match level {
            Level::Info => EVENTLOG_INFORMATION_TYPE,
            Level::Warning => EVENTLOG_WARNING_TYPE,
            Level::Error => EVENTLOG_ERROR_TYPE,
        };
        let text = wide(text);
        let strings = [text.as_ptr()];
        let sid = if source.user.is_empty() {
            std::ptr::null_mut()
        } else {
            // SAFETY: the buffer holds the TOKEN_USER GetTokenInformation wrote, kept for the
            // life of the process, so the SID it points into stays valid.
            unsafe { (*source.user.as_ptr().cast::<TOKEN_USER>()).User.Sid }
        };
        // SAFETY: the handle came from RegisterEventSourceW and is never closed; one string,
        // NUL-terminated, alive for the call; no raw data.
        unsafe {
            ReportEventW(
                source.handle as HANDLE,
                kind,
                0,
                id,
                sid,
                1,
                0,
                strings.as_ptr(),
                std::ptr::null(),
            );
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn init() {}
    pub fn write(_: super::Level, _: u32, _: &str) {}
}

/// Opens the event source. Until it is called, [`write`] writes nothing.
pub fn init() {
    imp::init();
}

/// Writes an event, if [`init`] was called and the log took the source.
pub fn write(level: Level, id: u32, text: &str) {
    imp::write(level, id, text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_has_the_summary_then_the_fields_that_have_a_value() {
        let t = text(
            "devstribution stopped a run.",
            &[("App", "acme/tool 1.0.0"), ("Detail", ""), ("Run", "r1")],
        );
        assert_eq!(
            t,
            "devstribution stopped a run.\r\nApp: acme/tool 1.0.0\r\nRun: r1"
        );
    }

    #[test]
    fn long_text_is_cut_on_a_character() {
        let t = text("é".repeat(3000).as_str(), &[]);
        assert!(t.len() <= MAX_TEXT + '…'.len_utf8());
        assert!(t.ends_with('…'));
    }

    /// Writes one event to this machine's Application log, to look at in Event Viewer:
    /// `cargo test -p devstribution-host eventlog -- --ignored`.
    #[test]
    #[ignore = "writes to this machine's Event Log"]
    fn writes_an_event() {
        init();
        write(
            Level::Info,
            id::RUN_OK,
            &text("devstribution test event.", &[("Run", "test")]),
        );
    }

    #[test]
    fn writes_nothing_before_init() {
        // Not initialised in tests: must not panic or touch the machine's log.
        write(Level::Error, id::GUARD_STOP, "test");
    }
}
