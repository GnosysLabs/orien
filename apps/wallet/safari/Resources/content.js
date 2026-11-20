// On orien.network's sign-in pages (its account page, and Sign in with
// Orien's): the page always shows a "Sign in with Orien Wallet" button.
// This tells the page the extension is here, and how things stand, on
// <html data-orien-wallet="…">:
//   ready       Orien Wallet is running and signed in to Orien Network
//   app-closed  Orien Wallet isn't running on this computer
//   signed-out  it's running, but not signed in to Orien Network
// Without the attribute the page offers to install the extension.
// When ready, a real click on the button is taken here (the page's own
// handler doesn't run): Touch ID on Macs, then Orien Wallet hands over a
// one-minute, single-use pass, which the page trades for a sign-in. No
// wallet keys are used.
const ask = (message) => (globalThis.browser || chrome).runtime.sendMessage(message);
const html = document.documentElement;

async function check() {
  const wallet = await ask({ ask: "wallet" }).catch(() => null);
  html.dataset.orienWallet = !wallet ? "app-closed" : wallet.signed_in ? "ready" : "signed-out";
}
check();
// Looked at again whenever the person comes back to the page, so opening
// the app or signing in to it is noticed.
document.addEventListener("visibilitychange", () => { if (!document.hidden) check(); });

document.addEventListener("click", async (event) => {
  const button = event.target instanceof Element && event.target.closest("[data-orien-wallet-signin]");
  if (!button || !event.isTrusted || html.dataset.orienWallet !== "ready") return;
  event.preventDefault();
  event.stopImmediatePropagation();
  if (button.dataset.busy) return;
  button.dataset.busy = "1";
  const label = button.textContent;
  button.textContent = /Macintosh/.test(navigator.userAgent) ? "Confirm with Touch ID…" : "Signing in…";
  const answer = await ask({ ask: "signin" }).catch(() => null);
  if (answer && answer.pass) {
    location.hash = "pass=" + answer.pass;
    location.reload();
    return;
  }
  delete button.dataset.busy;
  button.textContent = label;
  if (!answer) html.dataset.orienWallet = "app-closed";
  else if (answer.problem && answer.problem.startsWith("Sign in to Orien Network")) html.dataset.orienWallet = "signed-out";
}, true);
