// The in-page points chip (standalone only: linked, the desktop app shows its own pill).
//
// It runs only on guarded sites the user allowed: the popup asks for those sites with
// `permissions.request` (optional host permissions, user gesture), then we register a content
// script for exactly those sites. The chip reads its numbers from `storage.local` ("chip") and
// sends nothing.

import { ext } from "../ext.ts";

const SCRIPT_ID = "chip";

export type ChipData = { show: boolean; points: number; perMin: number; low: boolean };

/** Match patterns for a guarded site and its subdomains. */
export const sitePatterns = (sites: readonly string[]) => sites.flatMap((s) => [`*://${s}/*`, `*://*.${s}/*`]);

/** Register the chip for the guarded sites we have permission for; remove it otherwise. */
export async function syncChipScript(on: boolean, sites: readonly string[]): Promise<void> {
  const allowed: string[] = [];
  if (on) {
    for (const site of sites) {
      if (await ext.permissions.contains({ origins: sitePatterns([site]) })) allowed.push(site);
    }
  }
  const existing = await ext.scripting.getRegisteredContentScripts({ ids: [SCRIPT_ID] });
  const matches = sitePatterns(allowed);
  if (existing.length && JSON.stringify(existing[0].matches) === JSON.stringify(matches)) return;
  if (existing.length) await ext.scripting.unregisterContentScripts({ ids: [SCRIPT_ID] });
  if (!matches.length) return;
  await ext.scripting.registerContentScripts([
    { id: SCRIPT_ID, js: ["chip.js"], matches, runAt: "document_idle", persistAcrossSessions: true },
  ]);
}

let lastChip = "";

/** Publish the chip's numbers (only when they changed: every write wakes every chip). */
export async function publishChip(data: ChipData): Promise<void> {
  const key = JSON.stringify(data);
  if (key === lastChip) return;
  lastChip = key;
  await ext.storage.local.set({ chip: data });
}
