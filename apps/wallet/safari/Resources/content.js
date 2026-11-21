// On orien.network's sign-in pages (its account page, and Sign in with
// Orien's): the page always shows a "Sign in with Orien Wallet" button.
// This tells the page the extension is here, and how things stand, on
// <html data-orien-wallet="…">:
//   ready       Orien Wallet is running and signed in to Orien Network
//   app-closed  Orien Wallet isn't running on this computer
//   signed-out  it's running, but not signed in to Orien Network
// Without the attribute the page offers to install the extension.
// When ready, a real click on the button is taken here (the page's own
// handler doesn't run): Orien Wallet hands over a
// one-minute, single-use pass, which the page trades for a sign-in. No
// wallet keys are used.
const ask = (message) => (globalThis.browser || chrome).runtime.sendMessage(message);
// The answer, or undefined if none came in `ms` (the browser can lose a
// question on its way to Orien Wallet; null means the app isn't running).
const within = (asked, ms) => Promise.race([asked.catch(() => null), new Promise((done) => setTimeout(done, ms))]);
const html = document.documentElement;

async function check() {
  const wallet = await ask({ ask: "wallet" }).catch(() => null);
  html.dataset.orienWallet = !wallet ? "app-closed" : wallet.signed_in ? "ready" : "signed-out";
}
check();
// Looked at again whenever the person comes back to the page, so opening
// the app or signing in to it is noticed.
document.addEventListener("visibilitychange", () => { if (!document.hidden) check(); });

// A pass from Orien Wallet. While waiting it asks every few seconds whether
// Orien Wallet is confirming; if it isn't, the question was lost on the
// way, so it is asked again (once). Gives up after two minutes.
async function signIn() {
  const until = Date.now() + 120_000;
  let asked = ask({ ask: "signin" });
  let again = 1;
  while (Date.now() < until) {
    const answer = await within(asked, 3000);
    if (answer !== undefined) return answer;
    const wallet = await within(ask({ ask: "wallet" }), 3000);
    if (wallet === null) return null;
    // Older versions of the app don't say; then just wait.
    if (wallet && "confirming" in wallet && !wallet.confirming) {
      if (!again--) break;
      asked = ask({ ask: "signin" });
    }
  }
  return { problem: "Orien Wallet didn't answer." };
}

// The button's words, leaving anything else in it (the sun) alone.
function words(button) {
  const walk = document.createTreeWalker(button, NodeFilter.SHOW_TEXT, (node) =>
    node.textContent.trim() && !node.parentElement.closest(".orien-mark") ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP);
  let found = null;
  while (walk.nextNode()) found = walk.currentNode;
  return found || button.appendChild(document.createTextNode(""));
}

document.addEventListener("click", async (event) => {
  const button = event.target instanceof Element && event.target.closest("[data-orien-wallet-signin]");
  if (!button || !event.isTrusted || html.dataset.orienWallet !== "ready") return;
  event.preventDefault();
  event.stopImmediatePropagation();
  if (button.dataset.busy) return;
  button.dataset.busy = "1";
  const text = words(button);
  const label = text.textContent;
  text.textContent = "Signing in…";
  const answer = await signIn();
  if (answer && answer.pass) {
    location.hash = "pass=" + answer.pass;
    location.reload();
    return;
  }
  delete button.dataset.busy;
  text.textContent = label;
  if (!answer) html.dataset.orienWallet = "app-closed";
  else if (answer.problem && answer.problem.startsWith("Sign in to Orien Network")) html.dataset.orienWallet = "signed-out";
}, true);
