import type { MockMessageMeta, MockNamespace } from '@/wire/schemas/mock'

import { useQuery } from '@tanstack/react-query'
import { FlaskConical } from 'lucide-react'
import { useSearchParams } from 'react-router'

import { AdminEmptyState, AdminErrorState, AdminPageShell } from '@/components/admin-page'
import { AddressMinter } from '@/components/mock/address-minter'
import { isMockDisabled } from '@/components/mock/errors'
import { MessageDetail } from '@/components/mock/message-detail'
import { MessageList } from '@/components/mock/message-list'
import { NamespaceList } from '@/components/mock/namespace-list'
import { SendForm } from '@/components/mock/send-form'
import { SmtpPanel } from '@/components/mock/smtp-panel'
import { cn } from '@/lib/cn'
import { mockKeys } from '@/lib/query-keys'
import { wireListMockMessages, wireListMockNamespaces } from '@/wire/endpoints/mock'

const EMPTY_NAMESPACES: readonly MockNamespace[] = []
const EMPTY_MESSAGES: readonly MockMessageMeta[] = []
const POLL_MS = 5000
const TITLE = 'Mock mail'
const SUBTITLE = 'Disposable mailboxes for tests: sign up with an address, read the code here'

type Tool = 'send' | 'smtp'

/** `/mock?ns=<namespace>&msg=<id>` — the selection lives in the URL. */
export function MockMail() {
  const [params, setParams] = useSearchParams()
  const query = useQuery({
    queryKey: mockKeys.namespaces(),
    queryFn: ({ signal }) => wireListMockNamespaces(signal),
    // a 404 here is a settled answer: the deployment has no mock domain
    retry: (count, error) => !isMockDisabled(error) && count < 1,
  })

  if (query.isError && isMockDisabled(query.error)) {
    return (
      <AdminPageShell title={TITLE}>
        <AdminEmptyState
          description="This server has no mock mail domain configured, so there are no test mailboxes to show"
          icon={<FlaskConical className="h-10 w-10" />}
          title="Mock mail is not enabled on this deployment"
        />
      </AdminPageShell>
    )
  }
  if (query.isError) {
    return (
      <AdminPageShell title={TITLE}>
        <AdminErrorState error={query.error} onRetry={() => void query.refetch()} />
      </AdminPageShell>
    )
  }

  const namespaces = query.data ?? EMPTY_NAMESPACES
  const selected = namespaces.find((n) => n.name === params.get('ns')) ?? null

  // a new namespace starts with nothing open; within one, `tool` stays put
  const selectNamespace = (ns: null | string) =>
    setParams(withParam(new URLSearchParams(), 'ns', ns))
  const selectMessage = (msg: null | string) => setParams(withParam(params, 'msg', msg))
  const selectTool = (tool: null | Tool) => setParams(withParam(params, 'tool', tool))

  return (
    <AdminPageShell subtitle={SUBTITLE} title={TITLE}>
      <div className="grid gap-6 lg:grid-cols-[280px_minmax(0,1fr)]">
        <NamespaceList
          namespaces={namespaces}
          onSelect={selectNamespace}
          selected={selected?.name ?? null}
        />
        {selected === null && (
          <p className="text-fg-muted py-12 text-center text-sm">
            {pickPrompt(namespaces.length, query.isPending)}
          </p>
        )}
        {selected !== null && (
          <NamespaceView
            key={selected.name}
            messageId={params.get('msg')}
            namespace={selected}
            onSelectMessage={selectMessage}
            onSelectTool={selectTool}
            tool={parseTool(params.get('tool'))}
          />
        )}
      </div>
    </AdminPageShell>
  )
}

function NamespaceView({
  messageId,
  namespace,
  onSelectMessage,
  onSelectTool,
  tool,
}: {
  messageId: null | string
  namespace: MockNamespace
  onSelectMessage: (id: null | string) => void
  onSelectTool: (tool: null | Tool) => void
  tool: null | Tool
}) {
  const messages = useQuery({
    // React Query pauses the interval while the tab is hidden
    queryKey: mockKeys.messages(namespace.name),
    refetchInterval: POLL_MS,
    queryFn: ({ signal }) => wireListMockMessages(namespace.name, signal),
  })
  const list = messages.data?.messages ?? EMPTY_MESSAGES
  const dropped = messages.data?.dropped ?? namespace.dropped

  return (
    <div className="min-w-0 space-y-5">
      <AddressMinter namespace={namespace.name} />

      <div className="flex flex-wrap gap-2">
        <ToolTab
          active={tool === 'smtp'}
          label="SMTP sandbox"
          onClick={() => onSelectTool(toggle(tool, 'smtp'))}
        />
        <ToolTab
          active={tool === 'send'}
          label="Send test mail"
          onClick={() => onSelectTool(toggle(tool, 'send'))}
        />
      </div>
      {/* hidden rather than unmounted: the SMTP password exists only in the panel's state */}
      <div className={cn(tool !== 'smtp' && 'hidden')}>
        <SmtpPanel enabled={namespace.smtp_enabled} namespace={namespace.name} />
      </div>
      <div className={cn(tool !== 'send' && 'hidden')}>
        <SendForm namespace={namespace.name} />
      </div>

      <div className="grid gap-4 xl:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
        <div className="min-w-0">
          {messages.isError && (
            <p className="text-danger mb-2 text-xs" role="alert">
              {messages.error.message}
            </p>
          )}
          <MessageList
            dropped={dropped}
            messages={list}
            onSelect={onSelectMessage}
            selected={messageId}
          />
        </div>
        <div className="min-w-0">
          {messageId !== null && (
            <MessageDetail
              id={messageId}
              key={messageId}
              namespace={namespace.name}
              onDeleted={() => onSelectMessage(null)}
            />
          )}
        </div>
      </div>
    </div>
  )
}

function parseTool(raw: null | string): null | Tool {
  if (raw === 'send' || raw === 'smtp') return raw
  return null
}

function pickPrompt(count: number, pending: boolean): string {
  if (pending) return 'Loading…'
  if (count === 0) return 'Create a namespace to get test addresses'
  return 'Pick a namespace'
}

function toggle(current: null | Tool, clicked: Tool): null | Tool {
  if (current === clicked) return null
  return clicked
}

function ToolTab({
  active,
  label,
  onClick,
}: {
  active: boolean
  label: string
  onClick: () => void
}) {
  return (
    <button
      aria-pressed={active}
      className={cn(
        'rounded-md border px-3 py-1.5 text-sm transition-colors',
        active && 'border-accent bg-accent/5 text-accent',
        !active && 'border-border text-fg-secondary hover:bg-bg-secondary'
      )}
      onClick={onClick}
      type="button"
    >
      {label}
    </button>
  )
}

function withParam(base: URLSearchParams, key: string, value: null | string): URLSearchParams {
  const next = new URLSearchParams(base)
  if (value === null) next.delete(key)
  else next.set(key, value)
  return next
}
