import type { RetryState } from '@/lib/remote-images'
import type { RefObject } from 'react'

import { useEffect, useRef, useState } from 'react'

import { retryMissing, showPrefetched, watchMissing } from '@/lib/remote-images'
import { wirePrefetchThreadImages } from '@/wire/endpoints/mail'

/** What the note above a message says about its images. */
export type ImagesNote = 'held' | RetryState

/**
 * Point a message's remote images at the server's copies once they are
 * `shown`. With `imagesOf`, a miss asks the server to fetch the thread's
 * images now and the missing ones are asked for again until they come or
 * the schedule runs out; mail in Junk or held is not fetched for until the
 * reader says `showAnyway`. Every timer goes with the message.
 *
 * Runs after the effect that fills the shadow root; the listeners go on
 * before the images get their address, so no failure is missed.
 */
export function useRemoteImages(
  hostRef: RefObject<HTMLDivElement | null>,
  sanitized: string,
  shown: boolean,
  imagesOf?: string
): { note: ImagesNote; showAnyway: () => void } {
  const [note, setNote] = useState<ImagesNote>('none')
  // the current message's "show anyway"; replaced with the message
  const anyway = useRef<() => void>(() => {})

  useEffect(() => {
    const root = hostRef.current?.shadowRoot
    setNote('none')
    anyway.current = () => {}
    if (!root || !shown) return
    let gone = false
    const retry = retryMissing((state) => {
      if (!gone) setNote(state)
    })
    if (imagesOf) {
      const ask = (request: Promise<'junk' | 'queued'>) => {
        void request.then((answer) => {
          if (gone) return
          if (answer === 'junk') {
            setNote('held')
            return
          }
          retry.start()
        })
      }
      let asked = false
      watchMissing(root, (imgs) => {
        retry.add(imgs)
        if (asked) return
        asked = true
        ask(wirePrefetchThreadImages(imagesOf))
      })
      anyway.current = () => {
        setNote('loading')
        ask(wirePrefetchThreadImages(imagesOf, true))
      }
    }
    void showPrefetched(root)
    return () => {
      gone = true
      retry.stop()
    }
  }, [hostRef, sanitized, shown, imagesOf])

  return { note, showAnyway: () => anyway.current() }
}
