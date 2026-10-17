//! The app's side of the browser link: one [`Link`] per connected extension, what each last
//! reported, and the `state` message each one is sent. No sockets and no OS here (that is
//! `server.rs`), so the staging test drives this with a fake extension.
//!
//! Messages (JSON), extension -> app:
//! - `{"type":"hello","ext_version":"0.1.0"}` first.
//! - `{"type":"tab","site":"m.youtube.com"|null,"idle":false}` on every change.
//! - `{"type":"show_app"}` the user pressed "Open Warden".
//!
//! App -> extension:
//! - `{"type":"hello","app_version":"…"}` the reply.
//! - `{"type":"state",…}` right after hello and on every change (see [`Facts`]).
//! - `{"type":"close_tab","site":"youtube.com"}` Stop on the pill, or locked on a guarded site.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::config::APP_VERSION;

/// What the extension last said about the browser's active tab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabReport {
    /// Lowercase host of the active tab in the browser's last focused window; None for pages
    /// that are not a web site (new tab, settings, Warden's own blocked page).
    pub site: Option<String>,
    /// Screen locked, or no input for 5 minutes and the tab plays no sound.
    pub idle: bool,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FromExtension {
    Hello,
    Tab {
        site: Option<String>,
        #[serde(default)]
        idle: bool,
    },
    ShowApp,
}

/// What every linked extension is told, from the app's current state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    pub signed_in: bool,
    pub points: i64,
    pub low: bool,
    /// Guarded sites are blocked: 0 points, the balance token expired offline, or the daily max.
    pub locked: bool,
    pub guarded_sites: Vec<String>,
    pub draining: Option<Draining>,
    pub device_name: Option<String>,
}

/// A guarded site draining in one browser. Only that browser's extension hears about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Draining {
    pub browser: String,
    pub site: String,
    pub per_min: i64,
}

/// Something the app's UI should react to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkEvent {
    /// An extension linked or left: the popover's browser rows change.
    Changed,
    /// "Open Warden" in the extension: show the sign-in window or the popover.
    ShowApp,
}

pub type LinkId = u64;

struct Link {
    id: LinkId,
    /// The browser that started the host (bundle id / exe name). None when no known browser
    /// started it: such a link gets answers but never changes what drains.
    browser: Option<String>,
    hello: bool,
    tab: Option<(TabReport, Instant)>,
    outbox: UnboundedSender<Value>,
    /// The last `state` sent, so unchanged state is not sent again every second.
    last_state: Option<Value>,
}

#[derive(Default)]
struct Inner {
    next_id: LinkId,
    links: Vec<Link>,
    facts: Facts,
}

pub struct Links {
    inner: Mutex<Inner>,
    events: UnboundedSender<LinkEvent>,
}

impl Links {
    pub fn new() -> (Arc<Links>, UnboundedReceiver<LinkEvent>) {
        let (events, receiver) = unbounded_channel();
        (Arc::new(Links { inner: Mutex::new(Inner::default()), events }), receiver)
    }

    /// A host connected. Returns its id and the messages to send it.
    pub fn open(&self, browser: Option<String>) -> (LinkId, UnboundedReceiver<Value>) {
        let (outbox, receiver) = unbounded_channel();
        let mut inner = self.inner.lock().unwrap();
        inner.next_id += 1;
        let id = inner.next_id;
        inner.links.push(Link { id, browser, hello: false, tab: None, outbox, last_state: None });
        (id, receiver)
    }

    /// The host's connection ended (browser closed, extension reloaded).
    pub fn close(&self, id: LinkId) {
        let mut inner = self.inner.lock().unwrap();
        let was_linked = inner.links.iter().any(|l| l.id == id && l.hello);
        inner.links.retain(|l| l.id != id);
        drop(inner);
        if was_linked {
            let _ = self.events.send(LinkEvent::Changed);
        }
    }

    /// One message from an extension. Unknown or malformed messages are ignored.
    pub fn receive(&self, id: LinkId, body: &[u8]) {
        let Ok(message) = serde_json::from_slice::<FromExtension>(body) else {
            log::warn!("browser link: ignored a message it does not know");
            return;
        };
        let mut inner = self.inner.lock().unwrap();
        let facts = inner.facts.clone();
        let Some(link) = inner.links.iter_mut().find(|l| l.id == id) else {
            return;
        };
        let event = match message {
            FromExtension::Hello => {
                link.hello = true;
                let _ = link.outbox.send(json!({ "type": "hello", "app_version": APP_VERSION }));
                link.last_state = None;
                send_state(link, &facts);
                Some(LinkEvent::Changed)
            }
            // Nothing counts before hello: a host that never says hello is not an extension.
            FromExtension::Tab { .. } if !link.hello => None,
            FromExtension::Tab { site, idle } => {
                link.tab = Some((TabReport { site: site.as_deref().and_then(clean_host), idle }, Instant::now()));
                None
            }
            FromExtension::ShowApp => Some(LinkEvent::ShowApp),
        };
        drop(inner);
        if let Some(event) = event {
            let _ = self.events.send(event);
        }
    }

    /// What the extension in `browser` reports, or None when that browser has no linked
    /// extension (then the whole browser drains). With several links for one browser (one per
    /// Chrome profile) the most recent report wins: it comes from the window the user just used.
    pub fn tab_for(&self, browser: &str) -> Option<TabReport> {
        let inner = self.inner.lock().unwrap();
        let mut linked = inner.links.iter().filter(|l| l.hello && l.browser.as_deref() == Some(browser)).peekable();
        linked.peek()?;
        let latest = linked.filter_map(|l| l.tab.as_ref()).max_by_key(|(_, at)| *at);
        // Linked but no tab report yet: nothing known to be open, so nothing drains.
        Some(latest.map(|(tab, _)| tab.clone()).unwrap_or_default())
    }

    /// Browsers with a linked extension, for the popover.
    pub fn linked_browsers(&self) -> Vec<String> {
        let inner = self.inner.lock().unwrap();
        let mut browsers: Vec<String> =
            inner.links.iter().filter(|l| l.hello).filter_map(|l| l.browser.clone()).collect();
        browsers.dedup();
        browsers
    }

    /// The app's state changed: tell every linked extension what differs for it.
    pub fn publish(&self, facts: Facts) {
        let mut inner = self.inner.lock().unwrap();
        inner.facts = facts.clone();
        for link in inner.links.iter_mut().filter(|l| l.hello) {
            send_state(link, &facts);
        }
    }

    /// Ask the extension in `browser` to close its tab on `site` (a guarded site).
    pub fn close_tab(&self, browser: &str, site: &str) {
        let inner = self.inner.lock().unwrap();
        for link in inner.links.iter().filter(|l| l.hello && l.browser.as_deref() == Some(browser)) {
            let _ = link.outbox.send(json!({ "type": "close_tab", "site": site }));
        }
    }
}

fn send_state(link: &mut Link, facts: &Facts) {
    let draining = facts
        .draining
        .as_ref()
        .filter(|d| link.browser.as_deref() == Some(d.browser.as_str()))
        .map(|d| json!({ "site": d.site, "per_min": d.per_min }));
    let state = json!({
        "type": "state",
        "signed_in": facts.signed_in,
        "points": facts.points,
        "low": facts.low,
        "locked": facts.locked,
        "guarded_sites": facts.guarded_sites,
        "draining": draining,
        "device_name": facts.device_name,
    });
    if link.last_state.as_ref() != Some(&state) {
        let _ = link.outbox.send(state.clone());
        link.last_state = Some(state);
    }
}

/// A host name as Warden compares it: lowercase, no trailing dot, only the characters a host
/// name can have. Anything else (an IP literal with a port, garbage) counts as no site.
pub fn clean_host(raw: &str) -> Option<String> {
    let host = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    let valid = !host.is_empty()
        && host.len() <= 253
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    valid.then_some(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(rx: &mut UnboundedReceiver<Value>) -> Vec<Value> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    #[test]
    fn hello_gets_hello_and_state_then_only_changes() {
        let (links, mut events) = Links::new();
        links.publish(Facts { signed_in: true, points: 42, ..Default::default() });
        let (id, mut rx) = links.open(Some("com.google.Chrome".into()));
        assert_eq!(links.tab_for("com.google.Chrome"), None, "no hello yet: not linked");
        links.receive(id, br#"{"type":"hello","ext_version":"0.1.0"}"#);
        let sent = drain(&mut rx);
        assert_eq!(sent[0]["type"], "hello");
        assert_eq!((sent[1]["type"].as_str(), sent[1]["points"].as_i64()), (Some("state"), Some(42)));
        assert_eq!(events.try_recv().ok(), Some(LinkEvent::Changed));

        links.publish(Facts { signed_in: true, points: 42, ..Default::default() });
        assert!(drain(&mut rx).is_empty(), "same state is not sent again");
        links.publish(Facts { signed_in: true, points: 41, ..Default::default() });
        assert_eq!(drain(&mut rx)[0]["points"], 41);
    }

    #[test]
    fn tab_reports_are_per_browser_and_cleaned() {
        let (links, _events) = Links::new();
        let (chrome, _rx1) = links.open(Some("com.google.Chrome".into()));
        let (unknown, _rx2) = links.open(None);
        links.receive(chrome, br#"{"type":"tab","site":"m.youtube.com","idle":false}"#);
        assert_eq!(links.tab_for("com.google.Chrome"), None, "a tab before hello does not count");
        for id in [chrome, unknown] {
            links.receive(id, br#"{"type":"hello"}"#);
        }
        assert_eq!(links.tab_for("com.google.Chrome"), Some(TabReport::default()), "linked, nothing reported yet");
        links.receive(chrome, br#"{"type":"tab","site":"M.YouTube.com.","idle":false}"#);
        assert_eq!(links.tab_for("com.google.Chrome").unwrap().site.as_deref(), Some("m.youtube.com"));
        assert_eq!(links.tab_for("org.mozilla.firefox"), None);
        assert_eq!(
            links.linked_browsers(),
            vec!["com.google.Chrome".to_string()],
            "a link with no browser is not listed"
        );
        links.close(chrome);
        assert_eq!(links.tab_for("com.google.Chrome"), None, "closed: back to the whole browser");
    }

    #[test]
    fn draining_and_close_tab_go_only_to_that_browser() {
        let (links, _events) = Links::new();
        let (chrome, mut rx_chrome) = links.open(Some("com.google.Chrome".into()));
        let (firefox, mut rx_firefox) = links.open(Some("org.mozilla.firefox".into()));
        links.receive(chrome, br#"{"type":"hello"}"#);
        links.receive(firefox, br#"{"type":"hello"}"#);
        drain(&mut rx_chrome);
        drain(&mut rx_firefox);
        let draining = Draining { browser: "com.google.Chrome".into(), site: "youtube.com".into(), per_min: 1 };
        links.publish(Facts { draining: Some(draining), ..Default::default() });
        assert_eq!(drain(&mut rx_chrome)[0]["draining"]["site"], "youtube.com");
        assert!(drain(&mut rx_firefox).is_empty(), "nothing changed for Firefox");
        links.close_tab("com.google.Chrome", "youtube.com");
        assert_eq!(drain(&mut rx_chrome)[0], json!({ "type": "close_tab", "site": "youtube.com" }));
        assert!(drain(&mut rx_firefox).is_empty());
    }

    #[test]
    fn hosts_are_cleaned_or_dropped() {
        assert_eq!(clean_host("WWW.Example.COM.").as_deref(), Some("www.example.com"));
        assert_eq!(clean_host("[::1]:8080"), None);
        assert_eq!(clean_host(""), None);
        assert_eq!(clean_host("a b.com"), None);
    }
}
