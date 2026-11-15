'use client'

import Link from 'next/link'
import { useRef, useState } from 'react'
import { Camera, ImagePlus } from 'lucide-react'
import { ask, MarketError, pictureUrl, type Account } from '@/lib/market'
import { Avatar } from './Avatar'
import { Notice } from './Shell'
import { AutoTextarea } from '@/components/AutoTextarea'

const field = 'w-full rounded-xl border border-line bg-bg px-4 py-3 text-ink outline-none focus:border-pearl/60'

/** Your own profile: picture, name and bio, saved after the moderation check. */
export function ProfileEditor({ account, onSaved }: { account: Account; onSaved: () => void }) {
  const [name, setName] = useState(account.name)
  const [bio, setBio] = useState(account.bio)
  const [email, setEmail] = useState(account.contact.email)
  const [telegram, setTelegram] = useState(account.contact.telegram)
  const [x, setX] = useState(account.contact.x)
  const [busy, setBusy] = useState<'' | 'text' | 'picture' | 'banner'>('')
  const [trouble, setTrouble] = useState('')
  const [saved, setSaved] = useState(false)
  const chooser = useRef<HTMLInputElement>(null)
  const bannerChooser = useRef<HTMLInputElement>(null)
  const changed =
    name !== account.name ||
    bio !== account.bio ||
    email !== account.contact.email ||
    telegram !== account.contact.telegram ||
    x !== account.contact.x

  const say = (error: unknown) => {
    const message = (error as Error).message
    setTrouble(message.charAt(0).toUpperCase() + message.slice(1) + (message.endsWith('.') ? '' : '.'))
  }

  const save = async (event: React.FormEvent) => {
    event.preventDefault()
    setBusy('text')
    setTrouble('')
    setSaved(false)
    try {
      const saved = await ask<{ account: Account }>('/me', 'PUT', { name, bio, contact: { email, telegram, x } })
      // Shown as the server tidied them, such as without the @.
      setEmail(saved.account.contact.email)
      setTelegram(saved.account.contact.telegram)
      setX(saved.account.contact.x)
      setSaved(true)
      onSaved()
    } catch (error) {
      say(error)
    }
    setBusy('')
  }

  const upload = async (file: File | undefined) => {
    if (!file) return
    setBusy('picture')
    setTrouble('')
    try {
      const response = await fetch('/api/market/me/picture', { method: 'POST', credentials: 'same-origin', body: file })
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
        body: file ?? undefined,
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
    <form onSubmit={save} className="space-y-5 rounded-2xl border border-line bg-surface p-6 sm:p-8">
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
          <Link href={`/marketplace/profile/?id=${account.id}`} className="text-sm font-semibold text-ink underline underline-offset-4">
            See your public profile
          </Link>
        </div>
      </div>
      <label className="flex flex-col gap-2">
        <span className="text-sm font-semibold text-ink">Name</span>
        <input value={name} onChange={(e) => setName(e.target.value)} maxLength={50} required className={field} />
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
        <legend className="text-sm font-semibold text-ink">Other ways to reach you (optional)</legend>
        <p className="text-sm text-muted">
          Buyers can always message you here. Add these too if you like: everyone can see them.
        </p>
        <div className="grid gap-3 sm:grid-cols-3">
          <input
            type="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            placeholder="Email"
            className={field}
          />
          <input value={telegram} onChange={(e) => setTelegram(e.target.value)} placeholder="Telegram" className={field} />
          <input value={x} onChange={(e) => setX(e.target.value)} placeholder="X" className={field} />
        </div>
      </fieldset>
      {trouble && <Notice tone="bad">{trouble}</Notice>}
      <div className="flex items-center gap-4">
        <button
          type="submit"
          disabled={busy !== '' || !changed}
          className="pearl-fill flex h-11 items-center rounded-xl px-6 text-sm font-bold transition hover:brightness-105 disabled:opacity-60"
        >
          {busy === 'text' ? 'Saving…' : 'Save profile'}
        </button>
        {saved && !changed && <span className="text-sm text-go">Saved</span>}
      </div>
    </form>
  )
}
