"use client";

import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { Download, ShieldAlert, X } from "lucide-react";

export const WINDOWS_INSTALLER = "/wallet/Orien-Wallet-setup.exe";

/** A Windows download link that first explains Windows' warning for new
 * apps and what to click, with the download button in the window. */
export function WindowsDownload({
  className,
  children,
}: {
  className?: string;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <a
        href={WINDOWS_INSTALLER}
        className={className}
        onClick={(event) => {
          event.preventDefault();
          setOpen(true);
        }}
      >
        {children}
      </a>
      {open && <WindowsInstall onClose={() => setOpen(false)} />}
    </>
  );
}

/** The window itself, over the whole page. */
export function WindowsInstall({ onClose }: { onClose: () => void }) {
  useEffect(() => {
    const close = (event: KeyboardEvent) => event.key === "Escape" && onClose();
    document.addEventListener("keydown", close);
    return () => document.removeEventListener("keydown", close);
  }, [onClose]);
  return createPortal(
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="windows-install-title"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onMouseDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <div className="relative w-full max-w-[520px] space-y-5 rounded-2xl border border-line bg-surface p-6 shadow-2xl shadow-black/50 sm:p-8">
        <button
          type="button"
          aria-label="Close"
          onClick={onClose}
          className="absolute right-3 top-3 flex h-8 w-8 items-center justify-center rounded-lg text-muted transition hover:bg-raised hover:text-ink"
        >
          <X className="h-4 w-4" aria-hidden />
        </button>
        <div className="flex items-center gap-3">
          <ShieldAlert className="h-6 w-6 shrink-0 text-pearl" strokeWidth={1.75} aria-hidden />
          <h2 id="windows-install-title" className="text-xl font-bold tracking-tight text-ink">
            Installing Orien Wallet on Windows
          </h2>
        </div>
        <p className="leading-relaxed text-muted">
          Orien Wallet is a new app, so Windows doesn&apos;t recognise it yet and shows a warning before it opens.
          That&apos;s normal for new apps. Here&apos;s what to click:
        </p>
        <ol className="list-decimal space-y-2 pl-5 leading-relaxed text-muted marker:font-semibold marker:text-ink">
          <li>
            If your browser says the file isn&apos;t commonly downloaded, choose <strong className="text-ink">Keep</strong>{" "}
            (in Edge, under the <strong className="text-ink">…</strong> next to the download).
          </li>
          <li>
            Open <strong className="text-ink">Orien-Wallet-setup.exe</strong>.
          </li>
          <li>
            On the blue <strong className="text-ink">Windows protected your PC</strong> screen, click{" "}
            <strong className="text-ink">More info</strong>.
          </li>
          <li>
            Click <strong className="text-ink">Run anyway</strong>, and follow the installer.
          </li>
        </ol>
        <p className="text-sm leading-relaxed text-muted">
          After that, Orien Wallet opens like any other app and keeps itself up to date.
        </p>
        <a
          href={WINDOWS_INSTALLER}
          download
          onClick={onClose}
          className="pearl-fill flex h-11 w-full items-center justify-center gap-2 rounded-xl px-5 text-sm font-bold transition hover:brightness-105"
        >
          <Download className="h-4 w-4" aria-hidden /> Download for Windows
        </a>
      </div>
    </div>,
    document.body,
  );
}
