type Props = {
  children: React.ReactNode
  className?: string
  padding?: 'lg' | 'md' | 'sm'
}

const paddingStyles = {
  lg: 'p-6',
  md: 'p-4',
  sm: 'p-3',
}

export function Card({ children, className = '', padding = 'md' }: Props) {
  return (
    <div
      className={`border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] ${paddingStyles[padding]} ${className}`}
    >
      {children}
    </div>
  )
}
