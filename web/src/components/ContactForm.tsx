'use client'

import { useState } from 'react'
import { CheckCircle2, Send } from 'lucide-react'
import { ask, sentence } from '@/lib/market'
import { AutoTextarea } from '@/components/AutoTextarea'

const FIELD =
  'w-full rounded-xl border border-line bg-bg px-4 py-3 text-ink outline-none transition placeholder:text-muted/60 focus:border-pearl/60'

/** The contact form: a name, an email to reply to, and a message, emailed
 * to hello@orien.network. */
export function ContactForm() {
  const [name, setName] = useState('')
  const [email, setEmail] = useState('')
  const [message, setMessage] = useState('')
  // Hidden from people; a program filling it in is quietly ignored.
  const [website, setWebsite] = useState('')
  const [state, setState] = useState<'idle' | 'sending' | 'sent'>('idle')
  const [trouble, setTrouble] = useState('')

  if (state === 'sent') {
    return (
      <div className="flex flex-col items-center gap-3 rounded-2xl border border-line bg-surface p-10 text-center">
        <CheckCircle2 className="h-10 w-10 text-pearl" strokeWidth={1.75} aria-hidden />
        <p className="text-lg font-semibold text-ink">Thanks, {name.trim().split(/\s+/)[0]}. Your message is on its way.</p>
        <p className="text-sm text-muted">We’ll reply to {email.trim()}.</p>
      </div>
    )
  }

  const send = async (event: React.FormEvent) => {
    event.preventDefault()
    setState('sending')
    setTrouble('')
    try {
      await ask('/contact', 'POST', { name, email, message, website })
      setState('sent')
    } catch (error) {
      setTrouble(sentence((error as Error).message))
      setState('idle')
    }
  }

  return (
    <form onSubmit={send} className="flex flex-col gap-5 rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <label className="flex flex-col gap-2">
        <span className="text-sm font-semibold text-ink">Your name</span>
        <input value={name} onChange={(e) => setName(e.target.value)} required maxLength={100} autoComplete="name" className={FIELD} />
      </label>
      <label className="flex flex-col gap-2">
        <span className="text-sm font-semibold text-ink">Your email</span>
        <input
          type="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          required
          autoComplete="email"
          placeholder="So we can reply"
          className={FIELD}
        />
      </label>
      <label className="absolute -left-[10000px] h-px w-px overflow-hidden" aria-hidden>
        Website
        <input value={website} onChange={(e) => setWebsite(e.target.value)} tabIndex={-1} autoComplete="off" />
      </label>
      <label className="flex flex-col gap-2">
        <span className="text-sm font-semibold text-ink">Message</span>
        <AutoTextarea
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          required
          minLength={10}
          maxLength={5000}
          rows={6}
          className={FIELD}
        />
      </label>
      {trouble && <p className="text-sm text-stop">{trouble}</p>}
      <button
        type="submit"
        disabled={state === 'sending'}
        className="pearl-fill flex h-12 items-center justify-center gap-2 rounded-xl px-6 text-sm font-bold transition hover:brightness-105 disabled:opacity-60"
      >
        <Send className="h-4 w-4" aria-hidden /> {state === 'sending' ? 'Sending…' : 'Send message'}
      </button>
    </form>
  )
}
