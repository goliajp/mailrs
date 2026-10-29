import { useEffect, useState } from 'react'

import { MobileModal } from '@/components/mobile-modal'
import { btnPrimary, btnSecondary, inputClass } from '@/components/settings/_shared'
import { useSignOut } from '@/hooks/use-sign-out'
import { registerConfirmAsker } from '@/lib/confirm-asker'
import { wireConfirm, type WireConfirmOutcome } from '@/wire/endpoints/auth'

/**
 * Mounted once at the root. Shows the password dialog whenever a request is
 * refused for want of a recent confirmation, and tells the request whether
 * to try again.
 */
export function ConfirmPasswordHost() {
  const [resolve, setResolve] = useState<((confirmed: boolean) => void) | null>(null)

  useEffect(() => {
    registerConfirmAsker(
      () =>
        new Promise<boolean>((done) => {
          setResolve(() => done)
        })
    )
    return () => registerConfirmAsker(null)
  }, [])

  if (!resolve) return null
  return (
    <ConfirmPasswordDialog
      onDone={(confirmed) => {
        resolve(confirmed)
        setResolve(null)
      }}
    />
  )
}

function codeToSend(needsCode: boolean, code: string): string | undefined {
  if (!needsCode) return undefined
  return code
}

function ConfirmPasswordDialog({ onDone }: { onDone: (confirmed: boolean) => void }) {
  const [password, setPassword] = useState('')
  const [code, setCode] = useState('')
  const [needsCode, setNeedsCode] = useState(false)
  const [error, setError] = useState<null | string>(null)
  const [busy, setBusy] = useState(false)
  const signOut = useSignOut()

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    const outcome = await wireConfirm(password, codeToSend(needsCode, code)).catch(
      (): WireConfirmOutcome => ({ kind: 'refused', message: 'Could not reach the server' })
    )
    setBusy(false)
    switch (outcome.kind) {
      case 'code-needed':
        setNeedsCode(true)
        setError('Enter the code from your authenticator app')
        return
      case 'confirmed':
        onDone(true)
        return
      case 'refused':
        setError(outcome.message)
        return
      case 'signed-out':
        onDone(false)
        void signOut()
        return
      case 'wrong':
        setPassword('')
        setCode('')
        setError(wrongMessage(outcome.remaining))
        return
    }
  }

  return (
    <MobileModal onClose={() => onDone(false)} open>
      <form
        aria-labelledby="confirm-password-title"
        className="bg-surface w-full max-w-sm space-y-4 rounded-lg p-6 shadow-lg"
        onSubmit={submit}
      >
        <div className="space-y-1">
          <h2 className="text-base font-semibold" id="confirm-password-title">
            Confirm it’s you
          </h2>
          <p className="text-fg-secondary text-sm">
            Enter your password to continue. You won’t be asked again for 10 minutes.
          </p>
        </div>
        <label className="block space-y-1">
          <span className="text-fg-secondary text-sm font-medium">Password</span>
          <input
            autoComplete="current-password"
            autoFocus
            className={`${inputClass} w-full`}
            onChange={(e) => setPassword(e.target.value)}
            type="password"
            value={password}
          />
        </label>
        {needsCode && (
          <label className="block space-y-1">
            <span className="text-fg-secondary text-sm font-medium">Authenticator code</span>
            <input
              autoComplete="one-time-code"
              className={`${inputClass} w-full`}
              inputMode="numeric"
              onChange={(e) => setCode(e.target.value)}
              value={code}
            />
          </label>
        )}
        {error && (
          <p className="text-danger text-sm" role="alert">
            {error}
          </p>
        )}
        <div className="flex justify-end gap-2">
          <button className={btnSecondary} onClick={() => onDone(false)} type="button">
            Cancel
          </button>
          <button className={btnPrimary} disabled={busy || password === ''} type="submit">
            Continue
          </button>
        </div>
      </form>
    </MobileModal>
  )
}

function wrongMessage(remaining: number): string {
  if (remaining === 1) return 'Wrong password or code. 1 try left before you are signed out'
  return `Wrong password or code. ${remaining} tries left before you are signed out`
}
