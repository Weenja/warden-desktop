import assert from "node:assert/strict";
import { test } from "node:test";
import { guardedSiteFor, siteOf } from "../src/site.ts";

test("site of a tab: lowercase host of http(s) pages only", () => {
  assert.equal(siteOf("https://M.YouTube.com/watch?v=x"), "m.youtube.com");
  assert.equal(siteOf("http://example.com./a"), "example.com");
  assert.equal(siteOf("chrome://newtab/"), null);
  assert.equal(siteOf("moz-extension://abc/blocked.html?site=youtube.com"), null);
  assert.equal(siteOf("file:///Users/sam/a.html"), null);
  assert.equal(siteOf("not a url"), null);
  assert.equal(siteOf(undefined), null);
});

test("a host is guarded by an equal site or a parent domain, never a lookalike", () => {
  const sites = ["youtube.com", "reddit.com"];
  assert.equal(guardedSiteFor("youtube.com", sites), "youtube.com");
  assert.equal(guardedSiteFor("m.youtube.com", sites), "youtube.com");
  assert.equal(guardedSiteFor("notyoutube.com", sites), null);
  assert.equal(guardedSiteFor("youtube.com.evil.net", sites), null);
  assert.equal(guardedSiteFor("github.com", sites), null);
  assert.equal(guardedSiteFor(null, sites), null);
});

test("the most specific guarded site wins", () => {
  assert.equal(guardedSiteFor("music.youtube.com", ["youtube.com", "music.youtube.com"]), "music.youtube.com");
  assert.equal(guardedSiteFor("www.youtube.com", ["music.youtube.com", "youtube.com"]), "youtube.com");
  assert.equal(guardedSiteFor("m.youtube.com", ["YouTube.com"]), "youtube.com");
});
