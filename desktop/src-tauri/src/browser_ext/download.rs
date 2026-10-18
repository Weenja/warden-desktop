//! Where "Get extension" next to a browser goes.
//!
//! Every browser gets a short link on getwarden.app (`/chrome`, `/firefox`, ...). The site
//! forwards it to the right store page or file, so a store move needs no app update. The app
//! opens only these fixed links; the UI picks a browser, never a URL.

/// The live site, also in staging builds: the extension pages are the same for everyone.
const SITE: &str = "https://getwarden.app";

/// The extension page for the browser with this app id (bundle id on macOS, exe name on
/// Windows). Firefox and its forks use the Firefox add-on, Safari its own, Edge its own store,
/// and every other browser is Chromium-based and uses the Chrome extension.
pub fn extension_page(browser_id: &str) -> String {
    let id = browser_id.to_ascii_lowercase();
    let page = if id.starts_with("com.apple.safari") {
        "safari"
    } else if id.starts_with("com.microsoft.edge") || id == "msedge.exe" {
        "edge"
    } else if is_firefox_family(&id) {
        "firefox"
    } else {
        "chrome"
    };
    format!("{SITE}/{page}")
}

/// Firefox (all channels) and the Firefox forks that load Firefox add-ons.
fn is_firefox_family(id: &str) -> bool {
    const MARKERS: &[&str] = &["org.mozilla.", "firefox", "zen-browser", "librewolf", "waterfox", "floorp"];
    id == "zen.exe" || MARKERS.iter().any(|m| id.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_browser_gets_its_own_store() {
        let page = |id| extension_page(id).trim_start_matches("https://getwarden.app/").to_string();
        assert_eq!(page("com.apple.Safari"), "safari");
        assert_eq!(page("com.apple.SafariTechnologyPreview"), "safari");
        assert_eq!(page("com.microsoft.edgemac"), "edge");
        assert_eq!(page("msedge.exe"), "edge");
        assert_eq!(page("org.mozilla.firefox"), "firefox");
        assert_eq!(page("org.mozilla.nightly"), "firefox");
        assert_eq!(page("app.zen-browser.zen"), "firefox");
        assert_eq!(page("zen.exe"), "firefox");
        assert_eq!(page("librewolf.exe"), "firefox");
        assert_eq!(page("com.google.Chrome"), "chrome");
        assert_eq!(page("company.thebrowser.Browser"), "chrome");
        assert_eq!(page("brave.exe"), "chrome");
        assert_eq!(page("com.operasoftware.Opera"), "chrome");
    }
}
