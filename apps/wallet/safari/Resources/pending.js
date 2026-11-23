// Websites' sign-in requests, waiting for the person in the extension's
// window, as Phantom asks: the window opens by itself with "Sign in to
// example.com?", and only Sign in sends the request on to Orien Wallet.
// The background that loads this sets `appSignIn(body)`, its way of
// asking Orien Wallet to sign.
const pendingApi = globalThis.browser || chrome;
const PENDING = new Map();
let pendingCount = 0;

// A website's request: waits for the person's answer in the window.
function askToSignIn(origin, nonce) {
  return new Promise((resolve) => {
    const id = `${Date.now()}-${++pendingCount}`;
    const timer = setTimeout(() => finishSignIn(id, { problem: "There was no answer in Orien Wallet." }), 150000);
    PENDING.set(id, { origin, nonce, resolve, timer });
    openWindow().then((opened) => {
      // A browser that can't open the window: Orien Wallet asks instead.
      if (!opened && PENDING.has(id)) {
        PENDING.delete(id);
        clearTimeout(timer);
        resolve(appSignIn({ origin, nonce }));
      }
    });
  });
}

async function openWindow() {
  try {
    if (pendingApi.action && pendingApi.action.openPopup) { await pendingApi.action.openPopup(); return true; }
  } catch {}
  try {
    await pendingApi.windows.create({ url: pendingApi.runtime.getURL("popup.html"), type: "popup", width: 404, height: 620 });
    return true;
  } catch {}
  return false;
}

function finishSignIn(id, answer) {
  const waiting = PENDING.get(id);
  if (!waiting) return;
  clearTimeout(waiting.timer);
  PENDING.delete(id);
  waiting.resolve(answer);
}

// The window's questions: what's waiting, and the person's answer.
function pendingMessage(message) {
  if (message.ask === "pending") {
    const last = [...PENDING.entries()].pop();
    return Promise.resolve(last ? { id: last[0], origin: last[1].origin } : null);
  }
  if (message.ask === "answer") {
    const waiting = PENDING.get(message.id);
    if (!waiting) return Promise.resolve({ problem: "The website stopped waiting." });
    if (!message.approve) {
      finishSignIn(message.id, { problem: "You chose not to sign in." });
      return Promise.resolve({ ok: true });
    }
    return appSignIn({ origin: waiting.origin, nonce: waiting.nonce, approved: true, password: message.password || "" }).then((answer) => {
      // A password to type (first sign-in on this computer): keep waiting.
      if (answer && answer.problem && answer.problem.startsWith("password:")) return answer;
      finishSignIn(message.id, answer || { closed: true });
      return answer && answer.problem ? answer : { ok: true };
    }, () => {
      finishSignIn(message.id, { closed: true });
      return { closed: true };
    });
  }
  return null;
}
