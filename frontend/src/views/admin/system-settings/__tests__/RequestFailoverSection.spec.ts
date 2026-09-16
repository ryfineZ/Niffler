import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, type App } from '@/test/vue'
import RequestFailoverSection from '../RequestFailoverSection.vue'
const api = vi.hoisted(() => ({ getSystemConfig: vi.fn(), updateSystemConfig: vi.fn() }))
vi.mock('@/api/admin', () => ({ adminApi: api }))
const apps: Array<{ app: App; root: HTMLElement }> = []
async function flush() { for (let n = 0; n < 5; n++) { await Promise.resolve(); await nextTick() } }
async function mount() {
  const root = document.createElement('div'); document.body.append(root)
  const app = createApp(RequestFailoverSection); app.mount(root); apps.push({ app, root }); await flush(); return root
}
beforeEach(() => { vi.clearAllMocks(); api.getSystemConfig.mockResolvedValue({ value: null }); api.updateSystemConfig.mockResolvedValue({}) })
afterEach(() => { for (const { app, root } of apps.splice(0)) { app.unmount(); root.remove() } })
describe('global request failover settings', () => {
  it('defaults enabled and saves the global switch with bounded defaults', async () => {
    const root = await mount()
    const control = root.querySelector<HTMLButtonElement>('[role="switch"]')!
    expect(control.getAttribute('aria-checked')).toBe('true')
    control.click(); await flush()
    Array.from(root.querySelectorAll('button')).find(b => b.textContent?.trim() === '保存')!.click(); await flush()
    expect(api.updateSystemConfig).toHaveBeenCalledWith('request_failover', { enabled: false, max_account_switches: 2, max_wait_ms: 5000, max_buffer_bytes: 65536, cooldown_seconds: 30 }, '全局失败自动换号')
    expect(root.textContent).toContain('已保存')
  })
  it('keeps invalid input unsaved and surfaces save failures', async () => {
    const root = await mount()
    const input = root.querySelector<HTMLInputElement>('#failover-maxRetries')!
    input.value = '-1'; input.dispatchEvent(new Event('input', { bubbles: true })); await flush()
    expect(root.querySelector('[role="alert"]')?.textContent).toContain('换号次数')
    expect(api.updateSystemConfig).not.toHaveBeenCalled()
    input.value = '3'; input.dispatchEvent(new Event('input', { bubbles: true })); await flush()
    api.updateSystemConfig.mockRejectedValue(new Error('保存失败'))
    Array.from(root.querySelectorAll('button')).find(b => b.textContent?.trim() === '保存')!.click(); await flush()
    expect(root.textContent).toContain('保存失败')
  })
  it('disables editing when configuration cannot be loaded', async () => {
    api.getSystemConfig.mockRejectedValue(new Error('读取失败'))
    const root = await mount()
    expect(root.querySelector<HTMLButtonElement>('[role="switch"]')!.disabled).toBe(true)
    expect(root.querySelector('[role="alert"]')).not.toBeNull()
    expect(root.textContent).toContain('重试')
  })
})
