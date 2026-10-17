// The toolbar icon and its badge: points on mint, amber when low, coral at 0 (locked), and a
// grey icon with no badge when not linked or not signed in.

import { ext } from "../ext.ts";
import type { View } from "../view.ts";

const MINT = "#3DDC97";
const AMBER = "#FFB938";
const CORAL = "#FF7A5C";
const BADGE_INK = "#14110F";

const icons = (prefix: string) => ({
  16: `icons/${prefix}-16.png`,
  32: `icons/${prefix}-32.png`,
  48: `icons/${prefix}-48.png`,
});

export async function paintBadge(view: View): Promise<void> {
  const active = view.signedIn && view.mode !== "checking" && view.points !== null;
  await ext.action.setIcon({ path: icons(active ? "icon" : "grey") });
  if (!active) {
    await ext.action.setBadgeText({ text: "" });
    return;
  }
  const points = view.points ?? 0;
  const color = view.locked ? CORAL : view.low ? AMBER : MINT;
  await ext.action.setBadgeBackgroundColor({ color });
  await ext.action.setBadgeTextColor?.({ color: BADGE_INK });
  await ext.action.setBadgeText({ text: view.locked ? "0" : points > 999 ? "999+" : String(points) });
}
