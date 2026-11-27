'use client'

import { shrinkPicture } from '@/lib/shrink'
import { useEffect, useRef, useState } from 'react'
import Link from 'next/link'
import { Ban, Camera, ImagePlus } from 'lucide-react'
import { ask, MarketError, NAME_PATTERN, NAME_RULE, pictureUrl, type Account, type MessagesFrom, type WalletProof } from '@/lib/market'
import { WalletNetworkGlyph } from '@/components/WalletNetworkGlyph'
import { Avatar } from './Avatar'
import { Notice } from './Shell'
import { AutoTextarea } from '@/components/AutoTextarea'
import { DiscordIcon, TelegramIcon, XIcon } from '@/components/Header'

/** Who can message you, in the order shown. */
const MESSAGES_FROM: [MessagesFrom, string][] = [
  ['everyone', 'Everyone'],
  ['talked', 'People you’ve talked with'],
  ['nobody', 'No one'],
]

const field = 'w-full rounded-xl border border-line bg-bg px-4 py-3 text-ink outline-none focus:border-pearl/60'

/** Your own profile: picture, name and bio, saved by themselves a moment
 * after you stop typing, once the moderation check passes. */
export function ProfileEditor({ account, onSaved, wallets }: { account: Account; onSaved: () => void; wallets: WalletProof[] }) {
  const [savingPayout, setSavingPayout] = useState(false)
  const verified = wallets.filter(w => w.verified)
  const currentPayout = verified.find(w => w.address === account.wallet)?.address ?? ''
  async function choosePayout(address: string) {
    if (savingPayout || !verified.some(w => w.address === address)) return
    setSavingPayout(true)
    setTrouble('')
    try {
      await ask('/me/wallet', 'POST', { address })
      await onSaved()
    } catch (error) {
      setTrouble((error as Error).message)
    } finally {
      setSavingPayout(false)
    }
  }
  const [name, setName] = useState(account.name)
  const [bio, setBio] = useState(account.bio)
  const [discord, setDiscord] = useState(account.contact.discord ?? '')
  const [telegram, setTelegram] = useState(account.contact.telegram)
  const [x, setX] = useState(account.contact.x)
  const savedFrom = account.messages_from ?? 'everyone'
  const [messagesFrom, setMessagesFrom] = useState<MessagesFrom>(savedFrom)
  const [busy, setBusy] = useState<'' | 'text' | 'picture' | 'banner'>('')
  const [trouble, setTrouble] = useState('')
  const [saved, setSaved] = useState(false)
  const chooser = useRef<HTMLInputElement>(null)
  const bannerChooser = useRef<HTMLInputElement>(null)
  const changed =
    name !== account.name ||
    bio !== account.bio ||
    discord !== (account.contact.discord ?? '') ||
    telegram !== account.contact.telegram ||
    x !== account.contact.x ||
    messagesFrom !== savedFrom

  const say = (error: unknown) => {
    const message = (error as Error).message
    setTrouble(message.charAt(0).toUpperCase() + message.slice(1) + (message.endsWith('.') ? '' : '.'))
  }

  // Saves a moment after the typing stops. A name that can't be one waits,
  // with the rule shown, rather than asking the server.
  const nameOk = new RegExp(`^${NAME_PATTERN}$`).test(name)
  useEffect(() => {
    if (!changed || !nameOk) return
    const timer = setTimeout(async () => {
      const sent = { name, bio, discord, telegram, x }
      setBusy((b) => (b === '' ? 'text' : b))
      setTrouble('')
      setSaved(false)
      try {
        const answer = await ask<{ account: Account }>('/me', 'PUT', {
          name,
          bio,
          contact: { telegram, x, discord },
          messages_from: messagesFrom,
        })
        // Shown as the server tidied them, such as without the @, unless
        // they were changed again meanwhile.
        const c = answer.account.contact
        setDiscord((now) => (now === sent.discord ? c.discord : now))
        setTelegram((now) => (now === sent.telegram ? c.telegram : now))
        setX((now) => (now === sent.x ? c.x : now))
        setSaved(true)
        onSaved()
      } catch (error) {
        say(error)
      }
      setBusy((b) => (b === 'text' ? '' : b))
    }, 1500)
    return () => clearTimeout(timer)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [name, bio, discord, telegram, x, messagesFrom])

  const upload = async (file: File | undefined) => {
    if (!file) return
    setBusy('picture')
    setTrouble('')
    try {
      const response = await fetch('/api/market/me/picture', { method: 'POST', credentials: 'same-origin', body: await shrinkPicture(file) })
      const data = await response.json().catch(() => ({}))
      if (!response.ok) throw new MarketError(data.error ?? 'The picture could not be saved.', response.status)
      onSaved()
    } catch (error) {
      say(error)
    }
    setBusy('')
    if (chooser.current) chooser.current.value = ''
  }

  // The banner: sent whole, checked like the picture; or removed.
  const setBanner = async (file: File | undefined | null) => {
    if (file === undefined) return
    setBusy('banner')
    setTrouble('')
    try {
      const response = await fetch('/api/market/me/banner', {
        method: file ? 'POST' : 'DELETE',
        credentials: 'same-origin',
        body: file ? await shrinkPicture(file) : undefined,
      })
      const data = await response.json().catch(() => ({}))
      if (!response.ok) throw new MarketError(data.error ?? 'The banner could not be saved.', response.status)
      onSaved()
    } catch (error) {
      say(error)
    }
    setBusy('')
    if (bannerChooser.current) bannerChooser.current.value = ''
  }

  return (
    <form onSubmit={(event) => event.preventDefault()} className="space-y-5 rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <div className="space-y-2">
        <p className="text-sm font-semibold text-ink">{busy === 'banner' ? 'Checking your banner…' : 'Banner'}</p>
        <div className="relative flex aspect-[3/1] w-full items-end justify-end overflow-hidden rounded-xl border border-line bg-bg">
          {account.banner ? (
            // eslint-disable-next-line @next/next/no-img-element
            <img src={pictureUrl(account.banner)} alt="" className="absolute inset-0 h-full w-full object-cover" />
          ) : (
            <p className="absolute inset-0 flex items-center justify-center px-4 text-center text-sm text-muted">
              A wide picture across the top of your profile. 3:1 works best, like 1500 × 500.
            </p>
          )}
          <div className="relative m-3 flex gap-2">
            <button
              type="button"
              onClick={() => bannerChooser.current?.click()}
              disabled={busy !== ''}
              className="flex h-9 items-center gap-1.5 rounded-lg border border-line bg-surface/90 px-3 text-sm font-semibold text-ink backdrop-blur transition hover:bg-raised disabled:opacity-60"
            >
              <ImagePlus className="h-4 w-4" aria-hidden /> {account.banner ? 'Change banner' : 'Add banner'}
            </button>
            {account.banner && (
              <button
                type="button"
                onClick={() => setBanner(null)}
                disabled={busy !== ''}
                className="flex h-9 items-center rounded-lg border border-line bg-surface/90 px-3 text-sm font-semibold text-muted backdrop-blur transition hover:text-stop disabled:opacity-60"
              >
                Remove
              </button>
            )}
          </div>
        </div>
        <input ref={bannerChooser} type="file" accept="image/*" className="hidden" onChange={(e) => setBanner(e.target.files?.[0])} />
      </div>
      <div className="flex flex-wrap items-center gap-5">
        <button
          type="button"
          onClick={() => chooser.current?.click()}
          disabled={busy !== ''}
          className="group relative rounded-full"
          aria-label="Change your picture"
        >
          <Avatar name={name || account.name} picture={account.picture} size={88} />
          <span className="absolute inset-0 flex items-center justify-center rounded-full bg-black/50 opacity-0 transition group-hover:opacity-100">
            <Camera className="h-6 w-6 text-white" aria-hidden />
          </span>
        </button>
        <input ref={chooser} type="file" accept="image/*" className="hidden" onChange={(e) => upload(e.target.files?.[0])} />
        <div className="space-y-1">
          <p className="font-semibold text-ink">{busy === 'picture' ? 'Checking your picture…' : 'Your picture'}</p>
          <p className="text-sm text-muted">Click it to choose a new one. A photo of you or your logo works best.</p>
        </div>
      </div>
      <label className="flex flex-col gap-2">
        <span className="text-sm font-semibold text-ink">Username</span>
        <input
          value={name}
          onChange={(e) => setName(e.target.value.replace(/\s/g, '_'))}
          maxLength={20}
          minLength={3}
          pattern={NAME_PATTERN}
          title={`Your username: ${NAME_RULE}`}
          required
          spellCheck={false}
          autoCapitalize="off"
          className={field}
        />
        <span className="text-xs text-muted">
          Your profile is at orien.network/u/{name || account.name}. Use {NAME_RULE}; nobody else can have the same one.
        </span>
      </label>
      <label className="flex flex-col gap-2">
        <span className="text-sm font-semibold text-ink">About you</span>
        <AutoTextarea
          value={bio}
          onChange={(e) => setBio(e.target.value)}
          maxLength={1000}
          rows={4}
          placeholder="What you do, your experience, how you like to work."
          className={field}
        />
      </label>
      <fieldset className="space-y-3">
        <legend className="text-sm font-semibold text-ink">Socials</legend>
        <p className="text-sm text-muted">Show your accounts on your profile, if you like. Everyone can see them.</p>
        <div className="grid gap-3 sm:grid-cols-3">
          {(
            [
              ['Telegram', telegram, setTelegram, TelegramIcon],
              ['X', x, setX, XIcon],
              ['Discord', discord, setDiscord, DiscordIcon],
            ] as const
          ).map(([label, value, set, Icon]) => (
            <label key={label} className="relative block">
              <Icon className="pointer-events-none absolute left-4 top-1/2 h-4 w-4 -translate-y-1/2 text-muted" />
              <input
                value={value}
                onChange={(e) => set(e.target.value)}
                placeholder={label}
                aria-label={`Your ${label} username`}
                spellCheck={false}
                autoCapitalize="off"
                className={`${field} pl-11`}
              />
            </label>
          ))}
        </div>
      </fieldset>
      <fieldset className="space-y-3">
        <legend className="text-sm font-semibold text-ink">Who can message you</legend>
        <div role="radiogroup" aria-label="Who can message you" className="grid gap-2 sm:grid-cols-3">
          {MESSAGES_FROM.map(([value, label]) => {
            const on = messagesFrom === value
            return (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={on}
                onClick={() => setMessagesFrom(value)}
                className={`flex h-11 items-center justify-center rounded-xl border px-4 text-sm font-semibold transition ${
                  on ? 'border-pearl/60 bg-pearl/15 text-ink' : 'border-line bg-bg text-muted hover:bg-raised hover:text-ink'
                }`}
              >
                {label}
              </button>
            )
          })}
        </div>
        <div className="flex flex-wrap items-end gap-4">
          <div>
            <span className="mb-1 block text-sm text-muted">Blocked accounts</span>
            <Link href="/account/blocked/" className="inline-flex h-10 items-center gap-2 rounded-xl border border-line bg-bg px-4 text-sm font-semibold text-ink transition hover:bg-raised">
              <Ban className="h-4 w-4 text-muted" aria-hidden /> Manage blocks
            </Link>
          </div>
          {verified.length > 0 && (
            <label className="min-w-0 max-w-full">
              <span className="mb-1 block text-sm text-muted">Payout wallet</span>
              <span className="flex h-10 items-center rounded-xl border border-line bg-bg px-3 text-sm text-ink focus-within:border-pearl">
                <WalletNetworkGlyph />
                <select className="min-w-0 max-w-60 bg-bg font-semibold" value={currentPayout} disabled={savingPayout || (verified.length === 1 && !!currentPayout)} onChange={e => void choosePayout(e.target.value)}>
                  {!currentPayout && <option value="" disabled>Choose a wallet</option>}
                  {verified.map(w => <option key={w.address} value={w.address}>{w.name || 'Wallet'} · {w.address.slice(0,8)}…{w.address.slice(-8)}</option>)}
                </select>
              </span>
            </label>
          )}
        </div>
      </fieldset>
      {trouble && <Notice tone="bad">{trouble}</Notice>}
      <p className="h-5 text-sm" aria-live="polite">
        {busy === 'text' ? (
          <span className="text-muted">Saving…</span>
        ) : !nameOk ? (
          <span className="text-muted">Your username needs {NAME_RULE} before it saves.</span>
        ) : saved && !changed ? (
          <span className="text-go">Saved</span>
        ) : changed && !trouble ? (
          <span className="text-muted">Changes save by themselves.</span>
        ) : null}
      </p>
    </form>
  )
}
