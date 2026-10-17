// What the user is looking at: the site of the active tab in the last focused window, whether a
// browser window has focus, and whether the user stepped away. Read fresh on each change, so
// nothing is lost when the browser stops the background.

import { ext } from "../ext.ts";
import { siteOf } from "../site.ts";

/** No input for this long counts as away (unless the tab plays audio: a video keeps draining). */
export const IDLE_AFTER_SECS = 5 * 60;

export type Front = {
  site: string | null;
  tabId: number | null;
  /** A browser window has focus. Firefox on Android has no windows API: always true there. */
  focused: boolean;
  /** Screen locked, or no input for 5 min and the tab is not playing audio. */
  idle: boolean;
};

export async function readFront(): Promise<Front> {
  const [tab] = await ext.tabs.query({ active: true, lastFocusedWindow: true });
  const win = ext.windows ? await ext.windows.getLastFocused().catch(() => null) : null;
  // `idle` is missing on Firefox for Android; there the screen turning off suspends the browser.
  const state = ext.idle ? await ext.idle.queryState(IDLE_AFTER_SECS) : "active";
  return {
    site: siteOf(tab?.url),
    tabId: tab?.id ?? null,
    focused: win ? win.focused : true,
    idle: state === "locked" || (state === "idle" && !tab?.audible),
  };
}
