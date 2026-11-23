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
  ask,
  formatBtc,
  formatOrn,
  formatSol,
  sentence,
  type SwapOffer,
  profileHref,
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

/** The offer's price in its coin, whole units: SOL or BTC. */
const price = (offer: SwapOffer) =>
  offer.coin === "sol" ? offer.lamports / 1e9 : offer.sats / 1e8;

/** Its price for one ORN, in that coin, for sorting. */
const perOrnPrice = (offer: SwapOffer) => price(offer) / (offer.orn / 1e9);

/** Satoshis or SOL for one ORN, rounded for reading: "1,800 satoshis". */
const perOrn = (offer: SwapOffer) => {
  if (offer.coin === "sol") {
    const sol = perOrnPrice(offer);
    return `${sol >= 1 ? sol.toFixed(2) : Number(sol.toPrecision(3)).toLocaleString("en", { maximumSignificantDigits: 3 })} SOL`;
  }
  const sats = offer.sats / (offer.orn / 1_000_000_000);
  return `${sats >= 100 ? Math.round(sats).toLocaleString("en") : sats.toPrecision(3)} satoshis`;
};

type Prices = { sol: number | null; btc: number | null };

type Tab = "buy" | "sell" | "how";

const TABS: { id: Tab; label: string }[] = [
  { id: "buy", label: "Buy ORN" },
  { id: "sell", label: "Sell ORN" },
  { id: "how", label: "How it works" },
];

export function SwapBoard() {
  const [offers, setOffers] = useState<SwapOffer[] | null>(null);
  const [usd, setUsd] = useState<Prices>({ sol: null, btc: null });
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
    ask<{
      offers: SwapOffer[];
      bitcoin: { usd: number } | null;
      solana: { usd: number } | null;
    }>("/swaps?coins=sol,btc")
      .then((found) => {
        setOffers(
          [...found.offers].sort((a, b) => perOrnPrice(a) - perOrnPrice(b)),
        );
        setUsd({
          sol: found.solana?.usd ?? null,
          btc: found.bitcoin?.usd ?? null,
        });
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
          Buy ORN with SOL, or sell your ORN for SOL, directly with another
          person and with nobody in the middle. It takes about a minute, and
          either both of you get paid or both of you get your coins back.
          Bitcoin works too.
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
        <section className="space-y-8">
          <Offers
            title="Pay with SOL"
            note="Offers from people selling ORN for SOL right now, cheapest first. To buy one, open the Swap screen in Orien Wallet."
            offers={offers?.filter((o) => o.coin === "sol") ?? null}
            usd={usd.sol}
            coin="SOL"
            empty={
              <>
                No SOL offers right now. Have ORN to sell?{" "}
                <button
                  type="button"
                  onClick={() => show("sell")}
                  className="font-semibold text-ink underline underline-offset-4"
                >
                  Sell them in Orien Wallet
                </button>
                .
              </>
            }
          />
          {offers !== null && offers.some((o) => o.coin === "btc") && (
            <Offers
              title="Or pay with Bitcoin"
              note="Slower: Bitcoin swaps wait for Bitcoin to confirm, usually 20 to 60 minutes."
              offers={offers.filter((o) => o.coin === "btc")}
              usd={usd.btc}
              coin="Bitcoin"
              small
            />
          )}
          <InTheApp title="Buy in Orien Wallet">
            <p>
              Buying and selling happen in Orien Wallet, for Mac, Windows and
              Linux: it holds your keys, so the swap runs between your wallet
              and the seller&apos;s with nobody in the middle.
            </p>
            <p>
              Open its <strong>Swap</strong> screen, pick an offer and take it.
              The seller&apos;s wallet accepts within seconds and locks the ORN
              for you. The app then shows exactly how much SOL to send from
              your own SOL wallet, such as Phantom, with a QR code, and your
              ORN arrive by themselves, usually within a minute. You need no
              ORN to buy your first, just the SOL plus about 0.005 SOL for
              Solana&apos;s small fees, nearly all of which comes back.
            </p>
            <p>
              Paying with Bitcoin works the same way and takes longer, while
              Bitcoin confirms.
            </p>
          </InTheApp>
        </section>
      )}

      {tab === "sell" && (
        <InTheApp title="Sell in Orien Wallet">
          <p>
            Open the <strong>Swap</strong> screen in Orien Wallet and post an
            offer: how many ORN, the SOL you want for them, and the SOL address
            to be paid at, such as your Phantom wallet, and approve the sale
            once with Touch ID or your password. It appears here for buyers to
            see. The smallest offer is 0.01 SOL.
          </p>
          <p>
            You can ask for Bitcoin instead, paid at a Bitcoin address. The
            smallest Bitcoin offer is 0.00015 BTC, more when Bitcoin is busy,
            so any Bitcoin wallet can pay it.
          </p>
          <p>
            Offers come from a verified wallet, so buyers know who they&apos;re
            dealing with. Verify the wallet you sell from first, under{" "}
            <Link href="/account/#wallets" className="font-semibold text-ink underline underline-offset-4">
              Your wallets
            </Link>{" "}
            on your Orien Network profile. Buying needs no verifying.
          </p>
          <p>
            When someone takes it, your wallet accepts within seconds, even with
            its window closed: it locks your ORN for them, collects the SOL or
            Bitcoin once it&apos;s final, and delivers their ORN, all by itself.
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
                text: "Sellers offer ORN for SOL, or for Bitcoin, at their price. Take one in Orien Wallet and the seller’s wallet accepts within seconds; if it doesn’t within 30 minutes, the offer goes back on the board and nothing is lost.",
              },
              {
                icon: Wallet,
                title: "Your wallets do the rest",
                text: "The seller’s ORN are locked for you. You send the SOL from your own wallet, and Orien Wallet puts it in a lock only the seller can open, by revealing a secret that also opens the ORN.",
              },
              {
                icon: Clock,
                title: "Done in about a minute",
                text: "With SOL, usually within a minute; with Bitcoin, 20 to 60 minutes while it confirms. If either side stops, the ORN and the SOL or Bitcoin go back to their owners on their own.",
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
                The seller can only take the SOL or Bitcoin by revealing a
                secret, and that same secret is what lets the buyer take the
                ORN.
              </li>
              <li>
                The SOL wait in Orien&apos;s lock program on Solana, which
                can only pay the two people named in the lock, and which
                nobody, including us, can ever change.
              </li>
              <li>
                If the buyer never pays, the seller&apos;s ORN come back after
                about 4 hours with SOL, or 12 hours with Bitcoin. If the seller
                never takes the payment, it goes back to the buyer within
                about an hour with SOL, or about 6 hours with Bitcoin.
              </li>
              <li>
                Once the seller takes the payment, the seller&apos;s own wallet
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

/** One coin's offers: SOL first, Bitcoin below and smaller. */
function Offers({
  title,
  note,
  offers,
  usd,
  coin,
  empty,
  small,
}: {
  title: string;
  note: string;
  offers: SwapOffer[] | null;
  usd: number | null;
  coin: string;
  empty?: React.ReactNode;
  small?: boolean;
}) {
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="space-y-1">
          <h2
            className={`font-bold tracking-tight text-ink ${small ? "text-lg" : "text-xl"}`}
          >
            {title}
          </h2>
          <p className="text-sm text-muted">{note}</p>
        </div>
        {usd && (
          <p className="text-sm text-muted">
            {coin} at {dollars(usd)}, so dollar values are a guide
          </p>
        )}
      </div>
      {offers === null ? (
        <p className="text-muted">Loading…</p>
      ) : offers.length === 0 ? (
        <Notice>{empty}</Notice>
      ) : (
        <ul className="overflow-hidden rounded-2xl border border-line bg-surface">
          {offers.map((offer) => {
            const value = usd ? price(offer) * usd : null;
            // Only the seller's wallet can start a swap: while it's closed,
            // the offer waits, greyed.
            const offline = offer.seller_online === false;
            return (
              <li
                key={offer.id}
                className={`flex flex-wrap items-center gap-4 border-b border-line px-5 py-4 last:border-b-0 ${offline ? "opacity-55" : ""}`}
              >
                <Link
                  href={profileHref(offer.seller_name)}
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
                    {offer.coin === "sol"
                      ? `${formatSol(offer.lamports)} SOL`
                      : `${formatBtc(offer.sats)} BTC`}
                    {value !== null && (
                      <span className="font-normal text-muted">
                        {" "}
                        ≈ {dollars(value)}
                      </span>
                    )}
                  </p>
                  <p className="num text-xs text-muted">
                    {value !== null
                      ? dollars(value / (offer.orn / 1e9)) + " per ORN · "
                      : ""}
                    {perOrn(offer)} per ORN
                  </p>
                </div>
                {offline && (
                  <span className="shrink-0 rounded-full border border-line bg-bg px-3 py-1 text-xs font-semibold text-muted">
                    Seller offline
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
