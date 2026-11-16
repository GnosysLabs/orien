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
import { ask, formatBtc, formatOrn, sentence, type SwapOffer } from "@/lib/market";
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
  const [trouble, setTrouble] = useState("");
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
    const timer = setInterval(load, 20000);
    return () => clearInterval(timer);
  }, []);

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

      {trouble && <Notice tone="bad">{trouble}</Notice>}

      {tab === "buy" && (
        <section className="space-y-4">
          <div className="flex flex-wrap items-end justify-between gap-3">
            <div>
              <p className="text-sm text-muted">
                Offers from people selling ORN for Bitcoin right now. To buy
                one, open the Swap screen in Orien Wallet.
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
                Sell them in Orien Wallet
              </button>
              .
            </Notice>
          ) : (
            <ul className="overflow-hidden rounded-2xl border border-line bg-surface">
              {offers.map((offer) => {
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
                  </li>
                );
              })}
            </ul>
          )}
          <InTheApp title="Buy in Orien Wallet">
            <p>
              Buying and selling happen in Orien Wallet, for Mac, Windows and
              Linux: it holds your keys, so the swap runs between your wallet
              and the seller&apos;s with nobody in the middle.
            </p>
            <p>
              Open its <strong>Swap</strong> screen, pick an offer and take it.
              The seller has 30 minutes to accept. Then the app shows exactly
              how much Bitcoin to pay and where, and your ORN arrive once the
              seller takes the Bitcoin. You need no ORN to buy your first.
            </p>
          </InTheApp>
        </section>
      )}

      {tab === "sell" && (
        <InTheApp title="Sell in Orien Wallet">
          <p>
            Open the <strong>Swap</strong> screen in Orien Wallet and post an
            offer: how many ORN, the Bitcoin you want for them, and the Bitcoin
            address to be paid at, and approve the sale once with Touch ID or
            your password. It appears here for buyers to see.
          </p>
          <p>
            When someone takes it, your wallet accepts within seconds, even with
            its window closed: it locks your ORN for them, collects the Bitcoin
            once it confirms, and delivers their ORN, all by itself.
            Orien Wallet keeps running in the menu bar or system tray, so just
            leave it on. Nothing is locked until someone takes your offer, and
            you can withdraw it any time before.
          </p>
        </InTheApp>
      )}

      {tab === "how" && (
        <div className="space-y-6">
          <section className="grid gap-4 sm:grid-cols-3">
            {[
              {
                icon: ArrowLeftRight,
                title: "Pick an offer",
                text: "Sellers offer ORN for Bitcoin at their price. Take one in Orien Wallet, and the seller has 30 minutes to accept; if they don't, the offer goes back on the board and nothing is lost.",
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
                12 hours. If the seller never takes the Bitcoin, the
                buyer&apos;s wallet takes it back after about 6 hours.
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

/** Where every trade happens: Orien Wallet. */
function InTheApp({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className={`${card} flex flex-col gap-4`}>
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
