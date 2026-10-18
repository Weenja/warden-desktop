//! The guarded apps picker's list: installed apps, common picks first, current picks on.
//! The list is built and shown locally; only the apps the user turns on are sent.

use serde::Serialize;

use crate::api::GuardedItem;
use crate::sync::AppEntry;

/// Apps people most often guard (games, chat, video), shown on top when installed. macOS bundle
/// ids and Windows exe names; matched without case.
const COMMON_PICKS: &[&str] = &[
    "com.valvesoftware.steam",
    "com.hnc.Discord",
    "com.epicgames.EpicGamesLauncher",
    "net.battle.app",
    "net.battle.bootstrapper",
    "com.riotgames.RiotGames.RiotClient",
    "com.roblox.RobloxPlayer",
    "com.mojang.minecraftlauncher",
    "com.spotify.client",
    "com.netflix.Netflix",
    "com.amazon.aiv.AIVApp",
    "com.apple.TV",
    "net.whatsapp.WhatsApp",
    "ru.keepcoder.Telegram",
    "steam.exe",
    "discord.exe",
    "epicgameslauncher.exe",
    "battle.net.exe",
    "riotclientservices.exe",
    "robloxplayerbeta.exe",
    "minecraft.exe",
    "spotify.exe",
    "whatsapp.exe",
    "telegram.exe",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PickerApp {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub guarded: bool,
    pub common: bool,
}

/// Common picks first, then everything else, each by name. Apps guarded but not found on disk
/// (moved, or listed under another folder) stay in the list, switched on, so saving the
/// picker never drops them silently. Warden itself is left out.
pub fn picker_apps(installed: &[AppEntry], guarded: &[GuardedItem], own_id: &str) -> Vec<PickerApp> {
    let is_common = |id: &str| COMMON_PICKS.iter().any(|c| c.eq_ignore_ascii_case(id));
    let mut apps: Vec<PickerApp> = installed
        .iter()
        .filter(|a| a.id != own_id)
        .map(|a| PickerApp {
            id: a.id.clone(),
            name: a.name.clone(),
            icon: a.icon.clone(),
            guarded: guarded.iter().any(|g| g.token == a.id),
            common: is_common(&a.id),
        })
        .collect();
    for g in guarded.iter().filter(|g| !installed.iter().any(|a| a.id == g.token)) {
        apps.push(PickerApp {
            id: g.token.clone(),
            name: g.label.clone(),
            icon: None,
            guarded: true,
            common: is_common(&g.token),
        });
    }
    apps.sort_by_key(|a| (!a.common, a.name.to_lowercase()));
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str) -> AppEntry {
        AppEntry { id: id.into(), name: name.into(), icon: None, is_browser: false }
    }

    #[test]
    fn common_first_guarded_kept_self_hidden() {
        let installed = vec![
            app("com.apple.Music", "Music"),
            app("com.hnc.Discord", "Discord"),
            app("in.weenja.warden.desktop", "Warden"),
            app("com.valvesoftware.steam", "Steam"),
            app("com.figma.Desktop", "Figma"),
        ];
        let guarded = vec![
            GuardedItem { token: "com.valvesoftware.steam".into(), label: "Steam".into() },
            GuardedItem { token: "com.example.gone".into(), label: "Gone".into() },
        ];
        let list = picker_apps(&installed, &guarded, "in.weenja.warden.desktop");
        let names: Vec<&str> = list.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Discord", "Steam", "Figma", "Gone", "Music"]);
        assert!(list[1].guarded && !list[0].guarded);
        assert!(list.iter().find(|a| a.name == "Gone").unwrap().guarded);
    }
}
