//! Accounts are made on the phone only.
//!
//! Email sign-in asks the server not to create accounts (`create_user: false`). Google sign-in
//! cannot ask that: when the Google account is unknown, Supabase makes a new, empty Warden
//! account during the sign-in. Left alone, that empty account would link this computer with
//! 0 points and no phone, and it would own the Google identity, so the user's phone could later
//! fail to sign in with the same Google account.
//!
//! So right after a Google sign-in, before anything is written for it, we look for that case
//! and undo it with the user's own `delete-account` call.
//!
//! SAFETY INVARIANT: never delete an account that has any device or is older than
//! [`JUST_CREATED_SECS`]. Anything unclear (no or unreadable `created_at`, a `created_at` in the
//! future because of clock skew) keeps the account. Only [`verdict`] decides, and it is tested.

use crate::api::{Api, ApiError, TokenResponse};

/// An account younger than this may be one the sign-in itself just made.
pub const JUST_CREATED_SECS: i64 = 10 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// A real account: sign in as usual.
    Keep,
    /// The empty account this sign-in just made: delete it and say "No account".
    Reject,
}

/// True only when `created_at` is known and between 0 and 10 minutes ago. Older, unknown or in
/// the future (clock skew) is false. Lets the sign-in skip the device read for old accounts.
pub fn is_just_created(created_at: Option<i64>, now: i64) -> bool {
    match created_at {
        Some(created) => (0..JUST_CREATED_SECS).contains(&(now - created)),
        None => false,
    }
}

/// `device_platforms`: the platforms of the account's devices (`android`, `ios`, `macos`,
/// `windows`); one is enough. Reject only a just-created account with no device at all.
pub fn verdict(created_at: Option<i64>, now: i64, device_platforms: &[String]) -> Verdict {
    if is_just_created(created_at, now) && device_platforms.is_empty() {
        Verdict::Reject
    } else {
        Verdict::Keep
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screened {
    Kept,
    /// The empty account was deleted on the server. Nothing was stored locally.
    Deleted,
}

/// Run right after the Google code exchange, before the session is saved or the computer is
/// registered. At most one read (`devices`) and, only on [`Verdict::Reject`], one delete.
pub async fn screen_google_sign_in(api: &Api, tokens: &TokenResponse, now: i64) -> Result<Screened, ApiError> {
    let created_at = tokens.user.created_at.as_deref().and_then(unix_from_rfc3339);
    // Cheap pre-filter: an account older than 10 minutes is real; no request needed.
    if !is_just_created(created_at, now) {
        return Ok(Screened::Kept);
    }
    let platforms: Vec<String> = api.any_device(&tokens.access_token).await?.into_iter().map(|d| d.platform).collect();
    match verdict(created_at, now, &platforms) {
        Verdict::Keep => Ok(Screened::Kept),
        Verdict::Reject => {
            api.delete_account(&tokens.access_token).await?;
            Ok(Screened::Deleted)
        }
    }
}

/// Parse GoTrue's timestamps (`2026-10-17T12:00:00.123456Z`, or with a `+hh:mm` offset) into
/// Unix seconds. Anything else is `None`, which keeps the account. Avoids a date crate for one field.
pub fn unix_from_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || !matches!(b[10], b'T' | b't' | b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        let part = s.get(from..to)?;
        part.bytes().all(|c| c.is_ascii_digit()).then(|| part.parse().ok())?
    };
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }

    // Skip fractional seconds, then read the zone.
    let mut rest = &s[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        rest = &frac[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hm = rest[1..].replace(':', "");
            if hm.len() != 4 || !hm.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            sign * (hm[..2].parse::<i64>().ok()? * 3600 + hm[2..].parse::<i64>().ok()? * 60)
        }
    };
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's days_from_civil).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_792_238_400; // 2026-10-17T12:00:00Z

    fn platforms(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn old_account_is_kept_even_with_no_devices() {
        assert_eq!(verdict(Some(NOW - JUST_CREATED_SECS), NOW, &[]), Verdict::Keep);
        assert_eq!(verdict(Some(NOW - 86_400 * 30), NOW, &[]), Verdict::Keep);
    }

    #[test]
    fn young_account_with_a_phone_is_kept() {
        assert_eq!(verdict(Some(NOW - 60), NOW, &platforms(&["android"])), Verdict::Keep);
        assert_eq!(verdict(Some(NOW - 60), NOW, &platforms(&["ios"])), Verdict::Keep);
    }

    #[test]
    fn young_account_with_only_a_computer_is_kept() {
        assert_eq!(verdict(Some(NOW - 60), NOW, &platforms(&["macos"])), Verdict::Keep);
        assert_eq!(verdict(Some(NOW - 60), NOW, &platforms(&["windows"])), Verdict::Keep);
    }

    #[test]
    fn young_account_with_no_devices_is_rejected() {
        assert_eq!(verdict(Some(NOW), NOW, &[]), Verdict::Reject);
        assert_eq!(verdict(Some(NOW - 5), NOW, &[]), Verdict::Reject);
        assert_eq!(verdict(Some(NOW - JUST_CREATED_SECS + 1), NOW, &[]), Verdict::Reject);
    }

    #[test]
    fn clock_skew_or_unknown_age_keeps_the_account() {
        assert_eq!(verdict(Some(NOW + 1), NOW, &[]), Verdict::Keep, "created_at in the future");
        assert_eq!(verdict(Some(NOW + 3600), NOW, &[]), Verdict::Keep);
        assert_eq!(verdict(None, NOW, &[]), Verdict::Keep, "no created_at");
    }

    #[test]
    fn parses_gotrue_timestamps() {
        assert_eq!(unix_from_rfc3339("2026-10-17T12:00:00Z"), Some(NOW));
        assert_eq!(unix_from_rfc3339("2026-10-17T12:00:00.123456Z"), Some(NOW));
        assert_eq!(unix_from_rfc3339("2026-10-17T17:30:00+05:30"), Some(NOW));
        assert_eq!(unix_from_rfc3339("2026-10-17T11:00:00.5-01:00"), Some(NOW));
        assert_eq!(unix_from_rfc3339("2000-02-29T23:59:59Z"), Some(951_868_799), "leap day");
        assert_eq!(unix_from_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        for bad in ["", "2026-10-17", "2026-10-17T12:00:00", "2026-13-17T12:00:00Z", "2026-10-17T12:00:00.Z", "soon"] {
            assert_eq!(unix_from_rfc3339(bad), None, "{bad:?}");
        }
    }
}
