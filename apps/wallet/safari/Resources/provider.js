// Orien Wallet on a website, for websites to call: window.orien.
//
//   const { address, proof } = await window.orien.signIn({ nonce });
//
// The nonce comes from the website's own server, which then checks the
// proof (orien-sign-in.js, from orien.network). Orien Wallet asks the
// person first; the website's address is added by the browser, not the page.
(() => {
  if (window.orien) return;
  const waiting = new Map();
  let count = 0;
  window.addEventListener("message", (event) => {
    if (event.source !== window || !event.data || event.data.orienAnswer === undefined) return;
    const done = waiting.get(event.data.orienAnswer);
    if (!done) return;
    waiting.delete(event.data.orienAnswer);
    if (event.data.problem) done.reject(new Error(event.data.problem));
    else done.resolve(event.data.result);
  });
  const ask = (question) => new Promise((resolve, reject) => {
    const id = `${Date.now()}-${++count}`;
    waiting.set(id, { resolve, reject });
    window.postMessage({ orienAsk: id, ...question }, location.origin);
  });
  const orien = Object.freeze({
    isOrien: true,
    version: 1,
    /** Asks Orien Wallet to sign in to this website: { address, proof }. */
    signIn({ nonce } = {}) {
      if (typeof nonce !== "string" || !nonce || nonce.length > 200) {
        return Promise.reject(new Error("signIn needs a nonce from your server (up to 200 characters)."));
      }
      return ask({ signIn: nonce });
    },
  });
  Object.defineProperty(window, "orien", { value: orien, writable: false, configurable: false });
  window.dispatchEvent(new Event("orien#initialized"));
})();
