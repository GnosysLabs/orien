/*! Sign in with Orien Wallet: the checker for websites. MIT License, Gnosys Labs LLC.
 *
 * In the browser:
 *   <script src="https://orien.network/orien-sign-in.js"></script>
 *   const { address, proof } = await OrienSignIn.signIn({ nonce });   // nonce: from your server
 *
 * On your server (Node 18+, Deno, Bun, or the browser):
 *   const { verifySignIn } = require("./orien-sign-in.js");
 *   const address = verifySignIn(proof, { origin: "https://example.com", nonce });
 *   // Throws if the proof isn't good. Accept each nonce once, then forget it.
 *
 * Orien's signatures are quantum-safe one-time signatures built from the
 * BLAKE3 hash alone, so the checker needs nothing but BLAKE3, included
 * here. It needs no network and asks no server, orien.network included.
 */
(function (root) {
  "use strict";

  // ---------- BLAKE3 (hash mode only) ----------
  const IV = new Uint32Array([0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19]);
  const PERM = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];
  const CHUNK_START = 1, CHUNK_END = 2, PARENT = 4, ROOT = 8;
  const rotr = (x, n) => (x >>> n) | (x << (32 - n));

  function compress(cv, block, counter, blockLen, flags) {
    const s = new Uint32Array(16);
    s.set(cv, 0);
    s.set(IV.subarray(0, 4), 8);
    s[12] = counter >>> 0;
    s[13] = Math.floor(counter / 0x100000000) >>> 0;
    s[14] = blockLen;
    s[15] = flags;
    let m = Uint32Array.from(block);
    const g = (a, b, c, d, x, y) => {
      s[a] = (s[a] + s[b] + x) >>> 0; s[d] = rotr(s[d] ^ s[a], 16);
      s[c] = (s[c] + s[d]) >>> 0; s[b] = rotr(s[b] ^ s[c], 12);
      s[a] = (s[a] + s[b] + y) >>> 0; s[d] = rotr(s[d] ^ s[a], 8);
      s[c] = (s[c] + s[d]) >>> 0; s[b] = rotr(s[b] ^ s[c], 7);
    };
    for (let round = 0; round < 7; round++) {
      g(0, 4, 8, 12, m[0], m[1]); g(1, 5, 9, 13, m[2], m[3]); g(2, 6, 10, 14, m[4], m[5]); g(3, 7, 11, 15, m[6], m[7]);
      g(0, 5, 10, 15, m[8], m[9]); g(1, 6, 11, 12, m[10], m[11]); g(2, 7, 8, 13, m[12], m[13]); g(3, 4, 9, 14, m[14], m[15]);
      if (round < 6) m = Uint32Array.from(PERM, (i) => m[i]);
    }
    for (let i = 0; i < 8; i++) { s[i] ^= s[i + 8]; s[i + 8] ^= cv[i]; }
    return s;
  }

  function words(bytes, offset, length) {
    const block = new Uint32Array(16);
    for (let i = 0; i < length; i++) block[i >> 2] |= bytes[offset + i] << (8 * (i & 3));
    return block;
  }

  // One chunk (up to 1,024 bytes): its last block, not yet compressed, so
  // the caller can add ROOT when it is the only one.
  function chunkOutput(bytes, start, end, counter) {
    let cv = IV;
    let offset = start;
    let first = true;
    while (end - offset > 64) {
      cv = compress(cv, words(bytes, offset, 64), counter, 64, first ? CHUNK_START : 0).subarray(0, 8);
      offset += 64;
      first = false;
    }
    return { cv, block: words(bytes, offset, end - offset), counter, blockLen: end - offset, flags: (first ? CHUNK_START : 0) | CHUNK_END };
  }
  const chaining = (o) => compress(o.cv, o.block, o.counter, o.blockLen, o.flags).subarray(0, 8);
  const parentOutput = (left, right) => {
    const block = new Uint32Array(16);
    block.set(left, 0);
    block.set(right, 8);
    return { cv: IV, block, counter: 0, blockLen: 64, flags: PARENT };
  };

  function blake3(bytes) {
    const chunks = Math.max(1, Math.ceil(bytes.length / 1024));
    const stack = [];
    for (let c = 0; c < chunks - 1; c++) {
      let cv = chaining(chunkOutput(bytes, c * 1024, (c + 1) * 1024, c));
      let total = c + 1;
      while ((total & 1) === 0) { cv = chaining(parentOutput(stack.pop(), cv)); total >>= 1; }
      stack.push(cv);
    }
    let output = chunkOutput(bytes, (chunks - 1) * 1024, bytes.length, chunks - 1);
    while (stack.length) output = parentOutput(stack.pop(), chaining(output));
    const state = compress(output.cv, output.block, output.counter, output.blockLen, output.flags | ROOT);
    const out = new Uint8Array(32);
    for (let i = 0; i < 32; i++) out[i] = state[i >> 2] >>> (8 * (i & 3));
    return out;
  }

  // ---------- Orien's hashing ----------
  const TAG = { OtsChain: 0x01, OtsCommitment: 0x02, OtsMessage: 0x03, KeyTreeNode: 0x10, Address: 0x35, SignIn: 0x50, SignInTree: 0x51, SignInKey: 0x52 };
  const join = (...parts) => {
    const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
    let at = 0;
    for (const p of parts) { out.set(p, at); at += p.length; }
    return out;
  };
  const hash = (tag, ...parts) => blake3(join(Uint8Array.of(tag), ...parts));

  // One-time signatures (Winternitz, base 16): 51 chains of 24 bytes.
  const N = 24, TOP = 15, CHAINS = 51;
  function recoverOts(sig, message) {
    const digest = hash(TAG.OtsMessage, sig.pubSeed, message);
    const digits = [];
    for (let i = 0; i < N; i++) digits.push(digest[i] >> 4, digest[i] & 15);
    const checksum = digits.reduce((n, d) => n + (TOP - d), 0);
    digits.push((checksum >> 8) & 15, (checksum >> 4) & 15, checksum & 15);
    const ends = [];
    for (let chain = 0; chain < CHAINS; chain++) {
      let value = sig.chains[chain];
      for (let position = digits[chain]; position < TOP; position++) {
        value = blake3(join(Uint8Array.of(TAG.OtsChain), sig.pubSeed, Uint8Array.of(chain, position), value)).subarray(0, N);
      }
      ends.push(value);
    }
    return hash(TAG.OtsCommitment, sig.pubSeed, ...ends);
  }

  // A tree of one-time keys: the root this signature leads to, or null.
  function recoverRoot(height, sig, message) {
    if (sig.path.length !== height || sig.leaf >= 2 ** height) return null;
    let node = recoverOts(sig.ots, message);
    let position = sig.leaf;
    for (const sibling of sig.path) {
      node = position & 1 ? hash(TAG.KeyTreeNode, sibling, node) : hash(TAG.KeyTreeNode, node, sibling);
      position >>>= 1;
    }
    return node;
  }

  // ---------- The proof ----------
  const UPPER_HEIGHT = 8, BOTTOM_HEIGHT = 10, SPARE_HEIGHT = 4, BINDING_SPARE = 15;
  const MOST_SECONDS = 600, CLOCK_SLACK = 120;

  function fromBase64Url(text) {
    const b64 = text.replace(/-/g, "+").replace(/_/g, "/") + "===".slice((text.length + 3) % 4);
    if (typeof atob === "function") return Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    return new Uint8Array(Buffer.from(b64, "base64"));
  }
  const hex = (bytes) => Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  const same = (a, b) => a && b && a.length === b.length && a.every((x, i) => x === b[i]);

  function reader(bytes) {
    let at = 0;
    const need = (n) => { if (at + n > bytes.length) throw new Error("damaged proof"); };
    const take = (n) => { need(n); const out = bytes.subarray(at, at + n); at += n; return out; };
    const u32 = () => { const b = take(4); return (b[0] | (b[1] << 8) | (b[2] << 16) | (b[3] << 24)) >>> 0; };
    const u64 = () => { const low = u32(), high = u32(); return high * 0x100000000 + low; };
    const string = () => new TextDecoder("utf-8", { fatal: true }).decode(take(u32()));
    const tree = () => {
      const leaf = u32();
      const pubSeed = take(N);
      const chains = [];
      for (let i = 0; i < CHAINS; i++) chains.push(take(N));
      const count = u32();
      if (count > 32) throw new Error("damaged proof");
      const path = [];
      for (let i = 0; i < count; i++) path.push(take(32));
      return { leaf, ots: { pubSeed, chains }, path };
    };
    return { take, u32, u64, string, tree, get at() { return at; }, done: () => at === bytes.length };
  }

  function decode(proof) {
    const bytes = typeof proof === "string" ? fromBase64Url(proof) : proof;
    const r = reader(bytes);
    const message = { origin: r.string(), address: r.take(32), nonce: r.string(), issued: r.u64(), expires: r.u64() };
    const messageBytes = bytes.subarray(0, r.at);
    const bottom = r.tree();
    const upper = [r.tree(), r.tree(), r.tree()];
    const spare = r.tree();
    const firstKey = r.take(32);
    if (!r.done()) throw new Error("damaged proof");
    return { message, messageBytes, bottom, upper, spare, firstKey };
  }

  /**
   * Checks a sign-in proof from Orien Wallet. Returns the wallet's address
   * (64 hex characters) or throws an Error saying why not. `origin` is
   * your website as the browser shows it ("https://example.com"); `nonce`
   * is the one-time value your server handed out; `now` defaults to the
   * current time. Accept each nonce only once.
   */
  function verifySignIn(proof, { origin, nonce, now } = {}) {
    let p;
    try { p = decode(proof); } catch { throw new Error("This isn't an Orien Wallet sign-in."); }
    const m = p.message;
    const seconds = now === undefined ? Math.floor(Date.now() / 1000) : now;
    if (m.origin !== origin) throw new Error("It was signed for another website.");
    if (m.nonce !== nonce) throw new Error("It isn't the sign-in this website asked for.");
    if (m.expires <= m.issued || m.expires - m.issued > MOST_SECONDS || seconds >= m.expires || m.issued > seconds + CLOCK_SLACK) {
      throw new Error("It has expired.");
    }
    const bad = new Error("The signature doesn't match the wallet.");
    let node = recoverRoot(BOTTOM_HEIGHT, p.bottom, hash(TAG.SignIn, p.messageBytes));
    const [low, middle, top] = p.upper;
    const indices = [((top.leaf << 16) | (middle.leaf << 8) | low.leaf) >>> 0, ((top.leaf << 8) | middle.leaf) >>> 0, top.leaf];
    for (let level = 0; level < 3 && node; level++) {
      const index = new Uint8Array(4);
      new DataView(index.buffer).setUint32(0, indices[level], true);
      node = recoverRoot(UPPER_HEIGHT, p.upper[level], hash(TAG.SignInTree, Uint8Array.of(level), index, node));
    }
    if (!node || p.spare.leaf !== BINDING_SPARE) throw bad;
    const spareRoot = recoverRoot(SPARE_HEIGHT, p.spare, hash(TAG.SignInKey, node));
    if (!spareRoot || !same(hash(TAG.Address, p.firstKey, spareRoot), m.address)) throw bad;
    return hex(m.address);
  }

  /**
   * In the browser: asks Orien Wallet, through its extension, to sign in
   * to this website. Resolves to { address, proof }; send both to your
   * server, which checks them with verifySignIn.
   */
  async function signIn({ nonce } = {}) {
    const wait = (ms) => new Promise((done) => setTimeout(done, ms));
    if (!root.orien) await wait(300);   // the extension adds it as the page loads
    if (!root.orien) {
      throw new Error("Orien Wallet isn't set up for this website yet. Install the Orien Wallet extension (orien.network), "
        + "then click its button in the browser's toolbar and allow this website.");
    }
    return root.orien.signIn({ nonce });
  }

  /** A random one-time value, for servers that don't have their own. */
  function newNonce() {
    const bytes = new Uint8Array(16);
    (root.crypto || require("crypto").webcrypto).getRandomValues(bytes);
    return hex(bytes);
  }

  const api = { verifySignIn, signIn, newNonce, blake3 };
  if (typeof module === "object" && module.exports) module.exports = api;
  root.OrienSignIn = api;
})(typeof globalThis !== "undefined" ? globalThis : this);
