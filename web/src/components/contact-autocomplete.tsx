import { useQuery } from '@tanstack/react-query'
import { useCallback, useEffect, useRef, useState } from 'react'

import { contactsKeys } from '@/lib/query-keys'
import { fetchContactSuggestions } from '@/wire/endpoints/contacts'

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
  // debouncedQuery is the trailing segment of the recipient string after
  // the 300ms debounce. enabled-gating on length >= 2 mirrors the original
  // imperative guard so we never fire a request for short / empty input.
  const [debouncedQuery, setDebouncedQuery] = useState('')
  const [showSuggestions, setShowSuggestions] = useState(false)
  const [activeIndex, setActiveIndex] = useState(-1)
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(null)
  const containerRef = useRef<HTMLDivElement>(null)

  const suggestionsQuery = useQuery({
    enabled: debouncedQuery.length >= 2,
    queryKey: contactsKeys.search(debouncedQuery),
    staleTime: 30 * 1000,
    queryFn: async () => {
      try {
        return await fetchContactSuggestions(debouncedQuery, 5)
      } catch {
        return []
      }
    },
  })
  const suggestions = suggestionsQuery.data ?? []

  // surface results once they arrive; mirror the original imperative
  // setShowSuggestions(data.length > 0) + activeIndex reset.
  useEffect(() => {
    if (debouncedQuery.length < 2) {
      setShowSuggestions(false)
      setActiveIndex(-1)
      return
    }
    if (suggestionsQuery.isSuccess) {
      setShowSuggestions(suggestions.length > 0)
      setActiveIndex(-1)
    }
  }, [debouncedQuery, suggestionsQuery.isSuccess, suggestions.length])

  // debounced contact fetch triggered by input changes — preserve the
  // 300ms semantics. The debounce only feeds the query key; useQuery
  // handles deduping / caching.
  const scheduleFetch = useCallback((newValue: string) => {
    if (debounceRef.current) clearTimeout(debounceRef.current)
    const query = newValue.split(/[,;]/).pop()?.trim() ?? ''
    if (query.length < 2) {
      setDebouncedQuery('')
      return
    }
    debounceRef.current = setTimeout(() => {
      setDebouncedQuery(query)
    }, 300)
  }, [])

  const handleChange = useCallback(
    (newValue: string) => {
      onChange(newValue)
      scheduleFetch(newValue)
    },
    [onChange, scheduleFetch]
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
        <div className="border-border bg-surface absolute top-full left-0 z-50 mt-1 w-full max-w-md rounded-lg border shadow-lg">
          {suggestions.map((s, i) => (
            <button
              className={`block w-full px-3 py-2 text-left text-sm break-all whitespace-normal transition-colors ${
                i === activeIndex
                  ? 'bg-accent/10 text-accent'
                  : 'text-fg-secondary hover:bg-bg-secondary'
              }`}
              key={s}
              onPointerDown={() => selectSuggestion(s)}
            >
              {s}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
