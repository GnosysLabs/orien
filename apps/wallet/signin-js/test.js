// Checks orien-sign-in.js against proofs made by the Rust code:
//   ORIEN_SIGNIN_VECTORS=/tmp/v.json cargo test -p orien-crypto write_vectors -- --ignored
//   node apps/wallet/signin-js/test.js /tmp/v.json
const assert = require("assert");
const { verifySignIn, blake3 } = require("./orien-sign-in.js");
const v = JSON.parse(require("fs").readFileSync(process.argv[2], "utf8"));
const bytes = (h) => Uint8Array.from(Buffer.from(h, "hex"));
const b64 = (h) => Buffer.from(h, "hex").toString("base64url");
const hex = (u) => Buffer.from(u).toString("hex");

for (const [input, expected] of v.blake3) assert.strictEqual(hex(blake3(bytes(input))), expected, `blake3 of ${input.length / 2} bytes`);

const site = "https://example.com";
assert.strictEqual(verifySignIn(b64(v.good), { origin: site, nonce: "n-1", now: 1010 }), v.address);
assert.strictEqual(verifySignIn(bytes(v.good), { origin: site, nonce: "n-1", now: 1010 }), v.address);
assert.strictEqual(verifySignIn(b64(v.long), { origin: v.long_site, nonce: "x".repeat(200), now: 1010 }), v.address);
assert.throws(() => verifySignIn(b64(v.forged), { origin: site, nonce: "n-1", now: 1010 }), /doesn't match/);
assert.throws(() => verifySignIn(b64(v.good), { origin: "https://evil.example", nonce: "n-1", now: 1010 }), /another website/);
assert.throws(() => verifySignIn(b64(v.good), { origin: site, nonce: "n-2", now: 1010 }), /asked for/);
assert.throws(() => verifySignIn(b64(v.good), { origin: site, nonce: "n-1", now: 1300 }), /expired/);
assert.throws(() => verifySignIn("not a proof", { origin: site, nonce: "n-1", now: 1010 }), /isn't an Orien Wallet sign-in/);
// Any single changed byte is refused.
const good = bytes(v.good);
for (const at of [200, 1500, 3000, 6000, good.length - 5]) {
  const bent = good.slice(); bent[at] ^= 1;
  assert.throws(() => verifySignIn(bent, { origin: site, nonce: "n-1", now: 1010 }));
}
// profile(): the public profile's shape, read from orien.network.
(async () => {
  const { profile } = require("./orien-sign-in.js");
  const sample = { id: 12, name: "Maya", picture: "abc.jpg", wallet_name: "Savings", bio: "I draw logos.",
    picture_url: "https://orien.network/api/market/images/abc.jpg", banner_url: null, profile_url: "https://orien.network/u/Maya/",
    links: [{ kind: "x", handle: "maya_draws", url: "https://x.com/maya_draws" }, { kind: "discord", handle: "maya.draws", url: null }] };
  global.fetch = async (url) => url.endsWith("a".repeat(64)) ? { status: 200, ok: true, json: async () => sample } : { status: 404, ok: false };
  const p = await profile("A".repeat(64));
  assert.deepStrictEqual(p, { name: "Maya", bio: "I draw logos.", wallet: "Savings", picture: sample.picture_url, banner: null,
    profile: sample.profile_url, links: [{ kind: "x", handle: "maya_draws", url: "https://x.com/maya_draws" }, { kind: "discord", handle: "maya.draws", url: null }] });
  assert.strictEqual(await profile("b".repeat(64)), null);
  await assert.rejects(profile("nope"));
  console.log("orien-sign-in.js: all checks pass");
})();
