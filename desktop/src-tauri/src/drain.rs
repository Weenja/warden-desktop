//! The drain rules, free of the OS and the network so they can be tested. Mirrors Android's
//! `GuardCore` (warden-prd `apps/android/platform/.../GuardCore.kt`).
//!
//! - A guarded app in front drains.
//! - A known browser in front WITH the Warden extension linked drains only while its active tab
//!   is on a guarded site and the user is not idle; the tick names the site's host.
//! - A known browser WITHOUT the extension drains whole while the user has guarded sites,
//!   because Warden cannot see which site is open in it.
//! - Nothing in front (screen locked, asleep, login window) drains nothing.
//! - The balance shown is optimistic: the server's last balance minus what was used here and not
//!   charged yet. Used seconds are sent as spend ticks: every 30 s of use, at once when the app
//!   in front changes, and at once at zero.
//! - Each tick carries a one-time evidence key; a retry reuses it, so the server never charges
//!   a tick twice.
//! - Locked (the shield) at local zero, when the balance token expired (we cannot trust the
//!   number any more), or when the server says today's spending is used up.
//! - The server stays authoritative: every answer re-anchors the local balance.

use serde::Serialize;

use crate::api::SpendTarget;
use crate::browser_ext::link::TabReport;
use crate::platform::{FrontApp, KNOWN_BROWSERS};
use crate::sync::BalanceClaims;

/// Send a tick once this much use is collected for the app in front. Between 30 and 60 s keeps
/// the server close to real time without hitting its rate limit (240 ticks per 10 min).
pub const TICK_AFTER_SECS: f64 = 30.0;
/// The server takes at most 900 s per tick.
pub const MAX_TICK_SECS: u32 = 900;
/// Low battery, as on the phone: 10 points or less. The pill turns amber and a toast shows once.
pub const LOW_POINTS: i64 = 10;
/// The low toast shows again only after the balance went back above this. A few seconds of
/// server re-anchoring around 10 points must not show it twice.
const LOW_REARM_POINTS: i64 = 12;

/// A guarded app on this computer: the OS app id joined to its block-list row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedApp {
    /// Bundle id on macOS, exe name on Windows.
    pub app_id: String,
    /// The `block_targets.id` the server charges.
    pub target_id: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DrainKind {
    /// A guarded app.
    App,
    /// A whole browser: guarded sites exist and the browser has no Warden extension.
    Browser,
    /// A guarded site in a browser with the Warden extension. The label is the guarded site.
    Site,
}

/// The guarded thing in front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guarded {
    pub app_id: String,
    pub label: String,
    pub kind: DrainKind,
    pub charge: SpendTarget,
}

/// What one poll sees.
pub struct Inputs<'a> {
    /// The app in front. None while the screen is locked, the Mac sleeps, or nothing has focus.
    pub front: Option<&'a FrontApp>,
    pub guarded_apps: &'a [GuardedApp],
    /// Guarded sites from the phone, e.g. "youtube.com".
    pub guarded_sites: &'a [String],
    /// What the Warden extension in the browser in front reports. None when that browser has no
    /// linked extension (or the app in front is not a browser).
    pub extension: Option<&'a TabReport>,
    /// The latest balance token claims. None until the first sync (or after sign-out).
    pub balance: Option<&'a BalanceClaims>,
    /// Wall clock, seconds since the Unix epoch, for the token expiry.
    pub now: i64,
    /// Seconds in front since the last poll.
    pub dt: f64,
}

/// What one poll decided.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// The guarded app (or whole browser) in front, draining or locked.
    pub front: Option<Guarded>,
    /// True when `front` drained this poll.
    pub draining: bool,
    /// Locked: zero, expired token, or today's spending used up. With `front` set = shield.
    pub locked: bool,
    /// The balance reached zero on this poll.
    pub zero_now: bool,
    /// Show the one-time low battery toast now.
    pub low_now: bool,
}

/// Why the shield is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LockReason {
    /// Out of points.
    Zero,
    /// The balance token expired: offline too long to trust the local count.
    Offline,
    /// The server stopped charging for today (the crew's daily max is spent).
    DailyMax,
}

/// One spend tick to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tick {
    pub target: SpendTarget,
    pub seconds: u32,
    /// Idempotency key, unique per tick and reused on every retry of it.
    pub evidence: String,
}

pub struct Drain {
    /// Prefix that makes evidence keys unique per app run (the seq makes them unique per tick).
    evidence_prefix: String,
    next_seq: u64,
    /// The server's balance (seconds) from the last token or tick answer. None = not known yet.
    server_balance: Option<i64>,
    /// The claims last anchored, to notice a new token.
    anchored: Option<BalanceClaims>,
    /// Bedtime makes the server charge double; the local estimate follows (ticks stay raw seconds).
    multiplier: f64,
    expires_at: i64,
    /// Used, not yet sent, per target. A short list (one entry per recent target).
    unsent: Vec<(SpendTarget, f64)>,
    in_flight: Option<Tick>,
    current: Option<SpendTarget>,
    /// The server shielded with points left: today's spending is used up until this day ends.
    closed_day: Option<String>,
    was_zero: bool,
    low_armed: bool,
}

impl Drain {
    pub fn new(evidence_prefix: impl Into<String>) -> Drain {
        Drain {
            evidence_prefix: evidence_prefix.into(),
            next_seq: 0,
            server_balance: None,
            anchored: None,
            multiplier: 1.0,
            expires_at: 0,
            unsent: Vec::new(),
            in_flight: None,
            current: None,
            closed_day: None,
            was_zero: false,
            low_armed: true,
        }
    }

    /// Seconds guarded apps can still use: the server's balance minus everything used here and
    /// not charged yet (unsent and in flight), times the multiplier.
    pub fn left(&self) -> f64 {
        let Some(balance) = self.server_balance else {
            return 0.0;
        };
        let unsent: f64 = self.unsent.iter().map(|(_, s)| s).sum();
        let in_flight = self.in_flight.as_ref().map_or(0.0, |t| f64::from(t.seconds));
        balance as f64 - (unsent + in_flight) * self.multiplier
    }

    /// Whole points left, as the UI shows them (1 point = 1 minute).
    pub fn points(&self) -> i64 {
        (self.left().max(0.0) / 60.0).floor() as i64
    }

    /// Points drained per minute: 1, or 2 at bedtime.
    pub fn per_minute(&self) -> i64 {
        (self.multiplier.round() as i64).max(1)
    }

    pub fn known(&self) -> bool {
        self.server_balance.is_some()
    }

    pub fn lock_reason(&self, now: i64) -> Option<LockReason> {
        if self.left() <= 0.0 {
            Some(LockReason::Zero)
        } else if now >= self.expires_at {
            Some(LockReason::Offline)
        } else if self.closed_day.is_some() {
            Some(LockReason::DailyMax)
        } else {
            None
        }
    }

    pub fn step(&mut self, inputs: &Inputs) -> Step {
        self.anchor(inputs.balance);
        let front = inputs
            .front
            .and_then(|app| guarded_in_front(app, inputs.guarded_apps, inputs.guarded_sites, inputs.extension));
        if !self.known() {
            // Signed out or not synced yet: nothing to count against, so nothing drains or locks.
            self.current = None;
            return Step { front: None, draining: false, locked: false, zero_now: false, low_now: false };
        }

        let draining = front.is_some() && self.lock_reason(inputs.now).is_none();
        self.current = front.as_ref().filter(|_| draining).map(|g| g.charge.clone());
        if let Some(charge) = &self.current {
            self.add_unsent(charge.clone(), inputs.dt.max(0.0));
        }

        let locked = self.lock_reason(inputs.now).is_some();
        let zero = self.left() <= 0.0;
        let zero_now = zero && !self.was_zero;
        self.was_zero = zero;

        let points = self.points();
        if points >= LOW_REARM_POINTS {
            self.low_armed = true;
        }
        let low_now = self.low_armed && draining && !locked && points <= LOW_POINTS;
        if low_now {
            self.low_armed = false;
        }
        Step { front, draining, locked, zero_now, low_now }
    }

    /// A new balance token (heartbeat or tick answer). Re-anchor unless a tick is in flight:
    /// its answer carries a balance that already includes it.
    fn anchor(&mut self, claims: Option<&BalanceClaims>) {
        let Some(claims) = claims else {
            self.server_balance = None;
            self.anchored = None;
            return;
        };
        self.multiplier = claims.multiplier.max(1.0);
        self.expires_at = claims.exp;
        if self.closed_day.as_ref().is_some_and(|day| *day != claims.day_key) {
            self.closed_day = None;
        }
        if self.in_flight.is_none() && self.anchored.as_ref() != Some(claims) {
            self.server_balance = Some(claims.balance_seconds);
            self.anchored = Some(claims.clone());
        }
    }

    fn add_unsent(&mut self, target: SpendTarget, seconds: f64) {
        match self.unsent.iter_mut().find(|(t, _)| *t == target) {
            Some((_, s)) => *s += seconds,
            None => self.unsent.push((target, seconds)),
        }
    }

    /// The tick to send now, if any. While one is in flight it is returned again with the same
    /// evidence (a retry). Due: 30 s collected for the app in front, any whole second for an app
    /// no longer in front (the target changed), or any whole second at zero.
    pub fn next_tick(&mut self) -> Option<Tick> {
        if let Some(tick) = &self.in_flight {
            return Some(tick.clone());
        }
        let at_zero = self.left() <= 0.0;
        let current = self.current.clone();
        // Fractions of a second for apps no longer in front are dropped, as on Android.
        self.unsent.retain(|(t, s)| Some(t) == current.as_ref() || *s >= 1.0);
        let due = self
            .unsent
            .iter()
            .position(|(t, s)| *s >= TICK_AFTER_SECS || (*s >= 1.0 && (Some(t) != current.as_ref() || at_zero)))?;
        let (target, collected) = &mut self.unsent[due];
        let seconds = (collected.floor() as u32).min(MAX_TICK_SECS);
        *collected -= f64::from(seconds);
        let tick =
            Tick { target: target.clone(), seconds, evidence: format!("{}.{}", self.evidence_prefix, self.next_seq) };
        self.next_seq += 1;
        self.in_flight = Some(tick.clone());
        Some(tick)
    }

    /// The server charged the tick in flight. `balance` is its new balance; `shield` its verdict.
    pub fn ticked(&mut self, balance: i64, shield: bool, day_key: &str) {
        self.in_flight = None;
        self.server_balance = Some(balance);
        if shield && balance > 0 {
            // Points left but the server shields: today's spending is used up.
            self.closed_day = Some(day_key.to_string());
        }
    }

    /// The tick in flight failed. `drop`: the server refused it for good (keep the local
    /// estimate honest by forgetting those seconds). Otherwise it is retried with the same key.
    pub fn tick_failed(&mut self, drop: bool) {
        if drop {
            self.in_flight = None;
        }
    }
}

/// Which guarded thing `app` is, if any. A guarded app wins over the browser rules, so a
/// browser the user picked as an app is charged as that app. For a browser, `extension` is what
/// its linked Warden extension reports (None = no extension: the whole browser drains).
pub fn guarded_in_front(
    app: &FrontApp,
    guarded_apps: &[GuardedApp],
    guarded_sites: &[String],
    extension: Option<&TabReport>,
) -> Option<Guarded> {
    if let Some(g) = guarded_apps.iter().find(|g| g.app_id == app.id) {
        return Some(Guarded {
            app_id: g.app_id.clone(),
            label: g.label.clone(),
            kind: DrainKind::App,
            charge: SpendTarget::Target(g.target_id.clone()),
        });
    }
    if !KNOWN_BROWSERS.contains(&app.id.as_str()) {
        return None;
    }
    if let Some(tab) = extension {
        let host = tab.site.as_deref().filter(|_| !tab.idle)?;
        let site = guarded_site(host, guarded_sites)?;
        return Some(Guarded {
            app_id: app.id.clone(),
            label: site.to_string(),
            kind: DrainKind::Site,
            // The host as open (m.youtube.com); the server picks the guarded site it falls under.
            charge: SpendTarget::Site(host.to_string()),
        });
    }
    if !guarded_sites.is_empty() {
        return Some(Guarded {
            app_id: app.id.clone(),
            label: app.name.clone(),
            kind: DrainKind::Browser,
            charge: SpendTarget::WholeBrowser,
        });
    }
    None
}

/// The guarded site that covers `host`, the most specific one when several do. A host is
/// covered when it equals the site or is a subdomain of it (`m.youtube.com` under
/// `youtube.com`), never by a mere suffix (`notyoutube.com` is not under `youtube.com`).
/// Same rule as the server's `site_target` and the extension.
pub fn guarded_site<'a>(host: &str, guarded_sites: &'a [String]) -> Option<&'a str> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    guarded_sites
        .iter()
        .map(|site| site.as_str())
        .filter(|site| {
            let site = site.trim_end_matches('.').to_ascii_lowercase();
            !site.is_empty() && (host == site || host.ends_with(&format!(".{site}")))
        })
        .max_by_key(|site| site.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;

    fn claims(balance: i64) -> BalanceClaims {
        BalanceClaims {
            sub: "u".into(),
            balance_seconds: balance,
            day_key: "2026-10-17".into(),
            multiplier: 1.0,
            iat: NOW,
            exp: NOW + 900,
        }
    }

    fn steam() -> FrontApp {
        FrontApp { id: "com.valvesoftware.steam".into(), name: "Steam".into() }
    }

    fn chrome() -> FrontApp {
        FrontApp { id: "com.google.Chrome".into(), name: "Google Chrome".into() }
    }

    fn finder() -> FrontApp {
        FrontApp { id: "com.apple.finder".into(), name: "Finder".into() }
    }

    fn guarded() -> Vec<GuardedApp> {
        vec![GuardedApp {
            app_id: "com.valvesoftware.steam".into(),
            target_id: "t-steam".into(),
            label: "Steam".into(),
        }]
    }

    /// Poll `n` times, one second each, with `front` in front.
    fn run(d: &mut Drain, front: Option<&FrontApp>, balance: &BalanceClaims, n: usize) -> Step {
        let apps = guarded();
        let mut last = None;
        for _ in 0..n {
            last = Some(d.step(&Inputs {
                front,
                guarded_apps: &apps,
                guarded_sites: &[],
                extension: None,
                balance: Some(balance),
                now: NOW,
                dt: 1.0,
            }));
        }
        last.unwrap()
    }

    #[test]
    fn guarded_app_drains_locally_and_other_apps_do_not() {
        let mut d = Drain::new("t");
        let c = claims(600);
        let s = run(&mut d, Some(&steam()), &c, 10);
        assert!(s.draining);
        assert_eq!(s.front.unwrap().charge, SpendTarget::Target("t-steam".into()));
        assert_eq!(d.left(), 590.0);
        assert_eq!(d.points(), 9);
        let s = run(&mut d, Some(&finder()), &c, 10);
        assert!(!s.draining && s.front.is_none());
        assert_eq!(d.left(), 590.0, "a non-guarded app costs nothing");
        let s = run(&mut d, None, &c, 10);
        assert!(!s.draining, "locked screen / asleep: nothing in front, nothing drains");
    }

    #[test]
    fn ticks_batch_30_seconds_and_flush_on_target_change() {
        let mut d = Drain::new("t");
        let c = claims(3600);
        run(&mut d, Some(&steam()), &c, 29);
        assert_eq!(d.next_tick(), None, "29 s is not a tick yet");
        run(&mut d, Some(&steam()), &c, 1);
        let tick = d.next_tick().unwrap();
        assert_eq!((tick.seconds, tick.target.clone()), (30, SpendTarget::Target("t-steam".into())));
        d.ticked(3570, false, "2026-10-17");

        run(&mut d, Some(&steam()), &c, 12);
        assert_eq!(d.next_tick(), None);
        run(&mut d, Some(&finder()), &c, 1);
        let tick = d.next_tick().expect("switching away sends what was used at once");
        assert_eq!(tick.seconds, 12);
        d.ticked(3558, false, "2026-10-17");
        assert_eq!(d.next_tick(), None);
        assert_eq!(d.left(), 3558.0);
    }

    #[test]
    fn evidence_is_unique_per_tick_and_reused_on_retry() {
        let mut d = Drain::new("run1");
        let c = claims(3600);
        run(&mut d, Some(&steam()), &c, 30);
        let first = d.next_tick().unwrap();
        d.tick_failed(false);
        run(&mut d, Some(&steam()), &c, 30);
        let retry = d.next_tick().unwrap();
        assert_eq!(retry, first, "a failed tick is retried with the same key and seconds");
        assert_eq!(d.left(), 3540.0, "in-flight and new seconds both count locally");
        d.ticked(3570, false, "2026-10-17");
        let next = d.next_tick().unwrap();
        assert_ne!(next.evidence, first.evidence);
        assert_eq!(next.evidence, "run1.1");
        assert_eq!(next.seconds, 30);
    }

    #[test]
    fn a_refused_tick_is_dropped() {
        let mut d = Drain::new("t");
        let c = claims(600);
        run(&mut d, Some(&steam()), &c, 30);
        d.next_tick().unwrap();
        d.tick_failed(true);
        assert_eq!(d.next_tick(), None);
        assert_eq!(d.left(), 600.0, "seconds the server refused are not counted against you");
    }

    #[test]
    fn heartbeat_reanchors_minus_what_is_not_charged_yet() {
        let mut d = Drain::new("t");
        run(&mut d, Some(&steam()), &claims(600), 10);
        // The phone earned 5 points: a new token says 900 s. 10 s here are still unsent.
        let mut newer = claims(900);
        newer.iat += 1;
        run(&mut d, Some(&finder()), &newer, 1);
        assert_eq!(d.left(), 890.0);
    }

    #[test]
    fn no_reanchor_while_a_tick_is_in_flight() {
        let mut d = Drain::new("t");
        run(&mut d, Some(&steam()), &claims(600), 30);
        d.next_tick().unwrap();
        let mut stale = claims(600);
        stale.iat += 1;
        run(&mut d, Some(&finder()), &stale, 1);
        assert_eq!(d.left(), 570.0, "the in-flight 30 s stay counted");
    }

    #[test]
    fn low_toast_once_at_ten_points_and_rearms_above_twelve() {
        let mut d = Drain::new("t");
        let c = claims(11 * 60);
        let lows = (0..120).filter(|_| run(&mut d, Some(&steam()), &c, 1).low_now).count();
        assert_eq!(lows, 1, "one toast per crossing");
        assert!(d.points() <= LOW_POINTS);
        d.ticked(15 * 60, false, "2026-10-17");
        run(&mut d, Some(&finder()), &c, 1);
        d.ticked(10 * 60, false, "2026-10-17");
        assert!(run(&mut d, Some(&steam()), &c, 1).low_now, "back above 12 points re-arms it");
    }

    #[test]
    fn zero_locks_and_sends_the_last_seconds() {
        let mut d = Drain::new("t");
        let c = claims(20);
        let s = run(&mut d, Some(&steam()), &c, 19);
        assert!(s.draining && !s.locked);
        let s = run(&mut d, Some(&steam()), &c, 1);
        assert!(s.locked && s.zero_now, "the poll that reaches 0 locks");
        assert_eq!(d.lock_reason(NOW), Some(LockReason::Zero));
        let tick = d.next_tick().expect("at zero the rest goes at once");
        assert_eq!(tick.seconds, 20);
        d.ticked(0, true, "2026-10-17");
        let s = run(&mut d, Some(&steam()), &c, 5);
        assert!(s.locked && !s.draining && !s.zero_now && s.front.is_some(), "shield over the app, no more drain");
        assert_eq!(d.next_tick(), None);
        let s = run(&mut d, Some(&finder()), &c, 1);
        assert!(s.front.is_none(), "switching away: nothing to shield");
    }

    #[test]
    fn expired_token_locks_without_draining() {
        let mut d = Drain::new("t");
        let c = claims(600);
        let apps = guarded();
        let s = d.step(&Inputs {
            front: Some(&steam()),
            guarded_apps: &apps,
            guarded_sites: &[],
            extension: None,
            balance: Some(&c),
            now: c.exp,
            dt: 1.0,
        });
        assert!(s.locked && !s.draining);
        assert_eq!(d.lock_reason(c.exp), Some(LockReason::Offline));
        assert_eq!(d.left(), 600.0);
    }

    #[test]
    fn nothing_drains_or_locks_before_the_first_balance() {
        let mut d = Drain::new("t");
        let apps = guarded();
        let s = d.step(&Inputs {
            front: Some(&steam()),
            guarded_apps: &apps,
            guarded_sites: &[],
            extension: None,
            balance: None,
            now: NOW,
            dt: 1.0,
        });
        assert_eq!(s, Step { front: None, draining: false, locked: false, zero_now: false, low_now: false });
    }

    #[test]
    fn server_shield_with_points_left_closes_the_day() {
        let mut d = Drain::new("t");
        let c = claims(600);
        run(&mut d, Some(&steam()), &c, 30);
        d.next_tick().unwrap();
        d.ticked(570, true, "2026-10-17");
        assert_eq!(d.lock_reason(NOW), Some(LockReason::DailyMax));
        let mut tomorrow = claims(570);
        tomorrow.day_key = "2026-10-18".into();
        tomorrow.iat += 1;
        let s = run(&mut d, Some(&steam()), &tomorrow, 1);
        assert!(s.draining, "a new day opens it again");
    }

    #[test]
    fn bedtime_doubles_the_local_drain_but_ticks_raw_seconds() {
        let mut d = Drain::new("t");
        let mut c = claims(600);
        c.multiplier = 2.0;
        run(&mut d, Some(&steam()), &c, 30);
        assert_eq!(d.left(), 540.0);
        assert_eq!(d.per_minute(), 2);
        assert_eq!(d.next_tick().unwrap().seconds, 30, "the server multiplies");
    }

    fn sites() -> Vec<String> {
        vec!["youtube.com".into(), "music.youtube.com".into()]
    }

    fn tab(site: Option<&str>, idle: bool) -> TabReport {
        TabReport { site: site.map(str::to_owned), idle }
    }

    #[test]
    fn whole_browser_drains_only_while_sites_are_guarded() {
        let apps = guarded();
        assert_eq!(guarded_in_front(&chrome(), &apps, &[], None), None);
        let g = guarded_in_front(&chrome(), &apps, &sites(), None).unwrap();
        assert_eq!((g.kind, g.charge), (DrainKind::Browser, SpendTarget::WholeBrowser));
        let picked = vec![GuardedApp { app_id: chrome().id, target_id: "t-chrome".into(), label: "Chrome".into() }];
        let g = guarded_in_front(&chrome(), &picked, &sites(), None).unwrap();
        assert_eq!(g.charge, SpendTarget::Target("t-chrome".into()), "a picked browser is charged as that app");
        assert_eq!(guarded_in_front(&finder(), &apps, &sites(), None), None);
    }

    #[test]
    fn a_browser_with_the_extension_drains_only_on_guarded_sites() {
        let apps = guarded();
        let on = |t: TabReport| guarded_in_front(&chrome(), &apps, &sites(), Some(&t));
        let g = on(tab(Some("m.youtube.com"), false)).unwrap();
        assert_eq!(
            (g.kind, g.label.as_str(), g.charge),
            (DrainKind::Site, "youtube.com", SpendTarget::Site("m.youtube.com".into()))
        );
        assert_eq!(on(tab(Some("example.com"), false)), None, "a site that is not guarded costs nothing");
        assert_eq!(on(tab(None, false)), None, "new tab page, settings, blocked page");
        assert_eq!(on(tab(Some("youtube.com"), true)), None, "idle: nothing drains");
        assert_eq!(on(TabReport::default()), None, "linked, nothing reported yet");
        assert_eq!(
            guarded_in_front(&finder(), &apps, &sites(), Some(&tab(Some("youtube.com"), false))),
            None,
            "only a browser in front counts"
        );
    }

    #[test]
    fn site_match_is_by_whole_labels_and_most_specific() {
        let s = sites();
        assert_eq!(guarded_site("youtube.com", &s), Some("youtube.com"));
        assert_eq!(guarded_site("m.youtube.com", &s), Some("youtube.com"));
        assert_eq!(guarded_site("music.youtube.com", &s), Some("music.youtube.com"));
        assert_eq!(guarded_site("Www.YouTube.com.", &s), Some("youtube.com"));
        assert_eq!(guarded_site("notyoutube.com", &s), None);
        assert_eq!(guarded_site("youtube.com.evil.net", &s), None);
        assert_eq!(guarded_site("com", &s), None);
    }

    #[test]
    fn long_offline_use_is_sent_in_900_second_ticks() {
        let mut d = Drain::new("t");
        let c = claims(100_000);
        let apps = guarded();
        d.step(&Inputs {
            front: Some(&steam()),
            guarded_apps: &apps,
            guarded_sites: &[],
            extension: None,
            balance: Some(&c),
            now: NOW,
            dt: 1000.0,
        });
        assert_eq!(d.next_tick().unwrap().seconds, MAX_TICK_SECS);
        d.ticked(99_100, false, "2026-10-17");
        assert_eq!(d.next_tick().unwrap().seconds, 100);
    }
}
