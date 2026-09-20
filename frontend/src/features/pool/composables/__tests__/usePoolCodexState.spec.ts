import { afterEach, describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref, type EffectScope } from 'vue'
import { usePoolCodexState } from '../usePoolCodexState'

const api = vi.hoisted(() => ({ get: vi.fn() }))
vi.mock('@/api/endpoints/codex-state', () => ({ getCodexStateDiagnostics: api.get }))
const scopes: EffectScope[] = []
afterEach(() => { scopes.splice(0).forEach(scope => scope.stop()); api.get.mockReset(); vi.useRealTimers(); vi.restoreAllMocks() })
function setup(keys: { key_id: string; auth_type: string }[], type = 'codex') {
  const provider = ref<string | null>('provider-a')
  const providerType = ref(type)
  const accounts = ref(keys)
  const scope = effectScope(); scopes.push(scope)
  const states = scope.run(() => usePoolCodexState(provider, providerType, accounts))!
  return { provider, providerType, accounts, states, scope }
}
const empty = { enabled: true, observed_at: 1, items: [] }
async function flush() { await Promise.resolve(); await nextTick(); await Promise.resolve() }

describe('State follows the pool account page', () => {
  it('limits concurrency to four and does not query API keys or other providers', async () => {
    const finish: (() => void)[] = []
    api.get.mockImplementation(() => new Promise(resolve => { finish.push(() => resolve(empty)) }))
    const { states } = setup([
      ...Array.from({ length: 7 }, (_, i) => ({ key_id: String(i), auth_type: 'oauth' })),
      { key_id: 'api-key', auth_type: 'api_key' },
    ])
    expect(api.get).toHaveBeenCalledTimes(4)
    expect(states.value['api-key']).toBeUndefined()
    finish[0]?.(); await flush()
    expect(api.get).toHaveBeenCalledTimes(5)
    expect(states.value['0']?.loading).toBe(false)
    setup([{ key_id: 'other', auth_type: 'oauth' }], 'kiro')
    expect(api.get).toHaveBeenCalledTimes(5)
  })
  it('cancels a previous page and ignores its late result even when account IDs are reused', async () => {
    let resolveOld!: (value: typeof empty) => void
    api.get.mockReturnValueOnce(new Promise(resolve => { resolveOld = resolve }))
      .mockResolvedValue({ ...empty, enabled: false })
    const { provider, states, scope } = setup([{ key_id: 'account', auth_type: 'oauth' }])
    const oldSignal = api.get.mock.calls[0]?.[2].signal as AbortSignal
    provider.value = 'provider-b'; await flush()
    expect(oldSignal.aborted).toBe(true)
    resolveOld(empty); await flush()
    expect(states.value.account?.diagnostics?.enabled).toBe(false)
    expect(api.get.mock.calls[1]?.slice(0, 2)).toEqual(['provider-b', 'account'])
    const currentSignal = api.get.mock.calls[1]?.[2].signal as AbortSignal
    scope.stop(); expect(currentSignal.aborted).toBe(true)
  })
  it('keeps per-account errors explicit and refreshes when the account page reloads', async () => {
    api.get.mockRejectedValueOnce(new Error('offline')).mockResolvedValue(empty)
    const { accounts, states } = setup([{ key_id: 'account', auth_type: 'oauth' }])
    await flush(); expect(states.value.account?.read_failed).toBe(true)
    accounts.value = [...accounts.value]; await flush()
    expect(states.value.account?.read_failed).toBe(false)
    expect(states.value.account?.diagnostics?.items).toEqual([])
  })
})

describe('silent state updates', () => {
  it('keeps existing cells during refresh and polls without reloading accounts', async () => {
    vi.useFakeTimers()
    api.get.mockResolvedValue(empty)
    const { accounts, states, scope } = setup([{ key_id: 'account', auth_type: 'oauth' }])
    await flush()
    let finish!: (value: typeof empty) => void
    api.get.mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
    accounts.value = [...accounts.value]; await flush()
    expect(states.value.account?.loading).toBe(false)
    expect(states.value.account?.diagnostics).toEqual(empty)
    await vi.advanceTimersByTimeAsync(20000)
    expect(api.get).toHaveBeenCalledTimes(2)
    finish({ ...empty, observed_at: 2 }); await flush()
    await vi.advanceTimersByTimeAsync(10000)
    expect(api.get).toHaveBeenCalledTimes(3)
    scope.stop()
    await vi.advanceTimersByTimeAsync(20000)
    expect(api.get).toHaveBeenCalledTimes(3)
  })
})

it('pauses when the page is hidden and recovers failed reads on the next poll', async () => {
  vi.useFakeTimers()
  const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  api.get.mockResolvedValue(empty)
  const { states } = setup([{ key_id: 'account', auth_type: 'oauth' }])
  await vi.advanceTimersByTimeAsync(0)
  hidden.mockReturnValue(true)
  document.dispatchEvent(new Event('visibilitychange'))
  await vi.advanceTimersByTimeAsync(20000)
  expect(api.get).toHaveBeenCalledTimes(1)
  api.get.mockRejectedValueOnce(new Error('temporary'))
  hidden.mockReturnValue(false)
  document.dispatchEvent(new Event('visibilitychange'))
  await vi.advanceTimersByTimeAsync(0)
  expect(states.value.account?.read_failed).toBe(true)
  expect(states.value.account?.diagnostics).toEqual(empty)
  await vi.advanceTimersByTimeAsync(10000)
  expect(states.value.account?.read_failed).toBe(false)
})
