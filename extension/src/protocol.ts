// Messages between this extension and the Warden desktop app (native messaging, JSON).
// The browser frames each message with a 4-byte length; we only see the objects.
// The full protocol is in EXTENSION-BRIEF.md ("Native messaging protocol").

export type ToApp =
  | { type: "hello"; ext_version: string }
  /** The front tab's site (lowercase host, null for non-web pages) and whether the user is away. */
  | { type: "tab"; site: string | null; idle: boolean }
  /** The user pressed "Open Warden": the app shows its sign-in window or popover. */
  | { type: "show_app" };

export type AppState = {
  type: "state";
  signed_in: boolean;
  points: number;
  low: boolean;
  /** Guarded sites are blocked: 0 points, token expired offline, or today's max spent. */
  locked: boolean;
  guarded_sites: string[];
  draining: { site: string; per_min: number } | null;
  device_name: string | null;
  /** Optional: today's earning cap in points, for the battery. Hidden when the app leaves it out. */
  daily_max?: number;
};

export type FromApp =
  | { type: "hello"; app_version: string }
  | AppState
  /** Pill Stop, or a lock while a guarded site is open: close that tab. */
  | { type: "close_tab"; site: string };
