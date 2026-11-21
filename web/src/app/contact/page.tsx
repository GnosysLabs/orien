import type { Metadata } from 'next'
import { ContactForm } from '@/components/ContactForm'
import { DISCORD, X_ACCOUNT } from '@/lib/brand'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Contact',
  description: 'Write to the people behind Orien: questions, ideas, press, partnerships or trouble with your wallet.',
  path: '/contact/',
})

export default function ContactPage() {
  return (
    <div className="mx-auto max-w-[640px] pb-16">
      <header className="pb-8 pt-5 sm:pt-7">
        <p className="mb-4 text-xs font-semibold uppercase tracking-[0.2em] text-muted">Contact</p>
        <h1 className="text-balance text-4xl font-bold leading-[1.1] tracking-tight text-ink sm:text-5xl">Write to us</h1>
        <p className="mt-6 text-lg leading-relaxed text-muted">
          Questions, ideas, press, partnerships, or trouble with your wallet: we read every message and reply by email.
        </p>
      </header>
      <ContactForm />
      <p className="mt-8 text-sm leading-relaxed text-muted">
        For quick questions, the community is on{' '}
        <a href={DISCORD} target="_blank" rel="noreferrer" className="font-semibold text-ink underline underline-offset-4">
          Discord
        </a>{' '}
        and{' '}
        <a href={X_ACCOUNT} target="_blank" rel="noreferrer" className="font-semibold text-ink underline underline-offset-4">
          X
        </a>
        . Nobody from Orien will ever ask for your 24 words or your password.
      </p>
    </div>
  )
}
