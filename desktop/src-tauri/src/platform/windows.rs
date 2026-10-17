//! Windows implementation of [`Platform`]. SKELETON: compiles and reports "nothing in front",
//! so the app runs but nothing drains on Windows yet. The Windows builder fills it in with
//! Win32 (GetForegroundWindow + QueryFullProcessImageNameW, WM_CLOSE, Start-menu shortcuts,
//! WTS session notifications).

use super::{FrontApp, InstalledApp, Platform};

pub struct WindowsPlatform;

impl Platform for WindowsPlatform {
    fn frontmost_app(&self) -> Option<FrontApp> {
        None
    }

    fn terminate_app(&self, _id: &str) -> bool {
        false
    }

    fn installed_apps(&self) -> Vec<InstalledApp> {
        Vec::new()
    }

    fn screen_locked(&self) -> bool {
        false
    }

    fn device_name(&self) -> String {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".to_string())
    }
}
