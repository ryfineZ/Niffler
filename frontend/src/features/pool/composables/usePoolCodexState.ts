import { ref, watch, type Ref } from 'vue'
import { getCodexStateDiagnostics, type CodexStateDiagnostics } from '@/api/endpoints/codex-state'

export interface PoolCodexState {
  loading: boolean
  read_failed: boolean
  diagnostics: CodexStateDiagnostics | null
}

/** 状态跟随当前账号页加载，桌面和手机共用数据，不阻塞账号列表。 */
export function usePoolCodexState(
  providerId: Readonly<Ref<string | null>>,
  providerType: Readonly<Ref<string>>,
  accounts: Readonly<Ref<readonly { key_id: string; auth_type: string }[]>>,
) {
  const states = ref<Record<string, PoolCodexState>>({})
  watch([providerId, providerType, accounts], ([id, type, keys], _, onCleanup) => {
    const controller = new AbortController()
    onCleanup(() => controller.abort())
    states.value = {}
    if (!id || type !== 'codex') return
    const pending = keys.filter(key => key.auth_type.toLowerCase() === 'oauth')
    for (const key of pending) {
      states.value[key.key_id] = { loading: true, read_failed: false, diagnostics: null }
    }
    let cursor = 0
    async function worker() {
      while (!controller.signal.aborted && cursor < pending.length) {
        const key = pending[cursor++]
        if (!key) return
        try {
          const diagnostics = await getCodexStateDiagnostics(id as string, key.key_id, { signal: controller.signal })
          if (!controller.signal.aborted) {
            states.value[key.key_id] = { loading: false, read_failed: false, diagnostics }
          }
        } catch {
          if (!controller.signal.aborted) {
            states.value[key.key_id] = { loading: false, read_failed: true, diagnostics: null }
          }
        }
      }
    }
    for (let index = 0; index < Math.min(4, pending.length); index++) void worker()
  }, { immediate: true })
  return states
}
