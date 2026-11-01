import type { MockMessageMeta, MockNamespace } from '@/wire/schemas/mock'

import { useQuery } from '@tanstack/react-query'
import { useAtomValue } from 'jotai'
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
import { WebhookPanel } from '@/components/mock/webhook-panel'
import { useT } from '@/i18n'
import { cn } from '@/lib/cn'
import { mockKeys } from '@/lib/query-keys'
import { authAtom } from '@/store/auth'
import { wireListMockMessages, wireListMockNamespaces } from '@/wire/endpoints/mock'

const EMPTY_NAMESPACES: readonly MockNamespace[] = []
const EMPTY_MESSAGES: readonly MockMessageMeta[] = []
const POLL_MS = 5000

type Tool = 'send' | 'smtp' | 'webhook'

/** `/mock?ns=<namespace>&msg=<id>` — the selection lives in the URL. */
export function MockMail() {
  const t = useT()
  const [params, setParams] = useSearchParams()
  const admin = useAtomValue(authAtom)?.is_admin ?? false
  const everyone = admin && params.get('scope') === 'all'
  const query = useQuery({
    queryKey: mockKeys.namespaces(scopeOf(everyone)),
    queryFn: ({ signal }) => wireListMockNamespaces(signal, everyone),
    // a 404 here is a settled answer: the deployment has no mock domain
    retry: (count, error) => !isMockDisabled(error) && count < 1,
  })

  if (query.isError && isMockDisabled(query.error)) {
    return (
      <AdminPageShell title={t('nav.mock')}>
        <AdminEmptyState
          description={t('mock.disabled.description')}
          icon={<FlaskConical className="h-10 w-10" />}
          title={t('mock.disabled.title')}
        />
      </AdminPageShell>
    )
  }
  if (query.isError) {
    return (
      <AdminPageShell title={t('nav.mock')}>
        <AdminErrorState error={query.error} onRetry={() => void query.refetch()} />
      </AdminPageShell>
    )
  }

  const namespaces = query.data ?? EMPTY_NAMESPACES
  const selected = namespaces.find((n) => n.name === params.get('ns')) ?? null

  // a new namespace starts with nothing open; within one, `tool` stays put
  const selectNamespace = (ns: null | string) =>
    setParams(withParam(withParam(new URLSearchParams(), 'scope', scopeParam(everyone)), 'ns', ns))
  const selectScope = (all: boolean) =>
    setParams(withParam(new URLSearchParams(), 'scope', scopeParam(all)))
  const selectMessage = (msg: null | string) => setParams(withParam(params, 'msg', msg))
  const selectTool = (tool: null | Tool) => setParams(withParam(params, 'tool', tool))

  return (
    <AdminPageShell subtitle={t('mock.subtitle')} title={t('nav.mock')}>
      <div className="grid gap-6 lg:grid-cols-[280px_minmax(0,1fr)]">
        <div className="space-y-3">
          {admin && <ScopeSwitch everyone={everyone} onChange={selectScope} />}
          <NamespaceList
            everyone={everyone}
            namespaces={namespaces}
            onSelect={selectNamespace}
            selected={selected?.name ?? null}
          />
        </div>
        {selected === null && (
          <NothingOpen count={namespaces.length} everyone={everyone} pending={query.isPending} />
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
  const t = useT()
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
          label={t('mock.tool.smtp')}
          onClick={() => onSelectTool(toggle(tool, 'smtp'))}
        />
        <ToolTab
          active={tool === 'send'}
          label={t('mock.tool.send')}
          onClick={() => onSelectTool(toggle(tool, 'send'))}
        />
        <ToolTab
          active={tool === 'webhook'}
          label={t('mock.tool.webhook')}
          onClick={() => onSelectTool(toggle(tool, 'webhook'))}
        />
      </div>
      {/* hidden rather than unmounted: the SMTP password exists only in the panel's state */}
      <div className={cn(tool !== 'smtp' && 'hidden')}>
        <SmtpPanel enabled={namespace.smtp_enabled} namespace={namespace.name} />
      </div>
      <div className={cn(tool !== 'send' && 'hidden')}>
        <SendForm namespace={namespace.name} />
      </div>
      <div className={cn(tool !== 'webhook' && 'hidden')}>
        <WebhookPanel current={namespace.webhook_url} namespace={namespace.name} />
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

/** The right-hand side before a namespace is open. */
function NothingOpen({
  count,
  everyone,
  pending,
}: {
  count: number
  everyone: boolean
  pending: boolean
}) {
  const t = useT()
  if (pending) return <div aria-busy className="bg-surface h-40 animate-pulse rounded-lg" />
  if (count > 0) {
    return (
      <AdminEmptyState
        description={t('mock.pick.description')}
        icon={<FlaskConical className="h-10 w-10" />}
        title={t('mock.pick.title')}
      />
    )
  }
  if (everyone) {
    return (
      <AdminEmptyState
        description={t('mock.noneAnyone.description')}
        icon={<FlaskConical className="h-10 w-10" />}
        title={t('mock.noneAnyone.title')}
      />
    )
  }
  return (
    <AdminEmptyState
      description={t('mock.noneMine.description')}
      icon={<FlaskConical className="h-10 w-10" />}
      title={t('mock.noneMine.title')}
    />
  )
}

function parseTool(raw: null | string): null | Tool {
  if (raw === 'send' || raw === 'smtp' || raw === 'webhook') return raw
  return null
}

function ScopeButton({
  active,
  children,
  onClick,
}: {
  active: boolean
  children: React.ReactNode
  onClick: () => void
}) {
  return (
    <button
      aria-pressed={active}
      className={cn(
        'rounded px-2.5 py-1 transition-colors',
        active && 'bg-bg-secondary text-fg',
        !active && 'text-fg-muted hover:text-fg'
      )}
      onClick={onClick}
      type="button"
    >
      {children}
    </button>
  )
}

function scopeOf(everyone: boolean): 'all' | 'mine' {
  if (everyone) return 'all'
  return 'mine'
}

function scopeParam(everyone: boolean): null | string {
  if (everyone) return 'all'
  return null
}

/** Admins can switch between their own namespaces and everyone's. */
function ScopeSwitch({
  everyone,
  onChange,
}: {
  everyone: boolean
  onChange: (all: boolean) => void
}) {
  const t = useT()
  return (
    <div
      aria-label={t('mock.scope')}
      className="border-border inline-flex rounded-md border p-0.5 text-xs"
      role="group"
    >
      <ScopeButton active={!everyone} onClick={() => onChange(false)}>
        {t('mock.scope.mine')}
      </ScopeButton>
      <ScopeButton active={everyone} onClick={() => onChange(true)}>
        {t('mock.scope.everyone')}
      </ScopeButton>
    </div>
  )
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
