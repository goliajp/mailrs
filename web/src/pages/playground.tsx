import { useAtom } from 'jotai'
import { Moon, Plus, Search, Star, Sun, Trash2, X } from 'lucide-react'
import { useState } from 'react'

import { Avatar, Badge, Button, Card, Dialog, IconButton, Input } from '@/components/ui'
import { themeAtom } from '@/store/theme'
import type { ThemeMode } from '@/lib/theme'

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-3">
      <h2 className="text-lg font-semibold text-[var(--color-text-primary)]">{title}</h2>
      <div className="space-y-4">{children}</div>
    </section>
  )
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <p className="mb-1.5 text-xs font-medium text-[var(--color-text-tertiary)]">{label}</p>
      <div className="flex flex-wrap items-center gap-2">{children}</div>
    </div>
  )
}

export function Playground() {
  const [theme, setTheme] = useAtom(themeAtom)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [inputValue, setInputValue] = useState('')

  return (
    <div className="min-h-screen bg-[var(--color-bg-base)] p-6">
      <div className="mx-auto max-w-4xl space-y-10">
        {/* header */}
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-2xl font-bold text-[var(--color-text-primary)]">
              Component Playground
            </h1>
            <p className="mt-1 text-sm text-[var(--color-text-secondary)]">
              Design token system &middot; All components render with CSS custom properties
            </p>
          </div>
          <div className="flex items-center gap-1 border border-[var(--color-border-default)] p-0.5">
            {(['light', 'dark', 'system'] as ThemeMode[]).map((m) => (
              <button
                key={m}
                onClick={() => setTheme(m)}
                className={`flex items-center gap-1 px-2.5 py-1 text-xs font-medium transition-colors ${
                  theme === m
                    ? 'bg-[var(--color-brand-primary)] text-white'
                    : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
                }`}
              >
                {m === 'light' && <Sun className="h-3 w-3" />}
                {m === 'dark' && <Moon className="h-3 w-3" />}
                {m.charAt(0).toUpperCase() + m.slice(1)}
              </button>
            ))}
          </div>
        </div>

        {/* color tokens */}
        <Section title="Color Tokens">
          <Row label="Surfaces">
            {(['--color-bg-base', '--color-bg-raised', '--color-bg-overlay', '--color-bg-sunken'] as const).map((v) => (
              <div key={v} className="flex flex-col items-center gap-1">
                <div
                  className="h-12 w-12 border border-[var(--color-border-default)]"
                  style={{ background: `var(${v})` }}
                />
                <span className="text-[10px] text-[var(--color-text-tertiary)]">
                  {v.replace('--color-bg-', '')}
                </span>
              </div>
            ))}
          </Row>
          <Row label="Text">
            {([
              { v: '--color-text-primary', label: 'primary' },
              { v: '--color-text-secondary', label: 'secondary' },
              { v: '--color-text-tertiary', label: 'tertiary' },
            ]).map(({ v, label }) => (
              <span key={v} className="text-sm font-medium" style={{ color: `var(${v})` }}>
                {label}
              </span>
            ))}
          </Row>
          <Row label="Status">
            {(['success', 'warning', 'danger', 'info'] as const).map((s) => (
              <div key={s} className="flex items-center gap-1.5">
                <div
                  className="h-3 w-3 rounded-full"
                  style={{ background: `var(--color-status-${s})` }}
                />
                <span className="text-xs text-[var(--color-text-secondary)]">{s}</span>
              </div>
            ))}
          </Row>
          <Row label="Brand">
            <div className="flex items-center gap-2">
              <div
                className="h-8 w-20 flex items-center justify-center text-xs font-medium"
                style={{
                  background: 'var(--color-brand-primary)',
                  color: 'var(--color-brand-primary-text)',
                }}
              >
                Primary
              </div>
              <div
                className="h-8 w-20 flex items-center justify-center text-xs font-medium"
                style={{
                  background: 'var(--color-brand-primary-hover)',
                  color: 'var(--color-brand-primary-text)',
                }}
              >
                Hover
              </div>
              <div
                className="h-8 w-20 flex items-center justify-center text-xs font-medium border border-[var(--color-border-default)]"
                style={{
                  background: 'var(--color-brand-subtle)',
                  color: 'var(--color-brand-primary)',
                }}
              >
                Subtle
              </div>
            </div>
          </Row>
        </Section>

        {/* buttons */}
        <Section title="Button">
          <Row label="Variants">
            <Button variant="primary">Primary</Button>
            <Button variant="secondary">Secondary</Button>
            <Button variant="ghost">Ghost</Button>
            <Button variant="danger">Danger</Button>
          </Row>
          <Row label="Sizes">
            <Button size="xs" variant="primary">XS</Button>
            <Button size="sm" variant="primary">Small</Button>
            <Button size="md" variant="primary">Medium</Button>
            <Button size="lg" variant="primary">Large</Button>
          </Row>
          <Row label="States">
            <Button variant="primary" disabled>Disabled</Button>
            <Button variant="secondary" disabled>Disabled</Button>
          </Row>
          <Row label="With icons">
            <Button variant="primary" size="sm"><Plus className="h-3.5 w-3.5" /> New</Button>
            <Button variant="danger" size="sm"><Trash2 className="h-3.5 w-3.5" /> Delete</Button>
            <Button variant="ghost" size="sm"><Search className="h-3.5 w-3.5" /> Search</Button>
          </Row>
        </Section>

        {/* icon buttons */}
        <Section title="IconButton">
          <Row label="Sizes">
            <IconButton label="Extra small" size="xs"><Star className="h-3 w-3" /></IconButton>
            <IconButton label="Small" size="sm"><Star className="h-3.5 w-3.5" /></IconButton>
            <IconButton label="Medium" size="md"><Star className="h-4 w-4" /></IconButton>
            <IconButton label="Large" size="lg"><Star className="h-4.5 w-4.5" /></IconButton>
          </Row>
          <Row label="States">
            <IconButton label="Normal"><X className="h-4 w-4" /></IconButton>
            <IconButton label="Disabled" disabled><X className="h-4 w-4" /></IconButton>
          </Row>
        </Section>

        {/* badges */}
        <Section title="Badge">
          <Row label="Intents">
            <Badge intent="primary">Primary</Badge>
            <Badge intent="secondary">Secondary</Badge>
            <Badge intent="success">Success</Badge>
            <Badge intent="warning">Warning</Badge>
            <Badge intent="danger">Danger</Badge>
            <Badge intent="info">Info</Badge>
          </Row>
        </Section>

        {/* input */}
        <Section title="Input">
          <Row label="Default">
            <Input
              placeholder="Type something..."
              value={inputValue}
              onChange={(e) => setInputValue(e.target.value)}
              aria-label="Default input"
              className="max-w-xs"
            />
          </Row>
          <Row label="Error state">
            <Input
              placeholder="Invalid input"
              error
              aria-label="Error input"
              className="max-w-xs"
              defaultValue="bad value"
            />
          </Row>
          <Row label="Disabled">
            <Input
              placeholder="Disabled"
              disabled
              aria-label="Disabled input"
              className="max-w-xs"
            />
          </Row>
        </Section>

        {/* avatar */}
        <Section title="Avatar">
          <Row label="Sizes">
            <Avatar name="Alice" size="xs" />
            <Avatar name="Bob" size="sm" />
            <Avatar name="Charlie" size="md" />
            <Avatar name="Diana" size="lg" />
          </Row>
          <Row label="Color consistency">
            <Avatar name="alice@example.com" />
            <Avatar name="bob@example.com" />
            <Avatar name="Charlie Brown <charlie@example.com>" />
            <Avatar name="support@golia.jp" />
            <Avatar name="noreply@amazon.co.jp" />
          </Row>
        </Section>

        {/* card */}
        <Section title="Card">
          <Row label="Padding sizes">
            <Card padding="sm" className="w-40"><p className="text-sm">Small padding</p></Card>
            <Card padding="md" className="w-40"><p className="text-sm">Medium padding</p></Card>
            <Card padding="lg" className="w-40"><p className="text-sm">Large padding</p></Card>
          </Row>
        </Section>

        {/* dialog */}
        <Section title="Dialog">
          <Row label="Modal">
            <Button variant="secondary" onClick={() => setDialogOpen(true)}>
              Open Dialog
            </Button>
          </Row>
          <Dialog open={dialogOpen} onClose={() => setDialogOpen(false)} title="Confirm action">
            <p className="text-sm text-[var(--color-text-secondary)]">
              Are you sure you want to proceed? This action cannot be undone.
            </p>
            <div className="mt-4 flex justify-end gap-2">
              <Button variant="ghost" size="sm" onClick={() => setDialogOpen(false)}>
                Cancel
              </Button>
              <Button variant="danger" size="sm" onClick={() => setDialogOpen(false)}>
                Confirm
              </Button>
            </div>
          </Dialog>
        </Section>

        {/* typography scale */}
        <Section title="Typography">
          <div className="space-y-2">
            <p className="text-2xl font-bold text-[var(--color-text-primary)]">Heading 2XL (20px)</p>
            <p className="text-xl font-semibold text-[var(--color-text-primary)]">Heading XL (18px)</p>
            <p className="text-lg font-semibold text-[var(--color-text-primary)]">Heading LG (16px)</p>
            <p className="text-sm text-[var(--color-text-primary)]">Body base (14px) — The quick brown fox jumps over the lazy dog.</p>
            <p className="text-[13px] text-[var(--color-text-secondary)]">Body SM (13px) — Secondary text for descriptions and metadata.</p>
            <p className="text-[11px] text-[var(--color-text-tertiary)]">Caption XS (11px) — Timestamps, counts, and fine print.</p>
          </div>
        </Section>

        {/* shadows */}
        <Section title="Shadows">
          <Row label="Elevation levels">
            <div className="h-16 w-24 border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-2 text-xs text-[var(--color-text-secondary)]" style={{ boxShadow: 'var(--shadow-sm)' }}>sm</div>
            <div className="h-16 w-24 border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-2 text-xs text-[var(--color-text-secondary)]" style={{ boxShadow: 'var(--shadow-md)' }}>md</div>
            <div className="h-16 w-24 border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-2 text-xs text-[var(--color-text-secondary)]" style={{ boxShadow: 'var(--shadow-lg)' }}>lg</div>
          </Row>
        </Section>

        {/* spacing */}
        <Section title="Spacing Scale">
          <div className="flex items-end gap-1">
            {[1, 2, 3, 4, 5, 6, 8, 10, 12].map((n) => (
              <div key={n} className="flex flex-col items-center gap-1">
                <div
                  className="bg-[var(--color-brand-primary)]"
                  style={{ width: `${n * 4}px`, height: `${n * 4}px` }}
                />
                <span className="text-[10px] text-[var(--color-text-tertiary)]">{n}</span>
              </div>
            ))}
          </div>
        </Section>

        <footer className="border-t border-[var(--color-border-default)] pt-4 text-xs text-[var(--color-text-tertiary)]">
          mailrs design system &middot; Tokens auto-switch between light/dark via CSS custom properties
        </footer>
      </div>
    </div>
  )
}
