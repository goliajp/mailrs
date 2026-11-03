/**
 * The in-page half of the interaction-latency measurement.
 *
 * `installProbe` runs in the page before the app (addInitScript), so it
 * is self-contained: nothing it references may come from this module's
 * scope. It exposes `window.__lat`:
 *
 *   arm(eventType, predicate, arg)  start watching; the clock starts at the
 *                                   `timeStamp` of the next `eventType`
 *                                   event, captured before the app sees it
 *   result()                        resolves once the predicate holds and
 *                                   the frame showing it has been painted
 *
 * End of an interaction: the predicate is checked on every DOM mutation;
 * when it first holds, a requestAnimationFrame callback is queued (it runs
 * before the paint of the frame that contains the change) and from inside
 * it a MessageChannel message (it runs after that paint). The end mark is
 * taken there. So latency = input event → first frame painted with the
 * target content, including the input's queueing delay.
 */

export type ProbeResult = {
  end: number
  /** Event Timing for the input itself, when the browser reported one. */
  event: null | { duration: number; processingEnd: number; processingStart: number }
  resources: {
    name: string
    responseEnd: number
    responseStart: number
    startTime: number
    transferSize: number
  }[]
  start: number
  timedOut: boolean
}

export function installProbe() {
  type Pred = (arg: string) => boolean
  const rowNumber = (label: string): null | number => {
    const m = /#(\d+)(?:,|$)/.exec(label)
    if (!m) return null
    return Number(m[1])
  }
  const rowButtons = () =>
    Array.from(document.querySelectorAll<HTMLButtonElement>('[role="listitem"] > button'))
  const visible = (el: Element) => {
    const r = el.getBoundingClientRect()
    return r.width > 0 && r.height > 0 && r.bottom > 0 && r.top < window.innerHeight
  }
  const predicates: Record<string, Pred> = {
    // a row older than every row loaded before (thread numbers grow with
    // date, so the next page holds smaller ones), inside the list's box
    listHasNewPage: (oldest) => {
      const list = document.querySelector('[role="list"][aria-label]')
      if (!list) return false
      const box = list.getBoundingClientRect()
      return rowButtons().some((b) => {
        const n = rowNumber(b.getAttribute('aria-label') ?? '')
        if (n === null || n >= Number(oldest)) return false
        const r = b.getBoundingClientRect()
        return r.top < box.bottom && r.bottom > box.top
      })
    },
    // the reading pane shows a body of conversation N
    paneShows: (n) => {
      const pane = document.querySelector(
        '[role="group"][aria-label="Messages in this conversation"]'
      )
      if (!pane || !visible(pane)) return false
      // an HTML body renders into a shadow root, which textContent skips
      let text = pane.textContent ?? ''
      for (const host of Array.from(pane.querySelectorAll('*'))) {
        if (host.shadowRoot) text += host.shadowRoot.textContent ?? ''
      }
      return text.includes(`REF-${n} `)
    },
    rowGone: (n) =>
      !rowButtons().some((b) => rowNumber(b.getAttribute('aria-label') ?? '') === Number(n)),
    rowRead: (n) => {
      const b = rowButtons().find(
        (x) => rowNumber(x.getAttribute('aria-label') ?? '') === Number(n)
      )
      return !!b && !/, \d+ unread/.test(b.getAttribute('aria-label') ?? '')
    },
    // the first rows all match the term (an inbox page never does)
    searchShows: (term) => {
      const labels = rowButtons()
        .slice(0, 8)
        .map((b) => (b.getAttribute('aria-label') ?? '').toLowerCase())
      return labels.length > 0 && labels.every((l) => l.includes(term.toLowerCase()))
    },
  }

  const events: PerformanceEventTiming[] = []
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) events.push(e as PerformanceEventTiming)
    }).observe({ buffered: true, durationThreshold: 16, type: 'event' } as PerformanceObserverInit)
  } catch {
    // Event Timing is optional detail; the latency itself does not use it
  }
  performance.setResourceTimingBufferSize(100_000)

  let start: null | number = null
  let watchType = ''
  let pending: null | Promise<ProbeResult> = null

  for (const type of ['click', 'input', 'keydown', 'wheel']) {
    window.addEventListener(
      type,
      (e) => {
        if (start === null && type === watchType) {
          start = e.timeStamp
          performance.mark('lat-input', { startTime: e.timeStamp })
        }
      },
      { capture: true }
    )
  }

  // after the paint of the frame being produced now
  const postPaint = (resolve: (t: number) => void) => {
    const ch = new MessageChannel()
    ch.port1.onmessage = () => resolve(performance.now())
    ch.port2.postMessage(null)
  }

  function arm(eventType: string, predicate: string, arg: string, timeoutMs: number) {
    start = null
    watchType = eventType
    const pred = predicates[predicate]
    pending = new Promise<ProbeResult>((resolve) => {
      let fired = false
      let settled = false
      const finish = (end: number, timedOut: boolean) => {
        if (settled) return
        settled = true
        mo.disconnect()
        clearTimeout(timer)
        const s = start ?? end
        performance.mark('lat-painted', { startTime: end })
        const ev = events.find((e) => Math.abs(e.startTime - s) < 1 && e.name === eventType)
        resolve({
          end,
          event: ev
            ? {
                duration: ev.duration,
                processingEnd: ev.processingEnd,
                processingStart: ev.processingStart,
              }
            : null,
          resources: [],
          start: s,
          timedOut,
        })
      }
      const holds = () => !fired && !settled && start !== null && pred(arg)
      // A mutation is seen in the microtask after React commits, before
      // this frame's rAF: one rAF later is still this frame, then post-paint.
      const mo = new MutationObserver(() => {
        if (!holds()) return
        fired = true
        requestAnimationFrame(() => postPaint((t) => finish(t, false)))
      })
      mo.observe(document.body, {
        attributes: true,
        characterData: true,
        childList: true,
        subtree: true,
      })
      // Changes that are layout only (a scroll moving rows into view) make
      // no mutation, so every frame is checked as well. Seen from inside a
      // rAF, the change is already in this frame: post-paint directly.
      const poll = () => {
        if (fired || settled) return
        if (holds()) {
          fired = true
          postPaint((t) => finish(t, false))
          return
        }
        requestAnimationFrame(poll)
      }
      requestAnimationFrame(poll)
      const timer = setTimeout(() => finish(performance.now(), true), timeoutMs)
    })
  }

  async function result(): Promise<ProbeResult> {
    const r = await (pending as Promise<ProbeResult>)
    return r
  }

  /** Resource entries that started in [from, to], for request counts and bytes. */
  function resourcesBetween(from: number, to: number) {
    return (performance.getEntriesByType('resource') as PerformanceResourceTiming[])
      .filter((e) => e.startTime >= from - 0.5 && e.startTime <= to)
      .map((e) => ({
        name: e.name,
        responseEnd: e.responseEnd,
        responseStart: e.responseStart,
        startTime: e.startTime,
        transferSize: e.transferSize,
      }))
  }

  ;(window as unknown as { __lat: unknown }).__lat = {
    arm,
    predicates,
    resourcesBetween,
    result,
    now: () => performance.now(),
  }
}
