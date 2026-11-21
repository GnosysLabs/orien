"use client";

import { DISCORD, IPHONE_BETA, OPERATOR_URL, SOURCE_CODE, X_ACCOUNT, YOUTUBE } from "@/lib/brand";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import {
  Activity,
  ArrowLeftRight,
  BookOpen,
  Code2,
  FileText,
  History,
  KeyRound,
  ChevronDown,
  Download,
  Monitor,
  Smartphone,
  Terminal,
  Compass,
  Store,
  Users,
  Rocket,
  type LucideIcon,
  Landmark,
} from "lucide-react";
import { COMMUNITY_LIVE, MARKETPLACE_LIVE } from "@/lib/brand";
import { SIDEBAR } from "@/lib/nav";
import { AccountMenu } from "./AccountMenu";
import { useMe } from "@/lib/useMe";
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
  {
    href: "https://explorer.orien.network/",
    label: "Explorer",
    icon: Activity,
    match: () => false,
    wide: true,
  },
  ...(COMMUNITY_LIVE
    ? [
        {
          href: "/community/",
          label: "Community",
          icon: Users,
          match: (p: string) => p.startsWith("/community"),
          wide: true,
        },
      ]
    : []),
  ...(MARKETPLACE_LIVE
    ? [
        {
          href: "/swap/",
          label: "Buy & sell",
          short: "Buy/sell",
          icon: ArrowLeftRight,
          match: (p: string) => p.startsWith("/swap"),
        },
        {
          href: "/marketplace/",
          label: "Market",
          icon: Store,
          match: (p: string) => p.startsWith("/marketplace"),
        },
      ]
    : []),
  {
    href: "/try/",
    label: "Get started",
    icon: Rocket,
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
    href: IPHONE_BETA,
    label: "Orien Wallet for iPhone",
    detail: "Public beta, through TestFlight",
    icon: Smartphone,
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
        className={`flex h-9 items-center gap-2 whitespace-nowrap rounded-lg px-2.5 text-sm font-semibold transition ${
          open
            ? "bg-raised text-ink"
            : "text-muted hover:bg-raised/60 hover:text-ink"
        }`}
      >
        <Download className="hidden h-4 w-4 shrink-0 xl:block" strokeWidth={2} aria-hidden />
        <span>Download</span>
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
                {...(item.href.startsWith("https://")
                  ? { target: "_blank", rel: "noopener noreferrer" }
                  : {})}
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

type MenuItem = { href: string; label: string; detail: string; icon: LucideIcon };

/** A group of links in the top bar, opening below it. */
function NavMenu({ label, icon: Icon, items, on }: { label: string; icon: LucideIcon; items: MenuItem[]; on: boolean }) {
  const [open, setOpen] = useState(false);
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
        className={`flex h-9 items-center gap-1.5 whitespace-nowrap rounded-lg px-2.5 text-sm font-semibold transition ${
          open || on ? "bg-raised text-ink" : "text-muted hover:bg-raised/60 hover:text-ink"
        }`}
      >
        <Icon className={`hidden h-4 w-4 shrink-0 xl:block ${on ? "text-pearl" : ""}`} strokeWidth={2} aria-hidden />
        <span>{label}</span>
        <ChevronDown className={`h-3.5 w-3.5 transition ${open ? "rotate-180" : ""}`} strokeWidth={2.25} aria-hidden />
      </button>
      {open && (
        <div
          role="menu"
          className="absolute left-0 top-full mt-2 w-72 overflow-hidden rounded-xl border border-line bg-surface p-1.5 shadow-2xl shadow-black/40"
        >
          {items.map((item) => {
            const Icon = item.icon;
            const outside = item.href.startsWith("https://");
            return (
              <a
                key={item.href}
                href={item.href}
                {...(outside ? { target: "_blank", rel: "noopener noreferrer" } : {})}
                role="menuitem"
                onClick={() => setOpen(false)}
                className="flex items-start gap-3 rounded-lg px-3 py-2.5 transition hover:bg-raised"
              >
                <Icon className="mt-0.5 h-4 w-4 shrink-0 text-pearl" strokeWidth={2} aria-hidden />
                <span className="flex flex-col">
                  <span className="text-sm font-semibold text-ink">{item.label}</span>
                  <span className="text-xs leading-relaxed text-muted">{item.detail}</span>
                </span>
              </a>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** The top bar's groups, also the sidebar's. */
const TRADE: MenuItem[] = [
  { href: "/swap/", label: "Buy & sell", detail: "Swap ORN for SOL or Bitcoin with anyone", icon: ArrowLeftRight },
  { href: "/marketplace/", label: "Market", detail: "Buy and sell things and services for ORN", icon: Store },
];
const LEARN: MenuItem[] = [
  { href: "/how-it-works/", label: "How it works", detail: "Payments, signatures, checkpoints and mining, in plain words", icon: BookOpen },
  { href: "/paper/", label: "Technical paper", detail: "Every rule of the network, stated exactly", icon: FileText },
  { href: "/changelog/", label: "Changelog", detail: "What changed, release by release", icon: History },
];
const BUILD: MenuItem[] = [
  { href: "/developers/", label: "Developers", detail: "Everything you need to build with Orien", icon: Code2 },
  { href: "/developers/websites/", label: "Sign in with Orien", detail: "Register the websites people sign in to", icon: KeyRound },
  { href: "/developers/exchanges/", label: "Exchanges", detail: "Listing ORN: what an exchange needs", icon: Landmark },
  { href: "https://explorer.orien.network/api/", label: "Explorer API", detail: "The network's figures, as JSON", icon: Activity },
];

/** X's logo, which its brand guidelines let anyone use to link to an X account. */
export function XIcon({ className = "" }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden className={className}>
      <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z" />
    </svg>
  );
}

/** YouTube's mark, for linking to the channel. */
export function YouTubeIcon({ className = "" }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden className={className}>
      <path d="M23.498 6.186a3.016 3.016 0 0 0-2.122-2.136C19.505 3.545 12 3.545 12 3.545s-7.505 0-9.377.505A3.017 3.017 0 0 0 .502 6.186C0 8.07 0 12 0 12s0 3.93.502 5.814a3.016 3.016 0 0 0 2.122 2.136c1.871.505 9.376.505 9.376.505s7.505 0 9.377-.505a3.015 3.015 0 0 0 2.122-2.136C24 15.93 24 12 24 12s0-3.93-.502-5.814zM9.545 15.568V8.432L15.818 12l-6.273 3.568z" />
    </svg>
  );
}

/** Telegram's mark, for linking to a Telegram account. */
export function TelegramIcon({ className = "" }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden className={className}>
      <path d="M11.944 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0a12 12 0 0 0-.056 0zm4.962 7.224c.1-.002.321.023.465.14a.506.506 0 0 1 .171.325c.016.093.036.306.02.472-.18 1.898-.962 6.502-1.36 8.627-.168.9-.499 1.201-.82 1.23-.696.065-1.225-.46-1.9-.902-1.056-.693-1.653-1.124-2.678-1.8-1.185-.78-.417-1.21.258-1.91.177-.184 3.247-2.977 3.307-3.23.007-.032.014-.15-.056-.212s-.174-.041-.249-.024c-.106.024-1.793 1.14-5.061 3.345-.48.33-.913.49-1.302.48-.428-.008-1.252-.241-1.865-.44-.752-.245-1.349-.374-1.297-.789.027-.216.325-.437.893-.663 3.498-1.524 5.83-2.529 6.998-3.014 3.332-1.386 4.025-1.627 4.476-1.635z" />
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

/** A red dot on the Market when messages wait unread. */
function useUnreadDot() {
  const { account, unread } = useMe();
  return (item: NavItem) =>
    MARKETPLACE_LIVE && account && unread > 0 && item.href === "/marketplace/" ? unread : 0;
}

function Dot({ count, className }: { count: number; className: string }) {
  return (
    <span
      className={`absolute h-2.5 w-2.5 rounded-full bg-stop ring-2 ring-bg ${className}`}
      aria-label={`${count} unread message${count === 1 ? "" : "s"}`}
      role="status"
    />
  );
}

export function Header() {
  const pathname = usePathname();
  const unreadOn = useUnreadDot();
  const onTry = pathname.startsWith("/try");
  // Get started is for newcomers: once signed in, people are past it.
  const { account } = useMe();
  // The admin dashboard is its own place: just the wordmark.
  const admin = pathname.startsWith("/admin");

  return (
    <header className={`sticky top-0 z-30 print:hidden border-b border-line/70 bg-bg/55 backdrop-blur-md ${SIDEBAR && !admin ? "lg:hidden" : ""}`}>
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
            className="ml-3 hidden items-center gap-1 lg:flex"
          >
            {ITEMS.filter((item) => ["https://explorer.orien.network/", "/community/"].includes(item.href)).map((item) => {
              const on = isActive(item, pathname);
              const Icon = item.icon;
              return (
                <Link
                  key={item.href}
                  href={item.href}
                  aria-current={on ? "page" : undefined}
                  className={`relative flex h-9 items-center gap-2 whitespace-nowrap rounded-lg px-2.5 text-sm font-semibold transition ${
                    on
                      ? "bg-raised text-ink"
                      : "text-muted hover:bg-raised/60 hover:text-ink"
                  }`}
                >
                  <Icon
                    className={`hidden h-4 w-4 shrink-0 xl:block ${on ? "text-pearl" : ""}`}
                    strokeWidth={2}
                    aria-hidden
                  />
                  <span>{item.label}</span>
                </Link>
              );
            })}
            {MARKETPLACE_LIVE && (
              <span className="relative">
                <NavMenu label="Trade" icon={ArrowLeftRight} items={TRADE} on={TRADE.some((i) => pathname.startsWith(i.href))} />
                {unreadOn(ITEMS.find((i) => i.href === "/marketplace/")!) > 0 && (
                  <Dot count={unreadOn(ITEMS.find((i) => i.href === "/marketplace/")!)} className="right-0.5 top-1" />
                )}
              </span>
            )}
            <NavMenu label="Learn" icon={BookOpen} items={LEARN} on={LEARN.some((i) => pathname.startsWith(i.href))} />
            <NavMenu label="Build" icon={Code2} items={BUILD} on={pathname.startsWith("/developers")} />
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
              className="flex h-9 w-9 items-center justify-center rounded-lg text-muted transition hover:bg-raised/60 hover:text-ink"
            >
              <DiscordIcon className="h-5 w-5" />
            </a>
            <a
              href={X_ACCOUNT}
              target="_blank"
              rel="noopener noreferrer"
              aria-label="Follow Orien on X"
              title="Follow Orien on X"
              className="flex h-9 w-9 items-center justify-center rounded-lg text-muted transition hover:bg-raised/60 hover:text-ink"
            >
              <XIcon className="h-[18px] w-[18px]" />
            </a>
            <a
              href={YOUTUBE}
              target="_blank"
              rel="noopener noreferrer"
              aria-label="Watch Orien on YouTube"
              title="Watch Orien on YouTube"
              className="flex h-9 w-9 items-center justify-center rounded-lg text-muted transition hover:bg-raised/60 hover:text-ink"
            >
              <YouTubeIcon className="h-5 w-5" />
            </a>
            {!account && (
            <Link
              href="/try/"
              aria-current={onTry ? "page" : undefined}
              className="pearl-fill pearl-sheen ml-1.5 hidden h-10 items-center gap-2 whitespace-nowrap rounded-[10px] px-3.5 text-sm font-bold transition hover:brightness-105 lg:flex"
            >
              <Rocket className="h-4 w-4" strokeWidth={2.25} aria-hidden />
              <span>Get started</span>
            </Link>
            )}
            {MARKETPLACE_LIVE && (
              <div className="ml-1.5">
                <AccountMenu />
              </div>
            )}
          </div>
        )}
      </div>
    </header>
  );
}

export function BottomNav() {
  const pathname = usePathname();
  const unreadOn = useUnreadDot();
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
              <span className="relative">
                <Icon
                  className={`h-5 w-5 ${on ? "text-pearl" : ""}`}
                  strokeWidth={2}
                  aria-hidden
                />
                {unreadOn(item) > 0 && <Dot count={unreadOn(item)} className="-right-1 -top-0.5" />}
              </span>
            )}
            {item.short ?? item.label}
          </Link>
        );
      })}
    </nav>
  );
}

/** A heading over a group of the sidebar's links. */
function SidebarGroup({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="space-y-0.5">
      <p className="px-3 pb-1 pt-4 text-[11px] font-semibold uppercase tracking-[0.14em] text-muted/80">{title}</p>
      {children}
    </div>
  );
}

function SidebarLink({
  href,
  label,
  icon: Icon,
  on = false,
  dot = 0,
  onClick,
}: {
  href: string;
  label: string;
  icon: LucideIcon;
  on?: boolean;
  dot?: number;
  onClick?: (event: React.MouseEvent) => void;
}) {
  const outside = href.startsWith("https://");
  const className = `relative flex h-9 items-center gap-3 rounded-lg px-3 text-sm font-semibold transition ${
    on ? "bg-raised text-ink" : "text-muted hover:bg-raised/60 hover:text-ink"
  }`;
  const body = (
    <>
      <Icon className={`h-4 w-4 shrink-0 ${on ? "text-pearl" : ""}`} strokeWidth={2} aria-hidden />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {outside && <span className="text-xs text-muted/70" aria-hidden>↗</span>}
      {dot > 0 && <Dot count={dot} className="right-2 top-1/2 -translate-y-1/2" />}
    </>
  );
  return outside ? (
    <a href={href} target="_blank" rel="noopener noreferrer" className={className} onClick={onClick}>
      {body}
    </a>
  ) : (
    <Link href={href} aria-current={on ? "page" : undefined} className={className} onClick={onClick}>
      {body}
    </Link>
  );
}

/** The site's navigation on computers: down the left side, with room for
 * everything. Phones keep the top bar and the bottom bar. */
export function Sidebar() {
  const pathname = usePathname();
  const unreadOn = useUnreadDot();
  if (!SIDEBAR || pathname.startsWith("/admin")) return null;
  const item = (href: string) => ITEMS.find((i) => i.href === href);
  const main = ["/", "/how-it-works/", "https://explorer.orien.network/"].map(item).filter((i): i is NavItem => !!i);
  const trade = ["/swap/", "/marketplace/"].map(item).filter((i): i is NavItem => !!i);

  return (
    <aside
      aria-label="Site"
      className="fixed inset-y-0 left-0 z-30 hidden w-64 flex-col border-r border-line/70 bg-bg/55 backdrop-blur-md print:hidden lg:flex"
    >
      <div className="flex h-16 shrink-0 items-center px-5">
        <Link href="/" aria-label="Orien home" className="flex items-center text-2xl">
          <Wordmark />
        </Link>
      </div>

      <div className="px-4">
        <Link
          href="/try/"
          aria-current={pathname.startsWith("/try") ? "page" : undefined}
          className="pearl-fill pearl-sheen flex h-10 w-full items-center justify-center gap-2 rounded-[10px] text-sm font-bold transition hover:brightness-105"
        >
          <Rocket className="h-4 w-4" strokeWidth={2.25} aria-hidden />
          Get started
        </Link>
      </div>

      <nav aria-label="Pages" className="min-h-0 flex-1 overflow-y-auto px-3 pb-4 [scrollbar-width:thin]">
        <SidebarGroup title="Orien">
          {main.map((i) => (
            <SidebarLink key={i.href} href={i.href} label={i.label} icon={i.icon} on={isActive(i, pathname)} />
          ))}
        </SidebarGroup>
        {trade.length > 0 && (
          <SidebarGroup title="Trade">
            {trade.map((i) => (
              <SidebarLink key={i.href} href={i.href} label={i.label} icon={i.icon} on={isActive(i, pathname)} dot={unreadOn(i)} />
            ))}
          </SidebarGroup>
        )}
        <SidebarGroup title="Learn">
          <SidebarLink href="/paper/" label="Technical paper" icon={FileText} on={pathname.startsWith("/paper")} />
          <SidebarLink href="/changelog/" label="Changelog" icon={History} on={pathname.startsWith("/changelog")} />
        </SidebarGroup>
        <SidebarGroup title="Build">
          <SidebarLink
            href="/developers/"
            label="Developers"
            icon={Code2}
            on={pathname.startsWith("/developers") && !pathname.startsWith("/developers/websites")}
          />
          <SidebarLink
            href="/developers/websites/"
            label="Sign in with Orien"
            icon={KeyRound}
            on={pathname.startsWith("/developers/websites")}
          />
        </SidebarGroup>
      </nav>

      <div className="shrink-0 space-y-3 border-t border-line/70 p-4">
        {MARKETPLACE_LIVE && <AccountMenu inSidebar />}
        <div className="flex items-center justify-center gap-1">
          <a
            href={DISCORD}
            target="_blank"
            rel="noopener noreferrer"
            aria-label="Join Orien on Discord"
            title="Join Orien on Discord"
            className="flex h-9 w-9 items-center justify-center rounded-lg text-muted transition hover:bg-raised/60 hover:text-ink"
          >
            <DiscordIcon className="h-5 w-5" />
          </a>
          <a
            href={X_ACCOUNT}
            target="_blank"
            rel="noopener noreferrer"
            aria-label="Follow Orien on X"
            title="Follow Orien on X"
            className="flex h-9 w-9 items-center justify-center rounded-lg text-muted transition hover:bg-raised/60 hover:text-ink"
          >
            <XIcon className="h-[18px] w-[18px]" />
          </a>
          <a
            href={YOUTUBE}
            target="_blank"
            rel="noopener noreferrer"
            aria-label="Watch Orien on YouTube"
            title="Watch Orien on YouTube"
            className="flex h-9 w-9 items-center justify-center rounded-lg text-muted transition hover:bg-raised/60 hover:text-ink"
          >
            <YouTubeIcon className="h-5 w-5" />
          </a>
        </div>
        <nav aria-label="About" className="flex flex-wrap justify-center gap-x-3 gap-y-1 text-xs text-muted">
          {MARKETPLACE_LIVE && (
            <>
              <Link href="/privacy/" className="hover:text-ink">
                Privacy
              </Link>
              <Link href="/terms/" className="hover:text-ink">
                Terms
              </Link>
            </>
          )}
          <Link href="/contact/" className="hover:text-ink">
            Contact
          </Link>
          <a href={SOURCE_CODE} target="_blank" rel="noreferrer" className="hover:text-ink">
            Source code
          </a>
          <a href={OPERATOR_URL} target="_blank" rel="noreferrer" className="hover:text-ink">
            © 2026 Gnosys Labs
          </a>
        </nav>
      </div>
    </aside>
  );
}
