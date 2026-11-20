// Safari: questions go to Orien Wallet through the app's own extension.
window.orienAsk = (message) => browser.runtime.sendNativeMessage("network.orien.wallet", message);
