// 轻提示音：WebAudio 合成一段柔和双音，避免依赖外部音频文件。
// 供「倒计时到点提示音」等场景使用；静默失败（无音频上下文或用户策略）不打扰。
let ctx: AudioContext | null = null

function getCtx(): AudioContext | null {
  if (typeof window === 'undefined') return null
  const AC = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
  if (!AC) return null
  if (!ctx) ctx = new AC()
  if (ctx.state === 'suspended') void ctx.resume()
  return ctx
}

/** 播放柔和双音提示（约 0.7s） */
export function playChime() {
  const ac = getCtx()
  if (!ac) return
  const now = ac.currentTime
  const notes = [
    { freq: 784, at: 0, dur: 0.28 }, // G5
    { freq: 1046, at: 0.16, dur: 0.5 }, // C6
  ]
  for (const n of notes) {
    const osc = ac.createOscillator()
    const gain = ac.createGain()
    osc.type = 'sine'
    osc.frequency.value = n.freq
    gain.gain.setValueAtTime(0, now + n.at)
    gain.gain.linearRampToValueAtTime(0.18, now + n.at + 0.02)
    gain.gain.exponentialRampToValueAtTime(0.0001, now + n.at + n.dur)
    osc.connect(gain)
    gain.connect(ac.destination)
    osc.start(now + n.at)
    osc.stop(now + n.at + n.dur + 0.05)
  }
}
