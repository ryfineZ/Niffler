import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, type App } from '@/test/vue'
import PoolCapacityBadge from '../PoolCapacityBadge.vue'
import type { PoolCapacity } from '@/api/endpoints/pool'
const mounted: Array<{ app: App; root: HTMLElement }> = []
function mount(component: Parameters<typeof createApp>[0], props: Record<string, unknown>) {
  const root = document.createElement('div'); document.body.append(root)
  const app = createApp(component, props); app.mount(root); mounted.push({ app, root }); return root
}
afterEach(() => { vi.useRealTimers(); for (const { app, root } of mounted.splice(0)) { app.unmount(); root.remove() } })
const capacity: PoolCapacity = { count_24h: 12, models: [{ model: 'model-a', count_24h: 12, last_occurred_at_ms: 1700000000000, last_success_at_ms: null, state: 'pending', cooldown_ttl_seconds: 0, cooldown_expires_at_ms: 0, recent: [] }] }
describe('capacity account visibility', () => {
  it('shows count and pending recovery directly and opens details', async () => {
    const details = vi.fn(), root = mount(PoolCapacityBadge, { capacity, onDetails: details })
    expect(root.textContent).toContain('24小时 12 次'); expect(root.textContent).toContain('待验证')
    root.querySelector('button')!.click(); await nextTick(); expect(details).toHaveBeenCalledOnce()
  })
  it('changes expired cooldown to pending without marking the model recovered', async () => {
    vi.useFakeTimers()
    const now = Date.now()
    const root = mount(PoolCapacityBadge, { capacity: { ...capacity, models: [{ ...capacity.models[0], state: 'cooldown', cooldown_expires_at_ms: now + 2000 }] } })
    expect(root.textContent).toContain('容量冷却中')
    await vi.advanceTimersByTimeAsync(3000)
    await nextTick()
    expect(root.textContent).toContain('待验证')
    expect(root.textContent).not.toContain('已恢复')
  })
})
