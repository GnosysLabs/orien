// Passes websites' questions to Orien Wallet on this computer:
// orien.network's sign-in page (is it running and signed in, and after a
// click a pass that signs this browser in), and, on websites the person
// allowed (sites.js), signing in with the wallet.
importScripts("sites.js");
const ORIEN = "http://127.0.0.1:47211";
const toApp = (path, body) => fetch(`${ORIEN}/${path}`, {
  method: body ? "POST" : "GET",
  headers: { "X-Orien-Extension": "1", ...(body ? { "Content-Type": "application/json" } : {}) },
  body: body ? JSON.stringify(body) : undefined,
}).then((answer) => answer.json());
chrome.runtime.onMessage.addListener((message, sender, reply) => {
  if (!message) return;
  let asked;
  if (message.ask === "wallet") asked = toApp("wallet");
  else if (message.ask === "signin") asked = toApp("signin", {});
  else if (message.ask === "signin-site" && sender.tab) {
    const origin = siteOf(sender);
    asked = origin ? toApp("signin-site", { origin, nonce: String(message.nonce || "") }) : Promise.resolve({ problem: "This website can't be signed in to." });
  } else return;
  asked.then(reply, () => reply(null));   // null: Orien Wallet isn't running
  return true;
});
