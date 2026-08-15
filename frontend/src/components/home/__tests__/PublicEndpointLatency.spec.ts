import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, type App } from '@/test/vue'
import { i18n } from '@/i18n'

import PublicEndpointLatency from '../PublicEndpointLatency.vue'

const mountedApps: Array<{ app: App, root: HTMLElement }> = []
const fetchMock = vi.fn()

async function mountLatencyPanel() {
  const root = document.createElement('div')
  document.body.appendChild(root)
  const app = createApp(PublicEndpointLatency)
  app.mount(root)
  mountedApps.push({ app, root })
  await nextTick()
  return root
}

beforeEach(() => {
  i18n.global.locale.value = 'zh-CN'
  fetchMock.mockResolvedValue({ ok: true, status: 204 })
  vi.stubGlobal('fetch', fetchMock)
})

afterEach(() => {
  for (const { app, root } of mountedApps.splice(0)) {
    app.unmount()
    root.remove()
  }
  fetchMock.mockReset()
  vi.unstubAllGlobals()
  document.body.innerHTML = ''
})

describe('PublicEndpointLatency', () => {
  it('measures both public endpoints and allows a manual refresh', async () => {
    const root = await mountLatencyPanel()

    await vi.waitFor(() => {
      expect(root.querySelector('[data-endpoint-latency="us1"]')?.getAttribute('data-status')).toBe('ready')
      expect(root.querySelector('[data-endpoint-latency="us2"]')?.getAttribute('data-status')).toBe('ready')
    })

    expect(root.textContent).toContain('us1.niffler.org')
    expect(root.textContent).toContain('us2.niffler.org')
    expect(fetchMock).toHaveBeenCalledTimes(6)

    root.querySelector<HTMLButtonElement>('[data-refresh-endpoint-latency]')?.click()

    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(12))
  })

  it('shows a clear error when an endpoint cannot be reached', async () => {
    fetchMock.mockRejectedValue(new TypeError('network unavailable'))
    const root = await mountLatencyPanel()

    await vi.waitFor(() => {
      expect(root.querySelector('[data-endpoint-latency="us1"]')?.getAttribute('data-status')).toBe('error')
      expect(root.querySelector('[data-endpoint-latency="us2"]')?.getAttribute('data-status')).toBe('error')
    })

    expect(root.textContent).toContain('无法连接')
  })
})
