//! macOS implementation of [`Platform`] with AppKit (NSWorkspace, NSRunningApplication).
//!
//! Permissions: none. Reading the frontmost app, asking an app to quit and listing
//! /Applications need no TCC prompt. Warden does not read window titles or screen contents.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AnyThread;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSRunningApplication, NSWorkspace};
use objc2_core_foundation::CFDictionary;
use objc2_core_graphics::CGSessionCopyCurrentDictionary;
use objc2_foundation::{NSBundle, NSDictionary, NSNumber, NSPoint, NSRect, NSSize, NSString};
use objc2_system_configuration::SCDynamicStore;

use super::{info_plist, FrontApp, InstalledApp, Platform, MACOS_BROWSERS};

pub struct MacPlatform;

/// Icons in the picker are drawn at 32 pt; 64 px keeps them sharp on Retina.
const ICON_PX: f64 = 64.0;

impl Platform for MacPlatform {
    fn frontmost_app(&self) -> Option<FrontApp> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let id = app.bundleIdentifier()?.to_string();
        let name = app.localizedName().map(|n| n.to_string()).unwrap_or_else(|| id.clone());
        let is_browser = is_browser(&id, bundle_path(&app).as_deref());
        Some(FrontApp { id, name, is_browser })
    }

    fn terminate_app(&self, id: &str) -> bool {
        let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(id));
        // terminate() is the polite quit (the app may ask to save documents), not a kill.
        // Ask every instance (no short-circuit), then report whether any accepted.
        let asked: Vec<bool> = apps.iter().map(|app| app.terminate()).collect();
        asked.contains(&true)
    }

    fn installed_apps(&self) -> Vec<InstalledApp> {
        let mut apps: Vec<InstalledApp> =
            app_folders().iter().flat_map(|dir| find_app_bundles(dir)).filter_map(|path| read_app(&path)).collect();
        apps.sort_by_key(|a| a.name.to_lowercase());
        apps.dedup_by(|a, b| a.id == b.id);
        apps
    }

    fn screen_locked(&self) -> bool {
        // The login session dictionary has CGSSessionScreenIsLocked = true while locked. The key
        // is absent when unlocked. No public API reports this more directly.
        let Some(dict) = CGSessionCopyCurrentDictionary() else {
            return false;
        };
        // CFDictionary is toll-free bridged to NSDictionary.
        let dict: &NSDictionary<NSString, AnyObject> =
            unsafe { &*(&*dict as *const CFDictionary as *const NSDictionary<NSString, AnyObject>) };
        dict.objectForKey(&NSString::from_str("CGSSessionScreenIsLocked"))
            .and_then(|v| v.downcast::<NSNumber>().ok())
            .is_some_and(|n| n.boolValue())
    }

    fn device_name(&self) -> String {
        // The name from System Settings > General > Sharing ("Sam's MacBook Air").
        // SAFETY: no store (uses a temporary session) and no encoding out-parameter.
        let name = unsafe { SCDynamicStore::computer_name(None, std::ptr::null_mut()) };
        name.map(|n| n.to_string()).unwrap_or_else(|| "Mac".to_string())
    }
}

fn app_folders() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/Applications"), PathBuf::from("/System/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(Path::new(&home).join("Applications"));
    }
    dirs
}

/// `*.app` bundles in `dir` and one level of subfolders (e.g. /Applications/Utilities).
fn find_app_bundles(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if path.extension().is_some_and(|e| e == "app") {
            found.push(path);
        } else if path.is_dir() {
            if let Ok(inner) = std::fs::read_dir(&path) {
                found.extend(inner.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "app")));
            }
        }
    }
    found
}

fn read_app(path: &Path) -> Option<InstalledApp> {
    let path_ns = NSString::from_str(path.to_str()?);
    let bundle = NSBundle::bundleWithPath(&path_ns)?;
    let id = bundle.bundleIdentifier()?.to_string();
    let name = ["CFBundleDisplayName", "CFBundleName"]
        .iter()
        .find_map(|key| bundle.objectForInfoDictionaryKey(&NSString::from_str(key)))
        .and_then(|v| v.downcast::<NSString>().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned());
    let is_browser = is_browser(&id, Some(path));
    Some(InstalledApp { id, name, is_browser, icon_png: icon_png(&path_ns) })
}

/// Whether app `id`, installed at `bundle`, is a web browser: a well-known one, or one whose
/// Info.plist says so. Cached per (id, bundle): the drain loop asks every second. The id is part
/// of the key so that another app swapped in at the same path is checked again.
fn is_browser(id: &str, bundle: Option<&Path>) -> bool {
    static CHECKED: OnceLock<Mutex<HashMap<(String, PathBuf), bool>>> = OnceLock::new();
    if MACOS_BROWSERS.contains(&id) {
        return true;
    }
    let Some(bundle) = bundle else { return false };
    let key = (id.to_string(), bundle.to_path_buf());
    let checked = CHECKED.get_or_init(Default::default);
    if let Some(known) = checked.lock().unwrap().get(&key) {
        return *known;
    }
    let browser = info_plist::is_web_browser(bundle);
    checked.lock().unwrap().insert(key, browser);
    browser
}

/// Where a running app's bundle is (`/Applications/Zen.app`), wherever it was started from.
fn bundle_path(app: &NSRunningApplication) -> Option<PathBuf> {
    Some(PathBuf::from(app.bundleURL()?.path()?.to_string()))
}

fn icon_png(path: &NSString) -> Option<Vec<u8>> {
    let icon = NSWorkspace::sharedWorkspace().iconForFile(path);
    let mut rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(ICON_PX, ICON_PX));
    // SAFETY: rect is a valid NSRect for the call; no context or hints.
    let cg = unsafe { icon.CGImageForProposedRect_context_hints(&mut rect, None, None) }?;
    let rep: Retained<NSBitmapImageRep> = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &cg);
    // SAFETY: an empty properties dictionary is valid for PNG.
    let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
    Some(data.to_vec())
}

/// The parent process id, from the kernel's process info (no permission needed for processes
/// of this user).
pub(super) fn process_parent(pid: u32) -> Option<u32> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    let read = unsafe {
        libc::proc_pidinfo(pid as i32, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast::<std::ffi::c_void>(), size)
    };
    (read == size).then_some(info.pbi_ppid)
}

pub(super) fn process_exe(pid: u32) -> Option<PathBuf> {
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let len = unsafe { libc::proc_pidpath(pid as i32, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if len <= 0 {
        return None;
    }
    buffer.truncate(len as usize);
    Some(PathBuf::from(String::from_utf8(buffer).ok()?))
}

/// The bundle id of process `pid` when it is a browser (None for other apps, helpers and
/// command-line tools).
pub(super) fn process_browser_id(pid: u32) -> Option<String> {
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32)?;
    let id = app.bundleIdentifier()?.to_string();
    is_browser(&id, bundle_path(&app).as_deref()).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_system_apps_with_ids() {
        let apps = MacPlatform.installed_apps();
        let safari = apps.iter().find(|a| a.id == "com.apple.Safari").expect("Safari is installed on every Mac");
        assert_eq!(safari.name, "Safari");
        assert!(safari.icon_png.as_ref().is_some_and(|p| p.starts_with(b"\x89PNG")));
        assert!(safari.is_browser);
        let calculator = apps.iter().find(|a| a.id == "com.apple.calculator").expect("Calculator is on every Mac");
        assert!(!calculator.is_browser);
    }

    #[test]
    fn device_name_is_not_empty() {
        assert!(!MacPlatform.device_name().is_empty());
    }
}
