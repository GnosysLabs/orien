"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import {
  ArrowLeftRight,
  ArrowRight,
  Clock,
  ShieldCheck,
  Wallet,
} from "lucide-react";
import {
  ago,
  ask,
  formatOrn,
  formatSol,
  sentence,
  type SwapOffer,
  profileHref,
} from "@/lib/market";
import { Avatar } from "./Avatar";
import { useMe } from "@/lib/useMe";
import { Notice } from "./Shell";

const card = "rounded-2xl border border-line bg-surface";

/** Dollars, rounded for reading: "$654", "$1.31", "$0.0042". */
const dollars = (usd: number) =>
  usd >= 100
    ? "$" + Math.round(usd).toLocaleString("en")
    : usd >= 1
      ? "$" + usd.toFixed(2)
      : "$" + usd.toPrecision(3);

/** The offer's total price in whole SOL. */
const totalSol = (offer: SwapOffer) => offer.lamports / 1e9;

/** Its price for one ORN, in SOL, for sorting and spread math. */
const unitPrice = (offer: SwapOffer) => totalSol(offer) / (offer.orn / 1e9);

/** Compact SOL price for one ORN in an order-book row. */
const fmtSolUnit = (value: number) => {
  if (value >= 1) return value.toFixed(3);
  if (value >= 0.01) return value.toFixed(4);
  return Number(value.toPrecision(4)).toString();
};

/** Compact SOL total for a row. */
const fmtSolAmount = (amount: number) =>
  amount >= 10
    ? amount.toFixed(2)
    : amount >= 0.1
      ? amount.toFixed(3)
      : Number(amount.toPrecision(3)).toString();

type Tab = "market" | "how";

/** A finished swap, as the public history shows it: no names. */
type Traded = {
  orn: number;
  coin: "sol" | "btc";
  lamports?: number;
  sats?: number;
  usd: number | null;
  at: number;
};

const paidSol = (t: Traded) => (t.lamports ?? 0) / 1e9;
const tradedUnitSol = (t: Traded) => paidSol(t) / (t.orn / 1e9);
const tradedUnitUsd = (t: Traded) =>
  t.usd === null || t.orn <= 0 ? null : t.usd / (t.orn / 1e9);

export function SwapBoard() {
  const [offers, setOffers] = useState<SwapOffer[] | null>(null);
  const [solUsd, setSolUsd] = useState<number | null>(null);
  const [trouble, setTrouble] = useState("");
  const [tab, setTab] = useState<Tab>("market");
  const [last, setLast] = useState<Traded | null>(null);
  const [swaps, setSwaps] = useState<Traded[] | null>(null);
  const [more, setMore] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [historyTrouble, setHistoryTrouble] = useState("");

  useEffect(() => {
    const read = () => {
      const hash = window.location.hash.replace("#", "");
      if (hash === "how") {
        setTab("how");
      } else {
        setTab("market");
        if (hash === "history") {
          setTimeout(() => {
            document
              .getElementById("history")
              ?.scrollIntoView({ behavior: "smooth" });
          }, 80);
        }
      }
    };
    read();
    window.addEventListener("hashchange", read);
    return () => window.removeEventListener("hashchange", read);
  }, []);

  const show = (next: Tab) => {
    setTab(next);
    window.history.replaceState(null, "", next === "how" ? "#how" : "#market");
  };

  const loadOffers = () =>
    ask<{
      offers: SwapOffer[];
      solana: { usd: number } | null;
    }>("/swaps?coins=sol&sides=sell,buy")
      .then((found) => {
        setOffers(found.offers);
        setSolUsd(found.solana?.usd ?? null);
        setTrouble("");
      })
      .catch((error: Error) => {
        setOffers((had) => had ?? []);
        setTrouble(sentence(error.message));
      });

  const loadHistory = async (before?: number) => {
    if (before) setLoadingMore(true);
    try {
      const found = await ask<{
        last: Traded | null;
        swaps: Traded[];
        more: boolean;
      }>(`/swaps/history${before ? `?before=${before}` : ""}`);
      const solSwaps = found.swaps.filter((t) => t.coin === "sol");
      if (!before) {
        setLast(found.last?.coin === "sol" ? found.last : solSwaps[0] ?? null);
      }
      setSwaps((had) => (before && had ? [...had, ...solSwaps] : solSwaps));
      setMore(found.more);
      setHistoryTrouble("");
    } catch (error) {
      setSwaps((had) => had ?? []);
      setHistoryTrouble(sentence((error as Error).message));
    }
    if (before) setLoadingMore(false);
  };

  useEffect(() => {
    void loadOffers();
    void loadHistory();
    const timer = setInterval(() => {
      void loadOffers();
      void loadHistory();
    }, 20000);
    return () => clearInterval(timer);
  }, []);

  const { account } = useMe();
  const solOffers = offers?.filter((o) => o.coin === "sol") ?? null;
  const visible = (o: SwapOffer) =>
    o.seller_online !== false || o.seller === account?.id;

  const allAsks = solOffers?.filter((o) => o.side !== "buy") ?? [];
  const allBids = solOffers?.filter((o) => o.side === "buy") ?? [];
  const asksAsc = allAsks
    .filter(visible)
    .sort((a, b) => unitPrice(a) - unitPrice(b));
  const bidsDesc = allBids
    .filter(visible)
    .sort((a, b) => unitPrice(b) - unitPrice(a));
  const offlineCount =
    allAsks.filter((o) => !visible(o)).length +
    allBids.filter((o) => !visible(o)).length;

  const bestAsk = asksAsc[0] ?? null;
  const bestBid = bidsDesc[0] ?? null;
  const spread =
    bestAsk && bestBid ? unitPrice(bestAsk) - unitPrice(bestBid) : null;
  const spreadPct =
    spread !== null && bestAsk && unitPrice(bestAsk) > 0
      ? (spread / unitPrice(bestAsk)) * 100
      : null;

  const openOrnVolume = [...asksAsc, ...bidsDesc].reduce(
    (sum, o) => sum + o.orn / 1e9,
    0,
  );

  return (
    <div className="space-y-6">
      {/* Top exchange header bar */}
      <section className={`${card} p-5 sm:p-6`}>
        <div className="flex flex-wrap items-center justify-between gap-5">
          <div className="flex items-center gap-3 rounded-xl border border-line bg-bg px-4 py-2.5">
            <span className="text-base font-bold tracking-tight text-ink">
              ORN / SOL
            </span>
            <span className="rounded-full bg-raised px-2 py-0.5 text-[11px] font-semibold text-muted">
              Direct swap
            </span>
          </div>

          <div className="flex flex-1 flex-wrap items-center justify-between gap-x-6 gap-y-3 sm:px-2">
            <div>
              <p className="text-[11px] font-semibold uppercase tracking-wider text-muted">
                Last price
              </p>
              {last ? (
                <div className="mt-1 flex flex-wrap items-baseline gap-2">
                  <span className="num text-base font-bold tracking-tight text-ink">
                    {tradedUnitUsd(last) !== null
                      ? dollars(tradedUnitUsd(last)!)
                      : `${fmtSolUnit(tradedUnitSol(last))} SOL`}
                  </span>
                  <span className="num text-xs text-muted">
                    {fmtSolUnit(tradedUnitSol(last))} SOL
                  </span>
                </div>
              ) : (
                <p className="num mt-1 text-base font-bold text-ink">—</p>
              )}
            </div>
            <div>
              <p className="text-[11px] font-semibold uppercase tracking-wider text-muted">
                Lowest ask
              </p>
              <p className="num mt-1 text-base font-bold text-stop">
                {bestAsk ? `${fmtSolUnit(unitPrice(bestAsk))} SOL` : "—"}
              </p>
            </div>
            <div>
              <p className="text-[11px] font-semibold uppercase tracking-wider text-muted">
                Highest bid
              </p>
              <p className="num mt-1 text-base font-bold text-go">
                {bestBid ? `${fmtSolUnit(unitPrice(bestBid))} SOL` : "—"}
              </p>
            </div>
            <div>
              <p className="text-[11px] font-semibold uppercase tracking-wider text-muted">
                Open book
              </p>
              <p className="num mt-1 text-base font-bold text-ink">
                {openOrnVolume > 0
                  ? `${Math.round(openOrnVolume).toLocaleString("en")} ORN`
                  : "0 ORN"}
              </p>
            </div>
            <div>
              <p className="text-[11px] font-semibold uppercase tracking-wider text-muted">
                1 SOL
              </p>
              <p className="num mt-1 text-base font-bold text-ink">
                {solUsd ? dollars(solUsd) : "—"}
              </p>
            </div>
          </div>

          <Link
            href="/try/"
            className="pearl-fill flex h-10 shrink-0 items-center gap-2 rounded-xl px-4 text-sm font-bold transition hover:brightness-105"
          >
            Trade in Orien Wallet <ArrowRight className="h-4 w-4" aria-hidden />
          </Link>
        </div>
      </section>

      {/* View switcher */}
      <div
        role="tablist"
        aria-label="Swap market views"
        className="flex gap-1 border-b border-line"
      >
        {(
          [
            { id: "market", label: "Order book & chart" },
            { id: "how", label: "How direct swaps work" },
          ] as const
        ).map(({ id, label }) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={tab === id}
            onClick={() => show(id)}
            className={`-mb-px border-b-2 px-4 pb-3 pt-1 text-sm font-semibold transition ${
              tab === id
                ? "border-pearl text-ink"
                : "border-transparent text-muted hover:text-ink"
            }`}
          >
            {label}
          </button>
        ))}
      </div>

      {tab === "market" && (
        <div className="space-y-6">
          {(trouble || historyTrouble) && (
            <Notice tone="bad">{trouble || historyTrouble}</Notice>
          )}
          {/* Primary exchange grid: Chart + Trades on left, Stacked Order Book on right */}
          <div className="grid items-stretch gap-6 lg:grid-cols-[minmax(0,1.45fr)_minmax(0,1fr)]">
            <div className="flex flex-col space-y-6">
              <PriceChart swaps={swaps} />

              {/* Recent trades tape */}
              <section id="history" className={`${card} flex-1 overflow-hidden`}>
                <div className="flex flex-wrap items-center justify-between gap-3 border-b border-line px-5 py-4">
                  <div>
                    <h2 className="text-base font-bold tracking-tight text-ink">
                      Recent swaps
                    </h2>
                    <p className="text-xs text-muted">
                      Completed ORN / SOL trades, newest first
                    </p>
                  </div>
                  {swaps && swaps.length > 0 && (
                    <span className="num text-xs text-muted">
                      {swaps.length} shown
                    </span>
                  )}
                </div>

                {swaps === null ? (
                  <p className="px-5 py-10 text-center text-sm text-muted">
                    Loading swaps…
                  </p>
                ) : swaps.length === 0 ? (
                  <p className="px-5 py-10 text-center text-sm text-muted">
                    No swaps completed yet. The next swap to finish appears here.
                  </p>
                ) : (
                  <div>
                    <div className="grid grid-cols-[1.15fr_1fr_1fr_auto] gap-3 border-b border-line bg-bg/50 px-5 py-2.5 text-[11px] font-semibold uppercase tracking-wider text-muted">
                      <span>Price / ORN</span>
                      <span className="text-right">Amount</span>
                      <span className="text-right">Total</span>
                      <span className="w-16 text-right">Time</span>
                    </div>
                    <ul>
                      {swaps.map((t, i) => {
                        const unitUsd = tradedUnitUsd(t);
                        const unitSol = tradedUnitSol(t);
                        return (
                          <li
                            key={`${t.at}-${i}`}
                            className="grid grid-cols-[1.15fr_1fr_1fr_auto] items-center gap-3 border-b border-line/70 px-5 py-3 text-sm last:border-b-0 hover:bg-raised/40"
                          >
                            <div className="min-w-0">
                              <p className="num font-semibold text-ink">
                                {unitUsd !== null
                                  ? dollars(unitUsd)
                                  : `${fmtSolUnit(unitSol)} SOL`}
                              </p>
                              {unitUsd !== null && (
                                <p className="num text-[11px] text-muted">
                                  {fmtSolUnit(unitSol)} SOL
                                </p>
                              )}
                            </div>
                            <p className="num text-right text-ink">
                              {formatOrn(t.orn)} ORN
                            </p>
                            <p className="num text-right text-muted">
                              {fmtSolAmount(paidSol(t))} SOL
                            </p>
                            <span className="w-16 text-right text-xs text-muted">
                              {ago(t.at)}
                            </span>
                          </li>
                        );
                      })}
                    </ul>
                    {more && (
                      <div className="border-t border-line px-5 py-3.5">
                        <button
                          type="button"
                          onClick={() => loadHistory(swaps[swaps.length - 1]?.at)}
                          disabled={loadingMore}
                          className="flex h-9 items-center rounded-xl border border-line bg-bg px-4 text-xs font-semibold text-ink transition hover:bg-raised disabled:opacity-60"
                        >
                          {loadingMore ? "Loading…" : "Load more swaps"}
                        </button>
                      </div>
                    )}
                  </div>
                )}
              </section>
            </div>

            {/* Right column: Stacked Exchange Order Book */}
            <aside className={`${card} flex flex-col overflow-hidden`}>
              <div className="flex items-center justify-between gap-3 border-b border-line px-5 py-4">
                <div>
                  <h2 className="text-base font-bold tracking-tight text-ink">
                    Order book
                  </h2>
                  <p className="text-xs text-muted">ORN / SOL · viewing only</p>
                </div>
                <span className="num rounded-full border border-line bg-bg px-2.5 py-1 text-xs font-semibold text-muted">
                  {asksAsc.length + bidsDesc.length} open
                </span>
              </div>

              {/* Column headings */}
              <div className="grid grid-cols-[1.15fr_0.95fr_0.9fr] gap-2 border-b border-line bg-bg/50 px-5 py-2.5 text-[11px] font-semibold uppercase tracking-wider text-muted">
                <span>Price (SOL)</span>
                <span className="text-right">Size (ORN)</span>
                <span className="text-right">Total (SOL)</span>
              </div>

              {/* Asks (Sell offers) — highest down to lowest closest to the spread */}
              <div>
                <div className="flex items-center justify-between px-5 pb-1.5 pt-3 text-[11px] font-semibold uppercase tracking-wider text-stop/90">
                  <span>Asks · selling ORN</span>
                  <span className="num text-muted">{asksAsc.length}</span>
                </div>
                {offers === null ? (
                  <p className="px-5 py-6 text-center text-xs text-muted">
                    Loading order book…
                  </p>
                ) : asksAsc.length === 0 ? (
                  <p className="px-5 py-6 text-center text-xs text-muted">
                    No open sell offers right now.
                  </p>
                ) : (
                  <BookRows
                    offers={[...asksAsc].reverse()}
                    side="ask"
                    solUsd={solUsd}
                  />
                )}
              </div>

              {/* Center spread & last-price bar */}
              <div className="my-2 flex flex-wrap items-center justify-between gap-2 border-y border-line bg-bg/80 px-5 py-3.5">
                <div className="flex items-baseline gap-2">
                  <span className="num text-lg font-bold tracking-tight text-ink">
                    {last
                      ? tradedUnitUsd(last) !== null
                        ? dollars(tradedUnitUsd(last)!)
                        : `${fmtSolUnit(tradedUnitSol(last))} SOL`
                      : bestAsk
                        ? `${fmtSolUnit(unitPrice(bestAsk))} SOL`
                        : "—"}
                  </span>
                  {last && <span className="text-xs text-muted">last swap</span>}
                </div>
                <span className="num text-xs text-muted">
                  {spread !== null
                    ? `Spread ${fmtSolUnit(Math.max(0, spread))} SOL${
                        spreadPct !== null && spreadPct >= 0
                          ? ` (${spreadPct.toFixed(1)}%)`
                          : ""
                      }`
                    : "Direct swaps · ~1 min settlement"}
                </span>
              </div>

              {/* Bids (Buy offers) — highest closest to the spread down to lowest */}
              <div className="flex-1 pb-3">
                <div className="flex items-center justify-between px-5 pb-1.5 pt-2 text-[11px] font-semibold uppercase tracking-wider text-go/90">
                  <span>Bids · buying ORN</span>
                  <span className="num text-muted">{bidsDesc.length}</span>
                </div>
                {offers === null ? (
                  <p className="px-5 py-6 text-center text-xs text-muted">
                    Loading…
                  </p>
                ) : bidsDesc.length === 0 ? (
                  <p className="px-5 py-6 text-center text-xs text-muted">
                    No open buy offers right now.
                  </p>
                ) : (
                  <BookRows offers={bidsDesc} side="bid" solUsd={solUsd} />
                )}
              </div>

              {/* Footer note inside the book card */}
              <div className="space-y-3 border-t border-line bg-bg/40 px-5 py-4">
                {offlineCount > 0 && (
                  <p className="text-xs text-muted">
                    {offlineCount === 1
                      ? "1 additional offer is paused while its owner's Orien Wallet is closed."
                      : `${offlineCount} additional offers are paused while their owners' Orien Wallets are closed.`}
                  </p>
                )}
                <p className="text-xs leading-relaxed text-muted">
                  To buy, sell, or post your own offer to this book, open the{" "}
                  <strong className="font-semibold text-ink">Swap</strong> tab
                  in Orien Wallet. Nobody holds your cryptocurrency.
                </p>
              </div>
            </aside>
          </div>

          <InTheApp title="Trade directly in Orien Wallet">
            <p>
              Every order in this book settles directly between two wallets
              through Orien&apos;s locked swap program on Solana in about a
              minute. This website is viewing-only — all buying, selling, and
              posting happen in the <strong>Swap</strong> tab of Orien Wallet
              for Mac, Windows, and Linux.
            </p>
          </InTheApp>
        </div>
      )}

      {tab === "how" && (
        <div className="space-y-6">
          <section className="grid gap-4 sm:grid-cols-3">
            {[
              {
                icon: ArrowLeftRight,
                title: "Post or take either side",
                text: "Sellers list asks (ORN for SOL) and buyers post bids at the price they want. Take any open offer in the Swap tab of Orien Wallet.",
              },
              {
                icon: Wallet,
                title: "Your wallets settle it",
                text: "The seller's ORN lock on Orien first. The buyer sends SOL from their own wallet, and Orien Wallet places it in a lock only the seller can open by revealing a secret that unlocks the ORN.",
              },
              {
                icon: Clock,
                title: "Final in about a minute",
                text: "SOL swaps finish in about a minute with a 0.01 SOL minimum. If either side stops, both people's coins return automatically.",
              },
            ].map(({ icon: Icon, title, text }, n) => (
              <div key={title} className={`${card} p-6`}>
                <div className="mb-3 flex items-center gap-3">
                  <span className="flex h-8 w-8 items-center justify-center rounded-full bg-raised text-sm font-bold text-ink">
                    {n + 1}
                  </span>
                  <Icon
                    className="h-5 w-5 text-pearl"
                    strokeWidth={1.75}
                    aria-hidden
                  />
                </div>
                <h2 className="mb-2 font-bold text-ink">{title}</h2>
                <p className="text-sm leading-relaxed text-muted">{text}</p>
              </div>
            ))}
          </section>
          <section className={`${card} space-y-3 p-6`}>
            <div className="flex items-center gap-3">
              <ShieldCheck className="h-5 w-5 text-go" aria-hidden />
              <h2 className="text-lg font-bold tracking-tight text-ink">
                What keeps it safe
              </h2>
            </div>
            <ul className="list-disc space-y-1.5 pl-5 text-sm leading-relaxed text-muted marker:text-line">
              <li>
                Every swap runs in Orien Wallet, on your computer, with your
                keys. This site never holds anyone&apos;s cryptocurrency.
              </li>
              <li>
                The seller can only claim the SOL by revealing a secret, and
                that same secret unlocks the buyer&apos;s ORN.
              </li>
              <li>
                Locked SOL wait in Orien&apos;s Solana lock program, which can
                only pay the two people named in the lock and can never be
                changed by anyone.
              </li>
              <li>
                If the buyer never pays, the seller&apos;s ORN return on their
                own after about 4 hours. If the seller never claims, the
                buyer&apos;s SOL returns within about an hour.
              </li>
              <li>
                You need no ORN to buy your first or post a buy offer: your
                wallet only pays once it has verified the seller&apos;s locked
                ORN on the Orien network.
              </li>
            </ul>
          </section>
          <InTheApp title="Get Orien Wallet to trade">
            <p>
              Download Orien Wallet for Mac, Windows, or Linux, open the{" "}
              <strong>Swap</strong> tab, and sign in with your Orien Network
              account to place buy or sell offers directly on the book.
            </p>
          </InTheApp>
        </div>
      )}
    </div>
  );
}

/** Order-book depth rows for asks (red) or bids (green). */
function BookRows({
  offers,
  side,
  solUsd,
}: {
  offers: SwapOffer[];
  side: "ask" | "bid";
  solUsd: number | null;
}) {
  const maxOrn = Math.max(...offers.map((o) => o.orn), 1);
  return (
    <ul>
      {offers.map((offer) => {
        const pct = Math.max(8, Math.round((offer.orn / maxOrn) * 100));
        const unit = unitPrice(offer);
        const unitUsd = solUsd ? unit * solUsd : null;
        return (
          <li
            key={offer.id}
            className="relative overflow-hidden border-t border-line/50 first:border-t-0"
          >
            <div
              aria-hidden
              className={`pointer-events-none absolute inset-y-0 right-0 ${
                side === "ask" ? "bg-stop/10" : "bg-go/10"
              }`}
              style={{ width: `${pct}%` }}
            />
            <div className="relative grid grid-cols-[1.15fr_0.95fr_0.9fr] items-center gap-2 px-5 py-2.5 text-sm">
              <div className="min-w-0">
                <p
                  className={`num font-semibold ${
                    side === "ask" ? "text-stop" : "text-go"
                  }`}
                >
                  {fmtSolUnit(unit)}
                </p>
                <div className="mt-0.5 flex items-center gap-1.5 text-[11px] text-muted">
                  {unitUsd !== null && (
                    <span className="num">{dollars(unitUsd)}</span>
                  )}
                  <span>·</span>
                  <Link
                    href={profileHref(offer.seller_name)}
                    className="inline-flex min-w-0 items-center gap-1 truncate hover:text-ink"
                  >
                    <Avatar
                      name={offer.seller_name}
                      picture={offer.seller_picture}
                      size={14}
                    />
                    <span className="truncate">{offer.seller_name}</span>
                  </Link>
                </div>
              </div>
              <p className="num text-right font-medium text-ink">
                {formatOrn(offer.orn)}
              </p>
              <p className="num text-right text-muted">
                {formatSol(offer.lamports)}
              </p>
            </div>
          </li>
        );
      })}
    </ul>
  );
}

/** Interactive SVG price chart with USD or SOL toggle and point hover. */
function PriceChart({ swaps }: { swaps: Traded[] | null }) {
  const [unitMode, setUnitMode] = useState<"usd" | "sol">("usd");
  const [hovered, setHovered] = useState<number | null>(null);

  const points = (swaps ?? [])
    .map((t) => {
      const value = unitMode === "usd" ? tradedUnitUsd(t) : tradedUnitSol(t);
      return value !== null && value > 0
        ? { at: t.at, value, orn: t.orn / 1e9 }
        : null;
    })
    .filter((p): p is { at: number; value: number; orn: number } => p !== null)
    .reverse();

  const W = 640;
  const H = 220;
  const padL = 62;
  const padR = 20;
  const padT = 22;
  const padB = 30;
  const plotW = W - padL - padR;
  const plotH = H - padT - padB;

  const values = points.map((p) => p.value);
  const minRaw = values.length ? Math.min(...values) : 0;
  const maxRaw = values.length ? Math.max(...values) : 1;
  const span = maxRaw - minRaw;
  const minP = span > 0 ? Math.max(0, minRaw - span * 0.18) : minRaw * 0.7;
  const maxP = span > 0 ? maxRaw + span * 0.18 : maxRaw * 1.3 || 1;

  const xAt = (i: number) =>
    points.length <= 1
      ? padL + plotW / 2
      : padL + (i / (points.length - 1)) * plotW;
  const yAt = (v: number) =>
    padT + (1 - (v - minP) / (maxP - minP || 1)) * plotH;

  const fmtVal = (v: number) =>
    unitMode === "usd" ? dollars(v) : `${fmtSolUnit(v)} SOL`;

  const linePath = points
    .map(
      (p, i) =>
        `${i === 0 ? "M" : "L"}${xAt(i).toFixed(1)},${yAt(p.value).toFixed(1)}`,
    )
    .join(" ");
  const areaPath =
    points.length >= 2
      ? `${linePath} L${xAt(points.length - 1).toFixed(1)},${(padT + plotH).toFixed(1)} L${xAt(0).toFixed(1)},${(padT + plotH).toFixed(1)} Z`
      : "";

  const formatDate = (sec: number) =>
    new Date(sec * 1000).toLocaleDateString("en", {
      day: "numeric",
      month: "short",
    });

  const active = hovered !== null ? points[hovered] : points[points.length - 1];

  return (
    <section className={`${card} p-5 sm:p-6`}>
      <div className="mb-4 flex flex-wrap items-start justify-between gap-4">
        <div>
          <h2 className="text-base font-bold tracking-tight text-ink">
            ORN / SOL price chart
          </h2>
          <p className="mt-0.5 text-xs text-muted">
            {active ? (
              <>
                <span className="num font-semibold text-ink">
                  {fmtVal(active.value)}
                </span>{" "}
                per ORN ·{" "}
                {active.orn.toLocaleString("en", { maximumFractionDigits: 2 })}{" "}
                ORN · {ago(active.at)}
              </>
            ) : (
              "Completed swap prices over time"
            )}
          </p>
        </div>
        <div className="flex items-center gap-1 rounded-xl border border-line bg-bg p-1">
          <button
            type="button"
            onClick={() => setUnitMode("usd")}
            className={`rounded-lg px-3 py-1 text-xs font-semibold transition ${
              unitMode === "usd"
                ? "bg-raised text-ink"
                : "text-muted hover:text-ink"
            }`}
          >
            USD
          </button>
          <button
            type="button"
            onClick={() => setUnitMode("sol")}
            className={`rounded-lg px-3 py-1 text-xs font-semibold transition ${
              unitMode === "sol"
                ? "bg-raised text-ink"
                : "text-muted hover:text-ink"
            }`}
          >
            SOL
          </button>
        </div>
      </div>

      {swaps === null ? (
        <p className="py-14 text-center text-sm text-muted">Loading chart…</p>
      ) : points.length === 0 ? (
        <p className="py-14 text-center text-sm text-muted">
          The chart begins as soon as the first swap completes.
        </p>
      ) : (
        <svg
          viewBox={`0 0 ${W} ${H}`}
          className="h-auto w-full select-none overflow-visible"
          role="img"
          aria-label={`Price chart for ORN per ${unitMode === "usd" ? "US dollar" : "SOL"}`}
          onMouseLeave={() => setHovered(null)}
        >
          <defs>
            <linearGradient id="swapStroke" x1="0" y1="0" x2="1" y2="0">
              <stop offset="0%" stopColor="#f4b6d4" />
              <stop offset="35%" stopColor="#b8c7ff" />
              <stop offset="70%" stopColor="#a6ead6" />
              <stop offset="100%" stopColor="#f9dda4" />
            </linearGradient>
            <linearGradient id="swapFill" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#d3b6ff" stopOpacity="0.22" />
              <stop offset="100%" stopColor="#d3b6ff" stopOpacity="0.0" />
            </linearGradient>
          </defs>

          {[maxP, (minP + maxP) / 2, minP].map((tick, idx) => (
            <g key={idx}>
              <line
                x1={padL}
                y1={yAt(tick)}
                x2={W - padR}
                y2={yAt(tick)}
                stroke="#262633"
                strokeWidth="1"
                strokeDasharray={idx === 1 ? "4 4" : undefined}
              />
              <text
                x={padL - 8}
                y={yAt(tick) + 4}
                textAnchor="end"
                className="fill-muted text-[10px]"
                fontFamily="monospace"
              >
                {unitMode === "usd" ? dollars(tick) : fmtSolUnit(tick)}
              </text>
            </g>
          ))}

          {areaPath && <path d={areaPath} fill="url(#swapFill)" />}
          {points.length >= 2 && (
            <path
              d={linePath}
              fill="none"
              stroke="url(#swapStroke)"
              strokeWidth="2.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          )}

          {points.map((p, i) => (
            <g key={`${p.at}-${i}`} onMouseEnter={() => setHovered(i)}>
              <circle
                cx={xAt(i)}
                cy={yAt(p.value)}
                r={hovered === i || points.length === 1 ? 5.5 : 3.5}
                fill="#0b0b0f"
                stroke="#d3c4ff"
                strokeWidth="2"
              />
              <circle
                cx={xAt(i)}
                cy={yAt(p.value)}
                r={14}
                fill="transparent"
                className="cursor-pointer"
              />
            </g>
          ))}

          {points.length === 1 ? (
            <text
              x={xAt(0)}
              y={yAt(points[0].value) - 14}
              textAnchor="middle"
              className="fill-ink text-[11px] font-semibold"
              fontFamily="monospace"
            >
              {fmtVal(points[0].value)} · {formatDate(points[0].at)}
            </text>
          ) : (
            <>
              <text
                x={padL}
                y={H - 6}
                textAnchor="start"
                className="fill-muted text-[10px]"
                fontFamily="monospace"
              >
                {formatDate(points[0].at)}
              </text>
              <text
                x={W - padR}
                y={H - 6}
                textAnchor="end"
                className="fill-muted text-[10px]"
                fontFamily="monospace"
              >
                {formatDate(points[points.length - 1].at)}
              </text>
            </>
          )}
        </svg>
      )}
    </section>
  );
}

/** Where every trade happens: Orien Wallet. */
function InTheApp({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className={`${card} flex flex-col gap-4 p-6`}>
      <div className="flex items-center gap-3">
        <Wallet className="h-5 w-5 text-pearl" aria-hidden />
        <h2 className="text-xl font-bold tracking-tight text-ink">{title}</h2>
      </div>
      <div className="flex flex-col gap-3 text-sm leading-relaxed text-muted [&_strong]:font-semibold [&_strong]:text-ink">
        {children}
      </div>
      <Link
        href="/try/"
        className="pearl-fill flex h-11 w-fit items-center gap-2 rounded-xl px-5 text-sm font-bold transition hover:brightness-105"
      >
        Get Orien Wallet <ArrowRight className="h-4 w-4" aria-hidden />
      </Link>
    </section>
  );
}
