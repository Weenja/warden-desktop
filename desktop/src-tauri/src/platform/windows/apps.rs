//! Installed apps for the picker: the Start menu (all users + this user), then the programs
//! listed in Settings > Apps (the Uninstall registry keys). Read only; nothing is changed.

use std::path::{Path, PathBuf};

use windows::core::{Interface, HSTRING};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::UI::Shell::{
    FOLDERID_CommonPrograms, FOLDERID_Programs, IShellLinkW, SHGetKnownFolderPath, ShellLink, KF_FLAG_DEFAULT,
};
use windows_registry::{Key, CURRENT_USER, LOCAL_MACHINE};

use super::icon::exe_icon_png;
use crate::platform::exe_names::{dedupe_by_id, display_icon_exe, exe_id, is_setup_helper, squirrel_app};
use crate::platform::InstalledApp;

/// An app found on disk, before its icon is read.
struct Found {
    id: String,
    name: String,
    /// Where the picker icon comes from: a file (exe, dll or ico) and the icon's index in it.
    icon: (String, i32),
}

/// The 64-bit view, the 32-bit view on 64-bit Windows, and per-user installs.
const UNINSTALL_KEYS: [(&Key, &str); 3] = [
    (LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    (LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
    (CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
];

pub fn installed_apps() -> Vec<InstalledApp> {
    let _com = ComApartment::enter();
    let own_id = std::env::current_exe().ok().and_then(|p| exe_id(&p.to_string_lossy()));

    // Start-menu entries first: their names are the ones people know ("Steam", not "steam").
    let mut found = start_menu_apps();
    found.extend(uninstall_key_apps());
    found.retain(|app| !is_setup_helper(&app.id) && Some(&app.id) != own_id.as_ref());
    let mut found = dedupe_by_id(found, |app| &app.id);
    found.sort_by_key(|app| app.name.to_lowercase());

    found
        .into_iter()
        .map(|app| InstalledApp { icon_png: exe_icon_png(&app.icon.0, app.icon.1), id: app.id, name: app.name })
        .collect()
}

/// COM for this thread while reading shortcuts. Picker scans run on a worker thread that may
/// be reused, so COM is left again when done.
struct ComApartment {
    entered: bool,
}

impl ComApartment {
    fn enter() -> ComApartment {
        // S_FALSE (already entered) also needs a matching CoUninitialize. RPC_E_CHANGED_MODE
        // means the thread is already in the other apartment type, which works for shortcuts.
        let entered = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        ComApartment { entered }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.entered {
            unsafe { CoUninitialize() };
        }
    }
}

fn start_menu_apps() -> Vec<Found> {
    let Ok(shell_link) = (unsafe { CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) }) else {
        return Vec::new();
    };
    [FOLDERID_CommonPrograms, FOLDERID_Programs]
        .iter()
        .filter_map(known_folder)
        .flat_map(|dir| shortcuts_in(&dir))
        .filter_map(|shortcut| resolve_shortcut(&shell_link, &shortcut))
        .collect()
}

fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    let path = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok()?;
    let text = unsafe { path.to_string() }.ok();
    // The shell allocates the string; we free it.
    unsafe { CoTaskMemFree(Some(path.0 as *const _)) };
    text.map(PathBuf::from)
}

/// Every .lnk file under `dir`, subfolders included.
fn shortcuts_in(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("lnk")) {
                found.push(path);
            }
        }
    }
    found
}

/// The exe a shortcut starts, named after the shortcut. None for shortcuts to documents,
/// folders or web pages, and for Windows Installer "advertised" shortcuts with no plain target.
fn resolve_shortcut(shell_link: &IShellLinkW, shortcut: &Path) -> Option<Found> {
    let file: IPersistFile = shell_link.cast().ok()?;
    unsafe { file.Load(&HSTRING::from(shortcut.as_os_str()), STGM_READ) }.ok()?;

    let mut target = [0u16; 32_768];
    unsafe { shell_link.GetPath(&mut target, std::ptr::null_mut(), 0) }.ok()?;
    let target = from_wide(&target);
    let mut id = exe_id(&target)?;

    let mut args = [0u16; 4096];
    if unsafe { shell_link.GetArguments(&mut args) }.is_ok() {
        if let Some(app_id) = squirrel_app(&id, &from_wide(&args)) {
            id = app_id;
        }
    }
    let name = shortcut.file_stem()?.to_string_lossy().into_owned();
    Some(Found { id, name, icon: shortcut_icon(shell_link).unwrap_or((target, 0)) })
}

/// The icon the shortcut shows in the Start menu, when it sets one (Squirrel apps point it
/// at the real app instead of Update.exe).
fn shortcut_icon(shell_link: &IShellLinkW) -> Option<(String, i32)> {
    let mut path = [0u16; 32_768];
    let mut index = 0;
    unsafe { shell_link.GetIconLocation(&mut path, &mut index) }.ok()?;
    let path = expand_env(&from_wide(&path))?;
    (!path.is_empty()).then_some((path, index))
}

fn uninstall_key_apps() -> Vec<Found> {
    let mut found = Vec::new();
    for (root, path) in UNINSTALL_KEYS {
        let Ok(list) = root.open(path) else { continue };
        let Ok(names) = list.keys() else { continue };
        for name in names {
            if let Some(app) = list.open(&name).ok().and_then(|key| read_uninstall_entry(&key)) {
                found.push(app);
            }
        }
    }
    found
}

/// One Uninstall entry, when it is a visible program whose icon is its exe.
fn read_uninstall_entry(key: &Key) -> Option<Found> {
    // Updates and parts of other programs are hidden from Settings > Apps; hide them here too.
    if key.get_u32("SystemComponent").is_ok_and(|v| v == 1) {
        return None;
    }
    let name = key.get_hstring("DisplayName").ok()?.to_string_lossy();
    let icon = key.get_hstring("DisplayIcon").ok()?.to_string_lossy();
    let exe = expand_env(&display_icon_exe(&icon)?)?;
    if !Path::new(&exe).exists() {
        return None;
    }
    Some(Found { id: exe_id(&exe)?, name, icon: (exe, 0) })
}

/// `%ProgramFiles%\App\app.exe` -> `C:\Program Files\App\app.exe` (DisplayIcon may use
/// environment variables).
fn expand_env(text: &str) -> Option<String> {
    let source = HSTRING::from(text);
    let len = unsafe { ExpandEnvironmentStringsW(&source, None) };
    if len == 0 {
        return None;
    }
    let mut buffer = vec![0u16; len as usize];
    let written = unsafe { ExpandEnvironmentStringsW(&source, Some(&mut buffer)) };
    (written != 0 && written <= len).then(|| from_wide(&buffer))
}

/// A null-terminated UTF-16 buffer as a String.
fn from_wide(buffer: &[u16]) -> String {
    let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}
