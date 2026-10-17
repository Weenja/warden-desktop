//! Windows app ids, without Win32: turning paths, registry values and shortcut arguments into
//! the id Warden stores in the block list. Plain string work, so the tests run on any OS.
//!
//! On Windows an app's id is its lowercase exe file name (`steam.exe`). Windows compares file
//! names without case, and the running process and its Start-menu shortcut may spell it
//! differently (`Steam.exe` vs `steam.exe`).

/// The lowercase exe file name of a Windows path, or None when it is not an .exe.
/// `C:\Program Files (x86)\Steam\Steam.exe` -> `steam.exe`.
pub fn exe_id(path: &str) -> Option<String> {
    let name = file_name(path.trim().trim_matches('"')).to_lowercase();
    (name.len() > ".exe".len() && name.ends_with(".exe")).then_some(name)
}

/// The exe file name without `.exe`, case kept: the fallback display name (`Steam`).
pub fn exe_stem(path: &str) -> &str {
    let name = file_name(path.trim().trim_matches('"'));
    match name.len().checked_sub(4) {
        Some(cut) if name.is_char_boundary(cut) && name[cut..].eq_ignore_ascii_case(".exe") => &name[..cut],
        _ => name,
    }
}

/// Windows paths use `\`, but `/` is valid too. std's Path only splits on `\` when built for
/// Windows, and these helpers are also tested on macOS.
fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// The exe in an Uninstall key's `DisplayIcon` value, which is `path[,index]`, sometimes quoted:
/// `"C:\Program Files\App\app.exe",0` -> `C:\Program Files\App\app.exe`.
/// None for icons in .ico/.dll files: those do not name the program.
pub fn display_icon_exe(value: &str) -> Option<String> {
    let value = value.trim();
    let path = match value.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(rest),
        None => strip_icon_index(value),
    };
    exe_id(path)?;
    Some(path.to_string())
}

/// `C:\App\app.exe,-101` -> `C:\App\app.exe`. Only a trailing `,<number>` is an index; a comma
/// inside a folder name stays.
fn strip_icon_index(value: &str) -> &str {
    match value.rsplit_once(',') {
        Some((path, index)) if index.trim().parse::<i32>().is_ok() => path.trim(),
        _ => value,
    }
}

/// Apps installed with Squirrel (Discord, Slack, Teams classic) have Start-menu shortcuts to
/// `Update.exe --processStart "Discord.exe"`; the process that runs, and that Warden sees in
/// front, is `Discord.exe`. Returns that exe's id, or None for any other shortcut.
pub fn squirrel_app(target_id: &str, args: &str) -> Option<String> {
    if target_id != "update.exe" {
        return None;
    }
    let mut words = args.split_whitespace();
    while let Some(word) = words.next() {
        if word.eq_ignore_ascii_case("--processStart") {
            return exe_id(words.next()?);
        }
    }
    None
}

/// Installers, uninstallers and updaters reached from the Start menu or the Uninstall keys.
/// Nobody wants to guard them, so the picker leaves them out.
pub fn is_setup_helper(id: &str) -> bool {
    let stem = id.strip_suffix(".exe").unwrap_or(id);
    stem.starts_with("unins")
        || stem.starts_with("uninst")
        || stem.starts_with("setup")
        || stem.starts_with("install")
        || stem == "update"
}

/// Keep the first item for each id (callers list the best source first), in order.
pub fn dedupe_by_id<T>(items: Vec<T>, id: impl Fn(&T) -> &str) -> Vec<T> {
    let mut seen = std::collections::HashSet::new();
    items.into_iter().filter(|item| seen.insert(id(item).to_string())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_id_is_the_lowercase_file_name() {
        assert_eq!(exe_id(r"C:\Program Files (x86)\Steam\Steam.exe").as_deref(), Some("steam.exe"));
        assert_eq!(exe_id(r#""C:\Apps\Game.EXE""#).as_deref(), Some("game.exe"));
        assert_eq!(exe_id("C:/Apps/tool.exe").as_deref(), Some("tool.exe"));
        assert_eq!(exe_id("discord.exe").as_deref(), Some("discord.exe"));
    }

    #[test]
    fn exe_id_refuses_what_is_not_an_exe() {
        assert_eq!(exe_id(r"C:\Apps\readme.txt"), None);
        assert_eq!(exe_id(r"C:\Apps\icon.ico"), None);
        assert_eq!(exe_id(r"C:\Apps\.exe"), None);
        assert_eq!(exe_id(r"C:\Apps\exe"), None);
        assert_eq!(exe_id(""), None);
    }

    #[test]
    fn exe_stem_keeps_case_and_drops_the_extension() {
        assert_eq!(exe_stem(r"C:\Program Files\Steam\Steam.exe"), "Steam");
        assert_eq!(exe_stem(r"C:\x\Notepad.EXE"), "Notepad");
        assert_eq!(exe_stem(r"C:\x\noext"), "noext");
    }

    #[test]
    fn display_icon_handles_quotes_and_icon_indexes() {
        assert_eq!(
            display_icon_exe(r#""C:\Program Files\App\app.exe",0"#).as_deref(),
            Some(r"C:\Program Files\App\app.exe")
        );
        assert_eq!(display_icon_exe(r"C:\App\app.exe,-101").as_deref(), Some(r"C:\App\app.exe"));
        assert_eq!(display_icon_exe(r"C:\App\app.exe").as_deref(), Some(r"C:\App\app.exe"));
        assert_eq!(display_icon_exe(r"C:\Me, Myself\app.exe").as_deref(), Some(r"C:\Me, Myself\app.exe"));
        assert_eq!(display_icon_exe(r"C:\App\app.ico"), None);
        assert_eq!(display_icon_exe(r"C:\Windows\System32\shell32.dll,4"), None);
    }

    #[test]
    fn squirrel_shortcuts_point_at_the_real_app() {
        assert_eq!(squirrel_app("update.exe", r#"--processStart "Discord.exe""#).as_deref(), Some("discord.exe"));
        assert_eq!(
            squirrel_app("update.exe", "--processStart slack.exe --process-start-args x").as_deref(),
            Some("slack.exe")
        );
        assert_eq!(squirrel_app("update.exe", "--checkForUpdates"), None);
        assert_eq!(squirrel_app("steam.exe", "--processStart other.exe"), None);
    }

    #[test]
    fn setup_helpers_are_recognised() {
        for id in ["unins000.exe", "uninstall.exe", "uninst.exe", "setup.exe", "installer.exe", "update.exe"] {
            assert!(is_setup_helper(id), "{id}");
        }
        for id in ["steam.exe", "discord.exe", "chrome.exe", "updater-game.exe"] {
            assert!(!is_setup_helper(id), "{id}");
        }
    }

    #[test]
    fn dedupe_keeps_the_first_of_each_id_in_order() {
        let items = vec![("steam.exe", "Steam"), ("chrome.exe", "Google Chrome"), ("steam.exe", "steam")];
        assert_eq!(dedupe_by_id(items, |i| i.0), vec![("steam.exe", "Steam"), ("chrome.exe", "Google Chrome")]);
    }
}
