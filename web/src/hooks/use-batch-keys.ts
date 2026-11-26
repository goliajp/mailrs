import { useEffect } from 'react'

import { type ActionId, ACTIONS, keyName } from '@/lib/actions'
import { isEditableTarget } from '@/lib/editable-target'

/**
 * While conversations are picked, the action keys act on all of them and
 * Escape puts the selection away.
 *
 * Listens in the capture phase and stops the keys it takes: the list's
 * own handler reads the same keys for the one open conversation, and
 * would archive that too.
 */
export function useBatchKeys(
  ids: ActionId[],
  onRun: (id: ActionId) => void,
  onEscape: () => void,
  disabled: boolean
) {
  useEffect(() => {
    const handle = (e: KeyboardEvent) => {
      if (isEditableTarget(e.target)) return
      if (e.altKey || e.ctrlKey || e.metaKey) return
      // a dialog over the list has the keyboard
      if (document.querySelector('[role="dialog"]')) return
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        onEscape()
        return
      }
      const id = ids.find((a) => ACTIONS[a].keys.includes(keyName(e)))
      if (!id) return
      e.preventDefault()
      e.stopPropagation()
      if (!disabled) onRun(id)
    }
    window.addEventListener('keydown', handle, true)
    return () => window.removeEventListener('keydown', handle, true)
  }, [ids, onRun, onEscape, disabled])
}
