// Blocking guarded sites at 0 points.
//
// Two layers:
// 1. A declarativeNetRequest `block` rule for top-level pages of the guarded sites, so a page
//    never loads (or plays) while locked. Block rules need no host permission.
// 2. Any tab on a guarded site is sent to the extension's own blocked page, which says why.
// Both are removed when points come back.

import { ext } from "../ext.ts";
import { guardedSiteFor, siteOf } from "../site.ts";

const RULE_ID = 1;

export const blockedPageUrl = (site: string) => ext.runtime.getURL(`blocked.html?site=${encodeURIComponent(site)}`);

/** `sites` empty = nothing blocked. Idempotent: the rule is replaced each time. */
export async function setBlockRule(sites: readonly string[]): Promise<void> {
  const addRules: chrome.declarativeNetRequest.Rule[] = sites.length
    ? [
        {
          id: RULE_ID,
          priority: 1,
          action: { type: "block" as chrome.declarativeNetRequest.RuleActionType },
          condition: {
            requestDomains: [...sites],
            resourceTypes: ["main_frame" as chrome.declarativeNetRequest.ResourceType],
          },
        },
      ]
    : [];
  await ext.declarativeNetRequest.updateDynamicRules({ removeRuleIds: [RULE_ID], addRules });
}

/** Send one tab to the blocked page if it is on a guarded site. */
export async function blockTabIfGuarded(tabId: number, url: string | undefined, sites: readonly string[]) {
  const site = guardedSiteFor(siteOf(url), sites);
  if (site) await ext.tabs.update(tabId, { url: blockedPageUrl(site) }).catch(() => undefined);
}

/** At the moment of locking: every open tab on a guarded site gets the blocked page. */
export async function blockOpenTabs(sites: readonly string[]): Promise<void> {
  const tabs = await ext.tabs.query({});
  await Promise.all(tabs.map((t) => (t.id === undefined ? null : blockTabIfGuarded(t.id, t.url, sites))));
}
