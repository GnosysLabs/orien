// Websites the person lets ask for sign-in with Orien Wallet: each is
// allowed one at a time, from the extension's window, and only those get
// window.orien.
const sitesApi = globalThis.browser || chrome;
const SCRIPT_IDS = ["orien-provider", "orien-relay"];
const SAFARI = !/Chrome\/|Firefox\//.test(navigator.userAgent);

// The websites allowed so far, as match patterns: "https://example.com/*".
async function allowedSites() {
  const { origins = [] } = await sitesApi.permissions.getAll();
  return origins.filter((o) => /^https?:\/\//.test(o) && !o.startsWith("http://127.0.0.1:47211") && !o.startsWith("https://orien.network"));
}

// Keeps window.orien on exactly the allowed websites.
async function syncSites() {
  if (!sitesApi.scripting || !sitesApi.permissions || !sitesApi.runtime.getManifest().optional_host_permissions) return;
  const sites = await allowedSites();
  try { await sitesApi.scripting.unregisterContentScripts({ ids: SCRIPT_IDS }); } catch {}
  if (!sites.length) return;
  // Safari: relay.js adds window.orien to the page itself.
  await sitesApi.scripting.registerContentScripts([
    ...(SAFARI ? [] : [{ id: SCRIPT_IDS[0], matches: sites, js: ["provider.js"], runAt: "document_start", world: "MAIN", allFrames: false }]),
    { id: SCRIPT_IDS[1], matches: sites, js: ["relay.js"], runAt: "document_start", allFrames: false },
  ]);
}

// A website just allowed: its open tabs get window.orien at once, without
// reloading.
async function startOn(patterns) {
  const tabs = await sitesApi.tabs.query({ url: patterns }).catch(() => []);
  for (const tab of tabs) {
    if (!SAFARI) sitesApi.scripting.executeScript({ target: { tabId: tab.id }, files: ["provider.js"], world: "MAIN" }).catch(() => {});
    sitesApi.scripting.executeScript({ target: { tabId: tab.id }, files: ["relay.js"] }).catch(() => {});
  }
}

if (sitesApi.permissions && sitesApi.permissions.onAdded) {
  sitesApi.permissions.onAdded.addListener(async (added) => {
    await syncSites();
    if (added.origins && added.origins.length) startOn(added.origins);
  });
  sitesApi.permissions.onRemoved.addListener(() => syncSites());
  sitesApi.runtime.onInstalled.addListener(() => syncSites());
  sitesApi.runtime.onStartup.addListener(() => syncSites());
}

// The website a sign-in request came from, as the browser reports it.
function siteOf(sender) {
  try { return new URL(sender.origin || sender.url).origin; } catch { return null; }
}
