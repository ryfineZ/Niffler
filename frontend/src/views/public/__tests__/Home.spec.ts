import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, type App } from '@/test/vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import { i18n } from '@/i18n'

import Home from '../Home.vue'

const getPublicGlobalModels = vi.hoisted(() => vi.fn())
const authStore = vi.hoisted(() => ({
  isAuthenticated: false,
  canAccessAdmin: false,
}))
const sitePortal = vi.hoisted(() => ({
  value: { id: 'default' } as { id: string } | null,
}))

vi.mock('@/api/public-models', () => ({
  getPublicGlobalModels,
}))

vi.mock('@/stores/auth', () => ({
  useAuthStore: () => authStore,
}))

vi.mock('@/composables/useSiteInfo', () => ({
  useSiteInfo: () => ({ portal: sitePortal }),
}))

const mountedApps: Array<{ app: App, root: HTMLElement }> = []

async function mountHome() {
  const root = document.createElement('div')
  document.body.appendChild(root)
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: Home },
      { path: '/models', component: { template: '<div />' } },
      { path: '/guide/faq', component: { template: '<div />' } },
      { path: '/dashboard', component: { template: '<div />' } },
      { path: '/dashboard/image-studio', component: { template: '<div />' } },
    ],
  })
  await router.push('/')
  await router.isReady()

  const app = createApp(Home)
  app.use(router)
  app.mount(root)
  mountedApps.push({ app, root })
  await Promise.resolve()
  await nextTick()
  return root
}

beforeEach(() => {
  window.localStorage.clear()
  authStore.isAuthenticated = false
  authStore.canAccessAdmin = false
  sitePortal.value = { id: 'default' }
  getPublicGlobalModels.mockResolvedValue({ models: [], total: 0 })
  i18n.global.locale.value = 'zh-CN'
})

afterEach(() => {
  for (const { app, root } of mountedApps.splice(0)) {
    app.unmount()
    root.remove()
  }
  getPublicGlobalModels.mockReset()
  vi.unstubAllGlobals()
  document.body.innerHTML = ''
})

describe('home quick start and FAQ', () => {
  it('shows endpoint latency only on the default portal', async () => {
    const root = await mountHome()

    expect(root.querySelector('[data-endpoint-latency]')).not.toBeNull()
  })

  it('does not expose or request main-site endpoints on the official USD portal', async () => {
    sitePortal.value = { id: 'official_usd' }
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    const root = await mountHome()
    await new Promise(resolve => window.setTimeout(resolve, 300))

    expect(root.querySelector('[data-endpoint-latency]')).toBeNull()
    expect(root.textContent).not.toContain('niffler.org')
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('renders four cinematic scenes with one active indicator', async () => {
    const root = await mountHome()

    const indicators = Array.from(root.querySelectorAll<HTMLButtonElement>('.scene-indicator-button'))
    expect(indicators).toHaveLength(4)
    expect(indicators.filter(button => button.getAttribute('aria-current') === 'step')).toHaveLength(1)
    expect(root.querySelector('[data-active-scene="hero"]')).not.toBeNull()
    expect(root.querySelectorAll('.home-scene')).toHaveLength(4)
  })

  it('scrolls to a scene when its indicator is activated', async () => {
    const root = await mountHome()
    const target = root.querySelectorAll<HTMLElement>('.home-scene')[2]
    const scrollIntoView = vi.fn()
    target.scrollIntoView = scrollIntoView

    root.querySelectorAll<HTMLButtonElement>('.scene-indicator-button')[2]?.click()

    expect(scrollIntoView).toHaveBeenCalledWith({ behavior: 'smooth', block: 'start' })
  })

  it('keeps all scene content visible when reduced motion is preferred', async () => {
    const originalMatchMedia = window.matchMedia
    Object.defineProperty(window, 'matchMedia', {
      configurable: true,
      value: (query: string) => ({
        matches: query.includes('prefers-reduced-motion'),
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(),
      }),
    })

    try {
      const root = await mountHome()
      expect(root.classList.contains('motion-ready')).toBe(false)
      expect(root.querySelectorAll('.home-scene [data-motion]').length).toBeGreaterThan(0)
    } finally {
      if (originalMatchMedia) {
        Object.defineProperty(window, 'matchMedia', { configurable: true, value: originalMatchMedia })
      } else {
        Reflect.deleteProperty(window, 'matchMedia')
      }
    }
  })

  it('keeps the later scenes populated when the model API is empty', async () => {
    const root = await mountHome()

    expect(root.querySelectorAll('.tool-flow-step')).toHaveLength(3)
    expect(root.querySelectorAll('.model-chip')).toHaveLength(5)
    expect(root.querySelectorAll('.model-benefit')).toHaveLength(4)
    expect(root.querySelector('.model-benefit')?.textContent).toContain('一个密钥，所有模型')
    expect(root.querySelector('.home-scene-cta')).toBeNull()
    expect(root.querySelector('.quick-panel')?.textContent).toContain('充值、拿 Key、直接用')
    expect(root.textContent).toContain('DeepSeek')
  })

  it('keeps quick start closed by default and opens it only on request', async () => {
    const root = await mountHome()

    expect(root.querySelector('[role="dialog"]')).toBeNull()

    const openButton = Array.from(root.querySelectorAll('button')).find(button => button.textContent?.includes('怎么开始'))
    openButton?.click()
    await nextTick()

    expect(root.querySelector('[role="dialog"]')).not.toBeNull()

    root.querySelector<HTMLButtonElement>('[aria-label="关闭新手引导"]')?.click()
    await nextTick()
    await new Promise(resolve => window.setTimeout(resolve, 200))

    expect(root.querySelector('[role="dialog"]')).toBeNull()
  })

  it('lets the quick start launcher move without opening the dialog', async () => {
    const root = await mountHome()
    const launcher = Array.from(root.querySelectorAll<HTMLButtonElement>('button')).find(button => button.textContent?.includes('怎么开始'))
    expect(launcher).toBeDefined()
    if (!launcher) throw new Error('Quick start launcher not found')

    Object.defineProperties(launcher, {
      offsetWidth: { configurable: true, value: 128 },
      offsetHeight: { configurable: true, value: 44 },
      getBoundingClientRect: {
        configurable: true,
        value: () => ({ left: 820, top: 680, right: 948, bottom: 724, width: 128, height: 44, x: 820, y: 680, toJSON: () => ({}) }),
      },
    })

    const dispatchPointer = (type: string, clientX: number, clientY: number) => {
      const event = new Event(type, { bubbles: true, cancelable: true })
      Object.defineProperties(event, {
        pointerId: { value: 1 },
        button: { value: 0 },
        clientX: { value: clientX },
        clientY: { value: clientY },
      })
      launcher.dispatchEvent(event)
    }

    dispatchPointer('pointerdown', 850, 700)
    dispatchPointer('pointermove', 650, 540)
    dispatchPointer('pointerup', 650, 540)
    await nextTick()

    expect(launcher.style.left).toBe('620px')
    expect(launcher.style.top).toBe('520px')

    launcher.click()
    await nextTick()
    expect(root.querySelector('[role="dialog"]')).toBeNull()

    launcher.click()
    await nextTick()
    expect(root.querySelector('[role="dialog"]')).not.toBeNull()
  })

  it('keeps FAQ answers collapsed until a question is selected', async () => {
    const root = await mountHome()
    const question = Array.from(root.querySelectorAll('button')).find(button => button.textContent?.includes('和直接使用官方服务有什么区别'))

    expect(root.textContent).not.toContain('多个模型共用一份余额')
    question?.click()
    await nextTick()

    expect(root.textContent).toContain('多个模型共用一份余额')
  })
})
