// On orien.network's sign-in pages (its account page, and Sign in with
// Orien's), wherever the page has the wallet button: when Orien Wallet is
// running and signed in to Orien Network, the page's "Sign in with Orien
// Wallet" button shows. A click asks for Touch ID; Orien Wallet then hands
// over a one-minute, single-use pass, which the page trades for a sign-in.
// No wallet keys are used.
const ask = (message) => (globalThis.browser || chrome).runtime.sendMessage(message);
let ready = false;
const wired = new WeakSet();

function reveal() {
  if (!ready) return;
  for (const button of document.querySelectorAll("[data-orien-wallet-signin]")) {
    if (button.hidden) button.hidden = false;
    if (wired.has(button)) continue;
    wired.add(button);
    button.addEventListener("click", async () => {
      const label = button.textContent;
      button.disabled = true;
      button.textContent = "Confirm with Touch ID…";
      const answer = await ask({ ask: "signin" }).catch(() => null);
      if (answer && answer.pass) {
        location.hash = "pass=" + answer.pass;
        location.reload();
        return;
      }
      button.disabled = false;
      button.textContent = label;
      const problem = answer && answer.problem;
      if (problem && problem !== "Not confirmed.") button.title = problem;
    });
  }
}

ask({ ask: "wallet" }).then((wallet) => {
  ready = !!(wallet && wallet.signed_in);
  reveal();
}).catch(() => {});
new MutationObserver(reveal).observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ["hidden"] });
