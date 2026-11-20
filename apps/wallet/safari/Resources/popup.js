// The extension holds no wallet: it asks Orien Wallet, running on this
// Mac, for the address and balance, and asks it to send. Orien Wallet
// approves each payment with Touch ID or the password, and signs it.
const $ = (id) => document.getElementById(id);
const ask = (message) => browser.runtime.sendNativeMessage("network.orien.wallet", message);
const show = (id) => { for (const s of ["home", "send", "sent"]) $(s).hidden = s !== id; window.scrollTo(0, 0); };
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
  if (!wallet || !wallet.address) {
    $("balance").textContent = "—";
    $("home-buttons").hidden = true;
    return home("Orien Wallet isn't running. Open it once; it keeps running in the menu bar after you close its window.");
  }
  const [whole, cents] = (wallet.balance || "—").split(".");
  $("balance").textContent = whole;
  if (cents) { const c = document.createElement("span"); c.className = "cents"; c.textContent = "." + cents; $("balance").append(c); }
  $("balance").title = wallet.balance ? `${wallet.balance} ORN` : "";
  const name = wallet.name || "Wallet";
  $("wallet-name").textContent = name;
  $("wallet-coin").textContent = wallet.icon || name.slice(0, 1).toUpperCase();
  $("wallet-coin").className = wallet.icon ? "coin emoji" : "coin";
  $("wallet").title = name;
  $("wallet").hidden = false;
  $("home-buttons").hidden = false;
  home();
}

$("copy").onclick = async () => {
  await navigator.clipboard.writeText(wallet.address);
  $("copy").textContent = "Copied";
  setTimeout(() => { $("copy").textContent = "Copy address"; }, 1500);
};

// ---- Send ----
const cleanTo = () => $("to").value.replace(/\s+/g, "");
const toOk = () => /^[0-9a-f]{64}$/i.test(cleanTo()) && cleanTo().toLowerCase() !== wallet.address.toLowerCase();
let quoted = null;

$("go-send").onclick = () => {
  show("send");
  $("pw-field").hidden = wallet.approve !== "password";
  $("send-go").textContent = wallet.approve === "touch" ? "Send with Touch ID" : "Send";
  $("send-note").hidden = true;
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
$("to").oninput = ready;
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
    $("sent-to").textContent = `to ${spaced(answer.to).slice(0, 19)}…`;
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

$("sent-done").onclick = () => { $("to").value = $("amount").value = ""; quoted = null; load(); };

load();
