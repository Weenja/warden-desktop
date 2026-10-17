//! The drain loop: once a second it asks the OS which app is in front, runs the drain rules
//! ([`crate::drain`]), sends spend ticks in the background, and tells the UI what to show
//! (pill, shield, low toast). No Tauri in here, so the staging test runs this same loop with a
//! scripted fake [`Platform`](crate::platform::Platform) and a fake browser extension.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::task::JoinHandle;

use crate::api::ApiError;
use crate::browser_ext::link::Links;
use crate::drain::{Drain, DrainKind, Guarded, Inputs, LockReason, Step};
use crate::platform::FrontApp;
use crate::session::now_secs;
use crate::sync::{SyncError, SyncService};

/// How often the loop looks at the app in front.
pub const POLL_EVERY: Duration = Duration::from_secs(1);
/// A longer gap between two polls (sleep, a stalled machine) is not counted as use.
const MAX_POLL_GAP_SECS: f64 = 2.0;
/// Wait before retrying a tick that failed on the network or a server error.
const RETRY_AFTER: Duration = Duration::from_secs(5);
/// While one of Warden's own windows (popover, picker) is in front, the app under it keeps its
/// state this long, so opening the popover over Steam shows "Steam on this Mac" instead of
/// "Nothing draining". After that, Warden in front counts as nothing in front.
const OWN_WINDOW_GRACE: Duration = Duration::from_secs(30);
/// After "Close <app>" on the shield, the shield steps aside this long so the app can quit or
/// ask to save documents. If it is still in front after that, the shield comes back.
const CLOSE_GRACE: Duration = Duration::from_secs(10);
/// Locked on a guarded site, the extension shows its blocked page by itself. If the site is
/// still open after this long (an older extension, a page it could not replace), Warden asks
/// the extension to close the tab.
const CLOSE_TAB_AFTER: Duration = Duration::from_secs(3);

/// The pill: what drains and how much is left.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DrainView {
    pub app_id: String,
    pub label: String,
    pub kind: DrainKind,
    pub icon: Option<String>,
    /// Local balance in seconds (counts down between ticks).
    pub balance_seconds: i64,
    pub per_minute: i64,
    /// 10 points or less: amber.
    pub low: bool,
    /// Locked: coral. The shield is up over an app or a whole browser; on a site the
    /// extension blocks it.
    pub locked: bool,
}

/// The shield over a guarded app.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShieldView {
    pub app_id: String,
    pub label: String,
    pub kind: DrainKind,
    pub reason: LockReason,
}

/// Everything the UI needs from the drain loop.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuardView {
    /// The local balance when known.
    pub balance_seconds: Option<i64>,
    pub drain: Option<DrainView>,
    pub shield: Option<ShieldView>,
    /// Guarded things are blocked now (zero, offline too long, daily max), whatever is in front.
    pub locked: bool,
}

/// One poll's result for the UI.
#[derive(Debug, Clone)]
pub struct Update {
    pub view: GuardView,
    /// The view differs from the last poll: re-render.
    pub changed: bool,
    /// Show the one-time low battery toast.
    pub low_toast: bool,
}

struct State {
    drain: Drain,
    /// The last app in front that was not Warden, and when Warden took the front.
    last_front: Option<FrontApp>,
    own_front_since: Option<Instant>,
    retry_at: Option<Instant>,
    /// The whole browser whose pill the user hid with Stop (it keeps draining).
    pill_hidden_for: Option<String>,
    /// "Close <app>" was clicked on the shield: (app id, until).
    closing: Option<(String, Instant)>,
    /// Locked with a guarded site open: (browser, site, since, close_tab sent).
    locked_site: Option<(String, String, Instant, bool)>,
    view: GuardView,
}

pub struct Guard {
    sync: Arc<SyncService>,
    /// Linked browser extensions: which site is open in each browser.
    links: Arc<Links>,
    /// Warden's own app id: Warden in front is not "another app".
    own_id: String,
    state: Mutex<State>,
    tick_task: Mutex<Option<JoinHandle<()>>>,
}

impl Guard {
    pub fn new(sync: Arc<SyncService>, links: Arc<Links>, own_id: &str) -> Arc<Guard> {
        // Evidence keys are `d<start ms>.<seq>`: unique per run and per tick. The server adds
        // the device id in front, so two computers never collide.
        let started_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        Arc::new(Guard {
            sync,
            links,
            own_id: own_id.to_string(),
            state: Mutex::new(State {
                drain: Drain::new(format!("d{started_ms}")),
                last_front: None,
                own_front_since: None,
                retry_at: None,
                pill_hidden_for: None,
                closing: None,
                locked_site: None,
                view: GuardView::default(),
            }),
            tick_task: Mutex::new(None),
        })
    }

    pub fn view(&self) -> GuardView {
        self.state.lock().unwrap().view.clone()
    }

    /// Poll forever. `on_update` runs when the view changed or the low toast is due.
    pub async fn run(self: Arc<Self>, on_update: impl Fn(&Update)) {
        let mut last = Instant::now();
        loop {
            tokio::time::sleep(POLL_EVERY).await;
            let dt = last.elapsed().as_secs_f64().min(MAX_POLL_GAP_SECS);
            last = Instant::now();
            let update = self.poll(dt);
            if update.changed || update.low_toast {
                on_update(&update);
            }
        }
    }

    /// One poll with `dt` seconds since the last one. Starts a spend tick in the background
    /// when one is due.
    pub fn poll(self: &Arc<Self>, dt: f64) -> Update {
        let platform = self.sync.platform();
        // A locked screen counts as nothing in front, whatever app sits under the lock screen.
        let front = if platform.screen_locked() { None } else { platform.frontmost_app() };
        let guarded_apps = self.sync.guarded_for_drain();
        let guarded_sites: Vec<String> = self.sync.guarded_sites().into_iter().map(|t| t.token).collect();
        let balance = self.sync.balance();
        let now = now_secs();

        let mut state = self.state.lock().unwrap();
        let front = self.resolve_front(&mut state, front);
        let extension = front.as_ref().and_then(|app| self.links.tab_for(&app.id));
        let step = state.drain.step(&Inputs {
            front: front.as_ref(),
            guarded_apps: &guarded_apps,
            guarded_sites: &guarded_sites,
            extension: extension.as_ref(),
            balance: balance.as_ref(),
            now,
            dt,
        });
        if step.zero_now {
            log::info!("balance reached zero");
        }
        let view = self.build_view(&mut state, &step, now);
        self.close_locked_site(&mut state, &step);
        let changed = view != state.view;
        state.view = view.clone();
        drop(state);

        self.start_tick_if_due();
        Update { view, changed, low_toast: step.low_now }
    }

    /// Warden's own windows in front keep the app under them for a short grace.
    fn resolve_front(&self, state: &mut State, front: Option<FrontApp>) -> Option<FrontApp> {
        match front {
            Some(app) if app.id == self.own_id => {
                let since = *state.own_front_since.get_or_insert_with(Instant::now);
                if since.elapsed() < OWN_WINDOW_GRACE {
                    state.last_front.clone()
                } else {
                    None
                }
            }
            other => {
                state.own_front_since = None;
                state.last_front = other.clone();
                other
            }
        }
    }

    fn build_view(&self, state: &mut State, step: &Step, now: i64) -> GuardView {
        let balance_seconds = state.drain.known().then(|| state.drain.left().max(0.0).floor() as i64);
        let locked = state.drain.known() && state.drain.lock_reason(now).is_some();
        let Some(front) = &step.front else {
            state.pill_hidden_for = None;
            return GuardView { balance_seconds, drain: None, shield: None, locked };
        };
        if state.pill_hidden_for.as_ref().is_some_and(|id| *id != front.app_id) {
            state.pill_hidden_for = None;
        }
        let closing = state.closing.as_ref().is_some_and(|(id, until)| *id == front.app_id && Instant::now() < *until);
        // A site is blocked by the extension (its blocked page), not by a shield over the browser.
        let shield = match state.drain.lock_reason(now) {
            Some(reason) if step.locked && !closing && front.kind != DrainKind::Site => {
                Some(shield_view(front, reason))
            }
            _ => None,
        };
        // A whole browser's pill can be hidden (it keeps draining); at zero it always shows.
        let pill_hidden = !step.locked && state.pill_hidden_for.as_deref() == Some(front.app_id.as_str());
        let drain = (!pill_hidden).then(|| DrainView {
            app_id: front.app_id.clone(),
            label: front.label.clone(),
            kind: front.kind,
            icon: self.sync.app_icon(&front.app_id),
            balance_seconds: balance_seconds.unwrap_or(0),
            per_minute: state.drain.per_minute(),
            low: state.drain.points() <= crate::drain::LOW_POINTS,
            locked: step.locked,
        });
        GuardView { balance_seconds, drain, shield, locked }
    }

    /// Locked while a guarded site stays open for [`CLOSE_TAB_AFTER`]: ask the extension once
    /// to close that tab.
    fn close_locked_site(&self, state: &mut State, step: &Step) {
        let open_site = step.front.as_ref().filter(|f| step.locked && f.kind == DrainKind::Site);
        let Some(front) = open_site else {
            state.locked_site = None;
            return;
        };
        let same = state.locked_site.as_ref().is_some_and(|(b, s, _, _)| *b == front.app_id && *s == front.label);
        if !same {
            state.locked_site = Some((front.app_id.clone(), front.label.clone(), Instant::now(), false));
        }
        if let Some((browser, site, since, sent)) = &mut state.locked_site {
            if !*sent && since.elapsed() >= CLOSE_TAB_AFTER {
                self.links.close_tab(browser, site);
                *sent = true;
            }
        }
    }

    /// Stop on a whole browser's pill: hide the pill, leave the browser (it keeps draining while
    /// it is in front; closing every tab would be too much).
    pub fn hide_pill(&self, app_id: &str) {
        self.state.lock().unwrap().pill_hidden_for = Some(app_id.to_string());
    }

    /// "Close <app>" on the shield: step aside while the app quits.
    pub fn closing(&self, app_id: &str) {
        self.state.lock().unwrap().closing = Some((app_id.to_string(), Instant::now() + CLOSE_GRACE));
    }

    fn start_tick_if_due(self: &Arc<Self>) {
        let mut task = self.tick_task.lock().unwrap();
        if task.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        let tick = {
            let mut state = self.state.lock().unwrap();
            if state.retry_at.is_some_and(|at| Instant::now() < at) {
                return;
            }
            state.drain.next_tick()
        };
        let Some(tick) = tick else {
            return;
        };
        let guard = self.clone();
        *task = Some(tokio::spawn(async move {
            // Network call: spend-tick charges the seconds used (fields in DATA.md).
            let result = guard.sync.spend_tick(&tick.target, tick.seconds, &tick.evidence).await;
            guard.tick_done(result.map(|r| (r.balance_seconds, r.shield, r.wallet.day_key)));
        }));
    }

    fn tick_done(&self, result: Result<(i64, bool, String), SyncError>) {
        let mut state = self.state.lock().unwrap();
        match result {
            Ok((balance, shield, day_key)) => {
                log::info!("spend tick ok: balance {balance} s, shield {shield}");
                state.retry_at = None;
                state.drain.ticked(balance, shield, &day_key);
            }
            Err(e) if refused_for_good(&e) => {
                // E.g. the guarded app was removed on another device a moment ago. Retrying the
                // same request cannot succeed; the next block-list refresh fixes the cause.
                log::warn!("spend tick refused, dropped: {e}");
                state.drain.tick_failed(true);
            }
            Err(e) => {
                log::warn!("spend tick failed, will retry with the same key: {e}");
                state.drain.tick_failed(false);
                state.retry_at = Some(Instant::now() + RETRY_AFTER);
            }
        }
    }

    /// Wait for the tick in flight, if any (the staging test uses this to stay in step).
    pub async fn settle(&self) {
        let task = self.tick_task.lock().unwrap().take();
        if let Some(task) = task {
            let _ = task.await;
        }
    }
}

fn shield_view(front: &Guarded, reason: LockReason) -> ShieldView {
    ShieldView { app_id: front.app_id.clone(), label: front.label.clone(), kind: front.kind, reason }
}

/// A 4xx other than auth, timeout or rate limit: the server will never accept this tick.
fn refused_for_good(e: &SyncError) -> bool {
    matches!(e, SyncError::Api(ApiError::Http { status, .. }) if (400..500).contains(status) && ![401, 408, 429].contains(status))
}
