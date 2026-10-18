import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue'
import { formatResetClock, formatResetCountdown } from '../utils/quotaCountdown'

/**
 * 配额重置倒计时：`resetAt` 有值时每秒推进；到点调用 `onExpire`（组件借此重拉配额）。
 *
 * 到点后**不立即停表**：客户端与服务端时钟可能差几秒，跨过 0 点的那次刷新若仍拿到
 * 「旧的 resetAt + 剩余 0 次」，停表就会一直卡在「0 次」——改为每 5 秒重试一次，
 * 直到刷新后拿到的配额不再耗尽（resetAt 变新或变 null）才自然停下。
 *
 * `resetAt` 变化会重新武装；组件卸载自动停表。调用方应只在「额度已耗尽」时传入非空值，
 * 配额正常时不产生任何定时器。
 */
export function useQuotaReset(
  resetAt: Ref<number | null | undefined>,
  onExpire?: () => void,
) {
  const now = ref(Date.now())
  let timer: ReturnType<typeof setInterval> | null = null
  /** 上次触发重拉的时刻：到点后每 5s 最多重试一次，避免刷新失败时每秒打请求 */
  let lastFired = 0

  function stop() {
    if (timer) {
      clearInterval(timer)
      timer = null
    }
  }

  function tick() {
    const at = resetAt.value
    if (!at) {
      stop()
      return
    }
    now.value = Date.now()
    if (now.value >= at && now.value - lastFired >= 5000) {
      lastFired = now.value
      onExpire?.()
    }
  }

  watch(
    resetAt,
    (at) => {
      stop()
      if (!at) return
      now.value = Date.now()
      timer = setInterval(tick, 1000)
    },
    { immediate: true },
  )

  onBeforeUnmount(stop)

  return {
    /** 还有多久重置（3 小时 25 分 / 25 分 12 秒 / 即将重置） */
    countdown: computed(() => (resetAt.value ? formatResetCountdown(resetAt.value, now.value) : '')),
    /** 重置时刻（客户端本地 HH:MM） */
    resetClock: computed(() => (resetAt.value ? formatResetClock(resetAt.value) : '')),
  }
}
