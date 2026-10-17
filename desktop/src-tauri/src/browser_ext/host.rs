//! Native messaging host mode: the browser starts this same binary when the Warden extension
//! connects (`runtime.connectNative`). In this mode there is no window and no network: the
//! process only relays framed messages between the browser (stdin/stdout) and the running
//! Warden app (the local endpoint). If the app is not running, the host starts it and keeps
//! trying to connect. The host exits when the browser closes stdin.
//!
//! Only protocol frames go to stdout (the browser reads it); logs go to stderr.

use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use super::{endpoint, frame, FIREFOX_EXTENSION_ID};

/// Right after starting the app, try to connect this often (the app needs a moment to start)...
const FAST_RETRY: Duration = Duration::from_secs(1);
const FAST_RETRY_FOR: Duration = Duration::from_secs(15);
/// ...after that, this often, for as long as the browser keeps the host open.
const SLOW_RETRY: Duration = Duration::from_secs(5);
/// Start the app again if it is still not reachable after this long (it may have been quit).
const RESTART_APP_AFTER: Duration = Duration::from_secs(60);

/// True when the browser started this process as the native messaging host.
/// Chrome (and Chromium browsers) pass the extension's origin, `chrome-extension://<id>/`
/// (on Windows also `--parent-window=<n>`); Firefox passes the host manifest's path and the
/// add-on id. A normal app start or a `warden://` link has neither.
pub fn is_host_launch(args: &[String]) -> bool {
    args.iter().skip(1).any(|arg| arg.starts_with("chrome-extension://") || arg == FIREFOX_EXTENSION_ID)
}

/// Run the host until the browser closes it. Returns the process exit code.
pub fn run() -> i32 {
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("warden host: {e}");
            return 1;
        }
    };
    runtime.block_on(relay());
    0
}

/// The last `hello` and `tab` from the extension. Sent again after (re)connecting to the app,
/// so an app that started late, or restarted, knows the extension and the open site at once.
#[derive(Default)]
struct Replay {
    hello: Option<Vec<u8>>,
    tab: Option<Vec<u8>>,
}

impl Replay {
    fn remember(&mut self, body: &[u8]) {
        let kind = serde_json::from_slice::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_owned));
        match kind.as_deref() {
            Some("hello") => self.hello = Some(body.to_vec()),
            Some("tab") => self.tab = Some(body.to_vec()),
            _ => {}
        }
    }

    fn frames(&self) -> impl Iterator<Item = &Vec<u8>> {
        self.hello.iter().chain(self.tab.iter())
    }
}

async fn relay() {
    let mut from_browser = read_stdin();
    let mut replay = Replay::default();
    let mut app_started: Option<Instant> = None;
    loop {
        let Some(stream) = connect(&mut from_browser, &mut replay, &mut app_started).await else {
            return;
        };
        let (mut app_reader, mut app_writer) = tokio::io::split(stream);

        // App -> browser. The browser closing stdout means it is gone: exit.
        let mut to_browser = tokio::spawn(async move {
            let mut stdout = tokio::io::stdout();
            while let Ok(Some(body)) = frame::read(&mut app_reader).await {
                let Ok(frame) = frame::encode(&body) else { continue };
                if stdout.write_all(&frame).await.is_err() || stdout.flush().await.is_err() {
                    std::process::exit(0);
                }
            }
        });

        // Browser -> app, starting with the replay.
        let mut ok = true;
        for body in replay.frames() {
            ok = ok && write_frame(&mut app_writer, body).await;
        }
        while ok {
            tokio::select! {
                message = from_browser.recv() => match message {
                    None => return,
                    Some(body) => {
                        replay.remember(&body);
                        ok = write_frame(&mut app_writer, &body).await;
                    }
                },
                // The app closed the connection (quit or restart): connect again.
                _ = &mut to_browser => ok = false,
            }
        }
        to_browser.abort();
        log::warn!("warden host: lost the app, reconnecting");
    }
}

async fn write_frame<W: AsyncWriteExt + Unpin>(writer: &mut W, body: &[u8]) -> bool {
    match frame::encode(body) {
        Ok(frame) => writer.write_all(&frame).await.is_ok(),
        Err(_) => true,
    }
}

/// Frames from the browser on stdin. The channel closes when the browser closes stdin (the
/// extension disconnected or the browser quit) or sends a broken frame.
fn read_stdin() -> UnboundedReceiver<Vec<u8>> {
    let (sender, receiver) = unbounded_channel();
    tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        while let Ok(Some(body)) = frame::read(&mut stdin).await {
            if sender.send(body).is_err() {
                break;
            }
        }
    });
    receiver
}

/// Connect to the app, starting it when it is not reachable. While waiting, keep reading the
/// browser's messages (remembered for the replay). None = the browser closed the host.
async fn connect(
    from_browser: &mut UnboundedReceiver<Vec<u8>>,
    replay: &mut Replay,
    app_started: &mut Option<Instant>,
) -> Option<endpoint::Stream> {
    loop {
        match endpoint::connect().await {
            Ok(stream) => return Some(stream),
            Err(e) => log::info!("warden host: app not reachable yet: {e}"),
        }
        if app_started.is_none_or(|at| at.elapsed() >= RESTART_APP_AFTER) {
            start_app();
            *app_started = Some(Instant::now());
        }
        let fast = app_started.is_some_and(|at| at.elapsed() < FAST_RETRY_FOR);
        let wait = tokio::time::sleep(if fast { FAST_RETRY } else { SLOW_RETRY });
        tokio::pin!(wait);
        loop {
            tokio::select! {
                _ = &mut wait => break,
                message = from_browser.recv() => match message {
                    None => return None,
                    Some(body) => replay.remember(&body),
                },
            }
        }
    }
}

/// Start the Warden app (a second start of a running app does nothing: single instance).
/// macOS: through LaunchServices by bundle id, in the background (`-g`), so it does not take
/// the focus from the browser.
#[cfg(target_os = "macos")]
fn start_app() {
    let started = std::process::Command::new("/usr/bin/open")
        .args(["-g", "-b", super::APP_ID])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status();
    if let Err(e) = started {
        log::warn!("warden host: could not start the app: {e}");
    }
}

/// Windows: this same exe with no arguments is the app. It must not inherit the browser's
/// pipes (stdio null), and it should outlive the host: the browser may end the host's job
/// when the extension disconnects, so ask to break away from it (refused if the job forbids
/// it; then start it inside the job).
#[cfg(windows)]
fn start_app() {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let Ok(exe) = std::env::current_exe() else { return };
    let spawn = |flags: u32| {
        Command::new(&exe)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(flags)
            .spawn()
    };
    if let Err(e) = spawn(DETACHED_PROCESS | CREATE_BREAKAWAY_FROM_JOB).or_else(|_| spawn(DETACHED_PROCESS)) {
        log::warn!("warden host: could not start the app: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn browser_host_launches_are_recognised() {
        assert!(is_host_launch(&args(&[
            "/Applications/Warden β.app/Contents/MacOS/warden",
            "chrome-extension://abc/"
        ])));
        assert!(is_host_launch(&args(&["warden.exe", "chrome-extension://abc/", "--parent-window=0"])));
        assert!(is_host_launch(&args(&[
            "warden",
            "/Users/sam/Library/Application Support/Mozilla/NativeMessagingHosts/in.weenja.warden.staging.json",
            FIREFOX_EXTENSION_ID,
        ])));
    }

    #[test]
    fn normal_starts_are_not_host_launches() {
        assert!(!is_host_launch(&args(&["warden"])));
        assert!(!is_host_launch(&args(&["warden.exe", "warden://auth-callback?code=chrome-extension://x"])));
        // The program path itself is never read as an argument.
        assert!(!is_host_launch(&args(&["chrome-extension://abc/"])));
        assert!(!is_host_launch(&args(&["warden", "--autostart"])));
    }

    #[test]
    fn replay_keeps_the_last_hello_and_tab_in_order() {
        let mut replay = Replay::default();
        replay.remember(br#"{"type":"tab","site":"a.com"}"#);
        replay.remember(br#"{"type":"hello"}"#);
        replay.remember(br#"{"type":"show_app"}"#);
        replay.remember(br#"{"type":"tab","site":"b.com"}"#);
        let frames: Vec<&[u8]> = replay.frames().map(|f| f.as_slice()).collect();
        assert_eq!(frames, vec![br#"{"type":"hello"}"#.as_slice(), br#"{"type":"tab","site":"b.com"}"#.as_slice()]);
    }
}
