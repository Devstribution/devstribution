//! The proxy an app's downloads go through. uv reads a proxy only from `HTTPS_PROXY` and the
//! like; company PCs usually set none and configure the proxy in Windows instead: a fixed
//! proxy, an automatic configuration script (PAC), or automatic detection (WPAD). When no
//! variable is set, this asks Windows (WinHTTP, which runs the script) which proxy it would use
//! for each download host, and hands the answer to uv as `HTTPS_PROXY` and `NO_PROXY`.
//!
//! Only reads: nothing here changes a Windows setting.

use std::process::Command;

/// The variables uv reads a proxy from, in the order it prefers them.
const PROXY_VARS: &[&str] = &["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"];

/// The proxy for a set of download hosts, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// The proxy to give uv (`http://host:port`); `None` when uv should connect directly or
    /// already has a proxy from the environment.
    pub proxy: Option<String>,
    /// Hosts that go direct although others go through `proxy` (for `NO_PROXY`).
    pub direct: Vec<String>,
    /// What the user and IT see: the proxy and where it came from.
    pub shown: String,
}

impl Choice {
    /// Sets the proxy on a uv command. Call after the command's environment was cleared.
    pub fn apply(&self, cmd: &mut Command) {
        let Some(proxy) = &self.proxy else { return };
        cmd.env("HTTPS_PROXY", proxy).env("HTTP_PROXY", proxy);
        if !self.direct.is_empty() {
            let mut no_proxy = self.direct.join(",");
            if let Some(kept) = std::env::var("NO_PROXY")
                .or_else(|_| std::env::var("no_proxy"))
                .ok()
                .filter(|v| !v.trim().is_empty())
            {
                no_proxy = format!("{},{no_proxy}", kept.trim());
            }
            cmd.env("NO_PROXY", no_proxy);
        }
    }
}

/// The proxy for downloads from `hosts`: the environment's if it names one, else Windows'.
pub fn for_hosts(hosts: &[&str]) -> Choice {
    if let Some((var, value)) = PROXY_VARS
        .iter()
        .find_map(|k| std::env::var(k).ok().map(|v| (*k, v)))
        .filter(|(_, v)| !v.trim().is_empty())
    {
        return Choice {
            proxy: None,
            direct: Vec::new(),
            shown: format!("{} (from {var})", hide_login(value.trim())),
        };
    }
    from_windows(hosts)
}

#[cfg(windows)]
fn from_windows(hosts: &[&str]) -> Choice {
    decide(&windows::settings(), hosts, windows::proxy_for_url)
}

#[cfg(not(windows))]
fn from_windows(_hosts: &[&str]) -> Choice {
    Choice {
        proxy: None,
        direct: Vec::new(),
        shown: "none set (HTTPS_PROXY)".into(),
    }
}

/// Windows' proxy settings for the current user, as Internet Options and Settings → Network
/// show them, plus WinHTTP's own machine-wide setting (`netsh winhttp set proxy`).
#[derive(Debug, Default, Clone)]
pub struct Settings {
    /// "Automatically detect settings" (WPAD).
    pub auto_detect: bool,
    /// "Use setup script": the PAC script's address.
    pub script: Option<String>,
    /// "Use a proxy server": WinHTTP's proxy list (`host:port`, or `https=host:port;…`).
    pub fixed: Option<String>,
    /// Addresses that skip the fixed proxy (`*.corp.local;<local>`).
    pub bypass: Option<String>,
    /// WinHTTP's machine-wide proxy and its bypass list, used when the user has none.
    pub machine: Option<(String, Option<String>)>,
}

/// What the script or automatic detection said for one address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    Direct,
    /// WinHTTP's proxy list for the address.
    Proxy(String),
}

/// Picks the proxy for each host from `settings`, asking `lookup` (WinHTTP) when a script or
/// automatic detection is on. A script that cannot be read or found falls back to the fixed
/// proxy, as Windows does.
fn decide(
    settings: &Settings,
    hosts: &[&str],
    lookup: impl Fn(&str, &Settings) -> Result<Lookup, u32>,
) -> Choice {
    let automatic = settings.auto_detect || settings.script.is_some();
    let mut picked: Vec<(String, Option<String>)> = Vec::new();
    let mut how = None;
    let mut script_error = None;
    for host in hosts {
        let host = host.to_ascii_lowercase();
        let mut proxy = None;
        let mut decided = false;
        if automatic {
            match lookup(&format!("https://{host}/"), settings) {
                Ok(found) => {
                    decided = true;
                    how.get_or_insert_with(|| match &settings.script {
                        Some(url) => format!("Windows' setup script {url}"),
                        None => "Windows' automatic detection (WPAD)".into(),
                    });
                    if let Lookup::Proxy(list) = found {
                        proxy = pick(&list);
                    }
                }
                Err(code) => script_error = Some(code),
            }
        }
        if !decided {
            if let Some(fixed) = &settings.fixed {
                how.get_or_insert_with(|| "Windows' proxy setting".into());
                if !bypassed(&host, settings.bypass.as_deref()) {
                    proxy = pick(fixed);
                }
            } else if let Some((fixed, bypass)) = &settings.machine {
                how.get_or_insert_with(|| "WinHTTP's proxy setting (netsh)".into());
                if !bypassed(&host, bypass.as_deref()) {
                    proxy = pick(fixed);
                }
            }
        }
        picked.push((host, proxy));
    }

    let proxy = picked.iter().find_map(|(_, p)| p.clone());
    let direct: Vec<String> = picked
        .iter()
        .filter(|(_, p)| p.is_none())
        .map(|(h, _)| h.clone())
        .collect();
    let others: Vec<String> = picked
        .iter()
        .filter(|(_, p)| p.is_some() && *p != proxy)
        .map(|(h, p)| format!("{h} → {}", p.as_deref().unwrap_or_default()))
        .collect();

    // A setup script IT set that could not be used is worth showing. Automatic detection
    // finding nothing is what most networks without a proxy give: not shown.
    let unread = match (script_error, &settings.script) {
        (Some(code), Some(url)) => {
            format!("; Windows' setup script {url} could not be used (error {code})")
        }
        _ => String::new(),
    };

    let shown = match (&proxy, &how) {
        (Some(p), Some(how)) => {
            let mut s = format!("{} (from {how})", hide_login(p));
            if !direct.is_empty() {
                s += &format!("; direct to {}", direct.join(", "));
            }
            if !others.is_empty() {
                s += &format!(
                    "; the script names other proxies too, not used: {}",
                    others.join(", ")
                );
            }
            s
        }
        (None, Some(how)) => format!("none: direct, from {how}"),
        _ => "none set (in Windows or HTTPS_PROXY)".into(),
    } + &unread;
    let direct = if proxy.is_some() { direct } else { Vec::new() };
    Choice {
        proxy: proxy.map(|p| {
            if p.contains("://") {
                p
            } else {
                format!("http://{p}")
            }
        }),
        direct,
        shown,
    }
}

/// The proxy for an `https://` address from a WinHTTP proxy list: `host:port` entries
/// separated by `;` or spaces, each maybe `scheme=`-qualified. An `https=` entry wins, then
/// one with no scheme; `http=`, `ftp=` and `socks=` entries are for other kinds of address.
fn pick(list: &str) -> Option<String> {
    let entries: Vec<&str> = list
        .split([';', ' ', '\t', '\r', '\n'])
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .collect();
    let named = |scheme: &str| {
        entries.iter().find_map(|e| {
            let (s, rest) = e.split_once('=')?;
            s.eq_ignore_ascii_case(scheme).then_some(rest.trim())
        })
    };
    named("https")
        .or_else(|| entries.iter().copied().find(|e| !e.contains('=')))
        .filter(|p| !p.is_empty())
        .map(str::to_string)
}

/// Whether `host` is in a WinHTTP bypass list: entries separated by `;`, `,` or spaces, `*`
/// as a wildcard, `<local>` for names without a dot.
fn bypassed(host: &str, list: Option<&str>) -> bool {
    let Some(list) = list else { return false };
    list.split([';', ',', ' ', '\t'])
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .any(|entry| {
            if entry.eq_ignore_ascii_case("<local>") {
                return !host.contains('.');
            }
            let entry = entry.split_once("://").map_or(entry, |(_, rest)| rest);
            let entry = entry.trim_end_matches('/');
            wildcard(&entry.to_ascii_lowercase(), host)
        })
}

/// `*` matches any run of characters, anything else itself.
fn wildcard(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    if parts.is_empty() {
        return rest.is_empty();
    }
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            return rest.ends_with(part);
        }
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    true
}

/// A proxy address as it may be shown: a login in it is hidden.
pub fn hide_login(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => match rest.split_once('@') {
            Some((_, host)) => format!("{scheme}://…@{host}"),
            None => url.to_string(),
        },
        None => match url.split_once('@') {
            Some((_, host)) => format!("…@{host}"),
            None => url.to_string(),
        },
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;

    use windows_sys::Win32::Foundation::{GetLastError, GlobalFree};
    use windows_sys::Win32::Networking::WinHttp::{
        WINHTTP_ACCESS_TYPE_NAMED_PROXY, WINHTTP_ACCESS_TYPE_NO_PROXY,
        WINHTTP_AUTO_DETECT_TYPE_DHCP, WINHTTP_AUTO_DETECT_TYPE_DNS_A,
        WINHTTP_AUTOPROXY_AUTO_DETECT, WINHTTP_AUTOPROXY_CONFIG_URL, WINHTTP_AUTOPROXY_OPTIONS,
        WINHTTP_CURRENT_USER_IE_PROXY_CONFIG, WINHTTP_PROXY_INFO, WinHttpCloseHandle,
        WinHttpGetDefaultProxyConfiguration, WinHttpGetIEProxyConfigForCurrentUser,
        WinHttpGetProxyForUrl, WinHttpOpen, WinHttpSetTimeouts,
    };
    use windows_sys::core::PWSTR;

    use super::{Lookup, Settings};

    /// Reads a string WinHTTP allocated, and frees it.
    ///
    /// # Safety
    /// `p` is null or a NUL-terminated string allocated with `GlobalAlloc`, not used after.
    unsafe fn take(p: PWSTR) -> Option<String> {
        if p.is_null() {
            return None;
        }
        // SAFETY: `p` is NUL-terminated (the caller's contract).
        let len = (0..).take_while(|&i| unsafe { *p.add(i) } != 0).count();
        // SAFETY: `len` characters were just read from `p`.
        let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len) });
        // SAFETY: allocated by WinHTTP with GlobalAlloc, and not used after this.
        unsafe { GlobalFree(p as *mut c_void) };
        let s = s.trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn settings() -> Settings {
        let mut out = Settings::default();
        let mut ie = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
        // SAFETY: `ie` is a valid out-pointer; its strings are taken (and freed) below.
        if unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut ie) } != 0 {
            out.auto_detect = ie.fAutoDetect != 0;
            // SAFETY: WinHTTP allocated these strings (or left them null).
            unsafe {
                out.script = take(ie.lpszAutoConfigUrl);
                out.fixed = take(ie.lpszProxy);
                out.bypass = take(ie.lpszProxyBypass);
            }
        }
        let mut machine = WINHTTP_PROXY_INFO::default();
        // SAFETY: `machine` is a valid out-pointer; its strings are taken (and freed) below.
        if unsafe { WinHttpGetDefaultProxyConfiguration(&mut machine) } != 0 {
            // SAFETY: WinHTTP allocated these strings (or left them null).
            let (proxy, bypass) =
                unsafe { (take(machine.lpszProxy), take(machine.lpszProxyBypass)) };
            if machine.dwAccessType == WINHTTP_ACCESS_TYPE_NAMED_PROXY {
                out.machine = proxy.map(|p| (p, bypass));
            }
        }
        out
    }

    /// Asks WinHTTP which proxy it would use for `url`, running the setup script or automatic
    /// detection that `settings` turn on. The error is WinHTTP's code (12180: nothing was
    /// detected; 12167: the script could not be downloaded; 12166: it has an error).
    pub fn proxy_for_url(url: &str, settings: &Settings) -> Result<Lookup, u32> {
        let agent = wide("devstribution");
        // SAFETY: the strings are NUL-terminated and outlive the call.
        let session = unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_NO_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        };
        if session.is_null() {
            // SAFETY: no preconditions.
            return Err(unsafe { GetLastError() });
        }
        // The script's download: a few seconds, not WinHTTP's default of a minute and more.
        // SAFETY: `session` is a valid handle.
        unsafe { WinHttpSetTimeouts(session, 5_000, 5_000, 5_000, 10_000) };
        let script = settings.script.as_deref().map(wide);
        let mut options = WINHTTP_AUTOPROXY_OPTIONS {
            fAutoLogonIfChallenged: 1,
            ..Default::default()
        };
        if let Some(script) = &script {
            options.dwFlags |= WINHTTP_AUTOPROXY_CONFIG_URL;
            options.lpszAutoConfigUrl = script.as_ptr();
        }
        if settings.auto_detect {
            options.dwFlags |= WINHTTP_AUTOPROXY_AUTO_DETECT;
            options.dwAutoDetectFlags =
                WINHTTP_AUTO_DETECT_TYPE_DHCP | WINHTTP_AUTO_DETECT_TYPE_DNS_A;
        }
        let url = wide(url);
        let mut info = WINHTTP_PROXY_INFO::default();
        // SAFETY: valid session, NUL-terminated strings that outlive the call, valid out-pointer.
        let ok =
            unsafe { WinHttpGetProxyForUrl(session, url.as_ptr(), &mut options, &mut info) } != 0;
        // SAFETY: no preconditions.
        let error = unsafe { GetLastError() };
        // SAFETY: `session` is a valid handle, closed once.
        unsafe { WinHttpCloseHandle(session) };
        if !ok {
            return Err(error);
        }
        // SAFETY: WinHTTP allocated these strings (or left them null).
        let (proxy, _bypass) = unsafe { (take(info.lpszProxy), take(info.lpszProxyBypass)) };
        Ok(match proxy {
            Some(list) if info.dwAccessType == WINHTTP_ACCESS_TYPE_NAMED_PROXY => {
                Lookup::Proxy(list)
            }
            _ => Lookup::Direct,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOSTS: &[&str] = &["files.pythonhosted.org", "github.com"];

    fn never(_: &str, _: &Settings) -> Result<Lookup, u32> {
        panic!("no script or detection is on")
    }

    #[test]
    fn nothing_set_is_direct() {
        let c = decide(&Settings::default(), HOSTS, never);
        assert_eq!(c.proxy, None);
        assert!(c.direct.is_empty());
        assert_eq!(c.shown, "none set (in Windows or HTTPS_PROXY)");
    }

    #[test]
    fn a_fixed_proxy_and_its_bypass_list() {
        let s = Settings {
            fixed: Some("proxy.corp:8080".into()),
            bypass: Some("*.github.com;github.com;<local>".into()),
            ..Default::default()
        };
        let c = decide(&s, HOSTS, never);
        assert_eq!(c.proxy.as_deref(), Some("http://proxy.corp:8080"));
        assert_eq!(c.direct, ["github.com"]);
        assert_eq!(
            c.shown,
            "proxy.corp:8080 (from Windows' proxy setting); direct to github.com"
        );
    }

    #[test]
    fn the_https_entry_of_a_per_scheme_list() {
        assert_eq!(
            pick("http=a:80;https=b:443;socks=c:1080").as_deref(),
            Some("b:443")
        );
        assert_eq!(pick("http=a:80;b:8080").as_deref(), Some("b:8080"));
        assert_eq!(pick("http=a:80").as_deref(), None);
        assert_eq!(pick(" p1:3128; p2:3128 ").as_deref(), Some("p1:3128"));
    }

    #[test]
    fn bypass_entries() {
        assert!(bypassed("intranet", Some("<local>")));
        assert!(!bypassed("pypi.org", Some("<local>")));
        assert!(bypassed(
            "files.pythonhosted.org",
            Some("*.pythonhosted.org")
        ));
        assert!(bypassed("mirror.corp.local", Some("http://*.corp.local/")));
        assert!(!bypassed(
            "pythonhosted.org.evil",
            Some("*.pythonhosted.org")
        ));
        assert!(bypassed("10.1.2.3", Some("10.*")));
    }

    #[test]
    fn the_script_decides_per_host() {
        let s = Settings {
            script: Some("http://wpad/proxy.pac".into()),
            fixed: Some("fallback:8080".into()),
            ..Default::default()
        };
        let c = decide(&s, HOSTS, |url, _| {
            Ok(if url.contains("github") {
                Lookup::Direct
            } else {
                Lookup::Proxy("pac-proxy:3128; other:3128".into())
            })
        });
        assert_eq!(c.proxy.as_deref(), Some("http://pac-proxy:3128"));
        assert_eq!(c.direct, ["github.com"]);
        assert!(
            c.shown
                .starts_with("pac-proxy:3128 (from Windows' setup script http://wpad/proxy.pac)")
        );
    }

    #[test]
    fn a_script_that_cannot_be_read_falls_back_to_the_fixed_proxy() {
        let s = Settings {
            script: Some("http://wpad/proxy.pac".into()),
            fixed: Some("fallback:8080".into()),
            ..Default::default()
        };
        let c = decide(&s, HOSTS, |_, _| Err(12167));
        assert_eq!(c.proxy.as_deref(), Some("http://fallback:8080"));
        assert_eq!(
            c.shown,
            "fallback:8080 (from Windows' proxy setting); Windows' setup script \
             http://wpad/proxy.pac could not be used (error 12167)"
        );
    }

    #[test]
    fn detection_that_finds_nothing_is_not_an_error() {
        // Automatic detection is on by default in Windows; at home it finds no script.
        let s = Settings {
            auto_detect: true,
            ..Default::default()
        };
        let c = decide(&s, HOSTS, |_, _| Err(12180));
        assert_eq!(c.proxy, None);
        assert_eq!(c.shown, "none set (in Windows or HTTPS_PROXY)");
    }

    #[test]
    fn the_machine_setting_when_the_user_has_none() {
        let s = Settings {
            machine: Some(("netsh-proxy:8080".into(), None)),
            ..Default::default()
        };
        let c = decide(&s, HOSTS, never);
        assert_eq!(c.proxy.as_deref(), Some("http://netsh-proxy:8080"));
        assert!(c.direct.is_empty());
    }

    #[test]
    fn a_login_in_the_proxy_is_hidden() {
        assert_eq!(
            hide_login("http://user:pw@proxy:8080"),
            "http://…@proxy:8080"
        );
        assert_eq!(hide_login("user:pw@proxy:8080"), "…@proxy:8080");
        assert_eq!(hide_login("proxy:8080"), "proxy:8080");
    }

    /// The real WinHTTP, with a setup script served from this machine: the script decides per
    /// host, and no Windows setting is read or changed.
    #[cfg(windows)]
    #[test]
    fn winhttp_runs_a_setup_script() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let script = "function FindProxyForURL(url, host) {\n\
                 if (shExpMatch(host, \"*.pythonhosted.org\")) return \"PROXY 127.0.0.1:3128\";\n\
                 return \"DIRECT\";\n}\n";
            for stream in listener.incoming().take(8) {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-ns-proxy-autoconfig\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{script}",
                    script.len()
                );
            }
        });
        let s = Settings {
            script: Some(format!("http://127.0.0.1:{port}/proxy.pac")),
            ..Default::default()
        };
        let c = decide(&s, HOSTS, windows::proxy_for_url);
        assert_eq!(
            c.proxy.as_deref(),
            Some("http://127.0.0.1:3128"),
            "{}",
            c.shown
        );
        assert_eq!(c.direct, ["github.com"]);
    }
}
