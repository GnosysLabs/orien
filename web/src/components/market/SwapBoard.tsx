"use client";

import Link from "next/link";
import { ANY_DEVICE_SWAPS } from "@/lib/brand";
import { useEffect, useState } from "react";
import {
  ArrowLeftRight,
  Bitcoin,
  Clock,
  ShieldCheck,
  Wallet,
} from "lucide-react";
import {
  ask,
  formatBtc,
  formatOrn,
  me,
  parseBtc,
  parseOrn,
  sentence,
  type Account,
  type SwapOffer,
} from "@/lib/market";
import { Avatar } from "./Avatar";
import { Notice } from "./Shell";

const card = "rounded-2xl border border-line bg-surface p-6";

/** Dollars, rounded for reading: "$654", "$1.31", "$0.0042". */
const dollars = (usd: number) =>
  usd >= 100
    ? "$" + Math.round(usd).toLocaleString("en")
    : usd >= 1
      ? "$" + usd.toFixed(2)
      : "$" + usd.toPrecision(2);

/** Satoshis for one ORN, rounded for reading: "1,800". */
const perOrn = (offer: SwapOffer) => {
  const sats = offer.sats / (offer.orn / 1_000_000_000);
  return sats >= 100
    ? Math.round(sats).toLocaleString("en")
    : sats.toPrecision(3);
};

type Tab = "buy" | "sell" | "how";

const TABS: { id: Tab; label: string }[] = [
  { id: "buy", label: "Buy ORN" },
  { id: "sell", label: "Sell ORN" },
  { id: "how", label: "How it works" },
];

export function SwapBoard() {
  const [offers, setOffers] = useState<SwapOffer[] | null>(null);
  const [usd, setUsd] = useState<number | null>(null);
  const [account, setAccount] = useState<Account | null | undefined>(undefined);
  const [trouble, setTrouble] = useState("");
  const [taken, setTaken] = useState<SwapOffer | null>(null);
  const [tab, setTab] = useState<Tab>("buy");

  // The tab follows the address (#buy, #sell, #how), so links and the back
  // button land on the right one.
  useEffect(() => {
    const read = () => {
      const hash = window.location.hash.replace("#", "");
      setTab(
        hash === "sell" || hash === "post"
          ? "sell"
          : hash === "how"
            ? "how"
            : "buy",
      );
    };
    read();
    window.addEventListener("hashchange", read);
    return () => window.removeEventListener("hashchange", read);
  }, []);
  const show = (next: Tab) => {
    setTab(next);
    window.history.replaceState(null, "", `#${next}`);
  };

  const load = () =>
    ask<{ offers: SwapOffer[]; bitcoin: { usd: number } | null }>("/swaps")
      .then((found) => {
        setOffers(found.offers);
        setUsd(found.bitcoin?.usd ?? null);
      })
      .catch((error: Error) => setTrouble(sentence(error.message)));

  useEffect(() => {
    void load();
    me()
      .then((found) => setAccount(found?.account ?? null))
      .catch(() => setAccount(null));
    const timer = setInterval(load, 20000);
    return () => clearInterval(timer);
  }, []);

  const take = async (offer: SwapOffer) => {
    setTrouble("");
    try {
      await ask(`/swaps/${offer.id}/take`, "POST");
      setTaken(offer);
      void load();
    } catch (error) {
      setTrouble(sentence((error as Error).message));
    }
  };

  return (
    <div className="space-y-8">
      <header className="space-y-4">
        <h1 className="text-balance text-4xl font-bold leading-[1.1] tracking-tight text-ink sm:text-5xl">
          Buy and sell ORN
        </h1>
        <p className="max-w-[680px] text-lg leading-relaxed text-muted">
          Buy ORN with Bitcoin, or sell your ORN for Bitcoin, directly with
          another person and with nobody in the middle. Either both of you get
          paid, or both of you get your coins back.
        </p>
      </header>

      <div
        role="tablist"
        aria-label="Buy and sell ORN"
        className="flex gap-1 border-b border-line"
      >
        {TABS.map(({ id, label }) => (
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

      {taken && (
        <Notice>
          You took {formatOrn(taken.orn)} ORN for {formatBtc(taken.sats)} BTC.
          Open Orien Wallet, signed in to the Market, within 30 minutes: it
          carries the swap through from here.
        </Notice>
      )}
      {trouble && <Notice tone="bad">{trouble}</Notice>}

      {tab === "buy" && (
        <section className="space-y-4">
          <div className="flex flex-wrap items-end justify-between gap-3">
            <div>
              <p className="text-sm text-muted">
                Offers from people selling ORN for Bitcoin. Take one in Orien
                Wallet.
              </p>
            </div>
            <p className="text-sm text-muted">
              Cheapest first
              {usd && (
                <> · Bitcoin at {dollars(usd)}, so dollar values are a guide</>
              )}
            </p>
          </div>
          {offers === null ? (
            <p className="text-muted">Loading…</p>
          ) : offers.length === 0 ? (
            <Notice>
              No offers right now. Have ORN to sell?{" "}
              <button
                type="button"
                onClick={() => show("sell")}
                className="font-semibold text-ink underline underline-offset-4"
              >
                Post the first one
              </button>
              .
            </Notice>
          ) : (
            <ul className="overflow-hidden rounded-2xl border border-line bg-surface">
              {offers.map((offer) => {
                const mine = account?.id === offer.seller;
                return (
                  <li
                    key={offer.id}
                    className="flex flex-wrap items-center gap-4 border-b border-line px-5 py-4 last:border-b-0"
                  >
                    <Link
                      href={`/marketplace/profile/?id=${offer.seller}`}
                      className="flex min-w-0 items-center gap-3 sm:w-52"
                    >
                      <Avatar
                        name={offer.seller_name}
                        picture={offer.seller_picture}
                        size={36}
                      />
                      <span className="flex min-w-0 flex-col">
                        <span className="truncate text-sm font-semibold text-ink hover:underline">
                          {offer.seller_name}
                        </span>
                        <span className="text-xs text-muted">
                          {offer.seller_done === 0
                            ? "No swaps yet"
                            : `${offer.seller_done} swap${offer.seller_done === 1 ? "" : "s"} done`}
                        </span>
                      </span>
                    </Link>
                    <div className="flex-1">
                      <p className="num font-bold text-ink">
                        {formatOrn(offer.orn)} ORN{" "}
                        <span className="font-normal text-muted">for</span>{" "}
                        {formatBtc(offer.sats)} BTC
                        {usd && (
                          <span className="font-normal text-muted">
                            {" "}
                            ≈ {dollars((offer.sats / 1e8) * usd)}
                          </span>
                        )}
                      </p>
                      <p className="num text-xs text-muted">
                        {usd
                          ? dollars(
                              ((offer.sats / 1e8) * usd) / (offer.orn / 1e9),
                            ) + " per ORN · "
                          : ""}
                        {perOrn(offer)} satoshis per ORN
                      </p>
                    </div>
                    {mine ? (
                      <button
                        type="button"
                        onClick={() =>
                          ask(`/swaps/${offer.id}/close`, "POST")
                            .then(load)
                            .catch((e: Error) =>
                              setTrouble(sentence(e.message)),
                            )
                        }
                        className="h-10 rounded-xl border border-line px-4 text-sm font-semibold text-muted hover:bg-raised hover:text-ink"
                      >
                        Withdraw
                      </button>
                    ) : account ? (
                      <button
                        type="button"
                        onClick={() => take(offer)}
                        className="pearl-fill h-10 rounded-xl px-5 text-sm font-bold"
                      >
                        Take
                      </button>
                    ) : (
                      <Link
                        href="/marketplace/account/"
                        className="h-10 rounded-xl border border-line px-4 text-sm font-semibold leading-10 text-ink hover:bg-raised"
                      >
                        Sign in to take
                      </Link>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      )}

      {tab === "sell" && (
        <PostOffer account={account} onPosted={load} usd={usd} />
      )}

      {tab === "how" && (
        <div className="space-y-6">
          <section className="grid gap-4 sm:grid-cols-3">
            {[
              {
                icon: ArrowLeftRight,
                title: "Pick an offer",
                text: "Sellers offer ORN for Bitcoin at their price. Take one in Orien Wallet, and it is held for you for 30 minutes.",
              },
              {
                icon: Wallet,
                title: "Your wallets do the rest",
                text: "The seller’s ORN are locked for you. You pay the Bitcoin into a lock only the seller can open, by revealing a secret that also opens the ORN.",
              },
              {
                icon: Clock,
                title: "Done in under an hour",
                text: "Usually 20 to 60 minutes, while Bitcoin confirms. If either side stops, the ORN and the Bitcoin go back to their owners on their own.",
              },
            ].map(({ icon: Icon, title, text }, n) => (
              <div key={title} className={card}>
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
          <section className={`${card} space-y-3`}>
            <div className="flex items-center gap-3">
              <ShieldCheck className="h-5 w-5 text-go" aria-hidden />
              <h2 className="text-lg font-bold tracking-tight text-ink">
                What keeps it safe
              </h2>
            </div>
            <ul className="list-disc space-y-1.5 pl-5 text-sm leading-relaxed text-muted marker:text-line">
              <li>
                The swap runs in Orien Wallet, on your computer, with your keys.
                This site only passes messages between the two wallets.
              </li>
              <li>
                The seller can only take the Bitcoin by revealing a secret, and
                that same secret is what lets the buyer take the ORN.
              </li>
              <li>
                If the buyer never pays, the seller&apos;s ORN come back after
                48 hours. If the seller never takes the Bitcoin, the
                buyer&apos;s wallet takes it back after about 12 hours.
              </li>
              <li>
                Once the seller takes the Bitcoin, the seller&apos;s own wallet
                delivers the buyer&apos;s ORN, so they arrive even if the
                buyer&apos;s wallet is closed.
              </li>
              <li>
                You need no ORN to buy your first: the wallet only pays once it
                has seen the seller&apos;s ORN locked for you.
              </li>
              <li>Profiles show how many swaps each person has done.</li>
            </ul>
          </section>
        </div>
      )}
    </div>
  );
}

function PostOffer({
  account,
  onPosted,
  usd,
}: {
  account: Account | null | undefined;
  onPosted: () => void;
  usd: number | null;
}) {
  const [orn, setOrn] = useState("");
  const [btc, setBtc] = useState("");
  const [payout, setPayout] = useState("");
  // The account's wallets, by the names given them in Orien Wallet.
  const [wallets, setWallets] = useState<
    { address: string; name: string }[] | null
  >(null);
  const [from, setFrom] = useState("");
  const [trouble, setTrouble] = useState("");
  useEffect(() => {
    if (!ANY_DEVICE_SWAPS || !account) return;
    ask<{ wallets: { address: string; name: string }[] }>("/me/wallets")
      .then((found) => {
        setWallets(found.wallets);
        setFrom((now) => now || found.wallets[0]?.address || "");
      })
      .catch(() => setWallets([]));
  }, [account]);
  const [posted, setPosted] = useState(false);

  if (account === undefined) return null;
  const field =
    "num h-11 w-full rounded-xl border border-line bg-bg px-4 text-ink outline-none focus:border-pearl/60";

  const post = async (event: React.FormEvent) => {
    event.preventDefault();
    setTrouble("");
    setPosted(false);
    const units = parseOrn(orn);
    const sats = parseBtc(btc);
    if (units === null) return setTrouble("Write how many ORN, like 500.");
    if (sats === null)
      return setTrouble("Write the Bitcoin price in BTC, like 0.01.");
    if (!from) return setTrouble("Choose the wallet you sell from.");
    if (!payout.trim())
      return setTrouble("Write the Bitcoin address the buyer's Bitcoin go to.");
    try {
      await ask("/swaps", "POST", {
        orn: units,
        sats,
        payout: payout.trim(),
        wallet: from,
      });
      setOrn("");
      setBtc("");
      setPayout("");
      setPosted(true);
      onPosted();
    } catch (error) {
      setTrouble(sentence((error as Error).message));
    }
  };

  return (
    <form onSubmit={post} className={`${card} space-y-4`}>
      <div className="flex items-center gap-3">
        <Bitcoin className="h-5 w-5 text-pearl" aria-hidden />
        <h2 className="text-2xl font-bold tracking-tight text-ink">Sell ORN</h2>
        <p className="text-sm text-muted">
          Post an offer, and get Bitcoin for your ORN when someone takes it.
        </p>
      </div>
      {!ANY_DEVICE_SWAPS ? (
        <p className="text-sm leading-relaxed text-muted">
          Post your offer in{" "}
          <Link
            href="/try/"
            className="font-semibold text-ink underline underline-offset-4"
          >
            Orien Wallet
          </Link>
          , on its Swap screen. When someone takes it, the wallet locks your ORN,
          collects your Bitcoin and delivers theirs, so keep it running.
        </p>
      ) : !account ? (
        <p className="text-sm text-muted">
          <Link
            href="/marketplace/account/"
            className="font-semibold text-ink underline underline-offset-4"
          >
            Sign in
          </Link>{" "}
          to post an offer.
        </p>
      ) : wallets === null ? null : wallets.length === 0 ? (
        <p className="text-sm leading-relaxed text-muted">
          Sign in to the Market from Orien Wallet first, so the swap board knows
          your wallets.
        </p>
      ) : (
        <>
          <div className="grid gap-4 sm:grid-cols-2">
            <label className="flex flex-col gap-2">
              <span className="text-sm font-semibold text-ink">
                ORN you sell
              </span>
              <input
                value={orn}
                onChange={(e) => setOrn(e.target.value)}
                inputMode="decimal"
                placeholder="500"
                className={field}
              />
            </label>
            <label className="flex flex-col gap-2">
              <span className="text-sm font-semibold text-ink">
                Bitcoin you want, in BTC
              </span>
              <input
                value={btc}
                onChange={(e) => setBtc(e.target.value)}
                inputMode="decimal"
                placeholder="0.01"
                className={field}
              />
              {usd && parseBtc(btc) !== null && (
                <span className="num block text-xs text-muted">
                  ≈ {dollars(((parseBtc(btc) ?? 0) / 1e8) * usd)}
                  {parseOrn(orn) !== null &&
                    " · " +
                      dollars(
                        (((parseBtc(btc) ?? 0) / 1e8) * usd) /
                          ((parseOrn(orn) ?? 1) / 1e9),
                      ) +
                      " per ORN"}
                </span>
              )}
            </label>
          </div>
          <label className="flex flex-col gap-2">
            <span className="text-sm font-semibold text-ink">Sell from</span>
            <select
              value={from}
              onChange={(e) => setFrom(e.target.value)}
              className={`${field} appearance-none`}
            >
              {wallets.map((w) => (
                <option key={w.address} value={w.address}>
                  {w.name} · {w.address.slice(0, 6)}…{w.address.slice(-6)}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-2">
            <span className="text-sm font-semibold text-ink">
              Your Bitcoin address, where the buyer&apos;s Bitcoin go
            </span>
            <input
              value={payout}
              onChange={(e) => setPayout(e.target.value)}
              spellCheck={false}
              autoCapitalize="off"
              placeholder="bc1q…"
              className={`${field} font-mono text-sm`}
            />
          </label>
          <p className="text-sm text-muted">
            At least 0.00005 BTC, more when Bitcoin is busy. Nothing is locked
            until someone takes your offer; then Orien Wallet, running on any of
            your devices, finishes the swap. You get an email when it is taken.
          </p>
          {trouble && <Notice tone="bad">{trouble}</Notice>}
          {posted && (
            <p className="text-sm text-go">Your offer is on the board.</p>
          )}
          <button
            type="submit"
            className="pearl-fill h-11 rounded-xl px-6 text-sm font-bold"
          >
            Post offer
          </button>
        </>
      )}
    </form>
  );
}
