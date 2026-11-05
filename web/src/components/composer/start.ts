import type { Key } from '@/i18n'
import type { ComposeDraftSource, ComposeRedraftSource, ComposeReplySource } from '@/store/ui'

import { extractEmail } from '@/lib/avatar'
import { splitAddressList } from '@/lib/recipients'

/** What a floating window hands the page when it is expanded. */
export type ComposerCarry = {
  bcc: string
  body: string
  cc: string
  draftId: null | number
  from: string
  replyToThreadId: null | string
  scheduledAt: string
  subject: string
  to: string
}

/** How a compose session starts; every shell builds one of these. */
export type ComposerStart =
  | { carry: ComposerCarry; kind: 'carry' }
  | { ctx: ReplyContext; kind: 'inline'; mode: ReplyMode }
  | { draft: ComposeDraftSource; kind: 'draft' }
  | { kind: 'new' }
  | { kind: 'redraft'; redraft: ComposeRedraftSource }
  | { kind: 'reply'; reply: ComposeReplySource }

/**
 * Everything the composer needs about what is being replied to or
 * forwarded, computed once by the thread view for the inline reply.
 */
export type ReplyContext = {
  /** The connected mailbox this conversation arrived at, when it was one. */
  accountId?: string
  forwardAttachmentsUid: null | number
  forwardMessageId: null | string
  lastMessageId: string
  originalBody: string
  originalDate: string
  originalFrom: string
  originalHtmlBody: null | string
  /** The To and Cc of the message replied to, as written. */
  originalTo?: string
  replyAllRecipients: string
  replyRecipients: string
  subject: string
  threadId: string
}

export type ReplyMode = 'forward' | 'reply' | 'reply-all'

/** The attachments a draft a restore returned could not carry. */
export function attachmentsNotIncluded(s: ComposerStart): readonly string[] {
  if (s.kind === 'draft') return s.draft.attachmentsNotIncluded ?? []
  return []
}

export function initialBcc(s: ComposerStart): string {
  if (s.kind === 'draft') return s.draft.bcc
  if (s.kind === 'redraft') return s.redraft.bcc
  if (s.kind === 'carry') return s.carry.bcc
  return ''
}

/** The body a reopened session starts with, or null for an empty editor. */
export function initialBody(s: ComposerStart): null | string {
  if (s.kind === 'draft') return s.draft.body
  if (s.kind === 'redraft') return s.redraft.body
  if (s.kind === 'carry') return s.carry.body
  return null
}

export function initialCc(s: ComposerStart): string {
  if (s.kind === 'draft') return s.draft.cc
  if (s.kind === 'redraft') return s.redraft.cc
  if (s.kind === 'carry') return s.carry.cc
  return ''
}

export function initialDraftId(s: ComposerStart): null | number {
  if (s.kind === 'draft') return s.draft.id
  if (s.kind === 'carry') return s.carry.draftId
  return null
}

export function initialSubject(s: ComposerStart): string {
  switch (s.kind) {
    case 'carry':
      return s.carry.subject
    case 'draft':
      return s.draft.subject
    case 'inline':
      return modeSubject(s.ctx.subject, s.mode)
    case 'new':
      return ''
    // a re-edit is the same message being fixed, not a reply to it
    case 'redraft':
      return s.redraft.subject
    case 'reply':
      return withPrefix('Re:', s.reply.subject)
  }
}

export function initialTo(s: ComposerStart): string {
  switch (s.kind) {
    case 'carry':
      return s.carry.to
    case 'draft':
      return s.draft.to
    case 'inline':
      return modeTo(s.ctx, s.mode)
    case 'new':
      return ''
    case 'redraft':
      return s.redraft.to
    case 'reply':
      return extractReplyAddress(s.reply.sender)
  }
}

export function inReplyToOf(s: ComposerStart): string | undefined {
  if (s.kind === 'reply') return s.reply.messageId
  if (s.kind === 'redraft') return s.redraft.inReplyTo ?? undefined
  if (s.kind === 'inline') return threadUnlessForward(s.ctx.lastMessageId, s.mode)
  return undefined
}

export function isForward(s: ComposerStart): boolean {
  return s.kind === 'inline' && s.mode === 'forward'
}

/** Recipients and subject come with the start: only the body makes it worth saving. */
export function isPrefilled(s: ComposerStart): boolean {
  return s.kind === 'inline' || s.kind === 'reply'
}

export function isReplyLike(s: ComposerStart): boolean {
  return s.kind === 'reply' || (s.kind === 'inline' && s.mode !== 'forward')
}

export function modeSubject(subject: string, mode: ReplyMode): string {
  if (mode === 'forward') return withPrefix('Fwd:', subject)
  return withPrefix('Re:', subject)
}

export function modeTo(ctx: ReplyContext, mode: ReplyMode): string {
  switch (mode) {
    case 'forward':
      return ''
    case 'reply':
      return ctx.replyRecipients
    case 'reply-all':
      return ctx.replyAllRecipients
  }
}

/**
 * The user's masked address the message replied to was sent to, if any: a
 * reply goes out from it, so the site never learns the real address.
 */
export function replyMaskOf(s: ComposerStart, masks: readonly string[]): string {
  if (s.kind !== 'inline' || s.mode === 'forward') return ''
  const sentTo = splitAddressList(s.ctx.originalTo ?? '').map((a) => extractEmail(a).toLowerCase())
  return masks.find((m) => sentTo.includes(m.toLowerCase())) ?? ''
}

/** The conversation a sent message joins; the server resolves the parent from it. */
export function replyThreadOf(s: ComposerStart): string | undefined {
  switch (s.kind) {
    case 'carry':
      return s.carry.replyToThreadId ?? undefined
    case 'draft':
      return s.draft.replyToThreadId ?? undefined
    case 'inline':
      return threadUnlessForward(s.ctx.threadId, s.mode)
    case 'new':
    case 'redraft':
      return undefined
    case 'reply':
      return s.reply.threadId
  }
}

export function sentLabel(s: ComposerStart): Key {
  if (s.kind !== 'inline') return 'compose.sent'
  if (s.mode === 'forward') return 'compose.forwarded'
  return 'compose.replySent'
}

function extractReplyAddress(sender: string): string {
  const match = sender.match(/<([^>]+)>/)
  if (match) return match[1].trim()
  return sender.trim()
}

function threadUnlessForward(value: string, mode: ReplyMode): string | undefined {
  if (mode === 'forward') return undefined
  return value
}

function withPrefix(prefix: string, subject: string): string {
  const trimmed = subject.trim()
  if (trimmed.toLowerCase().startsWith(prefix.toLowerCase())) return trimmed
  return `${prefix} ${trimmed}`
}
