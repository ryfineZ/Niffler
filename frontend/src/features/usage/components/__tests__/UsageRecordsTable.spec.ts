import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, h, type App } from '@/test/vue'
import UsageRecordsTable from '../UsageRecordsTable.vue'
import type { UsageRecord } from '../../types'

vi.mock('@/components/ui', async () => {
  const { defineComponent, h } = await import('vue')

  const passthrough = (name: string, tag = 'div') => defineComponent({
    name,
    setup(_, { slots }) {
      return () => h(tag, [
        slots.default?.(),
        slots.actions?.(),
        slots.pagination?.(),
        slots.filter?.({ close: () => undefined }),
      ])
    },
  })

  return {
    TableCard: passthrough('TableCardStub', 'section'),
    Badge: passthrough('BadgeStub', 'span'),
    Button: passthrough('ButtonStub', 'button'),
    Input: defineComponent({
      name: 'InputStub',
      props: { modelValue: String },
      emits: ['update:modelValue'],
      setup(props, { attrs, emit }) {
        return () => h('input', {
          ...attrs,
          value: props.modelValue ?? '',
          onInput: (event: Event) => emit('update:modelValue', (event.target as HTMLInputElement).value),
        })
      },
    }),
    Select: passthrough('SelectStub'),
    SelectTrigger: passthrough('SelectTriggerStub'),
    SelectValue: passthrough('SelectValueStub', 'span'),
    SelectContent: passthrough('SelectContentStub'),
    SelectItem: passthrough('SelectItemStub'),
    Table: passthrough('TableStub', 'table'),
    TableHeader: passthrough('TableHeaderStub', 'thead'),
    TableBody: passthrough('TableBodyStub', 'tbody'),
    TableRow: passthrough('TableRowStub', 'tr'),
    TableHead: passthrough('TableHeadStub', 'th'),
    TableCell: passthrough('TableCellStub', 'td'),
    Pagination: passthrough('PaginationStub'),
    SortableTableHead: passthrough('SortableTableHeadStub', 'th'),
    TableFilterMenu: passthrough('TableFilterMenuStub'),
  }
})

vi.mock('@/components/common', async () => {
  const { defineComponent, h } = await import('vue')

  return {
    MultiSelect: defineComponent({
      name: 'MultiSelectStub',
      setup() {
        return () => h('div')
      },
    }),
    TimeRangePicker: defineComponent({
      name: 'TimeRangePickerStub',
      setup() {
        return () => h('div')
      },
    }),
  }
})

vi.mock('lucide-vue-next', async () => {
  const { defineComponent, h } = await import('vue')
  const Icon = defineComponent({
    name: 'IconStub',
    setup() {
      return () => h('span')
    },
  })

  return {
    RefreshCcw: Icon,
    Search: Icon,
    ChevronDown: Icon,
    Check: Icon,
  }
})

vi.mock('../ElapsedTimeText.vue', () => ({
  default: defineComponent({
    name: 'ElapsedTimeTextStub',
    setup() {
      return () => h('span', 'elapsed')
    },
  }),
}))

vi.mock('../ServerUserSelector.vue', () => ({
  default: defineComponent({
    name: 'ServerUserSelectorStub',
    setup() {
      return () => h('div', 'user selector')
    },
  }),
}))

const mountedApps: Array<{ app: App, root: HTMLElement }> = []

beforeEach(() => {
  const storage = new Map<string, string>()
  Object.defineProperty(window, 'localStorage', {
    configurable: true,
    value: {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, value)
      },
      removeItem: (key: string) => {
        storage.delete(key)
      },
    },
  })
})

function buildRecord(overrides: Partial<UsageRecord> = {}): UsageRecord {
  return {
    id: 'usage-1',
    model: 'gpt-5',
    input_tokens: 100,
    output_tokens: 50,
    total_tokens: 150,
    cost: 0.01,
    response_time_ms: 1000,
    first_byte_time_ms: 500,
    is_stream: true,
    upstream_is_stream: true,
    status: 'completed',
    created_at: '2026-05-06T12:00:00Z',
    ...overrides,
  }
}

function mountUsageRecordsTable(records: UsageRecord[], overrides: Record<string, unknown> = {}) {
  const root = document.createElement('div')
  document.body.appendChild(root)

  const app = createApp(UsageRecordsTable, {
    records,
    isAdmin: true,
    showActualCost: false,
    loading: false,
    timeRange: { preset: 'today', tz_offset_minutes: 0 },
    filterSearch: '',
    filterUser: '__all__',
    filterApiKeyGroup: '__all__',
    filterModel: '__all__',
    filterProvider: '__all__',
    filterApiFormat: '__all__',
    filterStatus: '__all__',
    filterClientFamily: '__all__',
    availableUsers: [],
    availableApiKeyGroups: [],
    availableModels: [],
    availableProviders: [],
    availableClientFamilies: [],
    currentPage: 1,
    pageSize: 20,
    totalRecords: records.length,
    pageSizeOptions: [20, 50],
    autoRefresh: false,
    ...overrides,
  })

  app.mount(root)
  mountedApps.push({ app, root })
  return root
}

afterEach(() => {
  for (const { app, root } of mountedApps.splice(0)) {
    app.unmount()
    root.remove()
  }
})

describe('UsageRecordsTable', () => {
  it('shows output TPS after the request completes', () => {
    const root = mountUsageRecordsTable([buildRecord()])

    expect(root.textContent).toContain('输出速度')
    expect(root.textContent).toContain('0.50s / 1.00s')
    expect(root.textContent).not.toContain('500ms')
    expect(root.textContent).toContain('100 tps')
    expect([...root.querySelectorAll<HTMLElement>('.text-muted-foreground')]
      .some((element) => element.textContent?.includes('100 tps'))).toBe(true)
    const tpsElements = [...root.querySelectorAll<HTMLElement>('.text-muted-foreground')]
      .filter((element) => element.textContent?.trim() === '100 tps')
    expect(tpsElements.some((element) => element.classList.contains('text-[11px]'))).toBe(false)

    const titles = [...root.querySelectorAll<HTMLElement>('[title]')].map((element) => element.title)
    expect(titles).toContain([
      '首字：0.50s',
      '总耗时：1.00s',
      '生成耗时：0.50s',
      '输出速度：100 tokens/s',
    ].join('\n'))
    expect(titles.join('\n')).not.toContain('500ms')
    expect(titles.join('\n')).not.toContain('首字后生成耗时')
  })

  it('shows an output speed placeholder when the rate is unavailable', () => {
    const root = mountUsageRecordsTable([buildRecord({
      output_tokens: 0,
      response_time_ms: 1000,
      first_byte_time_ms: 500,
    })])

    const performanceCell = root.querySelector('table tbody tr td:last-child') as HTMLElement
    expect(performanceCell.textContent).toContain('0.50s / 1.00s')
    expect([...performanceCell.querySelectorAll<HTMLElement>('.text-muted-foreground')]
      .some((element) => element.textContent?.trim() === '-')).toBe(true)

    const titles = [...root.querySelectorAll<HTMLElement>('[title]')].map((element) => element.title)
    expect(titles).toContain([
      '首字：0.50s',
      '总耗时：1.00s',
      '生成耗时：0.50s',
      '输出速度：-',
    ].join('\n'))
  })

  it('labels official cost, wallet debit and multiplier for admin records', () => {
    const root = mountUsageRecordsTable([
      buildRecord({
        official_cost: 1,
        cost: 0.15,
        sales_multiplier: 0.15,
        actual_cost: 0.8,
        rate_multiplier: 0.8,
      }),
    ], { showActualCost: true })

    expect(root.textContent).toContain('官方 $1.00')
    expect(root.textContent).toContain('钱包扣除 $0.15')
    expect(root.textContent).toContain('0.15x')
    expect(root.textContent).toContain('平台 $0.80')
    expect(root.textContent).toContain('成本倍率 0.8x')
  })

  it('uses discount terminology for dedicated portal usage records', () => {
    const root = mountUsageRecordsTable([
      buildRecord({
        official_cost: 1,
        cost: 1.2,
        discount: 1.2,
        charge_breakdown: {
          official_cost: 1,
          package_debit: 0,
          package_multiplier: null,
          wallet_debit: 1.2,
          wallet_discount: 1.2,
          user_debit: 1.2,
        },
      }),
    ], { isAdmin: false, showActualCost: false })

    expect(root.textContent).toContain('钱包扣除 $1.20')
    expect(root.textContent).toContain('折扣 1.2')
    expect(root.textContent).not.toContain('1.2x')
  })

  it('shows package debit without applying wallet multiplier', () => {
    const root = mountUsageRecordsTable([
      buildRecord({
        official_cost: 1,
        cost: 1,
        sales_multiplier: 0.15,
        charge_breakdown: {
          official_cost: 1,
          package_debit: 1,
          package_multiplier: 1,
          wallet_debit: 0,
          wallet_multiplier: 0.15,
          user_debit: 1,
        },
      }),
    ])

    expect(root.textContent).toContain('官方 $1.00')
    expect(root.textContent).toContain('套餐扣除 $1.00')
    expect(root.textContent).toContain('1x')
    expect(root.textContent).not.toContain('钱包扣除 $0.15')
  })

  it('labels admin records without user id as unauthenticated requests', () => {
    const root = mountUsageRecordsTable([buildRecord({
      user_id: undefined,
      username: undefined,
      user_email: undefined,
    })])

    expect(root.textContent).toContain('未认证请求')
    expect(root.textContent).not.toContain('已删除用户')
  })

  it('labels admin records with missing joined user as deleted users', () => {
    const root = mountUsageRecordsTable([buildRecord({
      user_id: 'user-deleted',
      username: undefined,
      user_email: undefined,
    })])

    expect(root.textContent).toContain('已删除用户')
    expect(root.textContent).not.toContain('User user-deleted')
  })

  it('shows package and wallet split when quota only covers part of the request', () => {
    const root = mountUsageRecordsTable([
      buildRecord({
        official_cost: 1,
        cost: 0.49,
        sales_multiplier: 0.15,
        charge_breakdown: {
          official_cost: 1,
          package_debit: 0.4,
          package_multiplier: 1,
          wallet_debit: 0.09,
          wallet_multiplier: 0.15,
          user_debit: 0.49,
        },
      }),
    ])

    expect(root.textContent).toContain('套餐扣除 $0.40')
    expect(root.textContent).toContain('钱包扣除 $0.09')
    expect(root.textContent).toContain('0.15x')
  })

  it('shows moderation cost as part of the existing cost column', () => {
    const root = mountUsageRecordsTable([
      buildRecord({
        official_cost: 1,
        cost: 0.1501,
        model_cost: 0.15,
        moderation_cost: 0.0001,
        actual_cost: 0.80008,
        actual_model_cost: 0.8,
        actual_moderation_cost: 0.00008,
        rate_multiplier: 0.80008,
      }),
    ], { showActualCost: true })

    expect(root.textContent).toContain('钱包扣除 $0.1501')
    expect(root.textContent).toContain('模型扣费 $0.15')
    expect(root.textContent).toContain('审查成本 $0.00010')
    const titles = [...root.querySelectorAll<HTMLElement>('[title]')].map((element) => element.title)
    expect(titles.some(title => title.includes('平台审查成本：$0.000080'))).toBe(true)
  })

  it('hides platform cost in the user usage table', () => {
    const root = mountUsageRecordsTable([
      buildRecord({
        official_cost: 1,
        cost: 0.15,
        sales_multiplier: 0.15,
        actual_cost: 0.8,
        rate_multiplier: 0.8,
      }),
    ], { isAdmin: false, showActualCost: false })

    expect(root.textContent).toContain('官方 $1.00')
    expect(root.textContent).toContain('钱包扣除 $0.15')
    expect(root.textContent).toContain('0.15x')
    expect(root.textContent).not.toContain('平台 $0.80')
    expect(root.textContent).not.toContain('成本倍率 0.8x')
  })

  it('keeps active request latency in one first-byte / live-total line without TPS', () => {
    const root = mountUsageRecordsTable([buildRecord({
      status: 'streaming',
      response_time_ms: null,
      first_byte_time_ms: 500,
    })])

    expect(root.textContent).toContain('0.50s')
    expect(root.textContent).toContain('elapsed')
    expect(root.textContent).toContain('0.50s / elapsed')
    expect(root.textContent).not.toContain('100 tps')
    expect(root.textContent).not.toContain('生成中')
    expect(root.textContent).not.toContain('等待首字')
    expect(root.querySelector('[data-active-latency-state="streaming"]')).toBeNull()
  })

  it('uses a first-byte placeholder and live total before the first byte arrives', () => {
    const root = mountUsageRecordsTable([buildRecord({
      status: 'pending',
      response_time_ms: null,
      first_byte_time_ms: null,
    })])

    expect(root.textContent).toContain('- / elapsed')
    expect(root.textContent).toContain('elapsed')
    expect(root.textContent).not.toContain('等待首字')
    expect(root.querySelector('[data-active-latency-state="waiting-first-byte"]')).toBeNull()
  })

  it('shows failed when Codex image progress fails before the usage record finalizes', () => {
    const root = mountUsageRecordsTable([buildRecord({
      status: 'pending',
      response_time_ms: null,
      first_byte_time_ms: null,
      image_progress: {
        phase: 'failed',
      },
    })])

    expect(root.textContent).toContain('失败')
    expect(root.textContent).not.toContain('等待中')
  })

  it('shows the real provider transfer path instead of repeating the final key name', () => {
    const root = mountUsageRecordsTable([buildRecord({
      provider: 'cc-max(zzshu)1.0',
      provider_route: ['cc-max(link)1.0', 'cc-max(zzshu)1.0'],
      provider_key_name: 'cc-max(zzshu)1.0',
      has_fallback: true,
    })])

    expect(root.textContent).toContain('cc-max(link)1.0')
    expect(root.textContent).toContain('→')
    expect(root.textContent).toContain('cc-max(zzshu)1.0')
    expect(root.textContent?.match(/cc-max\(zzshu\)1\.0/g)?.length).toBe(1)
    const titles = [...root.querySelectorAll<HTMLElement>('[title]')].map((element) => element.title)
    expect(titles.some(title => title.includes('cc-max(link)1.0 → cc-max(zzshu)1.0'))).toBe(true)
  })

  it('uses a single provider route instead of showing provider unknown', () => {
    const root = mountUsageRecordsTable([buildRecord({
      provider: 'unknown',
      provider_route: ['cc-max(link)1.0'],
      has_fallback: true,
    })])

    expect(root.textContent).toContain('cc-max(link)1.0')
    expect(root.textContent).not.toContain('unknown')
    expect(root.textContent).not.toContain('→')
    const titles = [...root.querySelectorAll<HTMLElement>('[title]')].map((element) => element.title)
    expect(titles.some(title => title.includes('服务切换'))).toBe(false)
  })

  it('still shows a distinct provider account under the provider name', () => {
    const root = mountUsageRecordsTable([buildRecord({
      provider: 'cc-max(zzshu)1.0',
      provider_key_account_label: 'account@example.com',
    })])

    expect(root.textContent).toContain('cc-max(zzshu)1.0')
    expect(root.textContent).toContain('account@example.com')
  })

  it('renders output TPS in the non-admin usage table', () => {
    const root = mountUsageRecordsTable([buildRecord()], { isAdmin: false })

    expect(root.textContent).toContain('100 tps')
    expect(root.textContent).toContain('0.50s / 1.00s')
    expect(root.textContent).toContain('gpt-5')
  })
  it('shows the provider default reasoning effort when the request omitted it', () => {
    const root = mountUsageRecordsTable([buildRecord({
      model: 'grok-4.5',
      requested_reasoning_effort: null,
      reasoning_effort: 'high',
    })])

    expect(root.querySelector('[data-usage-model-badge="reasoning"]')?.textContent?.trim())
      .toBe('high')
  })

})
