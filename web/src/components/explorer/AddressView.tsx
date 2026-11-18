'use client'

import { useEffect, useState } from 'react'
import { ArrowDownLeft, ArrowUpRight, Pickaxe, RotateCcw, Unlock, Landmark, KeyRound } from 'lucide-react'
import { CopyText } from '@/components/CopyText'
import { Avatar } from '@/components/market/Avatar'
import { profileHref } from '@/lib/market'
import {
  type AddressInfo,
  type Entry,
  type WalletProfile,
  addressHref,
  ago,
  checkpointHref,
  explore,
  label,
  namesFor,
  opened,
  orn,
  short,
  when,
} from '@/lib/explorer'
import { Card, Loading, Problem, Stat } from './Parts'

const KINDS: Record<Entry['kind'], { label: string; to?: string; icon: typeof Pickaxe; out?: boolean }> = {
  received: { label: 'Received', to: 'from', icon: ArrowDownLeft },
  sent: { label: 'Sent', to: 'to', icon: ArrowUpRight, out: true },
  mined: { label: 'Mining reward', icon: Pickaxe },
  fees: { label: 'Validator fees', icon: Landmark },
  claimed: { label: 'Locked payment received', to: 'from', icon: KeyRound },
  refunded: { label: 'Locked payment returned', icon: RotateCcw },
  unstaked: { label: 'Stake returned', icon: Unlock },
}

function Who({ address, names }: { address: string; names: Record<string, WalletProfile> }) {
  const profile = address ? names[address] : undefined
  return (
    <a href={addressHref(address)} className="num text-ink underline decoration-line underline-offset-4 hover:decoration-pearl">
      {profile ? label(profile) : short(address)}
    </a>
  )
}

export function AddressView() {
  const [address, setAddress] = useState<string | null>(null)
  useEffect(() => setAddress(opened('address', 'a').trim().toLowerCase()), [])
  const [info, setInfo] = useState<AddressInfo | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [names, setNames] = useState<Record<string, WalletProfile>>({})

  useEffect(() => {
    if (address === null) return
    setInfo(null)
    setError(null)
    if (!/^[0-9a-f]{64}$/.test(address)) {
      setError('That is not an Orien address. An address is 64 letters and digits, 0-9 and a-f.')
      return
    }
    let live = true
    explore<AddressInfo>(`/address/${address}`)
      .then((found) => {
        if (!live) return
        setInfo(found)
        const others = found.activity.map((e) => e.counterparty).filter((a): a is string => !!a)
        void namesFor([address, ...others]).then((n) => live && setNames(n))
      })
      .catch((e: Error) => live && setError(e.message))
    return () => {
      live = false
    }
  }, [address])

  const profile = address ? names[address] : undefined
  const empty = info && info.activity.length === 0 && info.balance === '0' && info.staked === '0'

  return (
    <div className="space-y-10">
      <div className="space-y-4">
        <h1 className="text-3xl font-bold tracking-tight text-ink">Address</h1>
        {profile && (
          <a
            href={profileHref(profile.name)}
            className="flex w-fit items-center gap-3 rounded-xl border border-line bg-surface py-2 pl-2 pr-4 transition hover:border-pearl"
          >
            <Avatar name={profile.name} picture={profile.picture} size={36} />
            <span>
              <span className="block font-semibold text-ink">{label(profile)}</span>
              <span className="block text-sm text-muted">Showed on the Market that they hold this wallet</span>
            </span>
          </a>
        )}
        {address && <CopyText text={address} />}
      </div>

      {error && <Problem text={error} />}
      {!error && !info && <Loading />}

      {info && (
        <>
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <Stat label="Balance" value={`${orn(info.balance, 4)} ORN`} />
            {(info.staked !== '0' || info.returning !== '0') && (
              <Stat
                label="Staked"
                value={`${orn(info.staked, 4)} ORN`}
                note={info.returning !== '0' && `${orn(info.returning, 4)} ORN on its way back`}
              />
            )}
            <Stat label="Payments sent" value={info.payments_made.toLocaleString('en')} />
          </div>

          <Card title="Activity" aside={info.more && <span className="text-sm text-muted">The latest 500</span>}>
            {empty ? (
              <p className="px-6 py-10 text-center text-muted">Nothing has happened at this address yet.</p>
            ) : info.activity.length === 0 ? (
              <p className="px-6 py-10 text-center text-muted">No activity is recorded for this address.</p>
            ) : (
              <ul className="divide-y divide-line">
                {info.activity.map((entry, index) => {
                  const kind = KINDS[entry.kind]
                  const Icon = kind.icon
                  return (
                    <li key={index} className="flex items-center gap-4 px-5 py-4 sm:px-6">
                      <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-raised">
                        <Icon className={`h-4 w-4 ${kind.out ? 'text-muted' : 'text-go'}`} aria-hidden />
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className="block text-ink">
                          {kind.label}
                          {kind.to && entry.counterparty && (
                            <>
                              {' '}
                              <span className="text-muted">{kind.to}</span> <Who address={entry.counterparty} names={names} />
                            </>
                          )}
                        </span>
                        <span className="block text-sm text-muted">
                          {entry.time ? ago(entry.time) : 'This hour'} ·{' '}
                          <a href={checkpointHref(entry.epoch)} className="hover:text-ink">
                            checkpoint {entry.epoch}
                          </a>
                        </span>
                      </span>
                      <span className={`num shrink-0 text-right font-semibold ${kind.out ? 'text-ink' : 'text-go'}`}>
                        {kind.out ? '−' : '+'}
                        {orn(entry.amount)}
                      </span>
                    </li>
                  )
                })}
              </ul>
            )}
          </Card>

          {info.since > 0 && (
            <p className="text-center text-sm text-muted">
              Activity is listed from checkpoint {info.since}
              {info.since_time ? ` (${when(info.since_time)})` : ''}, when validators began keeping these records.
            </p>
          )}
        </>
      )}
    </div>
  )
}
