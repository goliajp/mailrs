import type { Column } from '@/components/admin-page'
import type { AccountInfo } from '@/lib/types'

import { Copyable } from '@/components/copy-button'
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
}: {
  onDelete: (address: string) => void
  onToggle: (address: string, panel: AccountPanel) => void
  open: OpenPanel
}): Column<AccountInfo>[] {
  const toggleLabel = (address: string, panel: AccountPanel, idle: string) => {
    if (open?.address === address && open.panel === panel) return 'Hide'
    return idle
  }
  return [
    {
      header: 'Address',
      key: 'address',
      cell: (a) => (
        <Copyable value={a.address}>
          <span className="font-medium">{a.address}</span>
        </Copyable>
      ),
    },
    { header: 'Display name', key: 'name', cell: (a) => <DisplayNameCell account={a} /> },
    {
      header: 'Domain',
      key: 'domain',
      cell: (a) => <span className="text-fg-secondary">{a.domain}</span>,
    },
    { align: 'end', header: 'Quota', key: 'quota', cell: (a) => <QuotaCell address={a.address} /> },
    {
      header: 'Status',
      key: 'status',
      // the word next to the dot: colour alone does not say whether it is on
      cell: (a) => (
        <span className="flex items-center gap-1.5">
          <span className={stateDotClass(a.active, 'bg-border')} />
          <span className="text-fg-secondary text-xs">{activeLabel(a.active)}</span>
        </span>
      ),
    },
    { header: 'Password', key: 'password', cell: (a) => <PasswordCell account={a} /> },
    {
      align: 'end',
      header: 'Actions',
      hideHeader: true,
      key: 'actions',
      cell: (a) => (
        <div className="flex justify-end gap-3 text-xs">
          <button
            className="text-accent hover:opacity-80"
            onClick={() => onToggle(a.address, 'groups')}
            type="button"
          >
            {toggleLabel(a.address, 'groups', 'Groups')}
          </button>
          <button
            className="text-accent hover:opacity-80"
            onClick={() => onToggle(a.address, 'sieve')}
            type="button"
          >
            {toggleLabel(a.address, 'sieve', 'Sieve')}
          </button>
          <button
            className="text-danger hover:opacity-70"
            onClick={() => onDelete(a.address)}
            type="button"
          >
            Delete
          </button>
        </div>
      ),
    },
  ]
}
