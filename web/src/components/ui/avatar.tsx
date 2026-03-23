import type { Size } from '@/lib/tokens'

type Props = {
  className?: string
  name: string
  size?: Size
}

const sizeStyles: Record<Size, string> = {
  lg: 'h-9 w-9 text-sm',
  md: 'h-7 w-7 text-[11px]',
  sm: 'h-6 w-6 text-[10px]',
  xs: 'h-5 w-5 text-[9px]',
}

const COLORS = [
  'bg-blue-600',
  'bg-emerald-600',
  'bg-amber-600',
  'bg-rose-600',
  'bg-purple-600',
  'bg-cyan-600',
  'bg-pink-600',
  'bg-teal-600',
] as const

export function Avatar({ className = '', name, size = 'md' }: Props) {
  const color = COLORS[hashCode(name) % COLORS.length]
  const initial = getInitial(name)

  return (
    <div
      className={`inline-flex shrink-0 items-center justify-center rounded-full font-medium text-white ${color} ${sizeStyles[size]} ${className}`}
    >
      {initial}
    </div>
  )
}

function getInitial(name: string): string {
  const match = name.match(/^"?([^"<]+)"?\s*</)
  if (match) return match[1].trim()[0].toUpperCase()
  return (name[0] ?? '?').toUpperCase()
}

function hashCode(s: string): number {
  let h = 0
  for (let i = 0; i < s.length; i++) {
    h = ((h << 5) - h + s.charCodeAt(i)) | 0
  }
  return Math.abs(h)
}
