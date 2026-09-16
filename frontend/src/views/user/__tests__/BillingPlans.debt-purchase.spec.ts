/* eslint-disable vue/one-component-per-file, vue/require-default-prop */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, h, nextTick, type App } from '@/test/vue'

import BillingPlans from '../BillingPlans.vue'

const apiMocks = vi.hoisted(() => ({
  listPlans: vi.fn(),
  listEntitlements: vi.fn(),
  checkout: vi.fn(),
  getBalance: vi.fn(),
  listRechargeOptions: vi.fn(),
}))

vi.mock('@/api/billing', () => ({
  billingApi: {
    listPlans: apiMocks.listPlans,
    listEntitlements: apiMocks.listEntitlements,
    checkout: apiMocks.checkout,
  },
}))

vi.mock('@/api/wallet', () => ({
  walletApi: {
    getBalance: apiMocks.getBalance,
    listRechargeOptions: apiMocks.listRechargeOptions,
  },
}))

vi.mock('@/composables/useToast', () => ({
  useToast: () => ({ success: vi.fn(), error: vi.fn() }),
}))

vi.mock('@/utils/logger', () => ({
  log: { error: vi.fn() },
}))

vi.mock('lucide-vue-next', () => ({
  CreditCard: defineComponent({ name: 'CreditCardStub', setup: () => () => h('span') }),
}))

vi.mock('@/components/ui', () => {
  const passthrough = (name: string, tag = 'div') => defineComponent({
    name,
    setup(_, { attrs, slots }) {
      return () => h(tag, attrs, slots.default?.())
    },
  })
  return {
    Badge: passthrough('BadgeStub', 'span'),
    Button: defineComponent({
      name: 'ButtonStub',
      props: { disabled: Boolean, title: String },
      setup(props, { attrs, slots }) {
        return () => h('button', { ...attrs, disabled: props.disabled, title: props.title }, slots.default?.())
      },
    }),
    Card: passthrough('CardStub'),
    Select: passthrough('SelectStub'),
    SelectContent: passthrough('SelectContentStub'),
    SelectItem: passthrough('SelectItemStub'),
    SelectTrigger: passthrough('SelectTriggerStub'),
    SelectValue: passthrough('SelectValueStub', 'span'),
  }
})

vi.mock('@/components/layout', () => {
  const passthrough = (name: string, tag = 'div') => defineComponent({
    name,
    setup(_, { attrs, slots }) {
      return () => h(tag, attrs, slots.default?.())
    },
  })
  return {
    CardSection: passthrough('CardSectionStub', 'section'),
    PageContainer: passthrough('PageContainerStub', 'main'),
    PageHeader: defineComponent({
      name: 'PageHeaderStub',
      props: { title: String },
      setup(props) {
        return () => h('h1', props.title)
      },
    }),
  }
})

vi.mock('@/components/common', () => {
  const passthrough = (name: string) => defineComponent({
    name,
    setup(_, { attrs, slots }) {
      return () => h('div', attrs, slots.default?.())
    },
  })
  return {
    EmptyState: passthrough('EmptyStateStub'),
    LoadingState: passthrough('LoadingStateStub'),
  }
})

const mountedApps: Array<{ app: App, root: HTMLElement }> = []
let submitPaymentForm: ReturnType<typeof vi.spyOn>

async function settle() {
  for (let index = 0; index < 8; index += 1) {
    await Promise.resolve()
    await nextTick()
  }
}

function findButton(root: HTMLElement, text: string): HTMLButtonElement {
  const button = Array.from(root.querySelectorAll('button'))
    .find(item => item.textContent?.includes(text))
  if (!button) throw new Error(`button not found: ${text}`)
  return button
}

beforeEach(() => {
  apiMocks.listPlans.mockResolvedValue({
    items: [{
      id: 'plan-1',
      title: '标准套餐',
      description: '每月额度',
      price_amount: 1,
      price_currency: 'USD',
      duration_unit: 'month',
      duration_value: 1,
      enabled: true,
      sort_order: 0,
      max_active_per_user: 1,
      purchase_limit_scope: 'active_period',
      allowed_provider_ids: ['provider-1'],
      entitlements: [{ type: 'daily_quota', daily_quota_usd: 10 }],
    }],
    total: 1,
  })
  apiMocks.listEntitlements.mockResolvedValue({ items: [], total: 0 })
  apiMocks.getBalance.mockResolvedValue({
    actual_wallet_balance: -3,
    wallet_balance: -3,
    balance: -3,
    debt_usd: 3,
    billing_state: 'in_debt',
  })
  apiMocks.listRechargeOptions.mockResolvedValue({
    items: [{
      payment_method: 'epay',
      payment_provider: 'epay',
      payment_channel: 'alipay',
      display_name: '支付宝',
    }],
  })
  apiMocks.checkout.mockResolvedValue({
    order: {
      id: 'order-1',
      order_no: 'pp-1',
      wallet_id: 'wallet-1',
      user_id: 'user-1',
      amount_usd: 4,
      plan_amount_usd: 1,
      debt_repayment_usd: 3,
      pay_amount: 28.8,
      pay_currency: 'CNY',
      exchange_rate: 7.2,
      refunded_amount_usd: 0,
      refundable_amount_usd: 0,
      payment_method: 'epay',
      payment_provider: 'epay',
      payment_channel: 'alipay',
      order_kind: 'plan_purchase',
      product_id: 'plan-1',
      gateway_order_id: 'gateway-1',
      gateway_response: {},
      status: 'pending',
      created_at: '2026-08-10T00:00:00Z',
      paid_at: null,
      credited_at: null,
      expires_at: '2026-08-10T00:30:00Z',
    },
    payment_instructions: {
      payment_url: 'https://pay.example.test/checkout',
      payment_params: { order_no: 'pp-1' },
    },
  })
  submitPaymentForm = vi.spyOn(HTMLFormElement.prototype, 'submit').mockImplementation(() => {})
})

afterEach(() => {
  for (const { app, root } of mountedApps.splice(0)) {
    app.unmount()
    root.remove()
  }
  submitPaymentForm.mockRestore()
  vi.clearAllMocks()
  document.body.innerHTML = ''
})

describe('BillingPlans debt purchase', () => {
  it('shows remaining daily quota and the server-provided refresh time', async () => {
    apiMocks.listEntitlements.mockResolvedValueOnce({
      items: [{
        id: 'entitlement-1',
        user_id: 'user-1',
        plan_id: 'plan-1',
        payment_order_id: 'order-plan-1',
        status: 'active',
        starts_at: '2026-08-15T05:35:29Z',
        expires_at: '2026-09-15T05:35:29Z',
        allowed_provider_ids: ['provider-1'],
        entitlements: [{ type: 'daily_quota', daily_quota_usd: 10 }],
        active: true,
        created_at: '2026-08-15T05:35:29Z',
      }],
      total: 1,
      quota_summary: {
        user_id: 'user-1',
        entitlement_id: 'entitlement-1',
        plan_id: 'plan-1',
        plan_title: '标准套餐',
        starts_at: '2026-08-15T05:35:29Z',
        expires_at: '2026-09-15T05:35:29Z',
        quota_total_usd: 10,
        quota_used_usd: 3.5,
        quota_remaining_usd: 6.5,
        daily_total_usd: 10,
        daily_used_usd: 3.5,
        daily_remaining_usd: 6.5,
        daily_window_started_at: '2026-08-15T05:35:29Z',
        daily_window_ends_at: '2026-08-16T05:35:29Z',
      },
      quota_summary_status: 'ok',
    })
    const root = document.createElement('div')
    document.body.appendChild(root)
    const app = createApp(BillingPlans)
    app.mount(root)
    mountedApps.push({ app, root })
    await settle()

    expect(root.textContent).toContain('今日剩余')
    expect(root.textContent).toContain('$6.50 / $10.00')
    expect(root.textContent).toContain('今日已用')
    expect(root.textContent).toContain('$3.50')
    expect(root.textContent).toContain('下次刷新')
  })

  it('keeps daily quota separate from a tighter overall quota and uses the server plan title', async () => {
    apiMocks.listEntitlements.mockResolvedValueOnce({
      items: [{
        id: 'entitlement-retired',
        user_id: 'user-1',
        plan_id: 'retired-plan',
        payment_order_id: 'order-retired',
        status: 'active',
        starts_at: '2026-08-15T05:35:29Z',
        expires_at: '2026-09-15T05:35:29Z',
        allowed_provider_ids: ['provider-1'],
        entitlements: [{ type: 'daily_quota', daily_quota_usd: 80 }],
        active: true,
        created_at: '2026-08-15T05:35:29Z',
      }],
      total: 1,
      quota_summary: {
        user_id: 'user-1',
        entitlement_id: 'entitlement-retired',
        plan_id: 'retired-plan',
        plan_title: '已下架套餐',
        starts_at: '2026-08-15T05:35:29Z',
        expires_at: '2026-09-15T05:35:29Z',
        quota_total_usd: 80,
        quota_used_usd: 80,
        quota_remaining_usd: 0,
        daily_total_usd: 80,
        daily_used_usd: 10,
        daily_remaining_usd: 70,
        daily_window_started_at: '2026-08-15T05:35:29Z',
        daily_window_ends_at: '2026-08-16T05:35:29Z',
      },
      quota_summary_status: 'ok',
    })
    const root = document.createElement('div')
    document.body.appendChild(root)
    const app = createApp(BillingPlans)
    app.mount(root)
    mountedApps.push({ app, root })
    await settle()

    expect(root.textContent).toContain('已下架套餐')
    expect(root.textContent).toContain('$70.00 / $80.00')
    expect(root.textContent).toContain('今日已用')
    expect(root.textContent).toContain('$10.00')
    expect(root.textContent).toContain('当前剩余')
    expect(root.textContent).toContain('$0.00')
  })

  it('keeps the active plan visible when its quota summary is temporarily unavailable', async () => {
    apiMocks.listEntitlements.mockResolvedValueOnce({
      items: [{
        id: 'entitlement-1',
        user_id: 'user-1',
        plan_id: 'plan-1',
        payment_order_id: 'order-plan-1',
        status: 'active',
        starts_at: '2026-08-15T05:35:29Z',
        expires_at: '2026-09-15T05:35:29Z',
        allowed_provider_ids: ['provider-1'],
        entitlements: [{ type: 'daily_quota', daily_quota_usd: 10 }],
        active: true,
        created_at: '2026-08-15T05:35:29Z',
      }],
      total: 1,
      quota_summary: null,
      quota_summary_status: 'unavailable',
    })
    const root = document.createElement('div')
    document.body.appendChild(root)
    const app = createApp(BillingPlans)
    app.mount(root)
    mountedApps.push({ app, root })
    await settle()

    expect(root.textContent).toContain('标准套餐')
    expect(root.textContent).toContain('额度暂时无法读取')
    expect(root.textContent).not.toContain('今日剩余')
  })

  it('shows the exact breakdown before opening payment for a wallet in debt', async () => {
    const root = document.createElement('div')
    document.body.appendChild(root)
    const app = createApp(BillingPlans)
    app.mount(root)
    mountedApps.push({ app, root })
    await settle()

    const buyButton = root.querySelector('button')
    if (!buyButton) throw new Error('plan purchase button not found')
    expect(buyButton.textContent).toContain('购买套餐')
    expect(buyButton.disabled).toBe(false)
    buyButton.click()
    await settle()

    expect(apiMocks.checkout).toHaveBeenCalledOnce()
    expect(submitPaymentForm).not.toHaveBeenCalled()
    expect(root.textContent).toContain('套餐价格')
    expect(root.textContent).toContain('钱包欠款')
    expect(root.textContent).toContain('本次实付')
    expect(root.textContent).toContain('$4.00')

    findButton(root, '前往付款').click()
    expect(submitPaymentForm).toHaveBeenCalledOnce()
  })

  it('keeps opening payment immediately when the order has no wallet debt', async () => {
    apiMocks.getBalance.mockResolvedValueOnce({
      actual_wallet_balance: 5,
      wallet_balance: 5,
      balance: 5,
      debt_usd: 0,
      billing_state: 'available',
    })
    apiMocks.checkout.mockResolvedValueOnce({
      order: {
        order_no: 'pp-2',
        amount_usd: 1,
        plan_amount_usd: 1,
        debt_repayment_usd: 0,
        pay_amount: 7.2,
        pay_currency: 'CNY',
      },
      payment_instructions: {
        payment_url: 'https://pay.example.test/checkout',
        payment_params: { order_no: 'pp-2' },
      },
    })
    const root = document.createElement('div')
    document.body.appendChild(root)
    const app = createApp(BillingPlans)
    app.mount(root)
    mountedApps.push({ app, root })
    await settle()

    findButton(root, '购买套餐').click()
    await settle()

    expect(apiMocks.checkout).toHaveBeenCalledOnce()
    expect(submitPaymentForm).toHaveBeenCalledOnce()
  })
})
