import { useAtomValue } from 'jotai'

import { useT } from '@/i18n'
import { authAtom } from '@/store/auth'

import { cardClass } from './_shared'

/**
 * How to make masked addresses from Bitwarden, whose SimpleLogin and
 * addy.io forwarders take a server address. Both calls are served here
 * (`crates/webapi/src/handlers/forwarders.rs`) for a key with the
 * masked-addresses scope.
 */
export function MaskedForwardersHelp() {
  const t = useT()
  const auth = useAtomValue(authAtom)
  const domain = auth?.address.split('@')[1] ?? ''
  const server = window.location.origin
  return (
    <div className={cardClass}>
      <p className="text-fg text-sm font-medium">{t('settings.masked.bw.title')}</p>
      <p className="text-fg-muted mt-1 text-xs">{t('settings.masked.bw.intro')}</p>
      <dl className="mt-3 space-y-3 text-xs">
        <Service
          name="SimpleLogin"
          rows={[
            [t('settings.masked.bw.server'), server],
            [t('settings.masked.bw.key'), t('settings.masked.bw.keyValue')],
          ]}
        />
        <Service
          name="addy.io"
          rows={[
            [t('settings.masked.bw.server'), server],
            [t('settings.masked.bw.key'), t('settings.masked.bw.keyValue')],
            [t('settings.masked.bw.domain'), domain],
          ]}
        />
      </dl>
    </div>
  )
}

function Service({ name, rows }: { name: string; rows: [string, string][] }) {
  return (
    <div>
      <dt className="text-fg font-medium">{name}</dt>
      {rows.map(([label, value]) => (
        <dd className="text-fg-muted flex gap-2" key={label}>
          <span className="shrink-0">{label}</span>
          <span className="text-fg font-mono break-all">{value}</span>
        </dd>
      ))}
    </div>
  )
}
