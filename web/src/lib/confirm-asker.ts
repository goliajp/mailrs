/**
 * Who asks the user to type their password again.
 *
 * The server refuses some operations with 403 `confirm_required` until the
 * session has confirmed its password within the last ten minutes. The
 * dialog that asks lives in the React tree; `http` lives outside it. The
 * dialog registers itself here and `http` calls `askToConfirm`, which
 * resolves `true` once the server accepted the password, `false` when the
 * user gave up. Requests refused at the same moment share one dialog.
 */

type Asker = () => Promise<boolean>

let asker: Asker | null = null
let pending: null | Promise<boolean> = null

export function askToConfirm(): Promise<boolean> {
  if (pending) return pending
  if (!asker) return Promise.resolve(false)
  pending = asker().finally(() => {
    pending = null
  })
  return pending
}

export function registerConfirmAsker(fn: Asker | null): void {
  asker = fn
}
