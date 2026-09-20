import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, h, nextTick, type App } from '@/test/vue'
import CodexStateOverview from '../CodexStateOverview.vue'
import { summarizeCodexState } from '../codex-state-summary'
import type { CodexStateAccount, CodexStateObservation, CodexStateOverview as Overview } from '@/api/endpoints/codex-state'

const api = vi.hoisted(() => ({ get: vi.fn() }))
vi.mock('@/api/endpoints/codex-state', () => ({ getCodexStateOverview: api.get }))
vi.mock('../CodexStateDialog.vue', () => ({ default: defineComponent({ props: ['keyId'], setup(props) { return () => h('aside', props.keyId) } }) }))
vi.mock('@/components/ui', () => ({
  Card: defineComponent({ setup(_, { slots }) { return () => h('section', slots.default?.()) } }),
  Badge: defineComponent({ setup(_, { slots }) { return () => h('span', slots.default?.()) } }),
  Button: defineComponent({ setup(_, { slots, attrs }) { return () => h('button', attrs, slots.default?.()) } }),
  Input: defineComponent({ props: ['modelValue'], emits: ['update:modelValue'], setup(props, { emit, attrs }) { return () => h('input', { ...attrs, value: props.modelValue, onInput: (e: Event) => emit('update:modelValue', (e.target as HTMLInputElement).value) }) } }),
  Pagination: defineComponent({ emits: ['update:current'], setup(_, { emit }) { return () => h('button', { onClick: () => emit('update:current', 2) }, 'next page') } }),
}))
const active: { app: App; root: HTMLElement }[] = []
function mount() {
  const root = document.createElement('div'); document.body.appendChild(root)
  const app = createApp(CodexStateOverview); app.mount(root); active.push({ app, root })
  return root
}
const observation = (overrides: Partial<CodexStateObservation> = {}): CodexStateObservation => ({
  model: 'gpt-5.6-sol', egress: 'direct', node_id: null, instance: 'app-1', last_seen_at: 10,
  status: 'ready', expires_at: 100, retry_until: null, auth_status: null, cooldown_seconds: 0,
  last_probe: { at: 10, status: 200, accepted: true, reason: 'accepted' },
  last_use: { at: 20, mode: 'passthrough', http_status: 200 }, ...overrides,
})
const account = (items: CodexStateObservation[] = []): CodexStateAccount => ({
  key_id: 'account-1', key_name: 'Alice', provider_id: 'codex', provider_name: 'Codex Pool', active: true,
  read_failed: false, diagnostics: { enabled: true, observed_at: 20, items },
})
const overview = (accounts = [account()], total = accounts.length): Overview => ({ accounts, total, page: 1, page_size: 10 })
async function flush() { await Promise.resolve(); await nextTick(); await Promise.resolve(); await nextTick() }
beforeEach(() => { api.get.mockReset(); vi.useFakeTimers() })
afterEach(() => { for (const { app, root } of active.splice(0)) { app.unmount(); root.remove() }; vi.useRealTimers() })

describe('Codex State account overview', () => {
  it('shows account state and injection separately and opens details directly', async () => {
    api.get.mockResolvedValue(overview([account([observation()])]))
    const root = mount(); await flush()
    expect(root.textContent).toContain('1 / 1 个组合可用')
    expect(root.textContent).toContain('普通转发，未注入')
    expect(root.textContent).toContain('Codex Pool')
    root.querySelector<HTMLButtonElement>('[aria-label="查看 Alice 的 State 明细"]')!.click(); await flush()
    expect(root.querySelector('aside')?.textContent).toBe('account-1')
  })
  it('keeps auth rejection visible when another route is ready and picks events by their own time', () => {
    const result = summarizeCodexState(account([
      observation(), observation({ status: 'auth_rejected', last_probe: { at: 30, status: 401, accepted: false, reason: 'upstream_error' }, last_use: null }),
    ]))
    expect(result.ready).toBe(1); expect(result.total).toBe(2)
    expect(result.warnings).toEqual(['auth_rejected'])
    expect(result.probe?.last_probe?.status).toBe(401)
    expect(result.use?.last_use?.mode).toBe('passthrough')
  })
  it('does not confuse read failures, no observations and a disabled feature', async () => {
    const failed = { ...account(), key_id: 'failed', diagnostics: null, read_failed: true }
    const disabled = { ...account(), key_id: 'off', diagnostics: { enabled: false, observed_at: 20, items: [] } }
    api.get.mockResolvedValue(overview([account(), failed, disabled]))
    const root = mount(); await flush()
    expect(root.textContent).toContain('暂无观测'); expect(root.textContent).toContain('读取失败'); expect(root.textContent).toContain('功能关闭')
  })
  it('shows a retryable error and hides the panel only after a successful empty directory read', async () => {
    api.get.mockRejectedValueOnce(new Error('offline')).mockResolvedValue(overview([]))
    const root = mount(); await flush()
    expect(root.querySelector('[role="alert"]')).not.toBeNull()
    root.querySelector('button')!.click(); await flush()
    expect(root.querySelector('section')).toBeNull()
  })
  it('paginates across providers and debounces search without accepting old responses', async () => {
    let resolve!: (value: Overview) => void
    api.get.mockResolvedValueOnce(overview([account()], 20))
      .mockReturnValueOnce(new Promise(r => { resolve = r }))
      .mockResolvedValue(overview([]))
    const root = mount(); await flush()
    Array.from(root.querySelectorAll('button')).find(button => button.textContent === 'next page')!.click(); await flush()
    expect(api.get).toHaveBeenLastCalledWith({ page: 2, page_size: 10, search: '' })
    const input = root.querySelector('input')!; input.value = 'missing'; input.dispatchEvent(new Event('input')); await flush()
    resolve(overview([account()])); await flush()
    expect(root.textContent).not.toContain('Alice')
    await vi.advanceTimersByTimeAsync(300); await flush()
    expect(api.get).toHaveBeenLastCalledWith({ page: 1, page_size: 10, search: 'missing' })
    expect(root.textContent).toContain('没有匹配')
  })
})
