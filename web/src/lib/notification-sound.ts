// plays a short sine wave beep using the Web Audio API (no audio files needed)

const FREQUENCY_HZ = 400
const DURATION_MS = 150
const GAIN = 0.3

let audioCtx: AudioContext | null = null

function getAudioContext(): AudioContext | null {
  if (audioCtx && audioCtx.state !== 'closed') return audioCtx
  try {
    audioCtx = new AudioContext()
    return audioCtx
  } catch {
    return null
  }
}

export function playNotificationSound(): void {
  const ctx = getAudioContext()
  if (!ctx) return

  const resume = ctx.state === 'suspended' ? ctx.resume() : Promise.resolve()

  resume.then(() => {
    const oscillator = ctx.createOscillator()
    const gainNode = ctx.createGain()

    oscillator.connect(gainNode)
    gainNode.connect(ctx.destination)

    oscillator.type = 'sine'
    oscillator.frequency.setValueAtTime(FREQUENCY_HZ, ctx.currentTime)

    // fade out smoothly to avoid click artifacts
    gainNode.gain.setValueAtTime(GAIN, ctx.currentTime)
    gainNode.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + DURATION_MS / 1000)

    oscillator.start(ctx.currentTime)
    oscillator.stop(ctx.currentTime + DURATION_MS / 1000)
  }).catch(() => {
    // autoplay policy may block audio — silently ignore
  })
}
