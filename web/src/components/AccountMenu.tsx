"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { LogIn, LogOut, Settings, UserRound } from "lucide-react";
import { ask, profileHref } from "@/lib/market";
import { SIGNED_IN_OR_OUT, useMe } from "@/lib/useMe";
import { Avatar } from "./market/Avatar";

/** The top bar's account corner: your picture, with a menu to edit your
 * profile or sign out, or a Sign in button. */
export function AccountMenu() {
  const pathname = usePathname();
  const { account } = useMe();
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);

  // A click elsewhere, Escape, or moving to another page closes the menu.
  useEffect(() => setOpen(false), [pathname]);
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent | KeyboardEvent) => {
      if (event instanceof KeyboardEvent ? event.key === "Escape" : !box.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", close);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", close);
    };
  }, [open]);

  const signOut = async () => {
    setOpen(false);
    await ask("/signout", "POST").catch(() => {});
    window.dispatchEvent(new Event(SIGNED_IN_OR_OUT));
    // Market pages drawn for the signed-in account start over.
    if (pathname.startsWith("/marketplace")) window.location.reload();
  };

  // Nothing until it's known, so the bar doesn't flash Sign in.
  if (account === undefined) return <div className="w-9 sm:w-10" aria-hidden />;

  if (!account) {
    return (
      <Link
        href="/account/"
        aria-label="Sign in to Orien Network"
        className="flex h-9 items-center gap-2 whitespace-nowrap rounded-[10px] border border-line px-2.5 text-sm font-semibold text-ink transition hover:bg-raised sm:h-10 sm:px-3"
      >
        <LogIn className="h-4 w-4" strokeWidth={2.25} aria-hidden />
        <span className="hidden sm:inline lg:hidden xl:inline">Sign in</span>
      </Link>
    );
  }

  return (
    <div ref={box} className="relative">
      <button
        type="button"
        aria-label={`${account.name}: your account`}
        aria-expanded={open}
        aria-haspopup="menu"
        onClick={() => setOpen(!open)}
        className={`flex h-9 w-9 items-center justify-center rounded-full transition sm:h-10 sm:w-10 ${
          open ? "ring-2 ring-pearl/70" : "hover:ring-2 hover:ring-line"
        }`}
      >
        <Avatar name={account.name} picture={account.picture} size={34} />
      </button>
      {open && (
        <div
          role="menu"
          className="absolute right-0 top-full mt-2 w-56 overflow-hidden rounded-xl border border-line bg-surface p-1.5 shadow-2xl shadow-black/40"
        >
          <p className="truncate px-3 pb-2 pt-1.5 text-sm font-semibold text-ink">{account.name}</p>
          <Link
            href={profileHref(account.name)}
            role="menuitem"
            className="flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-semibold text-ink transition hover:bg-raised"
          >
            <UserRound className="h-4 w-4 text-pearl" strokeWidth={2} aria-hidden /> View profile
          </Link>
          <Link
            href="/account/"
            role="menuitem"
            className="flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-semibold text-ink transition hover:bg-raised"
          >
            <Settings className="h-4 w-4 text-pearl" strokeWidth={2} aria-hidden /> Your account
          </Link>
          <button
            type="button"
            role="menuitem"
            onClick={signOut}
            className="flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left text-sm font-semibold text-ink transition hover:bg-raised"
          >
            <LogOut className="h-4 w-4 text-muted" strokeWidth={2} aria-hidden /> Sign out
          </button>
        </div>
      )}
    </div>
  );
}
