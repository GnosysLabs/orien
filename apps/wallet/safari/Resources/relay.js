// On a website the person allowed: passes the page's sign-in requests
// (from window.orien, provider.js) to Orien Wallet, and the answers back.
// The website's address is taken by the browser, so a page can only ever
// ask to sign in to itself.
(() => {
  if (globalThis.__orienRelay) return;
  globalThis.__orienRelay = true;
  const api = globalThis.browser || chrome;
  // Safari adds window.orien from here; Chrome and Firefox add it
  // themselves, straight into the page.
  if (!/Chrome\/|Firefox\//.test(navigator.userAgent)) {
    const script = document.createElement("script");
    script.src = api.runtime.getURL("provider.js");
    script.onload = () => script.remove();
    (document.head || document.documentElement).append(script);
  }
  const closed = "Orien Wallet isn't running on this computer. Open it, then try again.";
  window.addEventListener("message", async (event) => {
    if (event.source !== window || !event.data || event.data.orienAsk === undefined || typeof event.data.signIn !== "string") return;
    let answer = null;
    try { answer = await api.runtime.sendMessage({ ask: "signin-site", nonce: event.data.signIn }); } catch {}
    const reply = !answer || answer.closed ? { problem: closed }
      : answer.problem ? { problem: answer.problem }
      : { result: { address: answer.address, proof: answer.proof } };
    window.postMessage({ orienAnswer: event.data.orienAsk, ...reply }, location.origin);
  });
})();
