"use client";

import { DISCORD, X_ACCOUNT } from "@/lib/brand";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import {
  ArrowLeftRight,
  BookOpen,
  ChevronDown,
  Compass,
  Download,
  Mail,
  Monitor,
  Store,
  Terminal,
  type LucideIcon,
} from "lucide-react";
import { MARKETPLACE_LIVE } from "@/lib/brand";
import { Wordmark } from "./Wordmark";
import { WINDOWS_INSTALLER, WindowsInstall } from "./WindowsDownload";

type NavItem = {
  href: string;
  label: string;
  /** A shorter label for the phone's bottom bar. */
  short?: string;
  icon: LucideIcon;
  match?: (pathname: string) => boolean;
  /** Left out of the phone's bottom bar, which has room for five. */
  wide?: boolean;
};

const ITEMS: NavItem[] = [
  {
    href: "/",
    label: "Home",
    icon: Compass,
    match: (p) => p === "/",
  },
  {
    href: "/how-it-works/",
    label: "How it works",
    icon: BookOpen,
    match: (p) => p.startsWith("/how-it-works"),
  },
  ...(MARKETPLACE_LIVE
    ? [
        {
          href: "/swap/",
          label: "Buy & sell ORN",
          short: "Buy/sell",
          icon: ArrowLeftRight,
          match: (p: string) => p.startsWith("/swap"),
        },
        {
          href: "/marketplace/",
          label: "Marketplace",
          short: "Market",
          icon: Store,
          match: (p: string) => p.startsWith("/marketplace"),
        },
      ]
    : []),
  {
    href: "/contact/",
    label: "Contact",
    icon: Mail,
    match: (p) => p.startsWith("/contact"),
    wide: true,
  },
  {
    href: "/try/",
    label: "Get started",
    icon: Terminal,
    match: (p) => p.startsWith("/try"),
  },
];

/** What can be downloaded. Add each app here as it is published. */
const DOWNLOADS: {
  href: string;
  label: string;
  detail: string;
  icon: LucideIcon;
}[] = [
  {
    href: "/wallet/Orien-Wallet.dmg",
    label: "Orien Wallet for Mac",
    detail: "Desktop wallet, for Macs with Apple chips",
    icon: Monitor,
  },
  {
    href: "/wallet/Orien-Wallet-setup.exe",
    label: "Orien Wallet for Windows",
    detail: "Desktop wallet",
    icon: Monitor,
  },
  {
    href: "/wallet/Orien-Wallet.AppImage",
    label: "Orien Wallet for Linux",
    detail: "Desktop wallet, as an AppImage",
    icon: Monitor,
  },
  {
    href: "/try/",
    label: "orien command line",
    detail: "Mac, Linux and Windows",
    icon: Terminal,
  },
];

function DownloadsMenu() {
  const [open, setOpen] = useState(false);
  // Windows warns about new apps: its download first explains what to click.
  const [windows, setWindows] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent | KeyboardEvent) => {
      if (
        event instanceof KeyboardEvent
          ? event.key === "Escape"
          : !box.current?.contains(event.target as Node)
      ) {
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

  return (
    <div ref={box} className="relative">
      <button
        type="button"
        aria-expanded={open}
        aria-haspopup="menu"
        onClick={() => setOpen(!open)}
        className={`flex h-9 items-center gap-2 whitespace-nowrap rounded-lg px-2.5 text-sm font-semibold transition lg:px-3 ${
          open
            ? "bg-raised text-ink"
            : "text-muted hover:bg-raised/60 hover:text-ink"
        }`}
      >
        <Download className="hidden h-4 w-4 shrink-0 xl:block" strokeWidth={2} aria-hidden />
        <span>Downloads</span>
        <ChevronDown
          className={`h-3.5 w-3.5 transition ${open ? "rotate-180" : ""}`}
          strokeWidth={2.25}
          aria-hidden
        />
      </button>
      {open && (
        <div
          role="menu"
          className="absolute left-0 top-full mt-2 w-64 overflow-hidden rounded-xl border border-line bg-surface p-1.5 shadow-2xl shadow-black/40"
        >
          {DOWNLOADS.map((item) => {
            const Icon = item.icon;
            return (
              <a
                key={item.href}
                href={item.href}
                role="menuitem"
                onClick={(event) => {
                  setOpen(false);
                  if (item.href === WINDOWS_INSTALLER) {
                    event.preventDefault();
                    setWindows(true);
                  }
                }}
                className="flex items-center gap-3 rounded-lg px-3 py-2.5 transition hover:bg-raised"
              >
                <Icon
                  className="h-4 w-4 shrink-0 text-pearl"
                  strokeWidth={2}
                  aria-hidden
                />
                <span className="flex flex-col">
                  <span className="text-sm font-semibold text-ink">
                    {item.label}
                  </span>
                  <span className="text-xs text-muted">{item.detail}</span>
                </span>
              </a>
            );
          })}
        </div>
      )}
      {windows && <WindowsInstall onClose={() => setWindows(false)} />}
    </div>
  );
}

/** X's logo, which its brand guidelines let anyone use to link to an X account. */
export function XIcon({ className = "" }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden className={className}>
      <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z" />
    </svg>
  );
}

/** Discord's mark, which its brand guidelines let anyone use to link to a server. */
export function DiscordIcon({ className = "" }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="currentColor"
      aria-hidden
      className={className}
    >
      <path d="M20.317 4.37a19.791 19.791 0 0 0-4.885-1.515.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0 12.64 12.64 0 0 0-.617-1.25.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057a.082.082 0 0 0 .031.057 19.9 19.9 0 0 0 5.993 3.03.078.078 0 0 0 .084-.028 14.09 14.09 0 0 0 1.226-1.994.076.076 0 0 0-.041-.106 13.107 13.107 0 0 1-1.872-.892.077.077 0 0 1-.008-.128 10.2 10.2 0 0 0 .372-.292.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.01c.12.098.246.198.373.292a.077.077 0 0 1-.006.127 12.299 12.299 0 0 1-1.873.892.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028 19.839 19.839 0 0 0 6.002-3.03.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03zM8.02 15.33c-1.183 0-2.157-1.085-2.157-2.419 0-1.333.956-2.419 2.157-2.419 1.21 0 2.176 1.096 2.157 2.42 0 1.333-.956 2.418-2.157 2.418zm7.975 0c-1.183 0-2.157-1.085-2.157-2.419 0-1.333.955-2.419 2.157-2.419 1.21 0 2.176 1.096 2.157 2.42 0 1.333-.946 2.418-2.157 2.418z" />
    </svg>
  );
}

const isActive = (item: NavItem, pathname: string) =>
  item.match ? item.match(pathname) : pathname === item.href;

export function Header() {
  const pathname = usePathname();
  const onTry = pathname.startsWith("/try");
  // The admin dashboard is its own place: just the wordmark.
  const admin = pathname.startsWith("/admin");

  return (
    <header className="sticky top-0 z-30 print:hidden border-b border-line/70 bg-bg/55 backdrop-blur-md">
      <div className="mx-auto flex h-16 max-w-[1200px] items-center gap-3 px-4">
        <Link
          href="/"
          aria-label="Orien home"
          className="flex items-center text-xl sm:text-2xl"
        >
          <Wordmark />
        </Link>

        {admin ? (
          <span className="ml-1 rounded-md border border-line px-2 py-0.5 text-xs font-semibold text-muted">
            Admin
          </span>
        ) : (
          <nav
            aria-label="Pages"
            className="ml-4 hidden items-center gap-1 lg:flex"
          >
            {ITEMS.filter((item) => item.href !== "/try/").map((item) => {
              const on = isActive(item, pathname);
              const Icon = item.icon;
              return (
                <Link
                  key={item.href}
                  href={item.href}
                  aria-current={on ? "page" : undefined}
                  className={`flex h-9 items-center gap-2 whitespace-nowrap rounded-lg px-2.5 text-sm font-semibold transition lg:px-3 ${
                    on
                      ? "bg-raised text-ink"
                      : "text-muted hover:bg-raised/60 hover:text-ink"
                  }`}
                >
                  {/* Icons and full labels where the bar has room for them. */}
                  <Icon
                    className={`hidden h-4 w-4 shrink-0 xl:block ${on ? "text-pearl" : ""}`}
                    strokeWidth={2}
                    aria-hidden
                  />
                  <span className="xl:hidden">{item.short ?? item.label}</span>
                  <span className="hidden xl:inline">{item.label}</span>
                </Link>
              );
            })}
            <DownloadsMenu />
          </nav>
        )}

        {!admin && (
          <div className="ml-auto flex items-center gap-1">
            <a
              href={DISCORD}
              target="_blank"
              rel="noopener noreferrer"
              aria-label="Join Orien on Discord"
              title="Join Orien on Discord"
              className="flex h-9 w-9 items-center justify-center rounded-lg text-muted transition hover:bg-raised/60 hover:text-ink sm:h-10 sm:w-10"
            >
              <DiscordIcon className="h-5 w-5" />
            </a>
            <a
              href={X_ACCOUNT}
              target="_blank"
              rel="noopener noreferrer"
              aria-label="Follow Orien on X"
              title="Follow Orien on X"
              className="flex h-9 w-9 items-center justify-center rounded-lg text-muted transition hover:bg-raised/60 hover:text-ink sm:h-10 sm:w-10"
            >
              <XIcon className="h-[18px] w-[18px]" />
            </a>
            <Link
              href="/try/"
              aria-current={onTry ? "page" : undefined}
              className="pearl-fill pearl-sheen ml-2 flex h-9 items-center gap-2 whitespace-nowrap rounded-[10px] px-3.5 text-sm font-bold transition hover:brightness-105 sm:h-10"
            >
              <Terminal className="h-4 w-4" strokeWidth={2.25} aria-hidden />
              <span>Get started</span>
            </Link>
          </div>
        )}
      </div>
    </header>
  );
}

export function BottomNav() {
  const pathname = usePathname();
  if (pathname.startsWith("/admin")) return null;
  // The changelog is in the footer, as on larger screens.
  const mobileItems = ITEMS.filter((item) => !item.wide);

  return (
    <nav
      aria-label="Mobile navigation"
      className={`fixed inset-x-0 bottom-0 z-40 grid ${MARKETPLACE_LIVE ? "grid-cols-5" : "grid-cols-3"} print:hidden border-t border-line/70 bg-bg/85 px-1 pb-[env(safe-area-inset-bottom)] backdrop-blur-md lg:hidden`}
    >
      {mobileItems.map((item) => {
        const on = isActive(item, pathname);
        const Icon = item.icon;
        const isPrimary = item.href === "/try/";
        return (
          <Link
            key={item.href}
            href={item.href}
            aria-current={on ? "page" : undefined}
            className={`flex h-16 flex-col items-center justify-center gap-1 text-[11px] font-semibold ${
              on ? "text-ink" : "text-muted"
            }`}
          >
            {isPrimary ? (
              <span className="pearl-fill flex h-8 w-8 items-center justify-center rounded-xl">
                <Icon className="h-4 w-4" strokeWidth={2.5} aria-hidden />
              </span>
            ) : (
              <Icon
                className={`h-5 w-5 ${on ? "text-pearl" : ""}`}
                strokeWidth={2}
                aria-hidden
              />
            )}
            {item.short ?? item.label}
          </Link>
        );
      })}
    </nav>
  );
}
