//! Windows implementation of [`Platform`] with Win32.
//!
//! Permissions: none. Finding the window in front, the exe of a process the user runs, the
//! Start menu and the Uninstall keys needs no admin rights and no prompt. Warden does not read
//! window titles or screen contents.

mod apps;
mod icon;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::core::{BOOL, HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_TIMEOUT, WPARAM};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::RemoteDesktop::{
    WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfoEx, WTSINFOEXW, WTS_CURRENT_SERVER_HANDLE,
    WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK,
};
use windows::Win32::System::SystemInformation::{ComputerNamePhysicalDnsHostname, GetComputerNameExW};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
    PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetForegroundWindow, GetWindow, GetWindowThreadProcessId, IsWindowVisible,
    PostMessageW, GW_OWNER, WM_CLOSE,
};

use super::exe_names::{exe_id, exe_stem};
use super::{FrontApp, InstalledApp, Platform, KNOWN_BROWSERS};

pub struct WindowsPlatform;

/// How long an app gets to close its windows (and ask about unsaved work) before Warden ends it.
const POLITE_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Store apps (UWP) run inside this frame process; the real app owns a child window.
const UWP_FRAME_HOST: &str = "applicationframehost.exe";

impl Platform for WindowsPlatform {
    fn frontmost_app(&self) -> Option<FrontApp> {
        let window = unsafe { GetForegroundWindow() };
        if window.is_invalid() {
            return None;
        }
        let mut pid = window_pid(window);
        let mut path = process_path(pid)?;
        if exe_id(&path).as_deref() == Some(UWP_FRAME_HOST) {
            pid = hosted_app_pid(window, pid)?;
            path = process_path(pid)?;
        }
        // Warden's own popover or shield in front is not an app the user is spending time in.
        if pid == unsafe { GetCurrentProcessId() } {
            return None;
        }
        Some(FrontApp { id: exe_id(&path)?, name: app_name(&path) })
    }

    fn terminate_app(&self, id: &str) -> bool {
        let own = unsafe { GetCurrentProcessId() };
        let pids: Vec<u32> = process_ids(id).into_iter().filter(|pid| *pid != own).collect();
        if pids.is_empty() {
            return false;
        }
        // Open the processes before asking, so a process id reused after the app exits can
        // never point the force-close at another program.
        let processes: Vec<Handle> =
            pids.iter().filter_map(|pid| Handle::process(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, *pid)).collect();
        close_windows_of(&pids);
        // Browsers hold many tabs and only drain through the whole-browser rule, so they are
        // only asked. Guarded apps are ended if they ignore the request: at 0 points the app
        // must close, and a "Save changes?" prompt would otherwise keep it open for free.
        if !KNOWN_BROWSERS.contains(&id) {
            std::thread::spawn(move || end_after_timeout(processes));
        }
        true
    }

    fn installed_apps(&self) -> Vec<InstalledApp> {
        apps::installed_apps()
    }

    fn screen_locked(&self) -> bool {
        session_locked().unwrap_or(false)
    }

    fn device_name(&self) -> String {
        computer_name().unwrap_or_else(|| "PC".to_string())
    }
}

/// An open kernel handle (a process or a process snapshot), closed on drop.
struct Handle(HANDLE);

// Kernel handles are valid on any thread of the process that opened them.
unsafe impl Send for Handle {}

impl Handle {
    fn process(access: PROCESS_ACCESS_RIGHTS, pid: u32) -> Option<Handle> {
        // Fails for elevated or protected processes when Warden runs as a normal user; those
        // are skipped (Warden never asks for admin rights).
        unsafe { OpenProcess(access, false, pid) }.ok().map(Handle)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn window_pid(window: HWND) -> u32 {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    pid
}

/// Full path of a process's exe. Limited query rights work for any process of this user.
fn process_path(pid: u32) -> Option<String> {
    let process = Handle::process(PROCESS_QUERY_LIMITED_INFORMATION, pid)?;
    // Long paths can reach 32 767 characters.
    let mut buffer = vec![0u16; 32_768];
    let mut len = buffer.len() as u32;
    unsafe { QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut len) }.ok()?;
    Some(String::from_utf16_lossy(&buffer[..len as usize]))
}

/// The pid of the Store app inside an ApplicationFrameHost window: the first child window
/// that belongs to another process.
fn hosted_app_pid(frame: HWND, host_pid: u32) -> Option<u32> {
    struct Search {
        host: u32,
        found: u32,
    }
    unsafe extern "system" fn visit(child: HWND, lparam: LPARAM) -> BOOL {
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let pid = window_pid(child);
        if pid != 0 && pid != search.host {
            search.found = pid;
            return false.into();
        }
        true.into()
    }
    let mut search = Search { host: host_pid, found: 0 };
    let _ = unsafe { EnumChildWindows(Some(frame), Some(visit), LPARAM(&mut search as *mut Search as isize)) };
    (search.found != 0).then_some(search.found)
}

/// The app's display name: the exe's FileDescription ("Google Chrome"), else the exe name.
/// Cached per path: the drain loop asks every second and the version block never changes
/// while the exe runs.
fn app_name(path: &str) -> String {
    static NAMES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let names = NAMES.get_or_init(Default::default);
    if let Some(name) = names.lock().unwrap().get(path) {
        return name.clone();
    }
    let name = file_description(path).unwrap_or_else(|| exe_stem(path).to_string());
    names.lock().unwrap().insert(path.to_string(), name.clone());
    name
}

/// FileDescription from the exe's version resource, in the first language the file lists.
pub(super) fn file_description(path: &str) -> Option<String> {
    let path = HSTRING::from(path);
    let size = unsafe { GetFileVersionInfoSizeW(&path, None) };
    if size == 0 {
        return None;
    }
    let mut block = vec![0u8; size as usize];
    unsafe { GetFileVersionInfoW(&path, None, size, block.as_mut_ptr().cast()) }.ok()?;

    // The translation table lists (language, code page) pairs as two u16 each. US English
    // Unicode is the common default when a file has none.
    let (language, code_page) = version_value(&block, r"\VarFileInfo\Translation")
        .filter(|words| words.len() >= 2)
        .map(|words| (words[0], words[1]))
        .unwrap_or((0x0409, 0x04B0));
    let key = format!(r"\StringFileInfo\{language:04x}{code_page:04x}\FileDescription");
    let text = String::from_utf16_lossy(&version_value(&block, &key)?);
    let text = text.trim_end_matches('\0').trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// A value from a version block as u16 words.
fn version_value(block: &[u8], key: &str) -> Option<Vec<u16>> {
    let mut value = std::ptr::null_mut();
    let mut len = 0u32;
    let found = unsafe { VerQueryValueW(block.as_ptr().cast(), &HSTRING::from(key), &mut value, &mut len) };
    if !found.as_bool() || value.is_null() || len == 0 {
        return None;
    }
    // `len` counts characters for strings and bytes for binary values (the translation table).
    let words = if key.starts_with(r"\StringFileInfo") { len as usize } else { len as usize / 2 };
    // The value points into `block`, which outlives this copy.
    Some(unsafe { std::slice::from_raw_parts(value as *const u16, words) }.to_vec())
}

/// Ids of the running processes whose exe file name is `id` (any case).
fn process_ids(id: &str) -> Vec<u32> {
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return Vec::new();
    };
    let snapshot = Handle(snapshot);
    let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut pids = Vec::new();
    let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) }.is_ok();
    while more {
        let len = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
        if exe_id(&String::from_utf16_lossy(&entry.szExeFile[..len])).as_deref() == Some(id) {
            pids.push(entry.th32ProcessID);
        }
        more = unsafe { Process32NextW(snapshot.0, &mut entry) }.is_ok();
    }
    pids
}

/// Post WM_CLOSE to the visible, unowned top-level windows of these processes: the same as
/// clicking their X, so the app can save or ask first.
fn close_windows_of(pids: &[u32]) {
    unsafe extern "system" fn visit(window: HWND, lparam: LPARAM) -> BOOL {
        let pids = unsafe { &*(lparam.0 as *const Vec<u32>) };
        let is_main_window =
            unsafe { IsWindowVisible(window) }.as_bool() && unsafe { GetWindow(window, GW_OWNER) }.is_err();
        if is_main_window && pids.contains(&window_pid(window)) {
            let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
        true.into()
    }
    let pids = pids.to_vec();
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&pids as *const Vec<u32> as isize)) };
}

/// Wait for the processes to exit; end the ones still running when the time is up.
fn end_after_timeout(processes: Vec<Handle>) {
    let deadline = Instant::now() + POLITE_CLOSE_TIMEOUT;
    for process in processes {
        let left = deadline.saturating_duration_since(Instant::now()).as_millis() as u32;
        if unsafe { WaitForSingleObject(process.0, left) } == WAIT_TIMEOUT {
            if let Err(e) = unsafe { TerminateProcess(process.0, 1) } {
                log::warn!("could not end a guarded app: {e}");
            }
        }
    }
}

/// Whether this session's desktop is locked, from Remote Desktop Services session info (works
/// for local sessions too). The lock flag is reliable on Windows 10 and later.
fn session_locked() -> Option<bool> {
    let mut buffer = PWSTR::null();
    let mut bytes = 0u32;
    unsafe {
        WTSQuerySessionInformationW(
            Some(WTS_CURRENT_SERVER_HANDLE),
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut bytes,
        )
    }
    .ok()?;
    let locked = (bytes as usize >= size_of::<WTSINFOEXW>()).then(|| {
        let info = unsafe { &*(buffer.0 as *const WTSINFOEXW) };
        info.Level == 1 && unsafe { info.Data.WTSInfoExLevel1.SessionFlags } == WTS_SESSIONSTATE_LOCK as i32
    });
    unsafe { WTSFreeMemory(buffer.0.cast()) };
    locked
}

/// The computer's name as set in Settings > System > About (keeps its case, unlike the
/// 15-character NetBIOS name).
fn computer_name() -> Option<String> {
    let mut len = 0u32;
    // The first call fails with ERROR_MORE_DATA and reports the size, null included.
    let _ = unsafe { GetComputerNameExW(ComputerNamePhysicalDnsHostname, None, &mut len) };
    let mut buffer = vec![0u16; len.max(1) as usize];
    unsafe { GetComputerNameExW(ComputerNamePhysicalDnsHostname, Some(PWSTR(buffer.as_mut_ptr())), &mut len) }.ok()?;
    let name = String::from_utf16_lossy(&buffer[..len as usize]);
    (!name.is_empty()).then_some(name)
}
