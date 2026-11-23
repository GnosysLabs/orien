// Chrome: questions go straight to Orien Wallet on this computer, over a
// connection that only this extension (and Safari's) may use.
const ORIEN = "http://127.0.0.1:47211";
window.orienAsk = async (message) => {
  const { ask, ...body } = message;
  const get = ["wallet", "people", "wallets", "activity"].includes(ask);
  let answer;
  try {
    answer = await fetch(`${ORIEN}/${ask}`, {
      method: get ? "GET" : "POST",
      headers: { "X-Orien-Extension": "1", ...(get ? {} : { "Content-Type": "application/json" }) },
      body: get ? undefined : JSON.stringify(body),
    });
  } catch {
    // Orien Wallet isn't running.
    if (ask === "wallet") throw new Error("closed");
    return { closed: true };
  }
  return answer.json();
};
