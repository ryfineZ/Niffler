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

function unmountLatestPanel() {
  const mounted = mountedApps.pop()
  if (!mounted) throw new Error('No mounted latency panel')
  mounted.app.unmount()
  mounted.root.remove()
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
  vi.restoreAllMocks()
  vi.useRealTimers()
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
    expect(fetchMock).toHaveBeenCalledTimes(9)
  })

  it('keeps measuring after one transient sample failure', async () => {
    const attemptsByHost = new Map<string, number>()
    fetchMock.mockImplementation(async (input: RequestInfo | URL) => {
      const host = new URL(String(input)).hostname
      const attempt = (attemptsByHost.get(host) ?? 0) + 1
      attemptsByHost.set(host, attempt)
      if (host === 'us2.niffler.org' && attempt === 1) {
        throw new TypeError('transient network failure')
      }
      return { ok: true, status: 204 }
    })

    const root = await mountLatencyPanel()

    await vi.waitFor(() => {
      expect(root.querySelector('[data-endpoint-latency="us1"]')?.getAttribute('data-status')).toBe('ready')
      expect(root.querySelector('[data-endpoint-latency="us2"]')?.getAttribute('data-status')).toBe('ready')
      expect(root.querySelector('[data-endpoint-latency="cn"]')?.getAttribute('data-status')).toBe('ready')
    })

    expect(attemptsByHost.get('us1.niffler.org')).toBe(3)
    expect(attemptsByHost.get('us2.niffler.org')).toBe(4)
    expect(attemptsByHost.get('cn.niffler.org')).toBe(3)
  })

  it('shows an error when fewer than two samples succeed within the retry budget', async () => {
    const attemptsByHost = new Map<string, number>()
    fetchMock.mockImplementation(async (input: RequestInfo | URL) => {
      const host = new URL(String(input)).hostname
      const attempt = (attemptsByHost.get(host) ?? 0) + 1
      attemptsByHost.set(host, attempt)
      if (host === 'us2.niffler.org' && attempt > 1) {
        throw new TypeError('network unavailable')
      }
      return { ok: true, status: 204 }
    })

    const root = await mountLatencyPanel()

    await vi.waitFor(() => {
      expect(root.querySelector('[data-endpoint-latency="us1"]')?.getAttribute('data-status')).toBe('ready')
      expect(root.querySelector('[data-endpoint-latency="us2"]')?.getAttribute('data-status')).toBe('error')
      expect(root.querySelector('[data-endpoint-latency="cn"]')?.getAttribute('data-status')).toBe('ready')
    })

    expect(attemptsByHost.get('us2.niffler.org')).toBe(4)
  })

  it('shows a result when exactly two of four samples succeed', async () => {
    vi.useFakeTimers()
    vi.spyOn(document, 'readyState', 'get').mockReturnValue('complete')
    const attemptsByHost = new Map<string, number>()
    fetchMock.mockImplementation((input: RequestInfo | URL) => {
      const host = new URL(String(input)).hostname
      const attempt = (attemptsByHost.get(host) ?? 0) + 1
      attemptsByHost.set(host, attempt)
      if (host === 'us2.niffler.org' && (attempt === 2 || attempt === 4)) {
        return Promise.reject(new TypeError('transient network failure'))
      }
      const delay = host === 'us2.niffler.org' ? (attempt === 1 ? 100 : 300) : 0
      return new Promise(resolve => window.setTimeout(() => resolve({ ok: true, status: 204 }), delay))
    })

    const root = await mountLatencyPanel()
    await vi.advanceTimersByTimeAsync(1000)

    expect(root.querySelector('[data-endpoint-latency="us2"]')?.getAttribute('data-status')).toBe('ready')
    expect(root.querySelector('[data-endpoint-latency="us2"]')?.textContent).toContain('200 ms')
    expect(attemptsByHost.get('us2.niffler.org')).toBe(4)
  })

  it('rejects a non-204 response even when it is otherwise successful', async () => {
    fetchMock.mockResolvedValue({ ok: true, status: 200 })
    const root = await mountLatencyPanel()

    await vi.waitFor(() => {
      expect(root.querySelector('[data-endpoint-latency="us1"]')?.getAttribute('data-status')).toBe('error')
      expect(root.querySelector('[data-endpoint-latency="us2"]')?.getAttribute('data-status')).toBe('error')
      expect(root.querySelector('[data-endpoint-latency="cn"]')?.getAttribute('data-status')).toBe('error')
    })
  })

  it('defers the initial measurement until after the page load settles', async () => {
    vi.useFakeTimers()
    vi.spyOn(document, 'readyState', 'get').mockReturnValue('complete')

    await mountLatencyPanel()

    expect(fetchMock).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(1000)

    expect(fetchMock).toHaveBeenCalledTimes(9)
  })

  it('waits for the load event and starts only one initial measurement', async () => {
    vi.useFakeTimers()
    vi.spyOn(document, 'readyState', 'get').mockReturnValue('loading')

    await mountLatencyPanel()

    expect(fetchMock).not.toHaveBeenCalled()
    window.dispatchEvent(new Event('load'))
    window.dispatchEvent(new Event('load'))
    await vi.advanceTimersByTimeAsync(249)
    expect(fetchMock).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(1)
    expect(fetchMock).toHaveBeenCalledTimes(9)
  })

  it('starts from the fallback timer when the load event never arrives', async () => {
    vi.useFakeTimers()
    vi.spyOn(document, 'readyState', 'get').mockReturnValue('loading')

    await mountLatencyPanel()
    await vi.advanceTimersByTimeAsync(1499)
    expect(fetchMock).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(1)
    expect(fetchMock).toHaveBeenCalledTimes(9)
  })

  it('uses the fallback deadline when load arrives just before it', async () => {
    vi.useFakeTimers()
    vi.spyOn(document, 'readyState', 'get').mockReturnValue('loading')

    await mountLatencyPanel()
    await vi.advanceTimersByTimeAsync(1490)
    window.dispatchEvent(new Event('load'))
    await vi.advanceTimersByTimeAsync(10)
    expect(fetchMock).toHaveBeenCalledTimes(9)

    await vi.advanceTimersByTimeAsync(1000)
    expect(fetchMock).toHaveBeenCalledTimes(9)
  })

  it('does not start the scheduled measurement after unmounting', async () => {
    vi.useFakeTimers()
    vi.spyOn(document, 'readyState', 'get').mockReturnValue('loading')

    await mountLatencyPanel()
    unmountLatestPanel()
    window.dispatchEvent(new Event('load'))
    await vi.advanceTimersByTimeAsync(2000)

    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('clears the post-load delay when unmounted before it expires', async () => {
    vi.useFakeTimers()
    vi.spyOn(document, 'readyState', 'get').mockReturnValue('loading')

    await mountLatencyPanel()
    window.dispatchEvent(new Event('load'))
    await vi.advanceTimersByTimeAsync(100)
    unmountLatestPanel()
    await vi.advanceTimersByTimeAsync(1000)

    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('aborts active probes on unmount without starting retries', async () => {
    vi.useFakeTimers()
    vi.spyOn(document, 'readyState', 'get').mockReturnValue('complete')
    const signals: AbortSignal[] = []

    fetchMock.mockImplementation((_input: RequestInfo | URL, init?: RequestInit) => {
      const signal = init?.signal
      if (!signal) throw new Error('Missing probe abort signal')
      signals.push(signal)
      return new Promise((_resolve, reject) => {
        signal.addEventListener('abort', () => {
          reject(new DOMException('The operation was aborted', 'AbortError'))
        }, { once: true })
      })
    })

    await mountLatencyPanel()
    await vi.advanceTimersByTimeAsync(250)
    expect(signals).toHaveLength(3)

    unmountLatestPanel()
    expect(signals.every(signal => signal.aborted)).toBe(true)

    await vi.advanceTimersByTimeAsync(20000)
    expect(fetchMock).toHaveBeenCalledTimes(3)
  })

  it('cancels a stale run and prevents its late result from overwriting the new run', async () => {
    vi.useFakeTimers()
    vi.spyOn(document, 'readyState', 'get').mockReturnValue('complete')
    const staleResolvers: Array<(value: { ok: boolean, status: number }) => void> = []
    const staleSignals: AbortSignal[] = []

    fetchMock.mockImplementation((_input: RequestInfo | URL, init?: RequestInit) => {
      if (staleResolvers.length < 3) {
        if (!init?.signal) throw new Error('Missing probe abort signal')
        staleSignals.push(init.signal)
        return new Promise(resolve => staleResolvers.push(resolve))
      }
      return Promise.resolve({ ok: true, status: 204 })
    })

    const root = await mountLatencyPanel()
    await vi.advanceTimersByTimeAsync(250)
    expect(fetchMock).toHaveBeenCalledTimes(3)

    root.querySelector<HTMLButtonElement>('[data-refresh-endpoint-latency]')?.dispatchEvent(
      new MouseEvent('click', { bubbles: true }),
    )
    await vi.advanceTimersByTimeAsync(1)

    expect(staleSignals.every(signal => signal.aborted)).toBe(true)
    expect(fetchMock).toHaveBeenCalledTimes(12)
    expect(root.querySelector('[data-endpoint-latency="us1"]')?.getAttribute('data-status')).toBe('ready')
    expect(root.querySelector('[data-endpoint-latency="us2"]')?.getAttribute('data-status')).toBe('ready')
    expect(root.querySelector('[data-endpoint-latency="cn"]')?.getAttribute('data-status')).toBe('ready')
    const settledText = root.textContent

    for (const resolve of staleResolvers) resolve({ ok: true, status: 204 })
    await vi.advanceTimersByTimeAsync(1)

    expect(fetchMock).toHaveBeenCalledTimes(12)
    expect(root.textContent).toBe(settledText)
  })

  it('recovers when the first cold probe for each endpoint times out', async () => {
    vi.useFakeTimers()
    vi.spyOn(document, 'readyState', 'get').mockReturnValue('complete')
    const attemptsByHost = new Map<string, number>()

    fetchMock.mockImplementation((input: RequestInfo | URL, init?: RequestInit) => {
      const host = new URL(String(input)).hostname
      const attempt = (attemptsByHost.get(host) ?? 0) + 1
      attemptsByHost.set(host, attempt)
      if (attempt > 1) return Promise.resolve({ ok: true, status: 204 })

      return new Promise((_resolve, reject) => {
        init?.signal?.addEventListener('abort', () => {
          reject(new DOMException('The operation was aborted', 'AbortError'))
        }, { once: true })
      })
    })

    const root = await mountLatencyPanel()
    await vi.advanceTimersByTimeAsync(6000)

    expect(root.querySelector('[data-endpoint-latency="us1"]')?.getAttribute('data-status')).toBe('ready')
    expect(root.querySelector('[data-endpoint-latency="us2"]')?.getAttribute('data-status')).toBe('ready')
    expect(root.querySelector('[data-endpoint-latency="cn"]')?.getAttribute('data-status')).toBe('ready')
    expect(attemptsByHost.get('us1.niffler.org')).toBe(4)
    expect(attemptsByHost.get('us2.niffler.org')).toBe(4)
    expect(attemptsByHost.get('cn.niffler.org')).toBe(4)
  })
})
