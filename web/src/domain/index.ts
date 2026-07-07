/**
 * Layer 1 barrel — the domain contract every other layer imports from.
 * Nothing above layer 1 should reach into a sub-module directly; use
 * this file.
 *
 * See `.claude/rfcs/20260707-v2.1-webapp-reconstruction.md` §2.1.
 */

export type {
  Attachment,
  Category,
  ConversationFilter,
  Folder,
  ImportanceLevel,
  Message,
  Participant,
  Thread,
  ThreadSummary,
} from './conversation'

export {
  canonicaliseFilter,
  FLAG_ANSWERED,
  FLAG_FLAGGED,
  FLAG_SEEN,
  OPTIMISTIC_VERSION,
} from './conversation'

export type { AccountId, AliasAddress, DomainName, DraftId, MessageId, ThreadId, Uid } from './ids'

export {
  asAccountId,
  asAliasAddress,
  asDomainName,
  asDraftId,
  asMessageId,
  asThreadId,
  asUid,
} from './ids'
