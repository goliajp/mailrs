/**
 * Wire-layer error taxonomy — layer 2 (see RFC §2.2).
 *
 * Every call into the wire client resolves either to a domain value or
 * rejects with a `WireError`. Downstream code branches on `kind`; no
 * component ever renders a bare HTTP status number as if it were user-
 * facing copy.
 */

import type { z } from 'zod'

export type WireError =
  | { readonly issues: readonly z.ZodIssue[]; readonly kind: 'validation' }
  | { readonly kind: 'aborted' }
  | { readonly kind: 'auth' }
  | { readonly kind: 'forbidden' }
  | { readonly kind: 'network' }
  | { readonly kind: 'not-found' }
  | { readonly kind: 'server'; readonly message: string; readonly status: number }

/**
 * A tiny wrapper so `throw` semantics still work while `catch` blocks
 * type-narrow properly. The class carries the discriminated payload;
 * `instanceof WireErrorException` acts as the type guard.
 */
export class WireErrorException extends Error {
  readonly detail: WireError

  constructor(detail: WireError, message?: string) {
    super(message ?? WireErrorException.messageFor(detail))
    this.detail = detail
    this.name = 'WireErrorException'
  }

  static messageFor(detail: WireError): string {
    switch (detail.kind) {
      case 'aborted':
        return 'Request aborted'
      case 'auth':
        return 'Authentication required'
      case 'forbidden':
        return 'Not authorised'
      case 'network':
        return 'Network error'
      case 'not-found':
        return 'Resource not found'
      case 'server':
        return detail.message
      case 'validation':
        return `Response failed validation (${detail.issues.length} issue${
          detail.issues.length === 1 ? '' : 's'
        })`
    }
  }
}

export function isWireError(err: unknown): err is WireErrorException {
  return err instanceof WireErrorException
}
