// Passes the sign-in page's two questions to Orien Wallet on this
// computer: is it running and signed in, and (after a click) a pass that
// signs this browser in.
const ORIEN = "http://127.0.0.1:47211";
chrome.runtime.onMessage.addListener((message, _sender, reply) => {
  if (!message || (message.ask !== "wallet" && message.ask !== "signin")) return;
  fetch(`${ORIEN}/${message.ask}`, { method: message.ask === "wallet" ? "GET" : "POST", headers: { "X-Orien-Extension": "1" } })
    .then((answer) => answer.json())
    .then(reply, () => reply(null));
  return true;
});
