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
} from '@goliapkg/gds'
import { Plus, Search, Star, Trash2, X } from 'lucide-react'
import { useState } from 'react'

export function Playground() {
  const [dialogOpen, setDialogOpen] = useState(false)
  const [inputValue, setInputValue] = useState('')

  return (
    <div className="bg-bg min-h-screen p-6">
      <div className="mx-auto max-w-4xl space-y-10">
        {/* header */}
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-fg text-2xl font-bold">Component Playground</h1>
            <p className="text-fg-secondary mt-1 text-sm">
              GDS design system &middot; All components render with CSS custom
              properties
            </p>
          </div>
          <ThemeToggle />
        </div>

        {/* color tokens */}
        <Section title="Color Tokens">
          <Row label="Surfaces">
            {(['--gds-bg', '--gds-surface', '--gds-bg-secondary'] as const).map(
              (v) => (
                <div className="flex flex-col items-center gap-1" key={v}>
                  <div
                    className="border-border h-12 w-12 border"
                    style={{ background: `var(${v})` }}
                  />
                  <span className="text-fg-muted text-[10px]">
                    {v.replace('--gds-', '')}
                  </span>
                </div>
              )
            )}
          </Row>
          <Row label="Text">
            {[
              { label: 'primary', v: '--gds-fg' },
              { label: 'secondary', v: '--gds-fg-secondary' },
              { label: 'muted', v: '--gds-fg-muted' },
            ].map(({ label, v }) => (
              <span
                className="text-sm font-medium"
                key={v}
                style={{ color: `var(${v})` }}
              >
                {label}
              </span>
            ))}
          </Row>
          <Row label="Status">
            {(['success', 'warning', 'danger', 'info'] as const).map((s) => (
              <div className="flex items-center gap-1.5" key={s}>
                <div
                  className="h-3 w-3 rounded-full"
                  style={{ background: `var(--gds-${s})` }}
                />
                <span className="text-fg-secondary text-xs">{s}</span>
              </div>
            ))}
          </Row>
          <Row label="Brand">
            <div className="flex items-center gap-2">
              <div
                className="flex h-8 w-20 items-center justify-center text-xs font-medium"
                style={{
                  background: 'var(--gds-accent)',
                  color: 'var(--gds-accent-fg)',
                }}
              >
                Primary
              </div>
              <div
                className="flex h-8 w-20 items-center justify-center text-xs font-medium"
                style={{
                  background: 'var(--gds-accent-hover)',
                  color: 'var(--gds-accent-fg)',
                }}
              >
                Hover
              </div>
              <div
                className="border-border flex h-8 w-20 items-center justify-center border text-xs font-medium"
                style={{
                  background:
                    'color-mix(in srgb, var(--gds-accent) 10%, transparent)',
                  color: 'var(--gds-accent)',
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
            <IconButton aria-label="Small" size="sm">
              <Star className="h-3.5 w-3.5" />
            </IconButton>
            <IconButton aria-label="Default">
              <Star className="h-4 w-4" />
            </IconButton>
            <IconButton aria-label="Large" size="lg">
              <Star className="h-4.5 w-4.5" />
            </IconButton>
          </Row>
          <Row label="States">
            <IconButton aria-label="Normal">
              <X className="h-4 w-4" />
            </IconButton>
            <IconButton aria-label="Disabled" disabled>
              <X className="h-4 w-4" />
            </IconButton>
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
          <Dialog
            onClose={() => setDialogOpen(false)}
            open={dialogOpen}
            title="Confirm action"
          >
            <p className="text-fg-secondary text-sm">
              Are you sure you want to proceed? This action cannot be undone.
            </p>
            <div className="mt-4 flex justify-end gap-2">
              <Button
                onClick={() => setDialogOpen(false)}
                size="sm"
                variant="ghost"
              >
                Cancel
              </Button>
              <Button
                onClick={() => setDialogOpen(false)}
                size="sm"
                variant="danger"
              >
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
            <p className="text-fg-secondary text-[13px]">
              Body SM (13px) — Secondary text for descriptions and metadata.
            </p>
            <p className="text-fg-muted text-[11px]">
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
            {[1, 2, 3, 4, 5, 6, 8, 10, 12].map((n) => (
              <div className="flex flex-col items-center gap-1" key={n}>
                <div
                  className="bg-accent"
                  style={{ height: `${n * 4}px`, width: `${n * 4}px` }}
                />
                <span className="text-fg-muted text-[10px]">{n}</span>
              </div>
            ))}
          </div>
        </Section>

        <footer className="border-border text-fg-muted border-t pt-4 text-xs">
          mailrs design system &middot; Powered by @goliapkg/gds
        </footer>
      </div>
    </div>
  )
}

function Row({
  children,
  label,
}: {
  children: React.ReactNode
  label: string
}) {
  return (
    <div>
      <p className="text-fg-muted mb-1.5 text-xs font-medium">{label}</p>
      <div className="flex flex-wrap items-center gap-2">{children}</div>
    </div>
  )
}

function Section({
  children,
  title,
}: {
  children: React.ReactNode
  title: string
}) {
  return (
    <section className="space-y-3">
      <h2 className="text-fg text-lg font-semibold">{title}</h2>
      <div className="space-y-4">{children}</div>
    </section>
  )
}
