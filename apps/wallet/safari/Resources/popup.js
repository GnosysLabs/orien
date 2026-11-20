// The extension holds no wallet: it asks Orien Wallet, which leaves the
// address and balance in the folder the two share.
const $ = (id) => document.getElementById(id);
browser.runtime.sendNativeMessage("network.orien.wallet", { ask: "wallet" }).then((wallet) => {
  if (!wallet || !wallet.address) {
    $("balance").textContent = "–";
    $("note").textContent = "Open Orien Wallet once, and your wallet shows here.";
    return;
  }
  $("balance").innerHTML = "";
  $("balance").append(wallet.balance || "–");
  const unit = document.createElement("small"); unit.textContent = "ORN"; $("balance").append(unit);
  $("address").textContent = wallet.address.replace(/(.{4})/g, "$1 ").trim();
  $("copy").hidden = false;
  $("copy").onclick = async () => {
    await navigator.clipboard.writeText(wallet.address);
    $("copy").textContent = "Copied";
  };
}).catch((error) => {
  $("balance").textContent = "–";
  $("note").textContent = "Orien Wallet didn't answer: " + error;
});
