// Build settings. Staging only for now; prod values come when Jojo approves prod.
//
// The Supabase URL and publishable key are public by design: every Warden client ships them,
// and the server protects data with row-level security and the user's own token.

export const SUPABASE_URL = "https://kgsrczwqhqcjdbeanlhh.supabase.co";
export const PUBLISHABLE_KEY = "sb_publishable_9bMuN208fLWpQv0l0jq50w_7NV95sKh";

/** The Warden desktop app's native messaging host (see extension/README.md). */
export const NATIVE_HOST = "in.weenja.warden.staging";

/** The only redirect the Supabase project allows. The extension never lets it reach the OS. */
export const REDIRECT_URI = "warden://auth-callback";

/** Where "Get Warden for your phone" points. Accounts are made in the phone app. */
export const GET_WARDEN_URL = "https://getwarden.app";
