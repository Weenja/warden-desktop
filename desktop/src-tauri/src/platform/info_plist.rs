//! What an app's Info.plist says about it. Warden reads one thing: whether the app is a web
//! browser, so a browser Warden has never heard of still drains (see "Browsers" in `mod.rs`).
//! Plain file parsing, no AppKit, so the tests need no installed apps.

use std::path::Path;

use serde::Deserialize;

/// The two Info.plist lists that make an app a web browser. Other keys are ignored.
#[derive(Debug, Deserialize)]
struct WebClaims {
    /// Link types the app opens (`CFBundleURLTypes`).
    #[serde(rename = "CFBundleURLTypes", default)]
    url_types: Vec<UrlType>,
    /// File types the app opens (`CFBundleDocumentTypes`).
    #[serde(rename = "CFBundleDocumentTypes", default)]
    document_types: Vec<DocumentType>,
}

#[derive(Debug, Deserialize)]
struct UrlType {
    #[serde(rename = "CFBundleURLSchemes", default)]
    schemes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DocumentType {
    /// Type ids, e.g. `public.html` (Chrome).
    #[serde(rename = "LSItemContentTypes", default)]
    content_types: Vec<String>,
    /// File extensions, e.g. `html` (Safari, Firefox, Zen).
    #[serde(rename = "CFBundleTypeExtensions", default)]
    extensions: Vec<String>,
}

/// True when the app bundle at `bundle` (`/Applications/Zen.app`) says it is a web browser:
/// it opens `http` and `https` links AND HTML files, as Safari, Chrome, Firefox and its forks
/// all declare. Links alone are not enough: some apps take web links only to show their own
/// pages (the ChatGPT app lists `http` and `https`), and counting those as browsers would drain
/// them the whole time they are in front.
pub fn is_web_browser(bundle: &Path) -> bool {
    plist::from_file::<_, WebClaims>(bundle.join("Contents/Info.plist")).is_ok_and(|info| info.is_web_browser())
}

impl WebClaims {
    fn is_web_browser(&self) -> bool {
        let opens_link =
            |scheme: &str| self.url_types.iter().flat_map(|t| &t.schemes).any(|s| s.eq_ignore_ascii_case(scheme));
        let opens_html = self.document_types.iter().any(|d| {
            d.content_types.iter().any(|t| t == "public.html")
                || d.extensions.iter().any(|e| e.eq_ignore_ascii_case("html"))
        });
        opens_link("http") && opens_link("https") && opens_html
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An Info.plist with these CFBundleURLTypes and CFBundleDocumentTypes entries.
    fn claims(url_types: &str, document_types: &str) -> WebClaims {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <plist version="1.0"><dict>
              <key>CFBundleIdentifier</key><string>test.app</string>
              <key>CFBundleURLTypes</key><array>{url_types}</array>
              <key>CFBundleDocumentTypes</key><array>{document_types}</array>
            </dict></plist>"#
        );
        plist::from_bytes(xml.as_bytes()).unwrap()
    }

    fn schemes(list: &[&str]) -> String {
        let items: String = list.iter().map(|s| format!("<string>{s}</string>")).collect();
        format!("<dict><key>CFBundleURLSchemes</key><array>{items}</array></dict>")
    }

    const HTML_BY_TYPE: &str = "<dict><key>LSItemContentTypes</key><array><string>public.html</string></array></dict>";
    const HTML_BY_EXTENSION: &str =
        "<dict><key>CFBundleTypeExtensions</key><array><string>htm</string><string>html</string></array></dict>";

    #[test]
    fn browsers_open_web_links_and_html_files() {
        // Chrome: one entry with both schemes, HTML by type id.
        assert!(claims(&schemes(&["http", "https"]), HTML_BY_TYPE).is_web_browser());
        // Zen and Safari: one entry per scheme, HTML by extension.
        let split = schemes(&["http"]) + &schemes(&["https"]);
        assert!(claims(&split, HTML_BY_EXTENSION).is_web_browser());
    }

    #[test]
    fn other_apps_are_not_browsers() {
        // ChatGPT: web links, no HTML files.
        assert!(!claims(&schemes(&["codex", "http", "https"]), "").is_web_browser());
        // An HTML editor: HTML files, no web links.
        assert!(!claims(&schemes(&["myeditor"]), HTML_BY_TYPE).is_web_browser());
        // Only one of the two web schemes.
        assert!(!claims(&schemes(&["http"]), HTML_BY_TYPE).is_web_browser());
    }

    #[test]
    fn reads_real_bundles() {
        assert!(is_web_browser(Path::new("/Applications/Safari.app")));
        assert!(!is_web_browser(Path::new("/System/Applications/Calculator.app")));
        assert!(!is_web_browser(Path::new("/nowhere/Missing.app")));
    }
}
