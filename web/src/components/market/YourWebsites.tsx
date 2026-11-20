'use client'

import { useEffect, useState } from 'react'
import { Code2, Plus } from 'lucide-react'
import { ask } from '@/lib/market'
import { CopyText } from '@/components/CopyText'
import { Notice } from './Shell'

type Site = { client_id: string; name: string; returns: string[]; has_secret: boolean; created: number; off: boolean }

const field = 'h-11 w-full rounded-xl border border-line bg-bg px-4 text-ink outline-none focus:border-pearl/60'
const small =
  'flex h-9 items-center gap-2 rounded-lg border border-line px-3 text-sm font-semibold text-ink transition hover:border-pearl disabled:opacity-60'
const main =
  'pearl-fill flex h-11 items-center justify-center rounded-xl px-6 text-sm font-bold transition hover:brightness-105 disabled:opacity-60'

/** Websites this account registered for Sign in with Orien. */
export function YourWebsites() {
  const [found, setFound] = useState<{ may_register: boolean; sites: Site[] } | null>(null)
  const [adding, setAdding] = useState(false)
  const [secret, setSecret] = useState<{ client_id: string; secret: string } | null>(null)
  const reload = () =>
    ask<{ may_register: boolean; sites: Site[] }>('/me/sites')
      .then(setFound)
      .catch(() => setFound(null))
  useEffect(() => {
    void reload()
  }, [])

  if (!found) return null

  return (
    <section className="space-y-5 rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <div className="flex flex-wrap items-start gap-3">
        <div className="min-w-0 flex-1 space-y-1.5">
          <h2 className="flex items-center gap-2 text-xl font-bold tracking-tight text-ink">
            <Code2 className="h-5 w-5 text-pearl" aria-hidden /> Your websites
          </h2>
        </div>
        {found.may_register && !adding && (
          <button type="button" onClick={() => setAdding(true)} className={small}>
            <Plus className="h-4 w-4" aria-hidden /> Add a website
          </button>
        )}
      </div>

      {adding && (
        <SiteForm
          onCancel={() => setAdding(false)}
          onSaved={(added) => {
            setAdding(false)
            if (added.secret) setSecret({ client_id: added.site.client_id, secret: added.secret })
            void reload()
          }}
        />
      )}

      {!found.may_register && found.sites.length === 0 && (
        <Notice>Registering websites opens soon.</Notice>
      )}
      {found.may_register && found.sites.length === 0 && !adding && <Notice>No websites yet.</Notice>}

      <ul className="space-y-4">
        {found.sites.map((site) => (
          <SiteRow
            key={site.client_id}
            site={site}
            secret={secret?.client_id === site.client_id ? secret.secret : null}
            onSecret={(value) => setSecret({ client_id: site.client_id, secret: value })}
            onChange={() => void reload()}
          />
        ))}
      </ul>
    </section>
  )
}

function SiteRow({
  site,
  secret,
  onSecret,
  onChange,
}: {
  site: Site
  secret: string | null
  onSecret: (secret: string) => void
  onChange: () => void
}) {
  const [editing, setEditing] = useState(false)
  const [trouble, setTrouble] = useState('')
  const [busy, setBusy] = useState(false)

  const run = async (work: () => Promise<void>) => {
    setBusy(true)
    setTrouble('')
    try {
      await work()
    } catch (error) {
      setTrouble((error as Error).message)
    }
    setBusy(false)
  }

  if (editing) {
    return (
      <li className="rounded-xl border border-line p-4 sm:p-5">
        <SiteForm
          site={site}
          onCancel={() => setEditing(false)}
          onSaved={() => {
            setEditing(false)
            onChange()
          }}
        />
      </li>
    )
  }

  return (
    <li className="space-y-4 rounded-xl border border-line p-4 sm:p-5">
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="min-w-0 flex-1 truncate text-lg font-semibold text-ink">{site.name}</h3>
        {site.off && <span className="rounded-md border border-stop/50 px-2 py-0.5 text-xs font-semibold text-stop">Switched off</span>}
      </div>
      <CopyText label="Client ID" text={site.client_id} />
      {secret && (
        <div className="space-y-2">
          <CopyText label="Client secret: copy it now, it won’t be shown again" text={secret} />
        </div>
      )}
      <div className="space-y-1.5">
        <p className="text-xs font-medium text-muted">Return addresses</p>
        <ul className="space-y-1">
          {site.returns.map((address) => (
            <li key={address} className="num break-all text-sm text-ink">
              {address}
            </li>
          ))}
        </ul>
      </div>
      <p className="text-sm text-muted">
        {site.has_secret ? 'Signs in from its server, with a secret.' : 'Signs in from its pages, with PKCE and no secret.'}
      </p>
      {trouble && <Notice tone="bad">{trouble}</Notice>}
      <div className="flex flex-wrap gap-2">
        <button type="button" onClick={() => setEditing(true)} disabled={busy} className={small}>
          Edit
        </button>
        {site.has_secret && (
          <button
            type="button"
            disabled={busy}
            className={small}
            onClick={() =>
              confirm('Make a new secret? The old one stops working at once.') &&
              void run(async () => {
                const { secret } = await ask<{ secret: string }>(`/me/sites/${site.client_id}/secret`, 'POST')
                onSecret(secret)
              })
            }
          >
            New secret
          </button>
        )}
        <button
          type="button"
          disabled={busy}
          className={`${small} text-stop`}
          onClick={() =>
            confirm(`Remove ${site.name}? People can no longer sign in to it with Orien.`) &&
            void run(async () => {
              await ask(`/me/sites/${site.client_id}`, 'DELETE')
              onChange()
            })
          }
        >
          Remove
        </button>
      </div>
    </li>
  )
}

function SiteForm({
  site,
  onCancel,
  onSaved,
}: {
  site?: Site
  onCancel: () => void
  onSaved: (added: { site: Site; secret?: string | null }) => void
}) {
  const [name, setName] = useState(site?.name ?? '')
  const [returns, setReturns] = useState(site?.returns.join('\n') ?? '')
  const [server, setServer] = useState(true)
  const [busy, setBusy] = useState(false)
  const [trouble, setTrouble] = useState('')

  const save = async (event: React.FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setTrouble('')
    const body = { name, returns: returns.split('\n').map((r) => r.trim()).filter(Boolean), confidential: server }
    try {
      const saved = site
        ? await ask<{ site: Site }>(`/me/sites/${site.client_id}`, 'PUT', body)
        : await ask<{ site: Site; secret: string | null }>('/me/sites', 'POST', body)
      onSaved(saved)
    } catch (error) {
      setTrouble((error as Error).message)
      setBusy(false)
    }
  }

  return (
    <form onSubmit={save} className="space-y-4">
      <label className="block space-y-1.5">
        <span className="text-sm font-semibold text-ink">Website name</span>
        <span className="block text-sm text-muted">Shown to people when they sign in.</span>
        <input value={name} onChange={(e) => setName(e.target.value)} maxLength={40} required className={field} />
      </label>
      <label className="block space-y-1.5">
        <span className="text-sm font-semibold text-ink">Return addresses</span>
        <span className="block text-sm text-muted">
          Where people come back to after signing in, one a line. https://, or http://localhost while you build.
        </span>
        <textarea
          value={returns}
          onChange={(e) => setReturns(e.target.value)}
          rows={3}
          required
          spellCheck={false}
          placeholder="https://example.com/auth/callback"
          className="num w-full rounded-xl border border-line bg-bg px-4 py-3 text-sm text-ink outline-none focus:border-pearl/60"
        />
      </label>
      {!site && (
        <fieldset className="space-y-2">
          <legend className="text-sm font-semibold text-ink">How it signs in</legend>
          <div className="grid gap-2 sm:grid-cols-2">
            {[
              { value: true, title: 'From its server', note: 'Gets a secret to keep on the server.' },
              { value: false, title: 'From its pages only', note: 'No secret; uses PKCE.' },
            ].map((choice) => (
              <button
                type="button"
                key={String(choice.value)}
                onClick={() => setServer(choice.value)}
                className={`rounded-xl border p-3 text-left transition ${server === choice.value ? 'border-pearl bg-raised' : 'border-line hover:border-pearl/60'}`}
              >
                <span className="block text-sm font-semibold text-ink">{choice.title}</span>
                <span className="block text-sm text-muted">{choice.note}</span>
              </button>
            ))}
          </div>
        </fieldset>
      )}
      {trouble && <Notice tone="bad">{trouble}</Notice>}
      <div className="flex flex-wrap gap-3">
        <button type="submit" disabled={busy} className={main}>
          {busy ? 'Saving…' : site ? 'Save' : 'Add website'}
        </button>
        <button
          type="button"
          onClick={onCancel}
          className="flex h-11 items-center rounded-xl border border-line px-5 text-sm font-semibold text-ink transition hover:border-pearl"
        >
          Cancel
        </button>
      </div>
    </form>
  )
}
