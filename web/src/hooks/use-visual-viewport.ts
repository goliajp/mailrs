import { useEffect, useState } from 'react'

type VisualViewportState = {
  isKeyboardOpen: boolean
  keyboardHeight: number
  viewportHeight: number
}

const KEYBOARD_THRESHOLD = 100

// subscribe to visualViewport changes to detect mobile keyboard
export function useVisualViewport(): VisualViewportState {
  const [state, setState] = useState<VisualViewportState>(() => ({
    isKeyboardOpen: false,
    keyboardHeight: 0,
    viewportHeight: typeof window !== 'undefined' ? window.innerHeight : 0,
  }))

  useEffect(() => {
    const vv = window.visualViewport
    if (!vv) return

    const update = () => {
      const fullHeight = window.innerHeight
      const viewportHeight = vv.height
      const keyboardHeight = Math.max(0, fullHeight - viewportHeight)
      setState({
        isKeyboardOpen: keyboardHeight > KEYBOARD_THRESHOLD,
        keyboardHeight,
        viewportHeight,
      })
    }

    vv.addEventListener('resize', update)
    vv.addEventListener('scroll', update)
    update()

    return () => {
      vv.removeEventListener('resize', update)
      vv.removeEventListener('scroll', update)
    }
  }, [])

  return state
}
