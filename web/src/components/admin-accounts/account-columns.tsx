import type { Column } from '@/components/admin-page'
import type { AccountInfo } from '@/lib/types'

import { Copyable } from '@/components/copy-button'
import { type Key, useT } from '@/i18n'
import { activeLabel, stateDotClass } from '@/lib/status-label'

import { DisplayNameCell } from './display-name-cell'
import { PasswordCell } from './password-cell'
import { QuotaCell } from './quota-cell'

export type AccountPanel = 'groups' | 'sieve'

export type OpenPanel = null | { address: string; panel: AccountPanel }

export function accountColumns({
  onDelete,
  onToggle,
  open,
  t,
}: {
  onDelete: (address: string) => void
  onToggle: (address: string, panel: AccountPanel) => void
  open: OpenPanel
  t: ReturnType<typeof useT>
}): Column<AccountInfo>[] {
  const toggleLabel = (address: string, panel: AccountPanel, idle: Key) => {
    if (open?.address === address && open.panel === panel) return t('admin.hide')
    return t(idle)
  }
  return [
    {
      header: t('admin.col.address'),
      key: 'address',
      cell: (a) => (
        <Copyable value={a.address}>
          <span className="font-medium">{a.address}</span>
        </Copyable>
      ),
    },
    { header: t('admin.displayName'), key: 'name', cell: (a) => <DisplayNameCell account={a} /> },
    {
      header: t('admin.col.domain'),
      key: 'domain',
      cell: (a) => <span className="text-fg-secondary">{a.domain}</span>,
    },
    {
      align: 'end',
      header: t('admin.col.quota'),
      key: 'quota',
      cell: (a) => <QuotaCell address={a.address} />,
    },
    {
      header: t('admin.col.status'),
      key: 'status',
      // the word next to the dot: colour alone does not say whether it is on
      cell: (a) => (
        <span className="flex items-center gap-1.5">
          <span className={stateDotClass(a.active, 'bg-border')} />
          <span className="text-fg-secondary text-xs">{t(activeLabel(a.active))}</span>
        </span>
      ),
    },
    { header: t('admin.password'), key: 'password', cell: (a) => <PasswordCell account={a} /> },
    {
      align: 'end',
      header: t('admin.col.actions'),
      hideHeader: true,
      key: 'actions',
      cell: (a) => (
        <div className="flex justify-end gap-3 text-xs">
          <button
            className="text-accent hover:opacity-80"
            onClick={() => onToggle(a.address, 'groups')}
            type="button"
          >
            {toggleLabel(a.address, 'groups', 'admin.accounts.groups')}
          </button>
          <button
            className="text-accent hover:opacity-80"
            onClick={() => onToggle(a.address, 'sieve')}
            type="button"
          >
            {toggleLabel(a.address, 'sieve', 'admin.accounts.sieve')}
          </button>
          <button
            className="text-danger hover:opacity-70"
            onClick={() => onDelete(a.address)}
            type="button"
          >
            {t('admin.delete')}
          </button>
        </div>
      ),
    },
  ]
}
