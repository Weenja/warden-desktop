//! macOS implementation of [`Platform`] with AppKit (NSWorkspace, NSRunningApplication).
//!
//! Permissions: none. Reading the frontmost app, asking an app to quit and listing
//! /Applications need no TCC prompt. Warden does not read window titles or screen contents.

use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AnyThread;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSRunningApplication, NSWorkspace};
use objc2_core_foundation::CFDictionary;
use objc2_core_graphics::CGSessionCopyCurrentDictionary;
use objc2_foundation::{NSBundle, NSDictionary, NSNumber, NSPoint, NSRect, NSSize, NSString};
use objc2_system_configuration::SCDynamicStore;

use super::{FrontApp, InstalledApp, Platform};

pub struct MacPlatform;

/// Icons in the picker are drawn at 32 pt; 64 px keeps them sharp on Retina.
const ICON_PX: f64 = 64.0;

impl Platform for MacPlatform {
    fn frontmost_app(&self) -> Option<FrontApp> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let id = app.bundleIdentifier()?.to_string();
        let name = app.localizedName().map(|n| n.to_string()).unwrap_or_else(|| id.clone());
        Some(FrontApp { id, name })
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
    Some(InstalledApp { id, name, icon_png: icon_png(&path_ns) })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_system_apps_with_ids() {
        let apps = MacPlatform.installed_apps();
        let safari = apps.iter().find(|a| a.id == "com.apple.Safari").expect("Safari is installed on every Mac");
        assert_eq!(safari.name, "Safari");
        assert!(safari.icon_png.as_ref().is_some_and(|p| p.starts_with(b"\x89PNG")));
    }

    #[test]
    fn device_name_is_not_empty() {
        assert!(!MacPlatform.device_name().is_empty());
    }
}
