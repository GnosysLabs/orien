"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { Bell } from "lucide-react";
import { ask } from "@/lib/market";
import { SIGNED_IN_OR_OUT, useMe } from "@/lib/useMe";
import { Avatar } from "./market/Avatar";
import { EYEBROWS, NetworkAvatar } from "./market/network";

type Person = { name: string; picture: string | null };
type Line = {
  kind: "reply" | "also" | "react" | "network" | "message";
  post?: number;
  reply?: boolean;
  post_kind?: string;
  conversation?: number;
  listing?: string | null;
  text: string;
  time: number;
  unseen: boolean;
  people: Person[];
  others: number;
  emojis: string[];
};

/** "Ana", "Ana and Sam", "Ana, Sam and 3 others". */
function names(people: Person[], others: number) {
  const list = people.map((p) => p.name);
  if (others > 0) list.push(`${others} other${others === 1 ? "" : "s"}`);
  if (list.length <= 1) return list[0] ?? "Someone";
  return `${list.slice(0, -1).join(", ")} and ${list[list.length - 1]}`;
}

function since(seconds: number) {
  const s = Math.max(0, Math.floor(Date.now() / 1000 - seconds));
  if (s < 60) return "now";
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  if (s < 86400 * 30) return `${Math.floor(s / 86400)}d`;
  return new Date(seconds * 1000).toLocaleDateString("en", {
    day: "numeric",
    month: "short",
  });
}

/** What a line says, after the names. */
function said(line: Line) {
  const theirs =
    line.post_kind && line.post_kind !== "post"
      ? "the network’s post about you"
      : line.reply
        ? "your reply"
        : "your post";
  switch (line.kind) {
    case "reply":
      return `replied to ${theirs}`;
    case "also":
      return "also replied to a post you replied to";
    case "react":
      return `reacted ${line.emojis.join(" ")} to ${theirs}`;
    case "message":
      return line.listing ? `wrote to you about ${line.listing}` : "wrote to you";
    default:
      return "";
  }
}

function href(line: Line) {
  return line.kind === "message"
    ? `/messages/?c=${line.conversation}`
    : `/community/?post=${line.post}`;
}

function Row({ line, onOpen }: { line: Line; onOpen: () => void }) {
  const network = line.kind === "network";
  const eyebrow = network
    ? EYEBROWS[line.post_kind as keyof typeof EYEBROWS]
    : null;
  return (
    <Link
      href={href(line)}
      onClick={onOpen}
      className={`flex gap-3 rounded-xl px-3 py-3 transition hover:bg-raised/70 ${line.unseen ? "bg-raised/40" : ""}`}
    >
      {network ? (
        <NetworkAvatar size={40} />
      ) : (
        <span className="relative shrink-0">
          <Avatar
            name={line.people[0]?.name ?? "?"}
            picture={line.people[0]?.picture ?? null}
            size={40}
          />
          {line.people[1] && (
            <span className="absolute -bottom-1 -right-1 rounded-full ring-2 ring-surface">
              <Avatar
                name={line.people[1].name}
                picture={line.people[1].picture}
                size={22}
              />
            </span>
          )}
        </span>
      )}
      <span className="min-w-0 flex-1">
        <span className="block text-sm leading-snug text-muted">
          {network ? (
            <>
              <span className="font-semibold text-ink">Orien Network</span>{" "}
              posted about you:{" "}
              <span className="font-semibold text-ink">
                {eyebrow ?? "news"}
              </span>
            </>
          ) : (
            <>
              <span className="font-semibold text-ink">
                {names(line.people, line.others)}
              </span>{" "}
              {said(line)}
            </>
          )}
        </span>
        {line.text && !network && (
          <span className="mt-1 block truncate text-sm text-ink/80">
            {line.text}
          </span>
        )}
        <span className="mt-1 block text-xs text-muted">
          {since(line.time)}
        </span>
      </span>
      {line.unseen && (
        <span
          className="mt-2 h-2.5 w-2.5 shrink-0 rounded-full bg-pearl"
          aria-label="New"
        />
      )}
    </Link>
  );
}

/** The top bar's bell, for people signed in: replies, reactions, the
 * network's news about them and messages, newest first. Opening it marks
 * everything seen. */
export function NotificationBell() {
  const pathname = usePathname();
  const { account, notices } = useMe();
  const [open, setOpen] = useState(false);
  const [lines, setLines] = useState<Line[] | null>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => setOpen(false), [pathname]);
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

  const show = async () => {
    if (open) return setOpen(false);
    setOpen(true);
    try {
      const found = await ask<{ notices: Line[] }>("/notices");
      setLines(found.notices);
      if (found.notices.some((line) => line.unseen)) {
        await ask("/notices/seen", "POST");
        window.dispatchEvent(new Event(SIGNED_IN_OR_OUT));
      }
    } catch {
      setLines((old) => old ?? []);
    }
  };

  if (!account) return null;
  return (
    <div ref={box} className="relative">
      <button
        type="button"
        onClick={show}
        aria-label={
          notices > 0 ? `Notifications: ${notices} new` : "Notifications"
        }
        aria-expanded={open}
        title="Notifications"
        className={`relative flex h-9 w-9 items-center justify-center rounded-lg transition hover:bg-raised/60 hover:text-ink ${
          open ? "bg-raised text-ink" : "text-muted"
        }`}
      >
        <Bell className="h-5 w-5" strokeWidth={2} aria-hidden />
        {notices > 0 && (
          <span className="num absolute -right-0.5 -top-0.5 flex h-[18px] min-w-[18px] items-center justify-center rounded-full bg-stop px-1 text-[11px] font-bold leading-none text-white ring-2 ring-bg">
            {notices > 99 ? "99+" : notices}
          </span>
        )}
      </button>
      {open && (
        <div className="fixed inset-x-3 top-[4.5rem] z-50 overflow-hidden rounded-2xl border border-line bg-surface shadow-2xl shadow-black/60 sm:absolute sm:inset-x-auto sm:right-0 sm:top-full sm:mt-2 sm:w-[400px]">
          <p className="border-b border-line px-5 py-4 text-base font-bold text-ink">
            Notifications
          </p>
          <div className="max-h-[min(70vh,560px)] space-y-1 overflow-y-auto p-2">
            {lines === null && (
              <p className="px-3 py-8 text-center text-sm text-muted">
                Loading…
              </p>
            )}
            {lines?.length === 0 && (
              <p className="px-6 py-10 text-center text-sm leading-relaxed text-muted">
                Nothing yet. Replies, reactions, messages and the network’s news
                about you will show up here.
              </p>
            )}
            {lines?.map((line, i) => (
              <Row
                key={`${line.kind}-${line.post ?? line.conversation}-${i}`}
                line={line}
                onOpen={() => setOpen(false)}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
