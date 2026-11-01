import type { ComposerWindow } from '@/store/composer-windows'

import { useAtomValue, useSetAtom } from 'jotai'
import { ChevronUp, Maximize2, Minus, X } from 'lucide-react'
import { useNavigate } from 'react-router'

import { type Key, useT } from '@/i18n'
import {
  closeComposerWindowAtom,
  composerWindowsAtom,
  toggleComposerWindowAtom,
} from '@/store/composer-windows'
import { composeCarrySourceAtom, composeReplySourceAtom, composingNewAtom } from '@/store/ui'

import { ComposerView } from './composer'
import { useComposer } from './use-composer'
import { useComposerAi } from './use-composer-ai'

/**
 * The floating shell: compose windows in the bottom-right corner, side by
 * side, each its own session. A folded window keeps everything typed in it.
 */
export function FloatingComposers() {
  const windows = useAtomValue(composerWindowsAtom)
  if (windows.length === 0) return null
  return (
    <div className="pointer-events-none fixed right-4 bottom-8 z-40 flex flex-row-reverse items-end gap-3">
      {windows.map((w) => (
        <FloatingComposer key={w.id} win={w} />
      ))}
    </div>
  )
}

function bodyClass(minimized: boolean): string {
  if (minimized) return 'hidden'
  return 'flex min-h-0 flex-1 flex-col'
}

function FloatingComposer({ win }: { win: ComposerWindow }) {
  const t = useT()
  const navigate = useNavigate()
  const closeWindow = useSetAtom(closeComposerWindowAtom)
  const toggle = useSetAtom(toggleComposerWindowAtom)
  const setCarry = useSetAtom(composeCarrySourceAtom)
  const setReplySource = useSetAtom(composeReplySourceAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const c = useComposer(win.start, { onSent: () => closeWindow(win.id) })
  const ai = useComposerAi(c)

  const close = () => {
    // what is typed is kept as a draft
    void c.saveNow()
    closeWindow(win.id)
  }

  const expand = async () => {
    setCarry(await c.carry())
    setReplySource(null)
    setComposingNew(true)
    closeWindow(win.id)
    navigate('/mail')
  }

  const title = c.subject.trim() || t('compose.newHeader')
  return (
    <section aria-label={title} className={frameClass(win.minimized)} role="dialog">
      <header className="bg-bg-secondary flex h-10 shrink-0 items-center gap-1 rounded-t-lg pr-1 pl-3">
        <span className="text-fg min-w-0 flex-1 truncate text-sm font-medium">{title}</span>
        <WindowButton label={minimizeLabel(win.minimized)} onClick={() => toggle(win.id)}>
          <MinimizeIcon minimized={win.minimized} />
        </WindowButton>
        <WindowButton label="composer.window.expand" onClick={() => void expand()}>
          <Maximize2 className="h-3.5 w-3.5" />
        </WindowButton>
        <WindowButton label="common.close" onClick={close}>
          <X className="h-4 w-4" />
        </WindowButton>
      </header>
      {/* folded, not unmounted: the session and what is typed survive */}
      <div className={bodyClass(win.minimized)}>
        <ComposerView ai={ai} c={c} compact fieldsOpen focusTo onCancel={close} />
      </div>
    </section>
  )
}

function frameClass(minimized: boolean): string {
  const base = 'border-border bg-bg pointer-events-auto flex flex-col rounded-lg border shadow-xl'
  if (minimized) return `${base} w-60`
  return `${base} h-[min(560px,calc(100vh-6rem))] w-[440px] max-w-[calc(100vw-2rem)]`
}

function MinimizeIcon({ minimized }: { minimized: boolean }) {
  if (minimized) return <ChevronUp className="h-4 w-4" />
  return <Minus className="h-4 w-4" />
}

function minimizeLabel(minimized: boolean): Key {
  if (minimized) return 'composer.window.restore'
  return 'composer.window.minimize'
}

function WindowButton({
  children,
  label,
  onClick,
}: {
  children: React.ReactNode
  label: Key
  onClick: () => void
}) {
  const t = useT()
  return (
    <button
      aria-label={t(label)}
      className="text-fg-muted hover:bg-bg-tertiary hover:text-fg flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-colors"
      onClick={onClick}
      title={t(label)}
      type="button"
    >
      {children}
    </button>
  )
}
