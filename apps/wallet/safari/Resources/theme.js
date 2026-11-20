// The theme the person chose in Orien Wallet, remembered from last time so
// the window opens in it; popup.js updates it once the app answers.
try { const t = localStorage.getItem("theme"); if (t) document.documentElement.dataset.theme = t; } catch {}
