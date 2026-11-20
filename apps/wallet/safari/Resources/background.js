// Passes the sign-in page's two questions to Orien Wallet: is it running
// and signed in, and (after Touch ID) a pass that signs this browser in.
browser.runtime.onMessage.addListener((message) => {
  if (message && (message.ask === "wallet" || message.ask === "signin")) {
    return browser.runtime.sendNativeMessage("network.orien.wallet", { ask: message.ask });
  }
});
