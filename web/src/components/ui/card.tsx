type Props = {
  padding?: 'sm' | 'md' | 'lg'
  children: React.ReactNode
  className?: string
}

const paddingStyles = {
  sm: 'p-3',
  md: 'p-4',
  lg: 'p-6',
}

export function Card({ padding = 'md', children, className = '' }: Props) {
  return (
    <div
      className={`border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] ${paddingStyles[padding]} ${className}`}
    >
      {children}
    </div>
  )
}
