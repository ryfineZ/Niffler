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
  it('shows all three public domains without exposing infrastructure names', async () => {
    const root = await mountLatencyPanel()

    await vi.waitFor(() => {
      expect(root.querySelector('[data-endpoint-latency="us1"]')?.getAttribute('data-status')).toBe('ready')
      expect(root.querySelector('[data-endpoint-latency="us2"]')?.getAttribute('data-status')).toBe('ready')
      expect(root.querySelector('[data-endpoint-latency="cn"]')?.getAttribute('data-status')).toBe('ready')
    })

    expect(root.textContent).toContain('美西线路 1')
    expect(root.textContent).toContain('美西线路 2')
    expect(root.textContent).toContain('三网优化')
    expect(root.textContent).toContain('us1.niffler.org')
    expect(root.textContent).toContain('us2.niffler.org')
    expect(root.textContent).toContain('cn.niffler.org')
    expect(root.textContent).not.toContain('OVH')
    expect(root.textContent).not.toContain('hd0526')
    expect(root.textContent).not.toContain('DMIT')
    expect(fetchMock).toHaveBeenCalledTimes(9)
    expect(fetchMock.mock.calls.map(([url]) => String(url))).toEqual(expect.arrayContaining([
      expect.stringContaining('https://us1.niffler.org/__niffler_latency'),
      expect.stringContaining('https://us2.niffler.org/__niffler_latency'),
      expect.stringContaining('https://cn.niffler.org/__niffler_latency'),
    ]))

    root.querySelector<HTMLButtonElement>('[data-refresh-endpoint-latency]')?.click()

    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(18))
  })

  it('shows a clear error when an endpoint cannot be reached', async () => {
    fetchMock.mockRejectedValue(new TypeError('network unavailable'))
    const root = await mountLatencyPanel()

    await vi.waitFor(() => {
      expect(root.querySelector('[data-endpoint-latency="us1"]')?.getAttribute('data-status')).toBe('error')
      expect(root.querySelector('[data-endpoint-latency="us2"]')?.getAttribute('data-status')).toBe('error')
      expect(root.querySelector('[data-endpoint-latency="cn"]')?.getAttribute('data-status')).toBe('error')
    })

    expect(root.textContent).toContain('无法连接')
  })
})
