import type { MockFullMessage } from '@/wire/schemas/mock'

import { toast } from '@goliapkg/gds'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { FileDown, Trash2 } from 'lucide-react'

import { HtmlFrame } from '@/components/html-frame'
import { mockKeys } from '@/lib/query-keys'
import { mockRawUrl, wireDeleteMockMessage, wireReadMockMessage } from '@/wire/endpoints/mock'

import { btnSecondary } from '../settings/_shared'
import { errorText } from './errors'
import { AttachmentList, CodeChips, LinkList } from './extracted'
import { formatReceived, subjectLabel } from './format'
import { DirectionBadge } from './message-list'

export function MessageBody({
  deleting,
  message,
  onDelete,
  rawUrl,
}: {
  deleting: boolean
  message: MockFullMessage
  onDelete: () => void
  rawUrl: string
}) {
  return (
    <article aria-label="Message" className="space-y-4">
      <header className="space-y-1">
        <div className="flex items-start gap-2">
          <DirectionBadge direction={message.direction} />
          <h3 className="min-w-0 flex-1 text-base font-semibold break-words">
            {subjectLabel(message.subject)}
          </h3>
          <a className={`${btnSecondary} inline-flex items-center gap-1`} download href={rawUrl}>
            <FileDown aria-hidden className="h-4 w-4" />
            .eml
          </a>
          <button
            aria-label="Delete message"
            className={`${btnSecondary} hover:text-danger`}
            disabled={deleting}
            onClick={onDelete}
            type="button"
          >
            <Trash2 aria-hidden className="h-4 w-4" />
          </button>
        </div>
        <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-0.5 text-xs">
          <HeaderRow label="From" value={message.from} />
          <HeaderRow label="To" value={message.to.join(', ')} />
          {message.cc.length > 0 && <HeaderRow label="Cc" value={message.cc.join(', ')} />}
          <HeaderRow label="Received" value={formatReceived(message.received_at)} />
        </dl>
      </header>

      <Block title="Codes">
        <CodeChips codes={message.codes} />
      </Block>
      <Block title="Links">
        <LinkList links={message.links} />
      </Block>
      {message.attachments.length > 0 && (
        <Block title="Attachments">
          <AttachmentList attachments={message.attachments} />
        </Block>
      )}
      {message.html !== null && (
        <Block title="HTML">
          <div className="border-border overflow-hidden rounded-md border">
            <HtmlFrame html={message.html} maxHeight="480px" />
          </div>
        </Block>
      )}
      <Block title="Text">
        <pre className="bg-bg-secondary max-h-[480px] overflow-auto rounded-md p-3 text-xs break-words whitespace-pre-wrap select-text">
          {message.text}
        </pre>
      </Block>
    </article>
  )
}

export function MessageDetail({
  id,
  namespace,
  onDeleted,
}: {
  id: string
  namespace: string
  onDeleted: () => void
}) {
  const queryClient = useQueryClient()
  const query = useQuery({
    // the previous message shown under the newly selected id would be the wrong mail
    placeholderData: undefined,
    queryKey: mockKeys.message(namespace, id),
    queryFn: ({ signal }) => wireReadMockMessage(namespace, id, signal),
  })

  const remove = useMutation({
    mutationFn: () => wireDeleteMockMessage(namespace, id),
    onError: (e) => toast.error(errorText(e, 'Could not delete the message')),
    onSuccess: () => {
      queryClient.removeQueries({ queryKey: mockKeys.message(namespace, id) })
      void queryClient.invalidateQueries({ queryKey: mockKeys.messages(namespace) })
      void queryClient.invalidateQueries({ queryKey: mockKeys.namespaces() })
      onDeleted()
    },
  })

  if (query.isError) {
    return (
      <p className="text-danger text-sm" role="alert">
        {errorText(query.error, 'Could not load the message')}
      </p>
    )
  }
  if (query.isPending) return <p className="text-fg-muted text-sm">Loading…</p>

  return (
    <MessageBody
      deleting={remove.isPending}
      message={query.data}
      onDelete={() => remove.mutate()}
      rawUrl={mockRawUrl(namespace, id)}
    />
  )
}

function Block({ children, title }: { children: React.ReactNode; title: string }) {
  return (
    <section className="space-y-1.5">
      <h4 className="text-fg-secondary text-xs font-semibold tracking-wide uppercase">{title}</h4>
      {children}
    </section>
  )
}

function HeaderRow({ label, value }: { label: string; value: string }) {
  return (
    <>
      <dt className="text-fg-muted">{label}</dt>
      <dd className="min-w-0 break-words">{value}</dd>
    </>
  )
}
