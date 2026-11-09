//! What the launcher adds to Windows for an app, at the user's request: a shortcut in the Start
//! menu (P9) and a Task Scheduler entry that runs it on a schedule (P10). Both start the launcher
//! itself, with the app's id, so every run goes through the same checks as one started by hand.
//! Both belong to the current user only (no administrator rights) and go with the app.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use devstribution_host::Paths;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The launcher's argument that opens an app's panel (the Start menu shortcut).
pub const OPEN_APP: &str = "--open-app";
/// The launcher's argument that runs an app with its scheduled inputs (Task Scheduler).
pub const SCHEDULED_RUN: &str = "--scheduled-run";

/// How often a scheduled app runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Every {
    Day,
    /// Monday to Friday.
    Weekday,
    Week,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub every: Every,
    /// For `week`: 1 is Monday, 7 is Sunday.
    #[serde(default)]
    pub day: Option<u8>,
    /// Local time, `HH:MM`.
    pub time: String,
    /// The inputs every scheduled run uses, as they were when the schedule was saved.
    #[serde(default)]
    pub inputs: Map<String, Value>,
}

const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

impl Schedule {
    /// The time as hours and minutes, if it is one.
    fn hm(&self) -> Option<(u32, u32)> {
        let (h, m) = self.time.split_once(':')?;
        let (h, m) = (h.parse().ok()?, m.parse().ok()?);
        (h < 24 && m < 60 && self.time.len() == 5).then_some((h, m))
    }

    pub fn check(&self) -> Result<()> {
        if self.hm().is_none() {
            bail!("`{}` is not a time: use HH:MM, such as 09:00", self.time);
        }
        match (self.every, self.day) {
            (Every::Week, Some(1..=7)) => Ok(()),
            (Every::Week, _) => bail!("a weekly schedule needs a day, 1 (Monday) to 7 (Sunday)"),
            _ => Ok(()),
        }
    }

    /// "every Monday at 09:00", as Settings and the task's description say it.
    pub fn describe(&self) -> String {
        match self.every {
            Every::Day => format!("every day at {}", self.time),
            Every::Weekday => format!("Monday to Friday at {}", self.time),
            Every::Week => format!(
                "every {} at {}",
                DAYS[usize::from(self.day.unwrap_or(1).clamp(1, 7)) - 1],
                self.time
            ),
        }
    }
}

/// What the launcher added to Windows for each app, by app id: kept in `state/`, so it can be
/// removed with the app even when its title changed since.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Added {
    #[serde(default)]
    shortcuts: BTreeMap<String, PathBuf>,
    #[serde(default)]
    schedules: BTreeMap<String, Schedule>,
}

fn added_file(paths: &Paths) -> PathBuf {
    paths.state().join("windows.json")
}

fn load(paths: &Paths) -> Added {
    fs::read(added_file(paths))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(paths: &Paths, added: &Added) -> Result<()> {
    let file = added_file(paths);
    fs::create_dir_all(file.parent().expect("in state/"))?;
    let tmp = file.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(added)?)?;
    fs::rename(&tmp, &file).context("saving what devstribution added to Windows")
}

pub fn shortcut(paths: &Paths, id: &str) -> Option<PathBuf> {
    load(paths)
        .shortcuts
        .get(id)
        .cloned()
        .filter(|p| p.is_file())
}

pub fn schedule(paths: &Paths, id: &str) -> Option<Schedule> {
    load(paths).schedules.get(id).cloned()
}

/// The Start menu folder the shortcuts go in.
fn start_menu() -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").ok_or_else(|| anyhow!("APPDATA is not set"))?;
    Ok(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\devstribution apps"))
}

/// A title as a file name: without the characters Windows refuses, nor trailing dots.
fn file_name(title: &str) -> String {
    let name: String = title
        .chars()
        .map(|c| {
            if c.is_control() || r#"\/:*?"<>|"#.contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name: String = name.chars().take(80).collect();
    name.trim_end_matches(['.', ' ']).to_string()
}

/// Adds the app to the Start menu: a shortcut that opens its panel in the launcher, with its
/// icon when it has one Windows can show.
pub fn add_shortcut(
    paths: &Paths,
    id: &str,
    title: &str,
    description: Option<&str>,
    icon: Option<&str>,
) -> Result<()> {
    let mut added = load(paths);
    if let Some(old) = added.shortcuts.remove(id) {
        let _ = fs::remove_file(old);
    }
    let folder = start_menu()?;
    fs::create_dir_all(&folder).context("creating the Start menu folder")?;
    let base = match file_name(title) {
        n if n.is_empty() => file_name(id),
        n => n,
    };
    let mut link = folder.join(format!("{base}.lnk"));
    if link.exists() {
        let namespace = id.split('/').next().unwrap_or(id);
        link = folder.join(format!("{base} ({}).lnk", file_name(namespace)));
    }
    let icon_file = icon.and_then(|i| write_icon(paths, id, i));
    let exe = dunce::canonicalize(std::env::current_exe()?)?;
    windows_shell::create_link(
        &link,
        &exe,
        &format!("{OPEN_APP} \"{id}\""),
        description.unwrap_or(title),
        icon_file.as_deref(),
    )?;
    added.shortcuts.insert(id.to_string(), link);
    save(paths, &added)
}

pub fn remove_shortcut(paths: &Paths, id: &str) -> Result<()> {
    let mut added = load(paths);
    if let Some(link) = added.shortcuts.remove(id) {
        match fs::remove_file(&link) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(e).context("removing the Start menu shortcut");
            }
            _ => {}
        }
    }
    let _ = fs::remove_file(icon_path(paths, id));
    save(paths, &added)
}

fn safe_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn icon_path(paths: &Paths, id: &str) -> PathBuf {
    paths
        .root
        .join("icons")
        .join(format!("{}.ico", safe_id(id)))
}

/// The app's icon as an `.ico` file for the shortcut: an ICO as it is, a PNG of at most 256
/// pixels inside one (Windows reads PNG in ICO). Others (SVG, JPEG, large PNG) get the
/// launcher's icon.
fn write_icon(paths: &Paths, id: &str, data_url: &str) -> Option<PathBuf> {
    use base64::Engine;
    let (head, b64) = data_url.split_once(";base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let ico = match head {
        "data:image/x-icon" => bytes,
        "data:image/png" => png_in_ico(&bytes)?,
        _ => return None,
    };
    let file = icon_path(paths, id);
    fs::create_dir_all(file.parent()?).ok()?;
    fs::write(&file, ico).ok()?;
    Some(file)
}

fn png_in_ico(png: &[u8]) -> Option<Vec<u8>> {
    if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" || &png[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(png[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(png[20..24].try_into().ok()?);
    if w == 0 || h == 0 || w > 256 || h > 256 {
        return None;
    }
    let mut ico = Vec::with_capacity(png.len() + 22);
    ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    // 256 is written as 0.
    ico.push((w % 256) as u8);
    ico.push((h % 256) as u8);
    ico.extend_from_slice(&[0, 0, 1, 0, 32, 0]);
    ico.extend_from_slice(&u32::try_from(png.len()).ok()?.to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(png);
    Some(ico)
}

/// The task's name in Task Scheduler, in its top folder (a user may not be able to create
/// folders there). Namespaces and names have no spaces, so the space keeps ids apart.
fn task_name(id: &str) -> String {
    format!("devstribution {}", safe_id(id).replacen('_', " ", 1))
}

/// Saves the schedule and registers the task that starts the launcher at those times. A
/// missed time (the computer was off) runs when it is next on; a run still going when the next
/// time comes is not started twice.
pub fn set_schedule(paths: &Paths, id: &str, title: &str, schedule: Schedule) -> Result<()> {
    schedule.check()?;
    let exe = dunce::canonicalize(std::env::current_exe()?)?;
    let xml = task_xml(&exe, id, title, &schedule, windows_shell::local_now());
    let file = paths.state().join(format!("task-{}.xml", safe_id(id)));
    fs::create_dir_all(paths.state())?;
    // Task Scheduler reads UTF-16 with a byte order mark.
    let mut bytes = vec![0xFF, 0xFE];
    for unit in xml.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(&file, bytes)?;
    let created = schtasks(&[
        "/Create".as_ref(),
        "/TN".as_ref(),
        task_name(id).as_ref(),
        "/XML".as_ref(),
        file.as_os_str(),
        "/F".as_ref(),
    ]);
    let _ = fs::remove_file(&file);
    created.context("adding the schedule to Windows Task Scheduler")?;
    let mut added = load(paths);
    added.schedules.insert(id.to_string(), schedule);
    save(paths, &added)
}

pub fn remove_schedule(paths: &Paths, id: &str) -> Result<()> {
    let mut added = load(paths);
    if added.schedules.remove(id).is_some() || cfg!(windows) {
        // Also when none is recorded: a task left from a state file that was lost.
        let _ = schtasks(&[
            "/Delete".as_ref(),
            "/TN".as_ref(),
            task_name(id).as_ref(),
            "/F".as_ref(),
        ]);
    }
    save(paths, &added)
}

/// Removes every shortcut and scheduled task the launcher added: the uninstaller runs this
/// (`devstribution.exe --forget-windows`), since they point at a launcher that is going.
pub fn forget_all(paths: &Paths) {
    let added = load(paths);
    for id in added.shortcuts.keys().chain(added.schedules.keys()) {
        let _ = remove_shortcut(paths, id);
        let _ = remove_schedule(paths, id);
    }
    if let Ok(folder) = start_menu() {
        let _ = fs::remove_dir(folder);
    }
}

fn schtasks(args: &[&std::ffi::OsStr]) -> Result<()> {
    if !cfg!(windows) {
        bail!("schedules need Windows Task Scheduler");
    }
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    let mut cmd = std::process::Command::new(Path::new(&root).join(r"System32\schtasks.exe"));
    cmd.args(args);
    devstribution_host::uv::hide_window(&mut cmd);
    let out = cmd.output().context("cannot start schtasks.exe")?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stderr);
        bail!("{}", text.trim());
    }
    Ok(())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The first time the schedule applies: today at its time, or tomorrow when that has passed
/// (a start in the past would count as missed and run at once).
fn start_boundary(schedule: &Schedule, now: (i32, u32, u32, u32, u32)) -> String {
    let (y, mo, d, h, mi) = now;
    let (sh, sm) = schedule.hm().unwrap_or((9, 0));
    let mut days = days_from_civil(y, mo, d);
    if (sh, sm) <= (h, mi) {
        days += 1;
    }
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{sh:02}:{sm:02}:00")
}

fn task_xml(
    exe: &Path,
    id: &str,
    title: &str,
    schedule: &Schedule,
    now: (i32, u32, u32, u32, u32),
) -> String {
    let when = match schedule.every {
        Every::Day => "<ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay>".to_string(),
        Every::Weekday => "<ScheduleByWeek><DaysOfWeek><Monday /><Tuesday /><Wednesday /><Thursday /><Friday /></DaysOfWeek><WeeksInterval>1</WeeksInterval></ScheduleByWeek>".to_string(),
        Every::Week => format!(
            "<ScheduleByWeek><DaysOfWeek><{} /></DaysOfWeek><WeeksInterval>1</WeeksInterval></ScheduleByWeek>",
            DAYS[usize::from(schedule.day.unwrap_or(1).clamp(1, 7)) - 1]
        ),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>devstribution</Author>
    <Description>{description}</Description>
  </RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>{start}</StartBoundary>
      <Enabled>true</Enabled>
      {when}
    </CalendarTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{arguments}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        description = xml_escape(&format!(
            "Runs {title} with devstribution {}. Change or remove it in the app's panel in devstribution.",
            schedule.describe()
        )),
        start = start_boundary(schedule, now),
        command = xml_escape(&exe.display().to_string()),
        arguments = xml_escape(&format!("{SCHEDULED_RUN} \"{id}\"")),
    )
}

/// Days since 1970-01-01 of a date in the proleptic Gregorian calendar (H. Hinnant's algorithm).
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(if m <= 2 { y - 1 } else { y });
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (yoe + era * 400 + i64::from(m <= 2)) as i32;
    (y, m, d)
}

/// The arguments the launcher was started with that are about one app.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AppArgs {
    pub open: Option<String>,
    pub scheduled: Option<String>,
}

pub fn app_args(args: &[String]) -> AppArgs {
    let after = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    AppArgs {
        open: after(OPEN_APP),
        scheduled: after(SCHEDULED_RUN),
    }
}

#[cfg(windows)]
mod windows_shell {
    use std::path::Path;

    use anyhow::{Context, Result};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize, IPersistFile,
    };
    use windows::Win32::System::SystemInformation::GetLocalTime;
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{HSTRING, Interface};

    /// Writes a `.lnk` file with the shell's own ShellLink object.
    pub fn create_link(
        link: &Path,
        target: &Path,
        arguments: &str,
        description: &str,
        icon: Option<&Path>,
    ) -> Result<()> {
        // SAFETY: COM is initialised on this thread for the duration of the calls below and
        // uninitialised after (when this call initialised it); every pointer passed is an
        // HSTRING that outlives the call.
        unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let result = (|| -> windows::core::Result<()> {
                let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                shell.SetPath(&HSTRING::from(target.as_os_str()))?;
                shell.SetArguments(&HSTRING::from(arguments))?;
                let description: String = description.chars().take(250).collect();
                shell.SetDescription(&HSTRING::from(description))?;
                if let Some(dir) = target.parent() {
                    shell.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
                }
                match icon {
                    Some(icon) => shell.SetIconLocation(&HSTRING::from(icon.as_os_str()), 0)?,
                    None => shell.SetIconLocation(&HSTRING::from(target.as_os_str()), 0)?,
                }
                let file: IPersistFile = shell.cast()?;
                file.Save(&HSTRING::from(link.as_os_str()), true)
            })();
            if init.is_ok() {
                CoUninitialize();
            }
            result.context("creating the Start menu shortcut")
        }
    }

    /// Local date and time: year, month, day, hour, minute.
    pub fn local_now() -> (i32, u32, u32, u32, u32) {
        // SAFETY: takes no arguments and returns a plain struct.
        let t = unsafe { GetLocalTime() };
        (
            i32::from(t.wYear),
            u32::from(t.wMonth),
            u32::from(t.wDay),
            u32::from(t.wHour),
            u32::from(t.wMinute),
        )
    }
}

#[cfg(not(windows))]
mod windows_shell {
    use std::path::Path;

    use anyhow::{Result, bail};

    pub fn create_link(_: &Path, _: &Path, _: &str, _: &str, _: Option<&Path>) -> Result<()> {
        bail!("Start menu shortcuts are made on Windows only")
    }

    pub fn local_now() -> (i32, u32, u32, u32, u32) {
        (1970, 1, 1, 0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weekly(day: u8, time: &str) -> Schedule {
        Schedule {
            every: Every::Week,
            day: Some(day),
            time: time.into(),
            inputs: Map::new(),
        }
    }

    #[test]
    fn schedules_are_checked_and_described() {
        assert!(weekly(1, "09:00").check().is_ok());
        assert!(weekly(8, "09:00").check().is_err());
        assert!(weekly(1, "9:00").check().is_err());
        assert!(weekly(1, "24:00").check().is_err());
        assert_eq!(weekly(1, "09:00").describe(), "every Monday at 09:00");
        let daily = Schedule {
            every: Every::Weekday,
            day: None,
            time: "17:30".into(),
            inputs: Map::new(),
        };
        assert!(daily.check().is_ok());
        assert_eq!(daily.describe(), "Monday to Friday at 17:30");
    }

    #[test]
    fn the_first_run_is_never_in_the_past() {
        let s = weekly(1, "09:00");
        assert_eq!(
            start_boundary(&s, (2026, 11, 9, 8, 59)),
            "2026-11-09T09:00:00"
        );
        assert_eq!(
            start_boundary(&s, (2026, 11, 9, 9, 0)),
            "2026-11-10T09:00:00"
        );
        assert_eq!(
            start_boundary(&s, (2026, 12, 31, 23, 0)),
            "2027-01-01T09:00:00"
        );
        assert_eq!(
            start_boundary(&s, (2028, 2, 28, 10, 0)),
            "2028-02-29T09:00:00"
        );
    }

    #[test]
    fn the_task_starts_the_launcher_for_the_app() {
        let xml = task_xml(
            Path::new(r"C:\Users\a\AppData\Local\devstribution\devstribution.exe"),
            "acme/report",
            "Sales & stock",
            &weekly(3, "07:15"),
            (2026, 11, 9, 12, 0),
        );
        assert!(xml.contains("<Arguments>--scheduled-run &quot;acme/report&quot;</Arguments>"));
        assert!(xml.contains("<Wednesday />"));
        assert!(xml.contains("Runs Sales &amp; stock with devstribution every Wednesday at 07:15"));
        assert!(xml.contains("<StartWhenAvailable>true</StartWhenAvailable>"));
        assert!(xml.contains("<StartBoundary>2026-11-10T07:15:00</StartBoundary>"));
        assert_eq!(
            task_name("acme/weekly-report"),
            "devstribution acme weekly-report"
        );
    }

    #[test]
    fn titles_become_file_names() {
        assert_eq!(file_name("Sales: Q3/Q4 report..."), "Sales Q3 Q4 report");
        assert_eq!(file_name("   "), "");
    }

    #[test]
    fn a_small_png_goes_inside_an_ico() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&64u32.to_be_bytes());
        png.extend_from_slice(&64u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        let ico = png_in_ico(&png).unwrap();
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 1, 0]);
        assert_eq!(ico[6], 64);
        assert_eq!(&ico[22..], &png[..]);
        let mut big = png.clone();
        big[16..20].copy_from_slice(&512u32.to_be_bytes());
        assert!(png_in_ico(&big).is_none());
    }

    #[test]
    fn app_arguments() {
        let args: Vec<String> = ["devstribution.exe", "--scheduled-run", "acme/report"]
            .map(String::from)
            .into();
        assert_eq!(app_args(&args).scheduled.as_deref(), Some("acme/report"));
        assert_eq!(app_args(&args).open, None);
        assert_eq!(app_args(&["x".into(), OPEN_APP.into()]), AppArgs::default());
    }

    #[cfg(windows)]
    #[test]
    fn the_shell_writes_the_shortcut() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("Report.lnk");
        let exe = std::env::current_exe().unwrap();
        windows_shell::create_link(&link, &exe, "--open-app \"acme/report\"", "Report", None)
            .unwrap();
        let bytes = fs::read(&link).unwrap();
        assert_eq!(&bytes[..4], &[0x4C, 0, 0, 0], "a shell link header");
    }

    /// Registers a real task and removes it again. Changes this computer's Task Scheduler for
    /// a moment, so it runs only when asked: `cargo test -- --ignored task_scheduler`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn task_scheduler_takes_the_task() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            root: dir.path().to_path_buf(),
        };
        let id = "devstribution-test/probe";
        set_schedule(&paths, id, "Probe", weekly(7, "03:00")).unwrap();
        assert!(schedule(&paths, id).is_some());
        remove_schedule(&paths, id).unwrap();
        assert!(schedule(&paths, id).is_none());
        assert!(schtasks(&["/Query".as_ref(), "/TN".as_ref(), task_name(id).as_ref()]).is_err());
    }
}
