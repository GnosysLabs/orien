import type { Metadata } from 'next'
import { SignInTest } from '@/components/market/SignInTest'

export const metadata: Metadata = { title: 'Sign in with Orien: test', robots: { index: false } }

export default function SignInTestPage() {
  return (
    <div className="mx-auto max-w-[1100px] pb-16 pt-5 sm:pt-7">
      <SignInTest />
    </div>
  )
}
