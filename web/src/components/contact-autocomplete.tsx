import { useCallback, useRef, useState } from 'react'

import { fetchJson } from '@/lib/api'

export function ContactAutocomplete({
  autoFocus = false,
  className = '',
  onChange,
  placeholder = 'recipient@example.com',
  value,
}: {
  autoFocus?: boolean
  className?: string
  onChange: (value: string) => void
  placeholder?: string
  value: string
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
        const data = await fetchJson<string[]>(`/contacts?q=${encodeURIComponent(query)}&limit=5`)
        setSuggestions(data)
        setShowSuggestions(data.length > 0)
        setActiveIndex(-1)
      } catch {
        setSuggestions([])
      }
    }, 300)
  }, [])

  const handleChange = useCallback(
    (newValue: string) => {
      onChange(newValue)
      fetchContacts(newValue)
    },
    [onChange, fetchContacts]
  )

  const selectSuggestion = useCallback(
    (selected: string) => {
      const parts = value.split(/[,;]/)
      parts.pop()
      parts.push(selected)
      onChange(parts.join(', ') + ', ')
      setShowSuggestions(false)
      setActiveIndex(-1)
    },
    [value, onChange]
  )

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (!showSuggestions || suggestions.length === 0) return

    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setActiveIndex((prev) => (prev < suggestions.length - 1 ? prev + 1 : 0))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setActiveIndex((prev) => (prev > 0 ? prev - 1 : suggestions.length - 1))
    } else if (e.key === 'Enter' && activeIndex >= 0) {
      e.preventDefault()
      selectSuggestion(suggestions[activeIndex])
    } else if (e.key === 'Escape') {
      setShowSuggestions(false)
      setActiveIndex(-1)
    }
  }

  return (
    <div className="relative flex-1" ref={containerRef}>
      <input
        autoFocus={autoFocus}
        className={className || 'text-fg w-full bg-transparent py-2 text-sm outline-none'}
        onBlur={() => setTimeout(() => setShowSuggestions(false), 150)}
        onChange={(e) => handleChange(e.target.value)}
        onFocus={() => suggestions.length > 0 && setShowSuggestions(true)}
        onKeyDown={handleKeyDown}
        placeholder={placeholder}
        type="text"
        value={value}
      />
      {showSuggestions && (
        <div className="border-border bg-surface absolute top-full left-0 z-50 mt-1 w-full max-w-72 rounded-lg border shadow-lg">
          {suggestions.map((s, i) => (
            <button
              className={`w-full px-3 py-2 text-left text-sm transition-colors ${
                i === activeIndex
                  ? 'bg-accent/10 text-accent'
                  : 'text-fg-secondary hover:bg-bg-secondary'
              }`}
              key={s}
              onMouseDown={() => selectSuggestion(s)}
            >
              {s}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
