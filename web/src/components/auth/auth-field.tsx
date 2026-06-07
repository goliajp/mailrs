import { Eye, EyeOff } from 'lucide-react'
import { useId, useState } from 'react'

type AuthFieldProps = {
  autoComplete?: string
  autoFocus?: boolean
  helperText?: string
  id?: string
  inputMode?: 'numeric' | 'text'
  invalid?: boolean
  invalidMessage?: string
  label: string
  onChange: (value: string) => void
  passwordToggle?: boolean
  placeholder?: string
  required?: boolean
  type?: 'email' | 'password' | 'text'
  value: string
}

export function AuthField({
  autoComplete,
  autoFocus,
  helperText,
  id: idProp,
  inputMode,
  invalid,
  invalidMessage,
  label,
  onChange,
  passwordToggle,
  placeholder,
  required,
  type = 'text',
  value,
}: AuthFieldProps) {
  const generatedId = useId()
  const id = idProp ?? generatedId
  const helperId = `${id}-helper`
  const errorId = `${id}-error`
  const [revealed, setRevealed] = useState(false)
  const effectiveType = passwordToggle && revealed ? 'text' : type

  const describedBy = [invalid && invalidMessage ? errorId : null, helperText ? helperId : null]
    .filter(Boolean)
    .join(' ')

  return (
    <div className="space-y-1.5">
      <label className="text-fg-secondary block text-xs font-medium" htmlFor={id}>
        {label}
      </label>
      <div className={passwordToggle ? 'relative' : undefined}>
        <input
          aria-describedby={describedBy || undefined}
          aria-invalid={invalid || undefined}
          autoComplete={autoComplete}
          autoFocus={autoFocus}
          className={
            'border-border bg-bg-secondary text-fg placeholder:text-fg-muted focus:border-accent focus:ring-accent/40 w-full rounded-md border px-3 py-2 text-sm outline-none focus:ring-1 ' +
            (passwordToggle ? 'pr-10 ' : '') +
            (invalid ? 'border-danger focus:border-danger focus:ring-danger/40' : '')
          }
          id={id}
          inputMode={inputMode}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          required={required}
          type={effectiveType}
          value={value}
        />
        {passwordToggle && (
          <button
            aria-label={revealed ? 'Hide password' : 'Show password'}
            className="text-fg-muted hover:text-fg-secondary absolute top-1/2 right-2 -translate-y-1/2"
            onClick={() => setRevealed((v) => !v)}
            tabIndex={-1}
            type="button"
          >
            {revealed ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
          </button>
        )}
      </div>
      {invalid && invalidMessage && (
        <p className="text-danger text-xs" id={errorId}>
          {invalidMessage}
        </p>
      )}
      {helperText && (
        <p className="text-fg-muted text-xs" id={helperId}>
          {helperText}
        </p>
      )}
    </div>
  )
}
