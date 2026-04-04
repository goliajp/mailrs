import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// ---------------------------------------------------------------------------
// minimal AudioContext mock
// ---------------------------------------------------------------------------

interface MockGainNode {
  connect: ReturnType<typeof vi.fn>
  gain: {
    exponentialRampToValueAtTime: ReturnType<typeof vi.fn>
    setValueAtTime: ReturnType<typeof vi.fn>
  }
}

interface MockOscillator {
  connect: ReturnType<typeof vi.fn>
  frequency: { setValueAtTime: ReturnType<typeof vi.fn> }
  start: ReturnType<typeof vi.fn>
  stop: ReturnType<typeof vi.fn>
  type: OscillatorType
}

type OscillatorType = 'custom' | 'sawtooth' | 'sine' | 'square' | 'triangle'

// factory that produces a fresh AudioContext mock and captures instances
function makeAudioContextClass(state: AudioContext['state'] = 'running') {
  const instances: MockAudioContextInstance[] = []

  class MockAudioContextInstance {
    close = vi.fn()

    currentTime = 0
    destination = {}
    gainNode = makeMockGainNode()
    oscillator = makeMockOscillator()
    resume = vi.fn(() => Promise.resolve())

    state: AudioContext['state'] = state

    constructor() {
      instances.push(this)
    }

    createGain = (): MockGainNode => this.gainNode

    createOscillator = (): MockOscillator => this.oscillator
  }

  const Ctor = vi.fn().mockImplementation(MockAudioContextInstance as any)

  return { Ctor, instances }
}

function makeMockGainNode(): MockGainNode {
  return {
    connect: vi.fn(),
    gain: {
      exponentialRampToValueAtTime: vi.fn(),
      setValueAtTime: vi.fn(),
    },
  }
}

function makeMockOscillator(): MockOscillator {
  return {
    connect: vi.fn(),
    frequency: { setValueAtTime: vi.fn() },
    start: vi.fn(),
    stop: vi.fn(),
    type: 'sine',
  }
}

describe('playNotificationSound', () => {
  beforeEach(() => {
    // reset module so the module-level `audioCtx` singleton is re-created fresh
    vi.resetModules()
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('creates an AudioContext and plays a sine oscillator', async () => {
    const { Ctor, instances } = makeAudioContextClass('running')
    vi.stubGlobal('AudioContext', Ctor)

    const { playNotificationSound } = await import('../notification-sound')
    playNotificationSound()

    // let micro-task queue flush (the .then() inside playNotificationSound)
    await new Promise((r) => setTimeout(r, 0))

    expect(Ctor).toHaveBeenCalledTimes(1)
    const ctx = instances[0]

    // oscillator wiring
    expect(ctx.oscillator.connect).toHaveBeenCalledWith(ctx.gainNode)
    expect(ctx.gainNode.connect).toHaveBeenCalledWith(ctx.destination)

    // correct waveform
    expect(ctx.oscillator.type).toBe('sine')

    // frequency set to 400 Hz
    expect(ctx.oscillator.frequency.setValueAtTime).toHaveBeenCalledWith(400, 0)

    // gain envelope
    expect(ctx.gainNode.gain.setValueAtTime).toHaveBeenCalledWith(0.3, 0)
    expect(ctx.gainNode.gain.exponentialRampToValueAtTime).toHaveBeenCalledWith(
      0.001,
      0.15 // 150ms / 1000
    )

    // oscillator lifecycle
    expect(ctx.oscillator.start).toHaveBeenCalledWith(0)
    expect(ctx.oscillator.stop).toHaveBeenCalledWith(0.15)
  })

  it('resumes a suspended AudioContext before playing', async () => {
    const { Ctor, instances } = makeAudioContextClass('suspended')
    vi.stubGlobal('AudioContext', Ctor)

    const { playNotificationSound } = await import('../notification-sound')
    playNotificationSound()

    // let resume() promise resolve then the .then() callback
    await Promise.resolve()
    await Promise.resolve()

    const ctx = instances[0]
    expect(ctx.resume).toHaveBeenCalledTimes(1)
    expect(ctx.oscillator.start).toHaveBeenCalled()
  })

  it('does nothing when AudioContext constructor throws', async () => {
    vi.stubGlobal(
      'AudioContext',
      vi.fn().mockImplementation(
        class {
          constructor() {
            throw new Error('not allowed')
          }
        } as any
      )
    )

    const { playNotificationSound } = await import('../notification-sound')
    // should not throw
    expect(() => playNotificationSound()).not.toThrow()
  })

  it('does nothing when AudioContext is not available', async () => {
    // simulate environments without Web Audio API
    vi.stubGlobal('AudioContext', undefined)

    const { playNotificationSound } = await import('../notification-sound')
    expect(() => playNotificationSound()).not.toThrow()
  })

  it('reuses the existing AudioContext on repeated calls', async () => {
    const { Ctor, instances } = makeAudioContextClass('running')
    vi.stubGlobal('AudioContext', Ctor)

    const { playNotificationSound } = await import('../notification-sound')
    playNotificationSound()
    await Promise.resolve()
    playNotificationSound()
    await Promise.resolve()

    // constructor called only once; the singleton is reused
    expect(Ctor).toHaveBeenCalledTimes(1)
    // but two oscillators were started (one per call)
    const ctx = instances[0]
    expect(ctx.oscillator.start).toHaveBeenCalledTimes(2)
  })
})
