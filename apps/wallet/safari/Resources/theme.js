// The theme the person chose in Orien Wallet, remembered from last time so
// the window opens in it; popup.js updates it once the app answers.
try { const t = localStorage.getItem("theme"); if (t) document.documentElement.dataset.theme = t; } catch {}
// Safari draws the popup's arrow in its own glass: there the popup is that
// glass too, with the wallet's panel set inside it (Chrome has no arrow).
if (!/Chrome\//.test(navigator.userAgent)) document.documentElement.classList.add("safari");
