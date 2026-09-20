import { onScopeDispose, ref, watch, type Ref } from 'vue'
import { getCodexStateDiagnostics, type CodexStateDiagnostics } from '@/api/endpoints/codex-state'

export interface PoolCodexState {
  loading: boolean
  read_failed: boolean
  diagnostics: CodexStateDiagnostics | null
}

/** 只读轮询当前账号页；刷新保留已有结果，不触发采集。 */
export function usePoolCodexState(
  providerId: Readonly<Ref<string | null>>,
  providerType: Readonly<Ref<string>>,
  accounts: Readonly<Ref<readonly { key_id: string; auth_type: string }[]>>,
) {
  const states = ref<Record<string, PoolCodexState>>({})
  let controller = new AbortController()
  let timer: ReturnType<typeof setTimeout> | undefined
  let context = ''
  let previousProvider: string | null = null
  let running = false
  let disposed = false
  const visible = () => typeof document === 'undefined' || !document.hidden
  const clearTimer = () => { clearTimeout(timer); timer = undefined }

  async function refresh() {
    if (disposed || running || !visible() || !providerId.value || providerType.value !== 'codex') return
    clearTimer()
    const id = providerId.value
    const signal = controller.signal
    const pending = accounts.value.filter(key => key.auth_type.toLowerCase() === 'oauth')
    if (!pending.length) return
    running = true
    let cursor = 0
    async function worker() {
      while (!signal.aborted && visible() && cursor < pending.length) {
        const key = pending[cursor++]
        if (!key) return
        try {
          const diagnostics = await getCodexStateDiagnostics(id, key.key_id, { signal })
          if (!signal.aborted) states.value[key.key_id] = { loading: false, read_failed: false, diagnostics }
        } catch {
          if (!signal.aborted) {
            states.value[key.key_id] = { loading: false, read_failed: true, diagnostics: states.value[key.key_id]?.diagnostics ?? null }
          }
        }
      }
    }
    try {
      await Promise.all(Array.from({ length: Math.min(4, pending.length) }, worker))
    } finally {
      if (!signal.aborted) {
        running = false
        if (!disposed && visible()) timer = setTimeout(() => { void refresh() }, 10000)
      }
    }
  }

  watch([providerId, providerType, accounts], ([id, type, keys]) => {
    const pending = keys.filter(key => key.auth_type.toLowerCase() === 'oauth')
    const nextContext = JSON.stringify([id, type, pending.map(key => key.key_id).sort()])
    if (context !== nextContext) {
      controller.abort()
      controller = new AbortController()
      clearTimer()
      running = false
      const sameProvider = previousProvider === id && type === 'codex'
      states.value = Object.fromEntries((id && type === 'codex' ? pending : []).map(key => [
        key.key_id, (sameProvider && states.value[key.key_id]) || { loading: true, read_failed: false, diagnostics: null },
      ]))
      context = nextContext
      previousProvider = id
    }
    void refresh()
  }, { immediate: true })

  const onVisibility = () => { if (visible()) void refresh(); else clearTimer() }
  if (typeof document !== 'undefined') document.addEventListener('visibilitychange', onVisibility)
  onScopeDispose(() => {
    disposed = true
    controller.abort()
    clearTimer()
    if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', onVisibility)
  })
  return states
}
