// Passes websites' questions to Orien Wallet: orien.network's sign-in page
// (is it running and signed in, and a pass that signs this browser in),
// and, on any website, signing in with the wallet (window.orien).
// sites.js comes first: listed before this file (Firefox) or loaded here
// (a service worker: Safari).
if (typeof siteOf === "undefined" && typeof importScripts === "function") importScripts("sites.js");
const api = globalThis.browser || chrome;
const toApp = (message) => api.runtime.sendNativeMessage("network.orien.wallet", message);
api.runtime.onMessage.addListener((message, sender) => {
  if (!message) return;
  if (message.ask === "wallet" || message.ask === "signin") return toApp({ ask: message.ask });
  if (message.ask === "signin-site" && sender.tab) {
    const origin = siteOf(sender);
    if (!origin) return Promise.resolve({ problem: "This website can't be signed in to." });
    return toApp({ ask: "signin-site", origin, nonce: String(message.nonce || "") });
  }
});
