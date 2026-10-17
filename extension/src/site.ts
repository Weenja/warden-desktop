// Which site a tab is on, and whether it is guarded. The desktop app has the same rule (and the
// server too, in `spend-tick`), so a site counts the same everywhere.

/** Lowercase hostname of an http(s) URL, or null for anything else (new tab, settings, files). */
export function siteOf(url: string | undefined | null): string | null {
  if (!url) return null;
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
  const host = parsed.hostname.toLowerCase().replace(/\.$/, "");
  return host || null;
}

/**
 * The guarded site that covers `host`, or null. A host is covered when it equals a guarded site
 * or ends with "." + it (`m.youtube.com` is covered by `youtube.com`, `notyoutube.com` is not).
 * When several cover it, the most specific (longest) wins, as on the server.
 */
export function guardedSiteFor(host: string | null, guardedSites: readonly string[]): string | null {
  if (!host) return null;
  let best: string | null = null;
  for (const raw of guardedSites) {
    const site = raw.toLowerCase();
    if (!site) continue;
    if ((host === site || host.endsWith("." + site)) && (best === null || site.length > best.length)) {
      best = site;
    }
  }
  return best;
}
