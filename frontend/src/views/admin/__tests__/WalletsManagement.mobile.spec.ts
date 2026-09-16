/* eslint-disable vue/one-component-per-file, vue/require-default-prop */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, type App, type Component } from '@/test/vue'

import WalletsManagement from '../WalletsManagement.vue'

const { batch, listAllWallets } = vi.hoisted(() => ({
  listAllWallets: vi.fn().mockResolvedValue([]),
  batch: {
    id: 'batch-mobile',
    name: '移动端回归批次',
    amount_usd: 10,
    currency: 'USD',
    balance_bucket: 'recharge',
    total_count: 1,
    redeemed_count: 0,
    active_count: 1,
    status: 'active',
    description: null,
    created_by: 'admin',
    expires_at: null,
    created_at: '2026-07-15T00:00:00Z',
    updated_at: '2026-07-15T00:00:00Z',
  },
}))

vi.mock('vue-router', () => ({
  useRoute: () => ({ query: { tab: 'redeem_codes' } }),
}))

vi.mock('@/api/admin-wallets', () => ({
  adminWalletApi: {
    listAllWallets,
    listLedger: vi.fn().mockResolvedValue({ items: [], total: 0 }),
    listGlobalRefunds: vi.fn().mockResolvedValue({ items: [], total: 0 }),
  },
}))

vi.mock('@/api/admin-payments', () => ({
  adminPaymentsApi: {
    listOrders: vi.fn().mockResolvedValue({
      items: [{
        id: 'order-mobile',
        order_no: 'po-order-mobile',
        wallet_id: 'wallet-mobile',
        user_id: 'user-mobile',
        owner_type: 'user',
        owner_name: 'Alice Wallet',
        amount_usd: 10,
        debt_repayment_usd: 0,
        pay_amount: null,
        pay_currency: null,
        exchange_rate: null,
        refunded_amount_usd: 0,
        refundable_amount_usd: 10,
        payment_method: 'wechat',
        order_kind: 'wallet_recharge',
        fulfillment_status: null,
        gateway_order_id: null,
        gateway_response: null,
        status: 'credited',
        created_at: '2026-07-15T00:00:00Z',
        paid_at: null,
        credited_at: '2026-07-15T00:00:00Z',
        expires_at: null,
      }],
      total: 1,
    }),
    listCallbacks: vi.fn().mockResolvedValue({ items: [], total: 0 }),
    listRedeemCodeBatches: vi.fn().mockResolvedValue({ items: [batch], total: 1 }),
    listRedeemCodes: vi.fn().mockResolvedValue({
      batch,
      items: [{
        id: 'code-mobile',
        batch_id: batch.id,
        code_prefix: 'ABCD',
        code_suffix: 'MNOP',
        masked_code: 'ABCD-****-****-MNOP',
        status: 'active',
        created_at: '2026-07-15T00:00:00Z',
        updated_at: '2026-07-15T00:00:00Z',
      }],
      total: 1,
    }),
  },
}))

vi.mock('@/composables/useToast', () => ({
  useToast: () => ({ success: vi.fn(), error: vi.fn() }),
}))

vi.mock('@/utils/logger', () => ({
  log: { error: vi.fn() },
}))

vi.mock('lucide-vue-next', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    X: defineComponent({
      name: 'XIconStub',
      setup: (_, { attrs }) => () => h('span', attrs),
    }),
  }
})

vi.mock('@/components/common', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    EmptyState: defineComponent({
      name: 'EmptyStateStub',
      props: { title: String, description: String },
      setup: props => () => h('div', [props.title, props.description]),
    }),
  }
})

vi.mock('@/components/ui', async () => {
  const { defineComponent, h } = await import('vue')

  const elementStub = (name: string, tag = 'div'): Component => defineComponent({
    name,
    inheritAttrs: false,
    setup: (_, { attrs, slots }) => () => h(tag, attrs, slots.default?.()),
  })
  const allSlotsStub = (name: string): Component => defineComponent({
    name,
    inheritAttrs: false,
    setup: (_, { attrs, slots }) => () => h(
      'div',
      attrs,
      Object.values(slots).flatMap(slot => slot?.() ?? [])
    ),
  })

  return {
    Badge: elementStub('BadgeStub', 'span'),
    Button: elementStub('ButtonStub', 'button'),
    Card: elementStub('CardStub'),
    Dialog: allSlotsStub('DialogStub'),
    Input: elementStub('InputStub', 'input'),
    Label: elementStub('LabelStub', 'label'),
    Pagination: elementStub('PaginationStub'),
    RefreshButton: elementStub('RefreshButtonStub', 'button'),
    Select: elementStub('SelectStub'),
    SelectContent: elementStub('SelectContentStub'),
    SelectItem: elementStub('SelectItemStub'),
    SelectTrigger: elementStub('SelectTriggerStub'),
    SelectValue: elementStub('SelectValueStub', 'span'),
    Switch: elementStub('SwitchStub', 'button'),
    Table: defineComponent({
      name: 'TableStub',
      inheritAttrs: false,
      props: { class: [String, Array, Object] },
      setup: (props, { slots }) => () => h('div', { class: 'relative w-full overflow-auto' }, [
        h('table', { class: props.class }, slots.default?.()),
      ]),
    }),
    TableBody: elementStub('TableBodyStub', 'tbody'),
    TableCell: elementStub('TableCellStub', 'td'),
    TableHeader: elementStub('TableHeaderStub', 'thead'),
    TableRow: elementStub('TableRowStub', 'tr'),
    SortableTableHead: elementStub('SortableTableHeadStub', 'th'),
    Tabs: elementStub('TabsStub'),
    TabsContent: elementStub('TabsContentStub'),
    TabsList: elementStub('TabsListStub'),
    TabsTrigger: elementStub('TabsTriggerStub', 'button'),
    Textarea: elementStub('TextareaStub', 'textarea'),
  }
})

const mountedApps: Array<{ app: App, root: HTMLElement }> = []

async function flushView(): Promise<void> {
  await Promise.resolve()
  await nextTick()
  await Promise.resolve()
  await nextTick()
}

beforeEach(() => {
  Object.defineProperty(window, 'innerWidth', {
    value: 375,
    configurable: true,
  })
})

afterEach(() => {
  for (const { app, root } of mountedApps.splice(0)) {
    app.unmount()
    root.remove()
  }
  document.body.innerHTML = ''
})

describe('wallet redeem codes drawer on narrow screens', () => {
  it('keeps all table columns reachable through a horizontal scroll container', async () => {
    const root = document.createElement('div')
    document.body.appendChild(root)
    const app = createApp(WalletsManagement)
    app.mount(root)
    mountedApps.push({ app, root })
    await flushView()

    expect(listAllWallets).not.toHaveBeenCalled()
    expect(root.textContent).toContain('Alice Wallet')

    const viewCodesButton = Array.from(root.querySelectorAll('button'))
      .find(button => button.textContent?.trim() === '查看码')
    expect(viewCodesButton).toBeTruthy()
    viewCodesButton?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await flushView()

    const drawer = document.body.querySelector<HTMLElement>('.drawer-panel')
    const scroller = document.body.querySelector<HTMLElement>('[data-testid="redeem-codes-table-scroll"]')
    const content = document.body.querySelector<HTMLElement>('[data-testid="redeem-codes-table-content"]')
    const table = content?.querySelector('table')

    expect(window.innerWidth).toBe(375)
    expect(drawer?.classList.contains('w-full')).toBe(true)
    expect(scroller?.classList.contains('overflow-x-auto')).toBe(true)
    expect(scroller?.classList.contains('overflow-hidden')).toBe(false)
    expect(content?.classList.contains('min-w-[780px]')).toBe(true)
    expect(table?.classList.contains('min-w-[780px]')).toBe(true)
    expect(table?.textContent).toContain('操作')
  })
})
