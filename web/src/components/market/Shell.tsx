"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import {
  ArrowLeftRight,
  MessageCircle,
  Plus,
  Store,
} from "lucide-react";
import { useMe } from "@/lib/useMe";

export { SIGNED_IN_OR_OUT } from "@/lib/useMe";

/** The bar the marketplace and the swap board share: the two, side by side,
 * then your messages and account, which serve both. */
export function MarketBar() {
  const pathname = usePathname();
  const { account, unread } = useMe();
  const swapping = pathname.startsWith("/swap");
  return (
    <div className="flex flex-wrap items-center gap-3 pb-8 pt-5 sm:pt-7">
      <Link
        href={swapping ? "/swap/" : "/marketplace/"}
        className="flex items-center gap-2.5"
      >
        {swapping ? (
          <ArrowLeftRight
            className="h-6 w-6 text-pearl"
            strokeWidth={1.75}
            aria-hidden
          />
        ) : (
          <Store
            className="h-6 w-6 text-pearl"
            strokeWidth={1.75}
            aria-hidden
          />
        )}
        <span className="text-xl font-bold tracking-tight text-ink sm:text-2xl">
          {swapping ? "Buy & sell ORN" : "Market"}
        </span>
      </Link>
      {/* Buying and selling ORN happen in Orien Wallet: the swap page needs no account. */}
      {!swapping && (
        <div className="ml-auto flex items-center gap-2">
          {account && (
            <Link
              href="/marketplace/messages/"
              aria-label={unread ? `Messages, ${unread} unread` : "Messages"}
              className="relative flex h-10 items-center gap-2 rounded-xl border border-line bg-surface px-3 text-sm font-semibold text-ink transition hover:bg-raised"
            >
              <MessageCircle className="h-4 w-4" aria-hidden />
              <span className="hidden sm:inline">Messages</span>
              {unread > 0 && (
                <span className="flex h-5 min-w-5 items-center justify-center rounded-full bg-pearl px-1.5 text-[11px] font-bold text-bg">
                  {unread}
                </span>
              )}
            </Link>
          )}
          <Link
            href="/marketplace/sell/"
            className="pearl-fill flex h-10 items-center gap-2 rounded-xl px-4 text-sm font-bold transition hover:brightness-105"
          >
            <Plus className="h-4 w-4" strokeWidth={2.5} aria-hidden /> Sell
            something
          </Link>
        </div>
      )}
    </div>
  );
}

export function Notice({
  tone = "plain",
  children,
}: {
  tone?: "plain" | "bad";
  children: React.ReactNode;
}) {
  return (
    <p
      className={`rounded-xl border px-4 py-3 text-sm ${
        tone === "bad"
          ? "border-stop/40 bg-stop/10 text-ink"
          : "border-line bg-surface text-muted"
      }`}
    >
      {children}
    </p>
  );
}
