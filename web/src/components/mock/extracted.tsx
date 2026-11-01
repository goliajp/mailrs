import type { MockAttachment, MockLink } from '@/wire/schemas/mock'

import { Paperclip } from 'lucide-react'

import { CopyButton } from '@/components/settings/api-keys/copy-button'
import { useT } from '@/i18n'
import { formatSize } from '@/lib/format'

export function AttachmentList({ attachments }: { attachments: readonly MockAttachment[] }) {
  const t = useT()
  if (attachments.length === 0) return null
  return (
    <ul aria-label={t('mock.attachments')} className="space-y-1">
      {attachments.map((a, i) => (
        // filenames repeat and the list never reorders, so position is the identity
        <li className="flex items-center gap-2 text-xs" key={`${i}-${a.filename}`}>
          <Paperclip aria-hidden className="text-fg-muted h-3.5 w-3.5 shrink-0" />
          <span className="min-w-0 truncate">{attachmentName(a.filename, t('mock.unnamed'))}</span>
          <span className="text-fg-muted shrink-0">
            {a.content_type} · {formatSize(a.size)}
          </span>
        </li>
      ))}
    </ul>
  )
}

/** Verification codes the server pulled out of the message, each one copyable. */
export function CodeChips({ codes }: { codes: readonly string[] }) {
  const t = useT()
  if (codes.length === 0) return <p className="text-fg-muted text-xs">{t('mock.noCodes')}</p>
  return (
    <ul aria-label={t('mock.codes')} className="flex flex-wrap gap-2">
      {codes.map((code) => (
        <li
          className="border-border bg-bg-secondary flex items-center gap-1 rounded-md border py-0.5 pr-0.5 pl-2"
          key={code}
        >
          <code className="text-sm font-semibold tracking-wider">{code}</code>
          <CopyButton label={t('mock.codeNamed', { code })} value={code} />
        </li>
      ))}
    </ul>
  )
}

/** Links found in the message, most likely verification links first, as the server ordered them. */
export function LinkList({ links }: { links: readonly MockLink[] }) {
  const t = useT()
  if (links.length === 0) return <p className="text-fg-muted text-xs">{t('mock.noLinks')}</p>
  return (
    <ul aria-label={t('mock.links')} className="space-y-1">
      {links.map((link) => (
        <li className="flex min-w-0 items-center gap-2" key={link.url}>
          <div className="min-w-0 flex-1">
            {link.text !== '' && <p className="truncate text-xs font-medium">{link.text}</p>}
            <a
              className="text-accent block truncate font-mono text-xs hover:underline"
              href={link.url}
              rel="noopener noreferrer"
              target="_blank"
              title={link.url}
            >
              {link.url}
            </a>
          </div>
          <CopyButton label={t('mock.link')} value={link.url} />
        </li>
      ))}
    </ul>
  )
}

function attachmentName(filename: string, unnamed: string): string {
  if (filename === '') return unnamed
  return filename
}
