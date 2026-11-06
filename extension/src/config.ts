// Build settings. `npm run build` makes two flavors from the same code: prod ("Warden") and
// staging ("Warden β", the test backend). scripts/build.mjs sets __WARDEN_ENV__ and reads BACKENDS
// for the manifest; the unit tests run the sources directly and get staging.
//
// The Supabase URL and publishable key are public by design: every Warden client ships them,
// and the server protects data with row-level security and the user's own token.

declare const __WARDEN_ENV__: "prod" | "staging";
const ENV = typeof __WARDEN_ENV__ === "undefined" ? "staging" : __WARDEN_ENV__;

export const BACKENDS = {
  prod: {
    supabaseUrl: "https://gnyfundwghbhnepqhkur.supabase.co",
    publishableKey: "sb_publishable_3Id4nmxiPQ4uDopYLS9gqw_FTKY7GqP",
    nativeHost: "in.weenja.warden",
  },
  staging: {
    supabaseUrl: "https://kgsrczwqhqcjdbeanlhh.supabase.co",
    publishableKey: "sb_publishable_9bMuN208fLWpQv0l0jq50w_7NV95sKh",
    nativeHost: "in.weenja.warden.staging",
  },
};

export const SUPABASE_URL = BACKENDS[ENV].supabaseUrl;
export const PUBLISHABLE_KEY = BACKENDS[ENV].publishableKey;

/** The Warden desktop app's native messaging host (see extension/README.md). */
export const NATIVE_HOST = BACKENDS[ENV].nativeHost;

/** The only redirect the Supabase project allows. The extension never lets it reach the OS. */
export const REDIRECT_URI = "warden://auth-callback";

/** Where "Get Warden for your phone" points. Accounts are made in the phone app. */
export const GET_WARDEN_URL = "https://getwarden.app";
