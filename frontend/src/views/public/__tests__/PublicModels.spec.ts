import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, type App } from '@/test/vue'

import PublicModels from '../PublicModels.vue'

const getPublicModelGroupCatalog = vi.hoisted(() => vi.fn())

vi.mock('@/api/public-models', () => ({
  getPublicModelGroupCatalog,
}))

vi.mock('@/composables/useClipboard', () => ({
  useClipboard: () => ({
    copyToClipboard: vi.fn(),
  }),
}))

const mountedApps: Array<{ app: App, root: HTMLElement }> = []

async function mountMarketplace() {
  const root = document.createElement('div')
  document.body.appendChild(root)
  const app = createApp(PublicModels)
  app.mount(root)
  mountedApps.push({ app, root })
  await Promise.resolve()
  await nextTick()
  return root
}

function catalogModel(overrides: Record<string, unknown> = {}) {
  return {
    id: 'gm-gpt-51',
    name: 'gpt-5.1',
    display_name: 'GPT 5.1',
    is_active: true,
    default_tiered_pricing: {
      tiers: [{ input_price_per_1m: 10, output_price_per_1m: 20 }],
    },
    default_price_per_request: null,
    supported_capabilities: ['chat'],
    config: null,
    usage_count: 0,
    health: {
      status: 'healthy',
      score: 0.9,
      active_providers: 1,
      active_endpoints: 1,
      providers: ['OpenAI Route'],
    },
    ...overrides,
  }
}

afterEach(() => {
  for (const { app, root } of mountedApps.splice(0)) {
    app.unmount()
    root.remove()
  }
  getPublicModelGroupCatalog.mockReset()
  document.body.innerHTML = ''
})

describe('public model marketplace', () => {
  it('uses real providers and shows the base price below the selected plan price', async () => {
    getPublicModelGroupCatalog.mockResolvedValue({
      groups: [
        {
          id: 'plan-a',
          name: '方案 A',
          sales_multiplier: 0.5,
          model_sales_multipliers: {},
          allowed_models: ['gpt-5.1'],
          allowed_models_mode: 'specific',
          models: [catalogModel()],
        },
        {
          id: 'plan-b',
          name: '方案 B',
          sales_multiplier: 0.8,
          model_sales_multipliers: {},
          allowed_models: ['gpt-5.1'],
          allowed_models_mode: 'specific',
          models: [catalogModel()],
        },
      ],
    })

    const root = await mountMarketplace()
    expect(root.textContent).toContain('OpenAI Route')
    expect(root.textContent).not.toContain('官方')
    expect(root.textContent).toMatch(/US\$5(?:\.00)?|\$5(?:\.00)?/)
    expect(root.textContent).toContain('原价')
    expect(root.textContent).toMatch(/US\$10(?:\.00)?|\$10(?:\.00)?/)

    const planB = Array.from(root.querySelectorAll('button')).find(button =>
      button.textContent?.includes('方案 B'),
    )
    expect(planB).toBeTruthy()
    planB?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await nextTick()

    expect(root.textContent).toMatch(/US\$8(?:\.00)?|\$8(?:\.00)?/)
    expect(root.textContent).toMatch(/US\$10(?:\.00)?|\$10(?:\.00)?/)
  })

  it('uses discount terminology and adjustable pricing for the dedicated portal', async () => {
    getPublicModelGroupCatalog.mockResolvedValue({
      groups: [{
        id: 'partner-plan',
        name: '专属方案',
        discount: 1.2,
        model_discounts: {},
        allowed_models: ['gpt-5.1'],
        allowed_models_mode: 'specific',
        models: [catalogModel()],
      }],
    })

    const root = await mountMarketplace()

    expect(root.textContent).toContain('折扣 1.2')
    expect(root.textContent).not.toContain('×1.2')
    expect(root.textContent).toContain('创建订单时的当前可用汇率')
    expect(root.textContent).not.toContain('1 元人民币 = 1 美元余额')
    expect(root.textContent).toMatch(/US\$12(?:\.00)?|\$12(?:\.00)?/)
  })

  it('classifies and filters models by manufacturer independently of the route provider', async () => {
    getPublicModelGroupCatalog.mockResolvedValue({
      groups: [{
        id: 'plan-all',
        name: '全部模型',
        sales_multiplier: 1,
        model_sales_multipliers: {},
        allowed_models: ['gpt-5.1', 'claude-sonnet-4-6'],
        allowed_models_mode: 'specific',
        models: [
          catalogModel(),
          catalogModel({
            id: 'gm-claude-sonnet',
            name: 'claude-sonnet-4-6',
            display_name: 'Claude Sonnet 4.6',
            health: {
              status: 'healthy',
              score: 0.9,
              active_providers: 1,
              active_endpoints: 1,
              providers: ['Shared Route'],
            },
          }),
        ],
      }],
    })

    const root = await mountMarketplace()
    expect(root.textContent).toContain('OpenAI')
    expect(root.textContent).toContain('Anthropic')

    const anthropic = Array.from(root.querySelectorAll('button')).find(button =>
      button.textContent?.includes('Anthropic'),
    )
    expect(anthropic).toBeTruthy()
    anthropic?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await nextTick()

    const cards = Array.from(root.querySelectorAll('article')).map(card => card.textContent || '')
    expect(cards).toHaveLength(1)
    expect(cards[0]).toContain('Claude Sonnet 4.6')
    expect(cards[0]).not.toContain('GPT 5.1')
  })
})
