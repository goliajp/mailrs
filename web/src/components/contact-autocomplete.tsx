import { useCallback, useRef, useState } from 'react'

import { fetchJson } from '@/lib/api'

export function ContactAutocomplete({
  value,
  onChange,
  placeholder = 'recipient@example.com',
  className = '',
  autoFocus = false,
}: {
  value: string
  onChange: (value: string) => void
  placeholder?: string
  className?: string
  autoFocus?: boolean
}) {
  const [suggestions, setSuggestions] = useState<string[]>([])
  const [showSuggestions, setShowSuggestions] = useState(false)
  const [activeIndex, setActiveIndex] = useState(-1)
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(null)
  const containerRef = useRef<HTMLDivElement>(null)

  // debounced contact fetch triggered by input changes
  const fetchContacts = useCallback((newValue: string) => {
    if (debounceRef.current) clearTimeout(debounceRef.current)
    const query = newValue.split(/[,;]/).pop()?.trim() ?? ''
    if (query.length < 2) {
      setSuggestions([])
      setShowSuggestions(false)
      return
    }
    debounceRef.current = setTimeout(async () => {
      try {
        const data = await fetchJson<string[]>(
          `/contacts?q=${encodeURIComponent(query)}&limit=5`,
        )
        setSuggestions(data)
        setShowSuggestions(data.length > 0)
        setActiveIndex(-1)
      } catch {
        setSuggestions([])
      }
    }, 300)
  }, [])

  const handleChange = useCallback((newValue: string) => {
    onChange(newValue)
    fetchContacts(newValue)
  }, [onChange, fetchContacts])

  const selectSuggestion = useCallback(
    (selected: string) => {
      const parts = value.split(/[,;]/)
      parts.pop()
      parts.push(selected)
      onChange(parts.join(', ') + ', ')
      setShowSuggestions(false)
      setActiveIndex(-1)
    },
    [value, onChange],
  )

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (!showSuggestions || suggestions.length === 0) return

    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setActiveIndex((prev) =>
        prev < suggestions.length - 1 ? prev + 1 : 0,
      )
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setActiveIndex((prev) =>
        prev > 0 ? prev - 1 : suggestions.length - 1,
      )
    } else if (e.key === 'Enter' && activeIndex >= 0) {
      e.preventDefault()
      selectSuggestion(suggestions[activeIndex])
    } else if (e.key === 'Escape') {
      setShowSuggestions(false)
      setActiveIndex(-1)
    }
  }

  return (
    <div ref={containerRef} className="relative flex-1">
      <input
        type="text"
        value={value}
        onChange={(e) => handleChange(e.target.value)}
        onFocus={() => suggestions.length > 0 && setShowSuggestions(true)}
        onBlur={() => setTimeout(() => setShowSuggestions(false), 150)}
        onKeyDown={handleKeyDown}
        className={
          className ||
          'w-full bg-transparent py-2 text-sm text-[var(--color-text-primary)] outline-none'
        }
        placeholder={placeholder}
        autoFocus={autoFocus}
      />
      {showSuggestions && (
        <div className="absolute left-0 top-full z-50 mt-1 w-full min-w-48 max-w-72 rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] shadow-lg">
          {suggestions.map((s, i) => (
            <button
              key={s}
              onMouseDown={() => selectSuggestion(s)}
              className={`w-full px-3 py-2 text-left text-sm transition-colors ${
                i === activeIndex
                  ? 'bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]'
                  : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
              }`}
            >
              {s}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
