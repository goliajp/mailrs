import type { Item } from './flow-model'

import { CircleAlert } from 'lucide-react'
import { Link } from 'react-router'

/** Nothing is drawn when there is nothing to do. */
export function Attention({ items }: { items: Item[] }) {
  if (items.length === 0) return null
  return (
    <section aria-label="Needs attention" className="border-warning/50 rounded-lg border px-4 py-3">
      <h3 className="text-fg mb-2 flex items-center gap-2 text-sm font-medium">
        <CircleAlert aria-hidden className="text-warning h-4 w-4" />
        Needs attention
      </h3>
      <ul className="space-y-1 text-sm">
        {items.map((item) => (
          <li key={item.text}>
            <ItemText item={item} />
          </li>
        ))}
      </ul>
    </section>
  )
}

function ItemText({ item }: { item: Item }) {
  if (item.to === undefined) return <span className="text-fg-secondary">{item.text}</span>
  return (
    <Link
      className="text-fg-secondary hover:text-fg underline-offset-2 hover:underline"
      to={item.to}
    >
      {item.text}
    </Link>
  )
}
