import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// ---------------------------------------------------------------------------
// minimal AudioContext mock
// ---------------------------------------------------------------------------

type OscillatorType = 'sine' | 'square' | 'sawtooth' | 'triangle' | 'custom'

interface MockOscillator {
  type: OscillatorType
  frequency: { setValueAtTime: ReturnType<typeof vi.fn> }
  connect: ReturnType<typeof vi.fn>
  start: ReturnType<typeof vi.fn>
  stop: ReturnType<typeof vi.fn>
}

interface MockGainNode {
  gain: {
    setValueAtTime: ReturnType<typeof vi.fn>
    exponentialRampToValueAtTime: ReturnType<typeof vi.fn>
  }
  connect: ReturnType<typeof vi.fn>
}

function makeMockOscillator(): MockOscillator {
  return {
    type: 'sine',
    frequency: { setValueAtTime: vi.fn() },
    connect: vi.fn(),
    start: vi.fn(),
    stop: vi.fn(),
  }
}

function makeMockGainNode(): MockGainNode {
  return {
    gain: {
      setValueAtTime: vi.fn(),
      exponentialRampToValueAtTime: vi.fn(),
    },
    connect: vi.fn(),
  }
}

// factory that produces a fresh AudioContext mock and captures instances
function makeAudioContextClass(state: AudioContext['state'] = 'running') {
  const instances: MockAudioContextInstance[] = []

  class MockAudioContextInstance {
    state: AudioContext['state'] = state
    currentTime = 0
    destination = {}
    oscillator = makeMockOscillator()
    gainNode = makeMockGainNode()

    resume = vi.fn(() => Promise.resolve())

    createOscillator(): MockOscillator {
      return this.oscillator
    }

    createGain(): MockGainNode {
      return this.gainNode
    }

    close = vi.fn()
  }

  const Ctor = vi.fn(() => {
    const instance = new MockAudioContextInstance()
    instances.push(instance)
    return instance
  })

  return { Ctor, instances }
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
    await Promise.resolve()

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
      0.15, // 150ms / 1000
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
      vi.fn(() => {
        throw new Error('not allowed')
      }),
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
