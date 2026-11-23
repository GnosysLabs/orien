// The extension holds no wallet: it asks Orien Wallet, running on this
// Mac, for the address and balance, and asks it to send. Orien Wallet
// approves each payment with Touch ID or the password, and signs it.
const $ = (id) => document.getElementById(id);
// How to reach Orien Wallet differs by browser: bridge.js says (Safari
// through the app's extension, Chrome over this computer's own connection).
const ask = (message) => window.orienAsk(message);
const show = (id) => { for (const s of ["home", "send", "sent", "wallets"]) $(s).hidden = s !== id; window.scrollTo(0, 0); };
const spaced = (address) => address.replace(/(.{4})/g, "$1 ").trim();
let wallet = null;

function home(note) {
  show("home");
  $("home-note").hidden = !note;
  $("home-note").textContent = note || "";
}

async function load() {
  try {
    wallet = await ask({ ask: "wallet" });
  } catch { wallet = null; }
  if (wallet && wallet.theme !== undefined) {
    if (wallet.theme) document.documentElement.dataset.theme = wallet.theme; else delete document.documentElement.dataset.theme;
    try { localStorage.setItem("theme", wallet.theme || ""); } catch {}
  }
  if (!wallet || !wallet.address) {
    $("balance").textContent = "—";
    $("home-buttons").hidden = true;
    return home("Orien Wallet isn't running. Open it once; it keeps running in the menu bar after you close its window.");
  }
  const name = wallet.name || "Wallet";
  $("wallet-name").textContent = name;
  $("wallet-coin").textContent = wallet.icon || name.slice(0, 1).toUpperCase();
  $("wallet-coin").className = wallet.icon ? "coin emoji" : "coin";
  $("wallet").title = name;
  $("wallet").hidden = false;
  $("home-buttons").hidden = false;
  home();
  paintBalance();   // once Home shows: hidden, it has no room to measure
  loadRecent();
}

// The balance as the app shows it: the shorter form (two decimals from
// 1 ORN up; under 1 ORN, enough decimals for its first two digits) until
// it's tapped for every decimal. Cut, never rounded up.
let full = false;
try { full = localStorage.getItem("balance-full") === "on"; } catch {}
function shortBalance(text) {
  const m = /^(-?[\d,]+)(?:\.(\d*))?$/.exec(String(text).trim());
  if (!m) return text;
  const whole = m[1], frac = (m[2] || "").padEnd(9, "0");
  if (/[1-9]/.test(whole)) return `${whole}.${frac.slice(0, 2)}`;
  const first = frac.search(/[1-9]/);
  return first < 0 ? `${whole}.00` : `${whole}.${frac.slice(0, Math.max(2, first + 2))}`;
}
function paintBalance() {
  const balance = wallet && wallet.balance;
  const el = $("balance");
  el.textContent = !balance ? "—" : full ? balance : shortBalance(balance);
  el.title = !balance ? "" : full ? "Click for the shorter balance" : "Click to see every decimal";
  // Smaller until it fits beside "ORN".
  el.style.fontSize = "";
  const room = el.parentElement.clientWidth - el.nextElementSibling.offsetWidth - 8;
  if (room <= 0) return;
  for (let size = 36; size > 16 && el.scrollWidth > room; size -= 1) el.style.fontSize = size - 1 + "px";
}
$("balance-card").onclick = () => {
  if (!wallet || !wallet.balance) return;
  full = !full;
  try { localStorage.setItem("balance-full", full ? "on" : "off"); } catch {}
  paintBalance();
  renderRecent();
};

$("copy").onclick = async () => {
  await navigator.clipboard.writeText(wallet.address);
  $("copy").textContent = "Copied";
  setTimeout(() => { $("copy").textContent = "Copy address"; }, 1500);
};

// ---- Send ----
// Names: the To box takes a name as well as an address. A picked name
// puts its address in the box and says who it is under it.
let people = { contacts: [], wallets: [] };
let market = { q: "", rows: [] }, marketTimer = 0;
const esc = (t) => String(t).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const short = (a) => `${a.slice(0, 4)} ${a.slice(4, 8)} … ${a.slice(-4)}`;
const isAddress = (t) => /^[0-9a-f]{64}$/i.test(t.replace(/\s+/g, ""));
function named(address) {
  const a = address.toLowerCase();
  const w = people.wallets.find((p) => p.address === a);
  if (w) return w.name;
  const c = people.contacts.find((p) => p.address === a);
  if (c) return c.name;
  const m = market.rows.find((r) => r.address === a);
  return m ? m.name : "";
}
function suggest() {
  const q = $("to").value.trim().toLowerCase();
  const list = $("suggest");
  if (!q || isAddress(q) || (chosen && $("to").value === chosen.name)) { list.hidden = true; list.replaceChildren(); return; }
  const places = [
    ...people.contacts.map((c) => ({ name: c.name, address: c.address, sub: short(c.address) })),
    ...people.wallets.map((w) => ({ name: w.name, address: w.address, sub: "Your wallet", icon: w.icon || "", mine: true })),
  ];
  // As the app does: a name matches where any of its words starts with
  // what's typed; an address only by its start, from 4 characters.
  const words = (name) => name.toLowerCase().split(/[\s\-–—_.]+/).filter(Boolean);
  const score = (p) => p.name.toLowerCase().startsWith(q) ? 0
    : words(p.name).some((w) => w.startsWith(q)) ? 1
    : q.length >= 4 && p.address.startsWith(q) ? 2 : 3;
  const found = places.filter((p) => score(p) < 3).sort((a, b) => score(a) - score(b) || a.name.localeCompare(b.name)).slice(0, 5);
  const known = new Set(found.map((p) => p.address));
  const extra = (market.q === q ? market.rows : []).filter((r) => !known.has(r.address)).slice(0, 4);
  if (q.length >= 2 && market.q !== q) searchMarket(q);
  const all = [...found, ...extra.map((r) => ({ name: r.name, address: r.address, sub: short(r.address), picture: r.picture }))];
  const unknown = all.filter((p) => !p.mine && !profiles.has(p.address)).map((p) => p.address);
  if (unknown.length) loadProfiles(unknown).then((got) => got && suggest());
  list.hidden = all.length === 0;
  list.replaceChildren(...all.map((p) => {
    const pick = make("button", "pick");
    pick.type = "button";
    pick.dataset.address = p.address;
    const who = make("span", "who");
    who.append(make("div", "title", p.name), make("div", p.mine ? "sub words" : "sub", p.sub));
    pick.append(faceOf(p), who);
    return pick;
  }));
}
// An element with a class and, if given, its text.
function make(tag, className, text) {
  const element = document.createElement(tag);
  element.className = className;
  if (text !== undefined) element.textContent = text;
  return element;
}
// Faces: a Market picture where the person has one, over their letter,
// as the app shows them.
const profiles = new Map(), pics = new Map(), picking = new Set();
async function loadProfiles(addresses) {
  const want = [...new Set(addresses)].filter((a) => !profiles.has(a)).slice(0, 100);
  if (!want.length) return false;
  const found = await ask({ ask: "profiles", addresses: want }).catch(() => null) || {};
  for (const a of want) profiles.set(a, found[a] || null);
  return want.some((a) => found[a] && found[a].picture);
}
function picture(name) {
  if (!name) return "";
  if (pics.has(name)) {
    if (!pics.get(name)) return "";
    const img = document.createElement("img");
    img.src = pics.get(name);
    img.alt = "";
    return img;
  }
  if (!picking.has(name)) {
    picking.add(name);
    ask({ ask: "image", name }).then((answer) => {
      pics.set(name, (answer && answer.url) || "");
      if (!$("suggest").hidden) suggest();
      if (!$("recent").hidden) renderRecent();
    }).catch(() => pics.set(name, ""));
  }
  return "";
}
const faceOf = (p) => {
  if (p.icon) return make("span", "coin emoji", p.icon);
  const info = profiles.get(p.address);
  const face = make("span", "coin", p.name.slice(0, 1).toUpperCase());
  face.append(picture((info && info.picture) || p.picture));
  return face;
};

function searchMarket(q) {
  clearTimeout(marketTimer);
  marketTimer = setTimeout(async () => {
    const found = await ask({ ask: "search", text: q }).catch(() => null);
    if (!Array.isArray(found)) return;
    const rows = [];
    for (const p of found) for (const w of p.wallets || []) {
      const address = (w.address || "").toLowerCase();
      if (!/^[0-9a-f]{64}$/.test(address) || address === wallet.address) continue;
      rows.push({ address, name: w.name ? `${p.name} – ${w.name}` : p.name, picture: p.picture || "" });
    }
    market = { q, rows };
    if ($("to").value.trim().toLowerCase() === q) suggest();
  }, 250);
}
$("suggest").onclick = (event) => {
  const pick = event.target.closest("[data-address]");
  if (!pick) return;
  chosen = { address: pick.dataset.address, name: named(pick.dataset.address) || pick.dataset.address };
  $("to").value = chosen.name;
  $("suggest").hidden = true;
  ready();
  $("amount").focus();
};

// A picked name stays in the box as the name; the address goes under it.
let chosen = null;
const cleanTo = () => chosen && $("to").value === chosen.name ? chosen.address : $("to").value.replace(/\s+/g, "");
const toOk = () => /^[0-9a-f]{64}$/i.test(cleanTo()) && cleanTo().toLowerCase() !== wallet.address.toLowerCase();
let quoted = null;

$("go-send").onclick = () => {
  show("send");
  $("pw-field").hidden = wallet.approve !== "password";
  $("send-go").textContent = wallet.approve === "touch" ? "Send with Touch ID" : "Send";
  $("send-note").hidden = true;
  ask({ ask: "people" }).then((p) => { if (p && p.contacts) people = p; }).catch(() => {});
  $("to").focus();
  ready();
};
$("back").onclick = () => home();

function ready() {
  const to = cleanTo();
  $("to-hint").className = "hint";
  $("to-hint").textContent = "";
  if (to && to.length >= 64 && !toOk()) {
    $("to-hint").className = "hint bad";
    $("to-hint").textContent = to.toLowerCase() === wallet.address.toLowerCase() ? "That's this wallet's own address." : "That isn't an Orien address.";
  } else if (toOk()) {
    const who = named(to);
    const picked = chosen && $("to").value === chosen.name;
    $("to-hint").className = "hint good";
    $("to-hint").textContent = picked ? short(to.toLowerCase()) : who ? `To ${who}` : `To ${short(to.toLowerCase())}`;
  }
  const pwOk = $("pw-field").hidden || $("pw").value.length > 0;
  $("send-go").disabled = !(toOk() && quoted && quoted.amount && !quoted.problem && pwOk);
}

let quoteTimer = 0;
async function quote() {
  const amount = $("amount").value.trim();
  quoted = null;
  $("amount-hint").className = "hint";
  $("amount-hint").textContent = "";
  ready();
  if (!amount) return;
  const q = await ask({ ask: "quote", amount }).catch(() => null);
  if ($("amount").value.trim() !== amount || !q) return;
  quoted = q;
  if (q.problem) { $("amount-hint").className = "hint bad"; $("amount-hint").textContent = q.problem; }
  else if (q.fee) $("amount-hint").textContent = `Fee ${q.fee} ORN · ${q.total} ORN in all`;
  ready();
}
$("amount").oninput = () => { clearTimeout(quoteTimer); quoteTimer = setTimeout(quote, 250); };
$("to").oninput = () => { if (chosen && $("to").value !== chosen.name) chosen = null; suggest(); ready(); };
$("pw").oninput = ready;
$("max").onclick = () => { if (wallet.most) { $("amount").value = wallet.most; quote(); } };

$("send-go").onclick = async () => {
  $("send-go").disabled = true;
  $("send-note").hidden = false;
  $("send-note").className = "note";
  $("send-note").textContent = wallet.approve === "touch" ? "Approve with Touch ID…" : "Sending…";
  const answer = await ask({ ask: "send", to: cleanTo(), amount: $("amount").value.trim(), password: $("pw").value }).catch(() => ({ closed: true }));
  $("pw").value = "";
  if (answer && answer.to) {
    $("sent-amount").textContent = `${answer.amount_text} ORN`;
    $("sent-to").textContent = `to ${named(answer.to) || short(answer.to)}`;
    $("sent-fee").textContent = `Fee ${answer.fee_text} ORN`;
    show("sent");
    return;
  }
  const problem = answer && answer.problem;
  $("send-note").className = "note bad";
  $("send-note").textContent = answer && answer.closed ? "Orien Wallet isn't running. Open it, then try again."
    : problem === "password: needed" ? "Type this wallet's password to send."
    : problem === "password: wrong" ? "That is not this wallet's password."
    : problem === "Not unlocked." ? "Not sent: Touch ID wasn't confirmed."
    : problem || "Not sent.";
  if (problem === "password: needed") $("pw-field").hidden = false;
  ready();
};

$("sent-done").onclick = () => { $("to").value = $("amount").value = ""; quoted = null; chosen = null; load(); };

// ---- Wallets: switch to another of the person's wallets ----
$("wallet").onclick = async () => {
  show("wallets");
  $("wallets-note").hidden = true;
  const list = await ask({ ask: "wallets" }).catch(() => null);
  const wallets = Array.isArray(list) ? list : [];
  $("wallet-list").replaceChildren(...wallets.map((w) => {
    const pick = make("button", "pick");
    pick.type = "button";
    const who = make("span", "who");
    who.append(make("div", "title", w.name), make("div", "sub", short(w.address)));
    pick.append(w.icon ? make("span", "coin emoji", w.icon) : make("span", "coin", w.name.slice(0, 1).toUpperCase()), who);
    if (w.current) {
      const tick = make("span", "tick-on", "✓");
      pick.append(tick);
    }
    pick.onclick = () => switchTo(w);
    return pick;
  }));
};
$("wallets-back").onclick = () => home();
async function switchTo(w) {
  if (w.current) return home();
  const answer = await ask({ ask: "use-wallet", address: w.address }).catch(() => null);
  if (!answer || answer.problem || answer.closed) {
    $("wallets-note").textContent = (answer && answer.problem) || "Orien Wallet isn't running.";
    $("wallets-note").hidden = false;
    return;
  }
  // The balance follows a moment later, as Orien Wallet refreshes.
  for (let tries = 0; tries < 12; tries++) {
    await load();
    if (wallet && wallet.address === w.address && wallet.balance) break;
    await new Promise((done) => setTimeout(done, 400));
  }
}

// ---- Recent: the three latest payments ----
const TITLES = {
  payment: "Received", claim: "Received", refund: "Refunded", fees: "Validator fees", mining: "Mining reward", unstaked: "Unstaked",
};
function when(time) {
  const d = new Date(time * 1000), today = new Date();
  if (d.toDateString() === today.toDateString()) return d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  const yesterday = new Date(today); yesterday.setDate(today.getDate() - 1);
  if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
  return d.toLocaleDateString([], { month: "short", day: "numeric" });
}
// The arrows the app shows for a payment with no picture.
function arrow(direction) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  for (const [k, v] of Object.entries({ width: 13, height: 13, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", "stroke-width": 2, "stroke-linecap": "round", "stroke-linejoin": "round" })) svg.setAttribute(k, v);
  for (const d of direction === "sent" ? ["M7 7h10v10", "M7 17 17 7"] : ["M17 7 7 17", "M17 17H7V7"]) {
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", d);
    svg.append(path);
  }
  return svg;
}
let recentEntries = [];
async function loadRecent() {
  const list = await ask({ ask: "activity" }).catch(() => null);
  recentEntries = Array.isArray(list) ? list.slice(0, 3) : [];
  $("recent").hidden = recentEntries.length === 0;
  if (!recentEntries.length) return;
  renderRecent();
  await loadProfiles(recentEntries.map((e) => e.counterparty).filter(Boolean));
  if (!people.contacts.length && !people.wallets.length) {
    const found = await ask({ ask: "people" }).catch(() => null);
    if (found && found.contacts) people = found;
  }
  renderRecent();
}
// As the app shows them: a person's picture over their first letter, or
// an arrow (green for money in).
function renderRecent() {
  $("txs").replaceChildren(...recentEntries.map((e) => {
    const sent = e.direction === "sent";
    const name = e.counterparty ? named(e.counterparty) : "";
    const info = e.counterparty && profiles.get(e.counterparty);
    const party = e.counterparty ? (name || (info && info.name) || short(e.counterparty)) : "";
    const title = sent ? "Sent" : TITLES[e.kind] || "Received";
    const sub = sent ? (party ? `To ${party}` : "Sent") : (party && (e.kind === "payment" || e.kind === "claim" || !e.kind) ? `From ${party}` : "");
    let icon;
    if (info && info.picture) {
      icon = make("span", "face", (name || info.name || "?").slice(0, 1).toUpperCase());
      icon.append(picture(info.picture));
    } else {
      icon = make("span", `tx-icon ${e.direction}`);
      icon.append(arrow(e.direction));
    }
    const main = make("div", "tx-main");
    main.append(make("div", "title", title));
    if (sub) main.append(make("div", "sub", sub));
    const side = make("div", "tx-side");
    side.append(make("div", `amt ${e.direction}`, (sent ? "−" : "+") + (full ? e.amount : shortBalance(e.amount))), make("div", "sub", when(e.time)));
    const row = make("div", "tx");
    row.append(icon, main, side);
    return row;
  }));
}
$("view-all").onclick = async () => {
  await ask({ ask: "open-app" }).catch(() => null);
  window.close();
};

// ---- Websites: signing in with Orien Wallet ----
// Chrome and Firefox give the extension every website when it's added.
// Safari leaves it to the person: until it's on, the window says how.
const tabsApi = globalThis.browser || globalThis.chrome;
const ALL_SITES = ["https://*/*", "http://localhost/*", "http://127.0.0.1/*"];
async function checkAccess() {
  if (!tabsApi || !tabsApi.permissions) return;
  const on = await tabsApi.permissions.contains({ origins: ["https://*/*"] }).catch(() => true);
  if (on) return;
  const text = $("allow-text"), go = $("allow-go");
  text.replaceChildren(make("b", "", "Sign in to websites with Orien Wallet"), document.createElement("br"), "Let websites ask Orien Wallet to sign you in. Nothing happens until you approve it in the app.");
  go.className = "primary pearl";
  go.textContent = "Turn on";
  // Asked straight from the click: browsers only ask the person then.
  go.onclick = () => {
    tabsApi.permissions.request({ origins: ALL_SITES }).then((granted) => {
      if (granted) $("allow").hidden = true;
    }, () => {
      text.replaceChildren("In Safari: Settings, then Extensions, then Orien Wallet: allow it on every website.");
      go.hidden = true;
    });
  };
  $("allow").hidden = false;
}
checkAccess();

load();
