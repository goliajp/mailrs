import { isWireError } from '@/wire/errors'

export { errorText } from '@/lib/error-text'

/**
 * The namespace list answers 404 when the deployment has no mock domain
 * (`enabled()` in crates/webapi/src/handlers/mock/service.rs). The wire
 * client folds every 404 into `kind: 'not-found'`, so on the list route a
 * 404 means exactly that.
 */
export function isMockDisabled(error: unknown): boolean {
  return isWireError(error) && error.detail.kind === 'not-found'
}
