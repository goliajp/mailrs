import type { ThreadMessage } from '@/lib/types'

import { toast } from '@goliapkg/gds'
import DOMPurify from 'dompurify'
import { MoreVertical, Paperclip } from 'lucide-react'
import { Fragment, memo, useCallback, useEffect, useRef, useState } from 'react'

import { InviteCard } from '@/components/invite-card'
import { SenderAvatar } from '@/components/sender-avatar'
import { type FeedbackAction, recordFeedback } from '@/lib/api'
import { formatTimeOfDay } from '@/lib/format'
import { highlightMentions } from '@/lib/mention'

type ThreadTimelineItemProps = {
  dateLabel: string
  displayName: string
  idx: number
  isOwn: boolean
  isSelected: boolean
  msg: ThreadMessage
  myEmail: string
  myName?: string
  onSelect: (idx: number) => void
  showDivider: boolean
  showSubject: boolean
  subjectText: string
}

function BubbleDateDivider({ label }: { label: string }) {
  return (
    <div className="flex justify-center py-2 select-none">
      <span className="bg-bg-secondary text-fg-muted rounded-full px-2.5 py-0.5 text-xs font-medium md:text-[10px]">
        {label}
      </span>
    </div>
  )
}

// memo'd timeline item — keeps mark-read / mark-flag / state toggles from
// re-rendering every message bubble. Without memoization the parent's
// .map() rebuilds every Fragment on every render of ThreadView, defeating
// React's diff. `onSelect` must be stable (useCallback in parent).
export const ThreadTimelineItem = memo(function ThreadTimelineItem({
  dateLabel,
  displayName,
  idx,
  isOwn,
  isSelected,
  msg,
  myEmail,
  myName,
  onSelect,
  showDivider,
  showSubject,
  subjectText,
}: ThreadTimelineItemProps) {
  const handleClick = useCallback(() => onSelect(idx), [onSelect, idx])
  return (
    <Fragment>
      {showDivider && <BubbleDateDivider label={dateLabel} />}
      <div
        className={`focus-visible:ring-accent/50 flex cursor-pointer gap-3 rounded-lg px-3 py-2.5 transition-colors focus-visible:ring-2 focus-visible:outline-none ${
          isSelected ? 'bg-accent/10' : 'hover:bg-bg-secondary'
        } ${isOwn ? 'ml-6' : ''}`}
        onClick={handleClick}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') e.currentTarget.click()
        }}
        role="button"
        tabIndex={0}
      >
        <SenderAvatar sender={msg.sender} size={28} />
        <div className="min-w-0 flex-1 space-y-1">
          <div className="flex items-center gap-2">
            <span className={`text-sm font-semibold ${isOwn ? 'text-accent' : 'text-fg'}`}>
              {isOwn ? 'Me' : displayName}
            </span>
            <span className="text-fg-muted text-xs">
              {formatTimeOfDay(msg.internal_date)}
              {msg.attachments.length > 0 && (
                <Paperclip className="ml-1 inline-block h-3 w-3 align-[-1px]" />
              )}
            </span>
          </div>
          {showSubject && subjectText && (
            <div className="text-fg truncate text-sm font-medium">{subjectText}</div>
          )}
          {msg.invite_method && <InviteCard compact messageUid={msg.uid} />}
          <BubbleBody msg={msg} myEmail={myEmail} myName={myName} subject={subjectText} />
        </div>
      </div>
    </Fragment>
  )
})

// strip invisible unicode: ZWJ, ZWNJ, ZW space, BOM, soft hyphen, directional marks, etc.
const INVISIBLE_RE =
  // eslint-disable-next-line no-misleading-character-class
  /[\u200B-\u200F\u2028-\u202F\u2060-\u2064\uFEFF\u00AD\u034F\u061C\u180E]/g

// box-drawing, table borders, repeated decorative lines
const NOISE_LINE = /^[\s│┼┬┴├┤┌┐└┘─━═╌╍╎╏║╔╗╚╝╠╣╦╩╬\-=_·•*#|+:>{}[\]~`]+$/

const BubbleBody = memo(function BubbleBody({
  msg,
  myEmail,
  myName,
  subject,
}: {
  msg: ThreadMessage
  myEmail: string
  myName?: string
  subject: string
}) {
  // 1) AI summary is always clean — use it when present
  if (msg.summary) {
    return (
      <p className="text-fg line-clamp-3 text-[13px] leading-relaxed select-text">
        {highlightMentions(msg.summary, myEmail, myName)}
      </p>
    )
  }

  // 2) cleaned text from html_body or plain text fallback. drop the
  //    subject if the body opens with it so the preview adds new info
  //    rather than echoing the line above.
  const text = bubbleText(msg)
  if (text && !looksLikeHtmlDump(text)) {
    const trimmed = stripSubjectFromPreview(text, subject)
    const preview = trimmed.length > 280 ? smartTruncate(trimmed, 280) : trimmed
    if (preview.length > 0) {
      return (
        <p className="text-fg line-clamp-3 text-[13px] leading-relaxed select-text">
          {highlightMentions(preview, myEmail, myName)}
        </p>
      )
    }
  }

  // 3) html-only or empty — show a clear placeholder; the open thread on
  //    the left already renders the rich version
  return (
    <p className="text-fg-muted text-xs italic">
      {msg.html_body ? 'Rich HTML message — click to view full content' : 'No preview available'}
    </p>
  )
})

export function HdrBtn({
  children,
  className,
  onClick,
  title,
}: {
  children: React.ReactNode
  className?: string
  onClick: () => void
  title: string
}) {
  return (
    <button
      className={`text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary flex h-7 w-7 items-center justify-center rounded-md transition-colors ${className ?? ''}`}
      onClick={onClick}
      title={title}
    >
      {children}
    </button>
  )
}

function bubbleText(msg: ThreadMessage): string {
  // prefer AI summary — always clean and readable
  if (msg.summary) return msg.summary
  // when html exists, extract the visible text from it. this avoids the
  // text/plain dump (markdown-table style `|cell |cell |`) that html-only
  // senders often produce; htmlToPreviewText reads the same words a human
  // would read in the rendered email.
  if (msg.html_body) {
    const fromHtml = htmlToPreviewText(msg.html_body)
    if (fromHtml.length > 0) return fromHtml
  }
  const raw = msg.new_content || msg.clean_text || msg.text_body || ''
  if (!raw) return ''
  return cleanTextForBubble(raw)
}

function cleanTextForBubble(raw: string): string {
  const lines = raw.replace(INVISIBLE_RE, '').split('\n')

  // find signature delimiter and remove everything after it
  let sigIdx = lines.length
  for (let i = 0; i < lines.length; i++) {
    if (lines[i] === '-- ' || lines[i] === '--') {
      sigIdx = i
      break
    }
  }

  return lines
    .slice(0, sigIdx)
    .filter((line) => !NOISE_LINE.test(line))
    .filter((line) => !line.startsWith('>')) // remove quoted lines
    .map((line) => line.replace(/\s{2,}/g, ' ').trim())
    .filter(Boolean)
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

// strip the subject from the start of preview text. many transactional
// emails (Stripe receipts, GitHub notifications, etc.) have the subject
// repeated as the first heading inside the body, so the bubble would
// show the same line twice (once as the subject row, once as the
// preview). matches the leading words case-insensitively, with a small
// allowance for trailing punctuation / sender names.
function stripSubjectFromPreview(text: string, subject: string): string {
  if (!subject) return text
  const normSubject = subject.toLowerCase().trim()
  if (!normSubject || normSubject.length < 6) return text
  const normText = text.toLowerCase()
  // search the first 200 chars for the subject and start the preview
  // after it (skipping common separator characters)
  const window = normText.slice(0, Math.min(200, normText.length))
  const at = window.indexOf(normSubject)
  if (at === -1) return text
  let cut = at + normSubject.length
  while (cut < text.length && /[\s·•|–—\-:,]/.test(text[cut])) cut++
  const remainder = text.slice(cut).trim()
  return remainder.length >= 30 ? remainder : text
}

// dedicated DOMPurify instance so this preview path never runs the
// global hooks (e.g. anchor target=_blank) that the message body uses
const previewPurifier = DOMPurify()

export function SmBtn({
  children,
  onClick,
  title,
}: {
  children: React.ReactNode
  onClick: () => void
  title: string
}) {
  return (
    <button
      className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary flex h-7 w-7 items-center justify-center rounded-md transition-all duration-150"
      onClick={onClick}
      title={title}
    >
      {children}
    </button>
  )
}

// extract a clean reading-order preview from html.
// the simple "strip all tags, keep textContent" approach pulled in url
// soup: anchors whose visible text is the URL itself, footer-style
// '[1]: https://…' link lists, and tracking redirects that stretch
// hundreds of characters with no break point. all of that overflowed
// the bubble and added zero signal. we now strip those before
// collapsing to text.
function htmlToPreviewText(html: string): string {
  // sanitize with structure preserved (so we can walk anchors), drop
  // dangerous tags + the noisy ones (head/style/script don't appear in
  // textContent anyway, but iframe/svg/img title attributes can leak).
  const cleanHtml = previewPurifier.sanitize(html, {
    FORBID_ATTR: ['style'],
    FORBID_TAGS: ['script', 'style', 'svg', 'iframe', 'img'],
  })
  // DOMParser is browser-only; we run in vite/jsdom so this is fine
  const doc = new DOMParser().parseFromString(`<div>${cleanHtml}</div>`, 'text/html')
  const root = doc.body.firstElementChild ?? doc.body

  // remove anchors whose visible text is just a URL — they're tracking
  // redirects ('https://c.gle/…' / 'https://email.stripe.com/…') that
  // bring no information into a tiny preview
  for (const a of Array.from(root.querySelectorAll('a'))) {
    const text = (a.textContent || '').trim()
    const href = a.getAttribute('href') || ''
    if (text === '' || /^https?:\/\//i.test(text) || text === href) {
      a.remove()
    }
  }

  const text = (root.textContent || '')
    // footer-style link lists: '[1]: https://…' / '[12]:https://…'
    .replace(/\[\d+\]:\s*https?:\/\/\S+/gi, '')
    // standalone bracketed URL refs '[ https://… ]' some clients emit
    .replace(/\[\s*https?:\/\/\S+\s*\]/gi, '')
    // any remaining long URL — collapse to placeholder so it can never
    // overflow the bubble or eat the entire 3-line clamp
    .replace(/https?:\/\/\S+/gi, '[link]')
    .replace(/&nbsp;/gi, ' ')
    .replace(/&amp;/gi, '&')
    .replace(/&lt;/gi, '<')
    .replace(/&gt;/gi, '>')
    .replace(/&quot;/gi, '"')
    // clean up footnote leftovers: <sup>1</sup> and <sub>x</sub> get
    // dropped to bare characters that look like '^' / orphan digits in
    // the middle of CJK text. strip the common ones.
    .replace(/[\u00B9\u00B2\u00B3\u2070-\u2079\u2080-\u2089]/g, '')
    .replace(/(?<=[\p{L}\p{N}])\^(?=[。\s,])/gu, '')
    .replace(/\s+/g, ' ')
    .trim()
  return text
}

// last-resort detection: when only a text/plain part exists and it looks
// like html-to-text noise (markdown-table dump, jammed brackets), drop
// it rather than render garbage. lower threshold + extra heuristic for
// any single line carrying 4+ pipes (clear table-row signature).
function looksLikeHtmlDump(text: string): boolean {
  if (!text || text.length < 40) return false
  const noise = (text.match(/[|{}[\]<>]/g) || []).length
  if (noise / text.length > 0.05) return true
  const lines = text.split('\n')
  if (lines.length >= 5) {
    const longLines = lines.filter((l) => l.length > 200).length
    if (longLines / lines.length > 0.3) return true
  }
  return lines.some((l) => (l.match(/\|/g) || []).length >= 4)
}

// truncate at nearest sentence/paragraph boundary instead of hard character cut
function smartTruncate(text: string, maxLen: number): string {
  if (text.length <= maxLen) return text
  const sub = text.slice(0, maxLen)
  // try to break at paragraph
  const lastNewline = sub.lastIndexOf('\n')
  if (lastNewline > maxLen * 0.5) return sub.slice(0, lastNewline).trimEnd()
  // try to break at sentence (。.!?！？)
  const sentenceEnd = sub.search(/[.。!！?？]\s*[^\s]*$/)
  if (sentenceEnd > maxLen * 0.4) return sub.slice(0, sentenceEnd + 1).trimEnd()
  // fall back to word boundary
  const lastSpace = sub.lastIndexOf(' ')
  if (lastSpace > maxLen * 0.5) return sub.slice(0, lastSpace).trimEnd() + '…'
  return sub.trimEnd() + '…'
}

const FEEDBACK_ITEMS: {
  action: FeedbackAction
  icon: string
  label: string
}[] = [
  { action: 'mark_important', icon: '!', label: 'Mark Important' },
  { action: 'mark_vip', icon: '\u2605', label: 'Mark VIP' },
  { action: 'mark_spam', icon: '\u26A0', label: 'Report Spam' },
  { action: 'block', icon: '\u2718', label: 'Block Sender' },
]

export function FeedbackMenu({ senderEmail }: { senderEmail: string }) {
  const [open, setOpen] = useState(false)
  const [confirming, setConfirming] = useState<FeedbackAction | null>(null)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const handle = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false)
        setConfirming(null)
      }
    }
    document.addEventListener('mousedown', handle)
    return () => document.removeEventListener('mousedown', handle)
  }, [open])

  const executeAction = async (action: FeedbackAction) => {
    setOpen(false)
    setConfirming(null)
    if (!senderEmail) return
    try {
      const result = await recordFeedback(senderEmail, action)
      if (result.success) {
        toast.success(result.message ?? 'Feedback recorded')
      } else {
        toast.error(result.message ?? 'Failed')
      }
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed')
    }
  }

  const handleAction = (action: FeedbackAction) => {
    if (action === 'block' || action === 'mark_spam') {
      setConfirming(action)
    } else {
      executeAction(action)
    }
  }

  return (
    <div className="relative" ref={ref}>
      <button
        className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary rounded-md p-1 transition-colors"
        onClick={() => setOpen((p) => !p)}
        title="Sender feedback"
      >
        <MoreVertical className="h-3.5 w-3.5" />
      </button>
      {open && (
        <div className="border-border bg-surface absolute top-full right-0 z-50 mt-1 w-48 rounded-lg border py-1 shadow-lg">
          {confirming ? (
            <div className="px-3 py-2">
              <p className="text-fg-secondary text-xs">
                {confirming === 'block' ? 'Block this sender?' : 'Report as spam?'}
              </p>
              <div className="mt-2 flex gap-2">
                <button
                  className="text-fg-muted hover:bg-bg-secondary rounded px-2 py-1 text-xs"
                  onClick={() => setConfirming(null)}
                >
                  Cancel
                </button>
                <button
                  className="bg-danger rounded px-2 py-1 text-xs text-white hover:opacity-90"
                  onClick={() => executeAction(confirming)}
                >
                  Confirm
                </button>
              </div>
            </div>
          ) : (
            <>
              <p className="text-fg-muted truncate px-3 py-1 text-xs md:text-[11px]">
                {senderEmail}
              </p>
              {FEEDBACK_ITEMS.map((item) => (
                <button
                  className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors ${
                    item.action === 'block' || item.action === 'mark_spam'
                      ? 'text-danger hover:bg-danger/10'
                      : 'text-fg-secondary hover:bg-bg-secondary'
                  }`}
                  key={item.action}
                  onClick={() => handleAction(item.action)}
                >
                  <span className="w-4 text-center">{item.icon}</span>
                  {item.label}
                </button>
              ))}
            </>
          )}
        </div>
      )}
    </div>
  )
}
