import type { ThemeToggleMode } from '@goliapkg/gds'

import {
  Avatar,
  Badge,
  Button,
  Card,
  CardContent,
  Dialog,
  IconButton,
  Input,
  ThemeToggle,
  useResolvedMode,
  useSetThemeMode,
} from '@goliapkg/gds'
import { Plus, Search, Star, Trash2, X } from 'lucide-react'
import { useState } from 'react'

// Token swatches: each token's bg/color class is enumerated literally below
// so Tailwind's purge keeps them. Dynamic `bg-[var(${token})]` would be
// stripped at build because the class string isn't known statically.
const SURFACE_TOKENS = [
  { className: 'bg-[var(--gds-bg)]', label: 'bg' },
  { className: 'bg-[var(--gds-surface)]', label: 'surface' },
  { className: 'bg-[var(--gds-bg-secondary)]', label: 'bg-secondary' },
] as const
const TEXT_TOKENS = [
  { className: 'text-[var(--gds-fg)]', label: 'primary' },
  { className: 'text-[var(--gds-fg-secondary)]', label: 'secondary' },
  { className: 'text-[var(--gds-fg-muted)]', label: 'muted' },
] as const
const STATUS_TOKENS = [
  { className: 'bg-[var(--gds-success)]', label: 'success' },
  { className: 'bg-[var(--gds-warning)]', label: 'warning' },
  { className: 'bg-[var(--gds-danger)]', label: 'danger' },
  { className: 'bg-[var(--gds-info)]', label: 'info' },
] as const
const SPACING_STEPS = [1, 2, 3, 4, 5, 6, 8, 10, 12] as const
const SPACING_SIZE_CLASS: Record<number, string> = {
  1: 'h-1 w-1',
  2: 'h-2 w-2',
  3: 'h-3 w-3',
  4: 'h-4 w-4',
  5: 'h-5 w-5',
  6: 'h-6 w-6',
  8: 'h-8 w-8',
  10: 'h-10 w-10',
  12: 'h-12 w-12',
}

export function Playground() {
  const [dialogOpen, setDialogOpen] = useState(false)
  const [inputValue, setInputValue] = useState('')
  const mode = useResolvedMode()
  const setMode = useSetThemeMode()

  return (
    <main className="bg-bg min-h-[100dvh] p-6">
      <div className="mx-auto max-w-4xl space-y-10">
        {/* header */}
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-fg text-2xl font-bold">Component Playground</h1>
            <p className="text-fg-secondary mt-1 text-sm">
              GDS design system &middot; All components render with CSS custom properties
            </p>
          </div>
          <ThemeToggle mode={mode as ThemeToggleMode} onChange={(m) => setMode(m)} />
        </div>

        {/* color tokens */}
        <Section title="Color Tokens">
          <Row label="Surfaces">
            {SURFACE_TOKENS.map((t) => (
              <div className="flex flex-col items-center gap-1" key={t.label}>
                <div className={`border-border h-12 w-12 border ${t.className}`} />
                <span className="text-fg-muted text-tiny">{t.label}</span>
              </div>
            ))}
          </Row>
          <Row label="Text">
            {TEXT_TOKENS.map((t) => (
              <span className={`text-sm font-medium ${t.className}`} key={t.label}>
                {t.label}
              </span>
            ))}
          </Row>
          <Row label="Status">
            {STATUS_TOKENS.map((t) => (
              <div className="flex items-center gap-1.5" key={t.label}>
                <div className={`h-3 w-3 rounded-full ${t.className}`} />
                <span className="text-fg-secondary text-xs">{t.label}</span>
              </div>
            ))}
          </Row>
          <Row label="Brand">
            <div className="flex items-center gap-2">
              <div className="flex h-8 w-20 items-center justify-center bg-[var(--gds-accent)] text-xs font-medium text-[var(--gds-accent-fg)]">
                Primary
              </div>
              <div className="flex h-8 w-20 items-center justify-center bg-[var(--gds-accent-hover)] text-xs font-medium text-[var(--gds-accent-fg)]">
                Hover
              </div>
              <div className="border-border flex h-8 w-20 items-center justify-center border bg-[color-mix(in_srgb,var(--gds-accent)_10%,transparent)] text-xs font-medium text-[var(--gds-accent)]">
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
            <Button size="sm" variant="primary">
              Small
            </Button>
            <Button size="default" variant="primary">
              Default
            </Button>
            <Button size="lg" variant="primary">
              Large
            </Button>
          </Row>
          <Row label="States">
            <Button disabled variant="primary">
              Disabled
            </Button>
            <Button disabled variant="secondary">
              Disabled
            </Button>
          </Row>
          <Row label="With icons">
            <Button size="sm" variant="primary">
              <Plus className="h-3.5 w-3.5" /> New
            </Button>
            <Button size="sm" variant="danger">
              <Trash2 className="h-3.5 w-3.5" /> Delete
            </Button>
            <Button size="sm" variant="ghost">
              <Search className="h-3.5 w-3.5" /> Search
            </Button>
          </Row>
        </Section>

        {/* icon buttons */}
        <Section title="IconButton">
          <Row label="Sizes">
            <IconButton icon={<Star />} size="sm" tooltip="Small" />
            <IconButton icon={<Star />} tooltip="Default" />
            <IconButton icon={<Star />} size="lg" tooltip="Large" />
          </Row>
          <Row label="States">
            <IconButton icon={<X />} tooltip="Normal" />
            <IconButton disabled icon={<X />} tooltip="Disabled" />
          </Row>
        </Section>

        {/* badges */}
        <Section title="Badge">
          <Row label="Variants">
            <Badge variant="default">Default</Badge>
            <Badge variant="success">Success</Badge>
            <Badge variant="warning">Warning</Badge>
            <Badge variant="danger">Danger</Badge>
            <Badge variant="info">Info</Badge>
          </Row>
        </Section>

        {/* input */}
        <Section title="Input">
          <Row label="Default">
            <Input
              aria-label="Default input"
              className="max-w-xs"
              onChange={(e) => setInputValue(e.target.value)}
              placeholder="Type something..."
              value={inputValue}
            />
          </Row>
          <Row label="Disabled">
            <Input
              aria-label="Disabled input"
              className="max-w-xs"
              disabled
              placeholder="Disabled"
            />
          </Row>
        </Section>

        {/* avatar */}
        <Section title="Avatar">
          <Row label="Sizes">
            <Avatar name="Alice" size="xs" />
            <Avatar name="Bob" size="sm" />
            <Avatar name="Charlie" />
            <Avatar name="Diana" size="lg" />
          </Row>
          <Row label="Color consistency">
            <Avatar name="alice@example.com" />
            <Avatar name="bob@example.com" />
            <Avatar name="Charlie Brown" />
            <Avatar name="support@golia.jp" />
            <Avatar name="noreply@amazon.co.jp" />
          </Row>
        </Section>

        {/* card */}
        <Section title="Card">
          <Row label="Cards">
            <Card className="w-40">
              <CardContent>
                <p className="text-sm">Default</p>
              </CardContent>
            </Card>
            <Card className="w-40">
              <CardContent className="p-2">
                <p className="text-sm">Compact</p>
              </CardContent>
            </Card>
            <Card className="w-40">
              <CardContent className="p-6">
                <p className="text-sm">Spacious</p>
              </CardContent>
            </Card>
          </Row>
        </Section>

        {/* dialog */}
        <Section title="Dialog">
          <Row label="Modal">
            <Button onClick={() => setDialogOpen(true)} variant="secondary">
              Open Dialog
            </Button>
          </Row>
          <Dialog onClose={() => setDialogOpen(false)} open={dialogOpen} title="Confirm action">
            <p className="text-fg-secondary text-sm">
              Are you sure you want to proceed? This action cannot be undone.
            </p>
            <div className="mt-4 flex justify-end gap-2">
              <Button onClick={() => setDialogOpen(false)} size="sm" variant="ghost">
                Cancel
              </Button>
              <Button onClick={() => setDialogOpen(false)} size="sm" variant="danger">
                Confirm
              </Button>
            </div>
          </Dialog>
        </Section>

        {/* typography scale */}
        <Section title="Typography">
          <div className="space-y-2">
            <p className="text-fg text-2xl font-bold">Heading 2XL (20px)</p>
            <p className="text-fg text-xl font-semibold">Heading XL (18px)</p>
            <p className="text-fg text-lg font-semibold">Heading LG (16px)</p>
            <p className="text-fg text-sm">
              Body base (14px) — The quick brown fox jumps over the lazy dog.
            </p>
            <p className="text-fg-secondary text-mid">
              Body SM (13px) — Secondary text for descriptions and metadata.
            </p>
            <p className="text-fg-muted text-mini">
              Caption XS (11px) — Timestamps, counts, and fine print.
            </p>
          </div>
        </Section>

        {/* shadows */}
        <Section title="Shadows">
          <Row label="Elevation levels">
            <div className="border-border bg-surface text-fg-secondary h-16 w-24 border p-2 text-xs shadow-sm">
              sm
            </div>
            <div className="border-border bg-surface text-fg-secondary h-16 w-24 border p-2 text-xs shadow-md">
              md
            </div>
            <div className="border-border bg-surface text-fg-secondary h-16 w-24 border p-2 text-xs shadow-lg">
              lg
            </div>
          </Row>
        </Section>

        {/* spacing */}
        <Section title="Spacing Scale">
          <div className="flex items-end gap-1">
            {SPACING_STEPS.map((n) => (
              <div className="flex flex-col items-center gap-1" key={n}>
                <div className={`bg-accent ${SPACING_SIZE_CLASS[n]}`} />
                <span className="text-fg-muted text-tiny">{n}</span>
              </div>
            ))}
          </div>
        </Section>

        <footer className="border-border text-fg-muted border-t pt-4 text-xs">
          mailrs design system &middot; Powered by @goliapkg/gds
        </footer>
      </div>
    </main>
  )
}

function Row({ children, label }: { children: React.ReactNode; label: string }) {
  return (
    <div>
      <p className="text-fg-muted mb-1.5 text-xs font-medium">{label}</p>
      <div className="flex flex-wrap items-center gap-2">{children}</div>
    </div>
  )
}

function Section({ children, title }: { children: React.ReactNode; title: string }) {
  return (
    <section className="space-y-3">
      <h2 className="text-fg text-lg font-semibold">{title}</h2>
      <div className="space-y-4">{children}</div>
    </section>
  )
}
