import { useAtom } from 'jotai'
import { Fragment, useCallback, useEffect, useState } from 'react'

import { deleteJson, fetchJson, postJson } from '@/lib/api'
import type { CheckResult, CheckStatus, DomainCheckReport, DomainInfo } from '@/lib/types'
import { domainsAtom } from '@/store/admin'

const statusIcon: Record<CheckStatus, string> = {
  pass: '\u2705',
  warn: '\u26A0\uFE0F',
  fail: '\u274C',
  skip: '\u23ED\uFE0F',
}

const statusColor: Record<CheckStatus, string> = {
  pass: 'text-green-600 dark:text-green-400',
  warn: 'text-yellow-600 dark:text-yellow-400',
  fail: 'text-red-600 dark:text-red-400',
  skip: 'text-zinc-400 dark:text-zinc-500',
}

function CheckResultRow({ check }: { check: CheckResult }) {
  const [expanded, setExpanded] = useState(false)

  return (
    <div className="border-b border-zinc-100 last:border-0 dark:border-zinc-800/50">
      <button
        onClick={() => check.details.length > 0 && setExpanded(!expanded)}
        className="flex w-full items-center gap-3 px-4 py-2 text-left text-sm hover:bg-zinc-50 dark:hover:bg-zinc-800/50"
      >
        <span className="w-5 text-center">{statusIcon[check.status]}</span>
        <span className="w-36 shrink-0 font-medium">{check.name}</span>
        <span className={`flex-1 ${statusColor[check.status]}`}>{check.message}</span>
        {check.details.length > 0 && (
          <span className="text-xs text-zinc-400">{expanded ? '\u25B2' : '\u25BC'}</span>
        )}
      </button>
      {expanded && check.details.length > 0 && (
        <div className="bg-zinc-50 px-4 py-2 dark:bg-zinc-900">
          {check.details.map((detail, i) => (
            <pre key={i} className="whitespace-pre-wrap break-all font-mono text-xs text-zinc-600 dark:text-zinc-400">
              {detail}
            </pre>
          ))}
        </div>
      )}
    </div>
  )
}

export function AdminDomains() {
  const [domains, setDomains] = useAtom(domainsAtom)
  const [adding, setAdding] = useState(false)
  const [newDomain, setNewDomain] = useState('')
  const [checking, setChecking] = useState<string | null>(null)
  const [reports, setReports] = useState<Record<string, DomainCheckReport>>({})

  const loadDomains = useCallback(async () => {
    try {
      const data = await fetchJson<DomainInfo[]>('/admin/domains')
      setDomains(data)
    } catch {
      // keep current state on error
    }
  }, [setDomains])

  useEffect(() => {
    loadDomains()
  }, [loadDomains])

  const handleAdd = async () => {
    if (!newDomain.trim()) return
    await postJson('/admin/domains', { name: newDomain.trim() })
    setNewDomain('')
    setAdding(false)
    loadDomains()
  }

  const handleDelete = async (name: string) => {
    await deleteJson(`/admin/domains/${encodeURIComponent(name)}`)
    loadDomains()
  }

  const handleCheck = async (name: string) => {
    setChecking(name)
    try {
      const report = await postJson<DomainCheckReport>(
        `/admin/domains/${encodeURIComponent(name)}/check`,
        {}
      )
      setReports((prev) => ({ ...prev, [name]: report }))
    } catch {
      // keep any previous report
    } finally {
      setChecking(null)
    }
  }

  const toggleReport = (name: string) => {
    if (reports[name]) {
      setReports((prev) => {
        const next = { ...prev }
        delete next[name]
        return next
      })
    } else {
      handleCheck(name)
    }
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Domains</h2>
        <button
          onClick={() => setAdding(true)}
          className="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
        >
          Add Domain
        </button>
      </div>

      {adding && (
        <div className="mb-4 flex gap-2">
          <input
            value={newDomain}
            onChange={(e) => setNewDomain(e.target.value)}
            placeholder="example.com"
            className="flex-1 rounded-md border border-zinc-300 px-3 py-1.5 text-sm dark:border-zinc-700 dark:bg-zinc-900"
            onKeyDown={(e) => e.key === 'Enter' && handleAdd()}
          />
          <button
            onClick={handleAdd}
            className="rounded-md bg-zinc-900 px-3 py-1.5 text-sm text-white dark:bg-zinc-100 dark:text-zinc-900"
          >
            Save
          </button>
          <button
            onClick={() => setAdding(false)}
            className="rounded-md px-3 py-1.5 text-sm text-zinc-500"
          >
            Cancel
          </button>
        </div>
      )}

      <div className="overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-800">
        <table className="w-full text-left text-sm">
          <thead className="border-b border-zinc-200 bg-zinc-50 dark:border-zinc-800 dark:bg-zinc-900">
            <tr>
              <th className="px-4 py-2.5 font-medium">Domain</th>
              <th className="px-4 py-2.5 font-medium">Created</th>
              <th className="px-4 py-2.5 text-right font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {domains.map((domain) => (
              <Fragment key={domain.name}>
                <tr className="border-b border-zinc-100 last:border-0 dark:border-zinc-800/50">
                  <td className="px-4 py-3 font-medium">{domain.name}</td>
                  <td className="px-4 py-3 text-zinc-500">
                    {new Date(domain.created_at * 1000).toLocaleDateString()}
                  </td>
                  <td className="px-4 py-3 text-right">
                    <button
                      onClick={() => toggleReport(domain.name)}
                      disabled={checking === domain.name}
                      className="mr-3 text-xs text-blue-600 hover:text-blue-800 disabled:opacity-50 dark:text-blue-400 dark:hover:text-blue-300"
                    >
                      {checking === domain.name
                        ? 'Checking...'
                        : reports[domain.name]
                          ? 'Hide'
                          : 'Check'}
                    </button>
                    <button
                      onClick={() => handleDelete(domain.name)}
                      className="text-xs text-red-500 hover:text-red-700"
                    >
                      Delete
                    </button>
                  </td>
                </tr>
                {reports[domain.name] && (
                  <tr>
                    <td colSpan={3} className="px-4 pb-4">
                      <div className="mt-1 overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-700">
                        <div className="flex items-center justify-between border-b border-zinc-200 bg-zinc-50 px-4 py-2 dark:border-zinc-700 dark:bg-zinc-800">
                          <span className="text-xs font-medium text-zinc-500">
                            Health Check: {domain.name}
                          </span>
                          <button
                            onClick={() => handleCheck(domain.name)}
                            disabled={checking === domain.name}
                            className="text-xs text-blue-600 hover:text-blue-800 disabled:opacity-50 dark:text-blue-400"
                          >
                            {checking === domain.name ? 'Running...' : 'Re-check'}
                          </button>
                        </div>
                        {reports[domain.name].checks.map((check) => (
                          <CheckResultRow key={check.name} check={check} />
                        ))}
                      </div>
                    </td>
                  </tr>
                )}
              </Fragment>
            ))}
            {domains.length === 0 && (
              <tr>
                <td colSpan={3} className="px-4 py-8 text-center text-zinc-400">
                  No domains configured
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  )
}
