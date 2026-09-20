import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, h, nextTick, reactive, type App } from '@/test/vue'
import CodexStateDialog from '../CodexStateDialog.vue'
import type { CodexStateDiagnostics } from '@/api/endpoints/codex-state'

const api = vi.hoisted(() => ({ get: vi.fn() }))
vi.mock('@/api/endpoints/codex-state', () => ({ getCodexStateDiagnostics: api.get }))
vi.mock('@/components/ui', () => ({
  Dialog: defineComponent({ setup(_, { slots }) { return () => h('section', slots.default?.()) } }),
  Badge: defineComponent({ setup(_, { slots }) { return () => h('span', slots.default?.()) } }),
  Button: defineComponent({ setup(_, { slots, attrs }) { return () => h('button', attrs, slots.default?.()) } }),
}))

const active: { app: App; root: HTMLElement }[] = []
function mount() {
  const props = reactive({ open: true, providerId: 'provider', keyId: 'account', keyName: 'Account' })
  const root = document.createElement('div')
  document.body.appendChild(root)
  const app = createApp({ setup: () => () => h(CodexStateDialog, props) })
  app.mount(root)
  active.push({ app, root })
  return { root, props }
}
async function flush() { await new Promise(resolve => setTimeout(resolve, 0)); await nextTick() }
beforeEach(() => api.get.mockReset())
afterEach(() => { for (const { app, root } of active.splice(0)) { app.unmount(); root.remove() } })

describe('CodexStateDialog', () => {
  it('shows loading then an empty observation state without triggering collection', async () => {
    let resolve!: (v: CodexStateDiagnostics) => void
    api.get.mockReturnValue(new Promise(r => { resolve = r }))
    const { root } = mount()
    expect(root.textContent).toContain('读取中')
    expect(root.querySelector('button')?.disabled).toBe(true)
    resolve({ enabled: true, observed_at: 1, items: [] })
    await flush()
    expect(root.textContent).toContain('暂无观测')
    expect(api.get).toHaveBeenCalledWith('provider', 'account', { signal: expect.any(AbortSignal) })
  })

  it('separates usable state from recent passthrough and shows the route', async () => {
    api.get.mockResolvedValue({ enabled: true, observed_at: 1, items: [{
      model: 'gpt-5.6-sol', egress: 'direct', instance: 'app-1', node_id: null,
      status: 'ready', expires_at: 3600, last_seen_at: 1, retry_until: null, auth_status: null, cooldown_seconds: 0,
      last_probe: { at: 1, status: 200, accepted: true, reason: 'accepted' },
      last_use: { at: 1, mode: 'passthrough', http_status: 200 },
    }] })
    const { root } = mount()
    await flush()
    expect(root.textContent).toContain('有合格 state')
    expect(root.textContent).toContain('普通转发，未注入')
    expect(root.textContent).toContain('app-1')
    expect(root.textContent).toContain('不代表生成成功')
  })

  it('supports retry after a read failure and displays the disabled state', async () => {
    api.get.mockRejectedValueOnce(new Error('offline')).mockResolvedValue({ enabled: false, observed_at: 1, items: [] })
    const { root } = mount()
    await flush()
    expect(root.querySelector('[role="alert"]')?.textContent).toContain('请重试')
    root.querySelector('button')?.click()
    await flush()
    expect(root.textContent).toContain('State 功能已关闭')
    expect(root.querySelector('[role="alert"]')).toBeNull()
  })

  it('does not show a late response from a different account', async () => {
    let resolve!: (v: CodexStateDiagnostics) => void
    api.get.mockReturnValueOnce(new Promise(r => { resolve = r })).mockResolvedValue({ enabled: false, observed_at: 1, items: [] })
    const { root, props } = mount()
    props.keyId = 'other-account'
    await flush()
    resolve({ enabled: true, observed_at: 2, items: [] })
    await flush()
    expect(root.textContent).toContain('State 功能已关闭')
  })
})

it('keeps history collapsed and preserves current rows during refresh', async () => {
  const record = {
    model: 'current-model', current: true, id: 'current', egress: 'direct', instance: 'app-1', node_id: null,
    status: 'ready', expires_at: 3600, last_seen_at: 1, retry_until: null, auth_status: null,
    cooldown_seconds: 0, last_probe: null, last_use: null,
  }
  const data = { enabled: true, observed_at: 1, items: [record,
    { ...record, id: 'old', model: 'old-model', current: false, status: 'egress_changed' }] }
  api.get.mockResolvedValueOnce(data)
  const { root, props } = mount()
  await flush()
  expect(root.textContent).toContain('current-model')
  expect(root.textContent).not.toContain('old-model')
  const currentRow = root.querySelector('h3')
  let finish!: (value: unknown) => void
  api.get.mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  root.querySelector('button')?.click()
  await flush()
  expect(root.querySelector('h3')).toBe(currentRow)
  expect(root.textContent).toContain('current-model')
  finish({ ...data, observed_at: 2 }); await flush()
  expect(root.querySelector('h3')).toBe(currentRow)
  root.querySelector<HTMLButtonElement>('button[aria-expanded]')?.click()
  await flush()
  expect(root.textContent).toContain('old-model')
  expect(root.textContent).toContain('出口已更换')
  const signal = api.get.mock.calls.at(-1)?.[2].signal as AbortSignal
  props.open = false; await flush()
  expect(signal.aborted).toBe(true)
})
