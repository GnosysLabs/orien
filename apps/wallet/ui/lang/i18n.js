// Orien Wallet language packs: offline JSON dictionaries with lossless DOM translation.
// Never translates user-generated content (chat messages, posts, usernames, addresses, recovery words).
(() => {
  const LANGS = [
    { code: "en", native: "English", english: "English" },
    { code: "es", native: "Español", english: "Spanish" },
    { code: "zh", native: "中文", english: "Chinese (Simplified)" },
    { code: "ja", native: "日本語", english: "Japanese" },
    { code: "ko", native: "한국어", english: "Korean" },
    { code: "pt", native: "Português", english: "Portuguese" },
    { code: "fr", native: "Français", english: "French" },
    { code: "de", native: "Deutsch", english: "German" },
  ];
  const KNOWN = new Set(LANGS.map((l) => l.code));
  const SKIP_TAGS = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "SVG", "CODE", "PRE", "TEXTAREA", "INPUT"]);
  const SKIP_SEL = "[data-no-i18n], .codebox, #sd-pay-to, #phrase-grid, #rv-words, #cm-feed .cm-text, #cm-thread .cm-text, .dm-body, .gc-body, .mk-desc, .mk-title, .addr, .hash";
  const ATTRS = ["placeholder", "aria-label", "title"];
  const CHECK_SVG = '<svg class="lang-check" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>';

  const cache = { en: { strings: {}, patterns: [], guides: {} } };
  const origText = new WeakMap();
  const lastText = new WeakMap();
  const origAttr = new WeakMap();
  const lastAttr = new WeakMap();

  let currentLang = "en";
  let currentPack = cache.en;
  let applying = false;
  let pendingNodes = new Set();
  let scheduled = false;

  const normKey = (s) => s.replace(/\s+/g, " ").replace(/[\u2018\u2019]/g, "'").trim();

  function translateCore(raw) {
    if (!raw || currentLang === "en") return raw;
    const m = raw.match(/^(\s*(?:[·•]\s*)?)([\s\S]*?)(\s*)$/);
    if (!m || !m[2]) return raw;
    const core = m[2];
    if (!/[A-Za-z]/.test(core)) return raw;
    if (/^[0-9a-f]{24,}$/i.test(core)) return raw;
    const key = normKey(core);
    const exact = currentPack.strings[key];
    if (exact !== undefined) return m[1] + exact + m[3];
    for (const [re, tpl] of currentPack.patterns) {
      const hit = key.match(re);
      if (hit) {
        const out = tpl.replace(/\{(\d+)\}/g, (_, i) => hit[Number(i) + 1] ?? "");
        return m[1] + out + m[3];
      }
    }
    return raw;
  }

  function skipTextNode(node) {
    const p = node.parentElement;
    if (!p) return true;
    if (SKIP_TAGS.has(p.tagName)) return true;
    if (p.closest(SKIP_SEL)) return true;
    return false;
  }

  function processText(node) {
    if (skipTextNode(node)) return;
    const cur = node.nodeValue || "";
    if (!origText.has(node) || (lastText.has(node) && cur !== lastText.get(node))) {
      origText.set(node, cur);
    }
    const base = origText.get(node);
    const next = currentLang === "en" ? base : translateCore(base);
    if (next !== cur) {
      lastText.set(node, next);
      node.nodeValue = next;
    } else if (currentLang === "en") {
      lastText.delete(node);
    }
  }

  function processAttrs(el) {
    if (!el || el.nodeType !== 1 || el.closest("[data-no-i18n]")) return;
    let oMap = origAttr.get(el);
    let lMap = lastAttr.get(el);
    for (const attr of ATTRS) {
      if (!el.hasAttribute(attr)) continue;
      const cur = el.getAttribute(attr) || "";
      if (!oMap) { oMap = {}; origAttr.set(el, oMap); }
      if (!lMap) { lMap = {}; lastAttr.set(el, lMap); }
      if (!(attr in oMap) || (attr in lMap && cur !== lMap[attr])) {
        oMap[attr] = cur;
      }
      const base = oMap[attr];
      const next = currentLang === "en" ? base : translateCore(base);
      if (next !== cur) {
        lMap[attr] = next;
        el.setAttribute(attr, next);
      } else if (currentLang === "en") {
        delete lMap[attr];
      }
    }
  }

  function walk(root) {
    if (!root) return;
    applying = true;
    try {
      if (root.nodeType === 3) {
        processText(root);
        return;
      }
      if (root.nodeType === 1) processAttrs(root);
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
      let n;
      while ((n = walker.nextNode())) {
        if (n.nodeType === 3) processText(n);
        else processAttrs(n);
      }
    } finally {
      applying = false;
    }
  }

  function flush() {
    scheduled = false;
    if (currentLang === "en" || !pendingNodes.size) { pendingNodes.clear(); return; }
    const batch = [...pendingNodes];
    pendingNodes.clear();
    for (const n of batch) {
      if (n.isConnected) walk(n);
    }
  }

  const observer = new MutationObserver((records) => {
    if (applying || currentLang === "en") return;
    for (const r of records) {
      if (r.type === "characterData") pendingNodes.add(r.target);
      else if (r.type === "attributes") pendingNodes.add(r.target);
      else if (r.type === "childList") {
        for (const n of r.addedNodes) pendingNodes.add(n);
      }
    }
    if (!scheduled && pendingNodes.size) {
      scheduled = true;
      queueMicrotask(flush);
    }
  });

  async function loadPack(code) {
    if (cache[code]) return cache[code];
    const res = await fetch(`lang/${code}.json`);
    if (!res.ok) throw new Error(`language pack ${code} not found`);
    const raw = await res.json();
    const strings = {};
    for (const [k, v] of Object.entries(raw)) {
      if (k === "_patterns" || k === "_meta" || k === "_guides" || typeof v !== "string") continue;
      strings[normKey(k)] = v;
    }
    const patterns = Array.isArray(raw._patterns)
      ? raw._patterns.map(([src, tpl]) => [new RegExp(src), tpl])
      : [];
    const guides = raw._guides && typeof raw._guides === "object" ? raw._guides : {};
    cache[code] = { strings, patterns, guides };
    return cache[code];
  }

  function paintLangList() {
    const list = document.getElementById("st-lang-list");
    const cur = document.getElementById("st-lang-cur");
    const active = LANGS.find((l) => l.code === currentLang) || LANGS[0];
    if (cur) cur.textContent = active.native;
    if (!list) return;
    list.innerHTML = LANGS.map((l) =>
      `<button class="set-row${l.code === currentLang ? " on" : ""}" data-lang="${l.code}" aria-pressed="${l.code === currentLang}"><span class="set-txt"><b>${l.native}</b><small>${l.english}</small></span>${CHECK_SVG}</button>`
    ).join("");
    for (const btn of list.querySelectorAll("[data-lang]")) {
      btn.onclick = () => setLang(btn.dataset.lang, true);
    }
  }

  async function setLang(code, save) {
    if (!KNOWN.has(code)) code = "en";
    try {
      const pack = await loadPack(code);
      currentLang = code;
      currentPack = pack;
      document.documentElement.lang = code;
      if (save) {
        try { localStorage.setItem("app-lang", code); } catch {}
        if (window.__TAURI__ && window.__TAURI__.core) {
          window.__TAURI__.core.invoke("set_app_pref", { name: "lang", value: code }).catch(() => {});
        }
      }
      paintLangList();
      if (window.__orienRepaintGuide) window.__orienRepaintGuide();
      walk(document.body);
    } catch (e) {
      console.error("Could not load language pack:", code, e);
    }
  }

  window.__orienSetLang = setLang;
  window.__orienLangs = LANGS;
  window.__orienGuide = (id) => (currentPack && currentPack.guides && currentPack.guides[id]) || null;
  window.t = (s) => translateCore(String(s));

  function boot() {
    paintLangList();
    observer.observe(document.body, {
      childList: true,
      subtree: true,
      characterData: true,
      attributes: true,
      attributeFilter: ATTRS,
    });
    let saved = "en";
    try { saved = localStorage.getItem("app-lang") || "en"; } catch {}
    if (saved && saved !== "en" && KNOWN.has(saved)) setLang(saved, false);
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot);
  else boot();
})();
