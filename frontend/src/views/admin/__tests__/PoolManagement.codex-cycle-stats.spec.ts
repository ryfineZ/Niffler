import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, type App } from '@/test/vue'

import PoolManagement from '@/views/admin/PoolManagement.vue'
import type { PoolKeyDetail, PoolOverviewItem, PoolKeysPageResponse } from '@/api/endpoints/pool'
import { POOL_MANAGEMENT_VIEW_STORAGE_KEY } from '@/features/pool/utils/poolManagementState'

const endpointMocks = vi.hoisted(() => ({
  getPoolOverview: vi.fn(),
  getPoolSchedulingPresets: vi.fn(),
  listPoolKeys: vi.fn(),
  clearPoolCooldown: vi.fn(),
  batchActionPoolKeys: vi.fn(),
  getPoolBatchDeleteTask: vi.fn(),
  resolvePoolKeySelection: vi.fn(),
  getProvider: vi.fn(),
  updateProvider: vi.fn(),
  revealEndpointKey: vi.fn(),
  exportKey: vi.fn(),
  deleteEndpointKey: vi.fn(),
  updateProviderKey: vi.fn(),
  refreshProviderQuota: vi.fn(),
  resetCodexQuota: vi.fn(),
  resetProviderKeyCycleStats: vi.fn(),
  refreshProviderOAuth: vi.fn(),
}))

const accountTestDialogMocks = vi.hoisted(() => ({
  openAccountTest: vi.fn(),
}))

const routeMocks = vi.hoisted(() => ({
  query: {} as Record<string, string>,
  patchQuery: vi.fn((patch: Record<string, string | undefined | null>) => {
    for (const [key, value] of Object.entries(patch)) {
      if (value == null || String(value).trim() === '') {
        delete routeMocks.query[key]
      } else {
        routeMocks.query[key] = String(value)
      }
    }
  }),
}))

const proxyStoreMocks = vi.hoisted(() => ({
  ensureLoaded: vi.fn(),
}))

vi.mock('@/api/endpoints/pool', () => ({
  getPoolOverview: endpointMocks.getPoolOverview,
  getPoolSchedulingPresets: endpointMocks.getPoolSchedulingPresets,
  listPoolKeys: endpointMocks.listPoolKeys,
  clearPoolCooldown: endpointMocks.clearPoolCooldown,
  batchActionPoolKeys: endpointMocks.batchActionPoolKeys,
  getPoolBatchDeleteTask: endpointMocks.getPoolBatchDeleteTask,
  resolvePoolKeySelection: endpointMocks.resolvePoolKeySelection,
}))

vi.mock('@/api/endpoints/keys', () => ({
  revealEndpointKey: endpointMocks.revealEndpointKey,
  exportKey: endpointMocks.exportKey,
  deleteEndpointKey: endpointMocks.deleteEndpointKey,
  updateProviderKey: endpointMocks.updateProviderKey,
  refreshProviderQuota: endpointMocks.refreshProviderQuota,
  resetCodexQuota: endpointMocks.resetCodexQuota,
  resetProviderKeyCycleStats: endpointMocks.resetProviderKeyCycleStats,
}))

vi.mock('@/api/endpoints/provider_oauth', () => ({
  refreshProviderOAuth: endpointMocks.refreshProviderOAuth,
}))

vi.mock('@/api/endpoints', () => ({
  getProvider: endpointMocks.getProvider,
  updateProvider: endpointMocks.updateProvider,
}))

vi.mock('@/composables/useRouteQuery', () => ({
  useRouteQuery: () => ({
    getQueryValue: (key: string) => routeMocks.query[key],
    patchQuery: routeMocks.patchQuery,
  }),
}))

vi.mock('@/stores/proxy-nodes', () => ({
  useProxyNodesStore: () => ({
    nodes: [],
    ensureLoaded: proxyStoreMocks.ensureLoaded,
  }),
}))

vi.mock('@/composables/useToast', () => ({
  useToast: () => ({
    success: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
  }),
}))

vi.mock('@/composables/useConfirm', () => ({
  useConfirm: () => ({
    confirm: vi.fn().mockResolvedValue(true),
  }),
}))

vi.mock('@/composables/useClipboard', () => ({
  useClipboard: () => ({
    copyToClipboard: vi.fn().mockResolvedValue(undefined),
  }),
}))

vi.mock('@/composables/useCountdownTimer', async () => {
  const { ref } = await import('vue')
  return {
    useCountdownTimer: () => ({
      tick: ref(0),
      start: vi.fn(),
    }),
    getCodexResetCountdown: () => ({
      isExpired: false,
      text: '1h',
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
    Search: Icon,
    Upload: Icon,
    ChevronDown: Icon,
    RefreshCw: Icon,
    Activity: Icon,
    Power: Icon,
    Database: Icon,
    KeyRound: Icon,
    Download: Icon,
    Copy: Icon,
    Shield: Icon,
    Globe: Icon,
    Repeat2: Icon,
    RotateCcw: Icon,
    SquarePen: Icon,
    Trash2: Icon,
    Users: Icon,
    Settings2: Icon,
    SlidersHorizontal: Icon,
    CircleHelp: Icon,
    Edit: Icon,
    Plug: Icon,
    Loader2: Icon,
    Play: Icon,
  }
})

vi.mock('@/components/ui', async () => {
  const { computed, defineComponent, h, inject, provide } = await import('vue')
  const passthrough = (name: string, tag = 'div') => defineComponent({
    name,
    inheritAttrs: false,
    setup(_, { attrs, slots }) {
      return () => h(tag, attrs, slots.default?.())
    },
  })

  const Button = defineComponent({
    name: 'ButtonStub',
    inheritAttrs: false,
    props: {
      disabled: Boolean,
    },
    setup(props, { attrs, slots }) {
      return () => h('button', { ...attrs, disabled: props.disabled, type: attrs.type ?? 'button' }, slots.default?.())
    },
  })

  const Input = defineComponent({
    name: 'InputStub',
    inheritAttrs: false,
    props: {
      modelValue: { type: [String, Number], default: '' },
    },
    emits: ['update:modelValue'],
    setup(props, { attrs, emit }) {
      return () => h('input', {
        ...attrs,
        value: props.modelValue ?? '',
        onInput: (event: Event) => emit('update:modelValue', (event.target as HTMLInputElement).value),
      })
    },
  })

  const Switch = defineComponent({
    name: 'SwitchStub',
    inheritAttrs: false,
    props: {
      modelValue: Boolean,
    },
    emits: ['update:modelValue'],
    setup(props, { attrs, emit }) {
      return () => h('input', {
        ...attrs,
        type: 'checkbox',
        role: 'switch',
        checked: props.modelValue,
        onChange: (event: Event) => emit('update:modelValue', (event.target as HTMLInputElement).checked),
      })
    },
  })

  const Checkbox = defineComponent({
    name: 'CheckboxStub',
    inheritAttrs: false,
    props: {
      checked: Boolean,
      indeterminate: Boolean,
      disabled: Boolean,
    },
    emits: ['update:checked'],
    setup(props, { attrs, emit }) {
      return () => h('input', {
        ...attrs,
        type: 'checkbox',
        checked: props.checked,
        disabled: props.disabled,
        'data-indeterminate': props.indeterminate ? 'true' : undefined,
        onChange: (event: Event) => emit('update:checked', (event.target as HTMLInputElement).checked),
      })
    },
  })

  const Pagination = defineComponent({
    name: 'PaginationStub',
    setup() {
      return () => h('nav')
    },
  })

  const popoverContextKey = Symbol('PopoverStubContext')

  const Popover = defineComponent({
    name: 'PopoverStub',
    inheritAttrs: false,
    props: {
      open: Boolean,
    },
    emits: ['update:open'],
    setup(props, { slots, emit }) {
      const context = {
        open: computed(() => props.open),
        toggle: () => emit('update:open', !props.open),
      }
      provide(popoverContextKey, context)
      return () => slots.default?.()
    },
  })

  const PopoverTrigger = defineComponent({
    name: 'PopoverTriggerStub',
    inheritAttrs: false,
    setup(_, { attrs, slots }) {
      const context = inject<{ open: { value: boolean }, toggle: () => void } | null>(popoverContextKey, null)
      return () => {
        return h('span', {
          ...attrs,
          onClickCapture: () => {
            context?.toggle()
          },
        }, slots.default?.())
      }
    },
  })

  const PopoverContent = defineComponent({
    name: 'PopoverContentStub',
    inheritAttrs: false,
    setup(_, { attrs, slots }) {
      const context = inject<{ open: { value: boolean } } | null>(popoverContextKey, null)
      return () => {
        if (!context?.open.value) return null
        return h('div', { ...attrs, 'data-state': 'open' }, slots.default?.())
      }
    },
  })

  return {
    Card: passthrough('CardStub'),
    Badge: passthrough('BadgeStub', 'span'),
    Button,
    Input,
    Select: passthrough('SelectStub'),
    SelectTrigger: passthrough('SelectTriggerStub', 'button'),
    SelectValue: passthrough('SelectValueStub', 'span'),
    SelectContent: passthrough('SelectContentStub'),
    SelectItem: passthrough('SelectItemStub'),
    Table: passthrough('TableStub', 'table'),
    TableHeader: passthrough('TableHeaderStub', 'thead'),
    TableBody: passthrough('TableBodyStub', 'tbody'),
    TableRow: passthrough('TableRowStub', 'tr'),
    TableHead: passthrough('TableHeadStub', 'th'),
    SortableTableHead: passthrough('SortableTableHeadStub', 'th'),
    TableFilterMenu: passthrough('TableFilterMenuStub'),
    TableCell: passthrough('TableCellStub', 'td'),
    Checkbox,
    Switch,
    Pagination,
    Popover,
    PopoverTrigger,
    PopoverContent,
  }
})

vi.mock('@/components/ui/refresh-button.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    default: defineComponent({
      name: 'RefreshButtonStub',
      setup(_, { attrs }) {
        return () => h('button', attrs, '刷新')
      },
    }),
  }
})

vi.mock('@/features/pool/components/PoolSchedulingDialog.vue', async () => {
  const { defineComponent } = await import('vue')
  return {
    default: defineComponent({
      name: 'PoolSchedulingDialogStub',
      setup() {
        return () => null
      },
    }),
  }
})
vi.mock('@/features/pool/components/PoolAdvancedDialog.vue', async () => {
  const { defineComponent } = await import('vue')
  return {
    default: defineComponent({
      name: 'PoolAdvancedDialogStub',
      setup() {
        return () => null
      },
    }),
  }
})
vi.mock('@/features/pool/components/PoolDemandMetricsDialog.vue', async () => {
  const { defineComponent } = await import('vue')
  return {
    default: defineComponent({
      name: 'PoolDemandMetricsDialogStub',
      setup() {
        return () => null
      },
    }),
  }
})
vi.mock('@/features/pool/components/PoolAccountBatchDialog.vue', async () => {
  const { defineComponent } = await import('vue')
  return {
    default: defineComponent({
      name: 'PoolAccountBatchDialogStub',
      setup() {
        return () => null
      },
    }),
  }
})
vi.mock('@/features/pool/components/PoolAccountTestDialog.vue', async () => {
  const { defineComponent } = await import('vue')
  return {
    default: defineComponent({
      name: 'PoolAccountTestDialogStub',
      setup(_, { expose }) {
        expose({ openAccountTest: accountTestDialogMocks.openAccountTest })
        return () => null
      },
    }),
  }
})
vi.mock('@/features/pool/components/ProviderProxyPopover.vue', async () => {
  const { defineComponent } = await import('vue')
  return {
    default: defineComponent({
      name: 'ProviderProxyPopoverStub',
      setup() {
        return () => null
      },
    }),
  }
})
vi.mock('@/features/providers/components/EndpointFormDialog.vue', async () => {
  const { defineComponent } = await import('vue')
  return {
    default: defineComponent({
      name: 'EndpointFormDialogStub',
      setup() {
        return () => null
      },
    }),
  }
})
vi.mock('@/features/providers/components/ProviderFormDialog.vue', async () => {
  const { defineComponent } = await import('vue')
  return {
    default: defineComponent({
      name: 'ProviderFormDialogStub',
      setup() {
        return () => null
      },
    }),
  }
})
vi.mock('@/features/providers/components/KeyAllowedModelsEditDialog.vue', async () => {
  const { defineComponent } = await import('vue')
  return {
    default: defineComponent({
      name: 'KeyAllowedModelsEditDialogStub',
      setup() {
        return () => null
      },
    }),
  }
})
vi.mock('@/features/providers/components/KeyFormDialog.vue', async () => {
  const { defineComponent } = await import('vue')
  return {
    default: defineComponent({
      name: 'KeyFormDialogStub',
      setup() {
        return () => null
      },
    }),
  }
})
vi.mock('@/features/providers/components/OAuthKeyEditDialog.vue', async () => {
  const { defineComponent } = await import('vue')
  return {
    default: defineComponent({
      name: 'OAuthKeyEditDialogStub',
      setup() {
        return () => null
      },
    }),
  }
})
vi.mock('@/features/providers/components/OAuthAccountDialog.vue', async () => {
  const { defineComponent } = await import('vue')
  return {
    default: defineComponent({
      name: 'OAuthAccountDialogStub',
      setup() {
        return () => null
      },
    }),
  }
})
vi.mock('@/features/providers/components/ProxyNodeSelect.vue', async () => {
  const { defineComponent } = await import('vue')
  return {
    default: defineComponent({
      name: 'ProxyNodeSelectStub',
      setup() {
        return () => null
      },
    }),
  }
})

const mountedApps: Array<{ app: App, root: HTMLElement }> = []

function createOverview(providerType: string): PoolOverviewItem {
  return {
    provider_id: `${providerType}-provider`,
    provider_name: `${providerType} Provider`,
    provider_type: providerType,
    total_keys: 1,
    active_keys: 1,
    cooldown_count: 0,
    pool_enabled: true,
  }
}

function createProvider(providerType: string, overrides: Record<string, unknown> = {}) {
  return {
    id: `${providerType}-provider`,
    name: `${providerType} Provider`,
    provider_type: providerType,
    is_active: true,
    api_formats: ['openai:chat'],
    proxy: null,
    pool_advanced: null,
    claude_code_advanced: null,
    ...overrides,
  }
}

function createPoolKey(providerType = 'codex', overrides: Partial<PoolKeyDetail> = {}): PoolKeyDetail {
  return {
    key_id: `${providerType}-key-1`,
    key_name: `${providerType} key`,
    is_active: true,
    auth_type: 'api_key',
    api_formats: ['openai:chat'],
    internal_priority: 50,
    account_quota: null,
    cooldown_reason: null,
    cooldown_ttl_seconds: null,
    cost_window_usage: 0,
    cost_limit: null,
    request_count: 9876,
    total_tokens: 4321000,
    total_cost_usd: '8.7654',
    sticky_sessions: 0,
    lru_score: null,
    created_at: '2026-05-05T00:00:00Z',
    imported_at: '2026-05-05T00:00:00Z',
    last_used_at: '2026-05-05T01:00:00Z',
    status_snapshot: {
      oauth: { code: 'none' },
      account: { code: 'ok', blocked: false },
      quota: {
        code: 'ok',
        exhausted: false,
        provider_type: providerType,
        windows: providerType === 'codex'
          ? [
              {
                code: '5h',
                remaining_ratio: 0.8,
                usage: { request_count: 7, total_tokens: 2500, total_cost_usd: '0.0045' },
              },
              {
                code: 'weekly',
                remaining_ratio: 0.5,
                usage: { request_count: 0, total_tokens: 0, total_cost_usd: '0.00000000' },
              },
            ]
          : [],
      },
    },
    ...overrides,
  }
}

function createKeyPage(key: PoolKeyDetail): PoolKeysPageResponse {
  return {
    total: 1,
    page: 1,
    page_size: 50,
    keys: [key],
  }
}

function resetQuery() {
  for (const key of Object.keys(routeMocks.query)) {
    delete routeMocks.query[key]
  }
}

function mountPoolManagement() {
  const root = document.createElement('div')
  document.body.appendChild(root)
  const app = createApp(PoolManagement)
  app.mount(root)
  mountedApps.push({ app, root })
  return root
}

async function settle() {
  for (let index = 0; index < 8; index += 1) {
    await Promise.resolve()
    await nextTick()
  }
}

function seedStoredStatsMode(statsMode: 'current_cycle' | 'account_total') {
  window.sessionStorage.setItem(
    POOL_MANAGEMENT_VIEW_STORAGE_KEY,
    JSON.stringify({ statsMode }),
  )
}

beforeEach(() => {
  resetQuery()
  window.sessionStorage.clear()
  routeMocks.patchQuery.mockClear()
  proxyStoreMocks.ensureLoaded.mockClear()

  endpointMocks.getPoolOverview.mockReset()
  endpointMocks.getPoolSchedulingPresets.mockReset()
  endpointMocks.listPoolKeys.mockReset()
  endpointMocks.clearPoolCooldown.mockReset()
  endpointMocks.batchActionPoolKeys.mockReset()
  endpointMocks.getPoolBatchDeleteTask.mockReset()
  endpointMocks.resolvePoolKeySelection.mockReset()
  endpointMocks.getProvider.mockReset()
  endpointMocks.updateProvider.mockReset()
  endpointMocks.revealEndpointKey.mockReset()
  endpointMocks.exportKey.mockReset()
  endpointMocks.deleteEndpointKey.mockReset()
  endpointMocks.updateProviderKey.mockReset()
  endpointMocks.refreshProviderQuota.mockReset()
  endpointMocks.resetCodexQuota.mockReset()
  endpointMocks.resetProviderKeyCycleStats.mockReset()
  endpointMocks.refreshProviderOAuth.mockReset()
  accountTestDialogMocks.openAccountTest.mockReset()

  endpointMocks.getPoolSchedulingPresets.mockResolvedValue([])
  endpointMocks.clearPoolCooldown.mockResolvedValue({ message: 'ok' })
  endpointMocks.batchActionPoolKeys.mockResolvedValue({ affected: 0, message: 'ok' })
  endpointMocks.getPoolBatchDeleteTask.mockResolvedValue({ task_id: 'task-1', status: 'completed', total: 0, deleted: 0, message: 'ok' })
  endpointMocks.resolvePoolKeySelection.mockResolvedValue({ total: 0, items: [] })
  endpointMocks.refreshProviderQuota.mockResolvedValue({ success: 0, failed: 0 })
  endpointMocks.resetCodexQuota.mockResolvedValue({
    message: '额度已重置',
    outcome: 'reset',
    reset_applied: true,
    windows_reset: 2,
    refresh_succeeded: true,
    quota_snapshot: null,
    refresh_message: null,
  })
  endpointMocks.resetProviderKeyCycleStats.mockResolvedValue({ message: '已重置周期统计', reset_at: 123, windows: 2 })
  accountTestDialogMocks.openAccountTest.mockResolvedValue(undefined)
})

afterEach(() => {
  for (const { app, root } of mountedApps.splice(0)) {
    app.unmount()
    root.remove()
  }
  vi.useRealTimers()
})

describe('PoolManagement Codex cycle stats mode', () => {
  it('renders Codex current-cycle stats by default with a header icon toggle', async () => {
    const codexKey = createPoolKey('codex')
    endpointMocks.getPoolOverview.mockResolvedValue({ items: [createOverview('codex')] })
    endpointMocks.listPoolKeys.mockResolvedValue(createKeyPage(codexKey))
    endpointMocks.getProvider.mockResolvedValue(createProvider('codex'))

    const root = mountPoolManagement()
    await settle()

    expect(root.querySelector('[data-testid="pool-stats-mode-switch"]')).toBeNull()
    const modeButton = root.querySelector<HTMLButtonElement>('[data-testid="pool-stats-mode-control"]')
    expect(modeButton).not.toBeNull()
    expect(modeButton?.getAttribute('title')).toBe('切换为账号总计')
    expect(root.querySelectorAll('[data-testid="pool-stats-cycle-group-5h"]').length).toBeGreaterThan(0)
    expect(root.querySelectorAll('[data-testid="pool-stats-cycle-group-weekly"]').length).toBeGreaterThan(0)
    expect(root.querySelector('[data-testid="pool-stats-5h-request_count"]')?.textContent?.trim()).toBe('7')
    expect(root.querySelector('[data-testid="pool-stats-weekly-total_tokens"]')?.textContent?.trim()).toBe('0')
    expect(root.querySelector('[data-testid="pool-stats-cycle-grid"]')?.getAttribute('style')).toContain('38px repeat(2, minmax(0, 1fr))')
    expect(root.querySelector('[data-testid="pool-stats-cycle-grid"]')?.className).toContain('min-w-[188px]')
    expect(root.querySelector('[data-testid="pool-stats-cycle-grid"]')?.className).toContain('min-h-16')
    expect(root.querySelector('[data-testid="pool-stats-5h-request_count"]')?.className).toContain('text-center')
    expect(root.querySelector('[data-testid="pool-stats-weekly-total_tokens"]')?.className).toContain('text-center')
    expect(endpointMocks.listPoolKeys).toHaveBeenLastCalledWith(
      'codex-provider',
      expect.objectContaining({
        sort_by: 'imported_at',
        sort_order: 'desc',
      }),
      expect.anything(),
    )
    expect(root.textContent).not.toContain('累计')
    expect(root.textContent).not.toContain('总计')
  })

  it('exposes account testing and Codex quota reset in the account list', async () => {
    const codexKey = createPoolKey('codex')
    if (codexKey.status_snapshot?.quota) {
      codexKey.status_snapshot.quota.reset_credits = { available_count: 1 }
    }
    endpointMocks.getPoolOverview.mockResolvedValue({ items: [createOverview('codex')] })
    endpointMocks.listPoolKeys.mockResolvedValue(createKeyPage(codexKey))
    endpointMocks.getProvider.mockResolvedValue(createProvider('codex'))

    const root = mountPoolManagement()
    await settle()

    const testButtons = root.querySelectorAll<HTMLButtonElement>('[data-testid="pool-test-account"]')
    const resetButtons = root.querySelectorAll<HTMLButtonElement>('[data-testid="pool-reset-codex-quota"]')
    expect(testButtons.length).toBeGreaterThan(0)
    expect(resetButtons.length).toBeGreaterThan(0)
    expect(resetButtons[0]?.disabled).toBe(false)
    expect(root.textContent).toContain('主动重置 1')

    testButtons[0]?.click()
    await settle()
    expect(accountTestDialogMocks.openAccountTest).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'codex-key-1', provider_id: 'codex-provider' }),
    )

    resetButtons[0]?.click()
    await settle()
    expect(endpointMocks.resetCodexQuota).toHaveBeenCalledWith(
      'codex-key-1',
      expect.stringMatching(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/),
    )
  })

  it('renders every upstream Codex quota window including monthly', async () => {
    const codexKey = createPoolKey('codex', {
      status_snapshot: {
        oauth: { code: 'none' },
        account: { code: 'ok', blocked: false },
        quota: {
          code: 'ok',
          exhausted: false,
          provider_type: 'codex',
          windows: [{
            code: '5h',
            label: '5H',
            scope: 'account',
            remaining_ratio: 0.8,
            reset_at: 2_000_000_000,
            window_minutes: 300,
            usage: { request_count: 1, total_tokens: 100, total_cost_usd: '0.01' },
          }, {
            code: 'weekly',
            label: '7D',
            scope: 'account',
            remaining_ratio: 0.7,
            reset_at: 2_000_000_000,
            window_minutes: 10_080,
            usage: { request_count: 2, total_tokens: 200, total_cost_usd: '0.02' },
          }, {
            code: '1m',
            label: '1M',
            scope: 'account',
            remaining_ratio: 0.6,
            reset_at: 2_000_000_000,
            window_minutes: 43_200,
            usage: { request_count: 3, total_tokens: 300, total_cost_usd: '0.03' },
          }],
        },
      },
    })
    endpointMocks.getPoolOverview.mockResolvedValue({ items: [createOverview('codex')] })
    endpointMocks.listPoolKeys.mockResolvedValue(createKeyPage(codexKey))
    endpointMocks.getProvider.mockResolvedValue(createProvider('codex'))

    const root = mountPoolManagement()
    await settle()

    const labels = Array.from(
      root.querySelectorAll('[data-testid="pool-quota-progress-label"]'),
      element => element.textContent?.trim(),
    )
    expect(labels).toContain('5H')
    expect(labels).toContain('周')
    expect(labels).toContain('月')
  })

  it('briefly reloads the current page until pending Codex cycle stats are ready', async () => {
    vi.useFakeTimers()
    const pendingKey = createPoolKey('codex', {
      status_snapshot: {
        oauth: { code: 'none' },
        account: { code: 'ok', blocked: false },
        quota: {
          code: 'ok',
          exhausted: false,
          provider_type: 'codex',
          windows: [{
            code: 'weekly',
            scope: 'account',
            reset_at: 2_000_000_000,
            window_minutes: 10_080,
            usage: null,
          }],
        },
      },
    })
    const readyKey = createPoolKey('codex', {
      status_snapshot: {
        oauth: { code: 'none' },
        account: { code: 'ok', blocked: false },
        quota: {
          code: 'ok',
          exhausted: false,
          provider_type: 'codex',
          windows: [{
            code: 'weekly',
            scope: 'account',
            reset_at: 2_000_000_000,
            window_minutes: 10_080,
            usage: { request_count: 3, total_tokens: 600, total_cost_usd: '0.12' },
          }],
        },
      },
    })
    endpointMocks.getPoolOverview.mockResolvedValue({ items: [createOverview('codex')] })
    endpointMocks.listPoolKeys
      .mockResolvedValueOnce(createKeyPage(pendingKey))
      .mockResolvedValueOnce(createKeyPage(readyKey))
    endpointMocks.getProvider.mockResolvedValue(createProvider('codex'))

    const root = mountPoolManagement()
    await settle()

    expect(endpointMocks.listPoolKeys).toHaveBeenCalledTimes(1)
    expect(root.textContent).toContain('统计中')

    await vi.advanceTimersByTimeAsync(1_000)
    await settle()

    expect(endpointMocks.listPoolKeys).toHaveBeenCalledTimes(2)
    expect(root.textContent).toContain('600')
    expect(root.textContent).not.toContain('统计中')

    await vi.advanceTimersByTimeAsync(6_000)
    await settle()
    expect(endpointMocks.listPoolKeys).toHaveBeenCalledTimes(2)
  })

  it('renders unified pool score in the key list with a calculation entry point', async () => {
    const scoredKey = createPoolKey('codex', {
      pool_score: {
        id: 'pms-account-score',
        capability: 'account',
        scope_kind: 'account',
        scope_id: null,
        score: 0.875,
        hard_state: 'available',
        score_version: 1,
        score_reason: { weights: { manual_priority: 0.3 } },
        last_ranked_at: 1_700_000_000,
        last_scheduled_at: 1_700_000_010,
        last_success_at: 1_700_000_020,
        last_failure_at: null,
        failure_count: 0,
        last_probe_attempt_at: 1_700_000_030,
        last_probe_success_at: 1_700_000_040,
        last_probe_failure_at: null,
        probe_failure_count: 0,
        probe_status: 'ok',
        updated_at: 1_700_000_050,
      },
    })
    endpointMocks.getPoolOverview.mockResolvedValue({ items: [createOverview('codex')] })
    endpointMocks.listPoolKeys.mockResolvedValue(createKeyPage(scoredKey))
    endpointMocks.getProvider.mockResolvedValue(createProvider('codex'))

    const root = mountPoolManagement()
    await settle()

    expect(root.textContent).toContain('0.875')
    expect(root.querySelectorAll('button[title="评分计算结果"]').length).toBeGreaterThan(0)
  })

  it('renders Grok OAuth billing quota and enables quota refresh', async () => {
    const grokOAuthKey = createPoolKey('grok_oauth', {
      auth_type: 'oauth',
      api_formats: ['openai:responses'],
      quota_updated_at: null,
      status_snapshot: {
        oauth: { code: 'valid' },
        account: { code: 'ok', blocked: false },
        quota: {
          code: 'ok',
          exhausted: false,
          provider_type: 'grok_oauth',
          plan_type: 'super',
          windows: [
            {
              code: 'weekly',
              label: '周',
              scope: 'account',
              remaining_ratio: 0.75,
            },
            {
              code: 'monthly',
              label: '月',
              scope: 'account',
              unit: 'usd',
              remaining_ratio: 0.7,
              remaining_value: 105,
              limit_value: 150,
            },
          ],
        },
      },
    })
    endpointMocks.getPoolOverview.mockResolvedValue({ items: [createOverview('grok_oauth')] })
    endpointMocks.listPoolKeys.mockResolvedValue(createKeyPage(grokOAuthKey))
    endpointMocks.getProvider.mockResolvedValue(createProvider('grok_oauth', {
      api_formats: ['openai:responses'],
    }))
    endpointMocks.refreshProviderQuota.mockResolvedValue({
      success: 1,
      failed: 0,
      total: 1,
      results: [],
    })

    const root = mountPoolManagement()
    await settle()

    expect(root.textContent).toContain('周')
    expect(root.textContent).toContain('月')
    expect(root.textContent).toContain('$105/$150')
    const refreshButton = root.querySelector('button[title="刷新数据和额度"]') as HTMLButtonElement | null
    expect(refreshButton).not.toBeNull()
    refreshButton?.click()
    await settle()
    expect(endpointMocks.refreshProviderQuota).toHaveBeenCalledWith(
      'grok_oauth-provider',
      ['grok_oauth-key-1'],
    )
  })

  it('opens only one score popover across desktop and mobile layouts', async () => {
    const scoredKey = createPoolKey('codex', {
      pool_score: {
        id: 'pms-account-score',
        capability: 'account',
        scope_kind: 'account',
        scope_id: null,
        score: 0.662,
        hard_state: 'available',
        score_version: 1,
        score_reason: {
          rules: {
            probe_failure_penalty: 0.05,
          },
        },
        last_ranked_at: 1_700_000_000,
        last_scheduled_at: null,
        last_success_at: null,
        last_failure_at: null,
        failure_count: 0,
        last_probe_attempt_at: null,
        last_probe_success_at: null,
        last_probe_failure_at: null,
        probe_failure_count: 0,
        probe_status: 'ok',
        updated_at: 1_700_000_050,
      },
    })
    endpointMocks.getPoolOverview.mockResolvedValue({ items: [createOverview('codex')] })
    endpointMocks.listPoolKeys.mockResolvedValue(createKeyPage(scoredKey))
    endpointMocks.getProvider.mockResolvedValue(createProvider('codex'))

    const root = mountPoolManagement()
    await settle()

    const helpButtons = root.querySelectorAll<HTMLButtonElement>('button[title="评分计算结果"]')
    expect(helpButtons.length).toBe(2)

    helpButtons[0]?.click()
    await settle()

    expect(root.querySelectorAll('pre').length).toBe(1)
    expect(root.textContent).toContain('评分计算结果')
    expect(root.textContent).toContain('0.662')
  })

  it('refreshes quota only for keys on the current page', async () => {
    const pageKeys = [
      createPoolKey('codex', { key_id: 'codex-page-key-1', quota_updated_at: null }),
      createPoolKey('codex', { key_id: 'codex-page-key-2', quota_updated_at: null }),
    ]
    endpointMocks.getPoolOverview.mockResolvedValue({
      items: [{ ...createOverview('codex'), total_keys: 120 }],
    })
    endpointMocks.listPoolKeys.mockResolvedValue({
      total: 120,
      page: 1,
      page_size: 50,
      keys: pageKeys,
    })
    endpointMocks.getProvider.mockResolvedValue(createProvider('codex'))
    endpointMocks.refreshProviderQuota.mockResolvedValue({
      success: 2,
      failed: 0,
      total: 2,
      results: [],
    })

    const root = mountPoolManagement()
    await settle()

    const refreshButton = root.querySelector('button[title="刷新数据和额度"]') as HTMLButtonElement | null
    expect(refreshButton).not.toBeNull()
    refreshButton?.click()
    await settle()

    expect(endpointMocks.refreshProviderQuota).toHaveBeenCalledTimes(1)
    expect(endpointMocks.refreshProviderQuota).toHaveBeenCalledWith(
      'codex-provider',
      ['codex-page-key-1', 'codex-page-key-2'],
    )
    expect(endpointMocks.refreshProviderQuota).not.toHaveBeenCalledWith('codex-provider')
  })


  it('shows clear-cooldown bulk action instead of enable on temporary-unavailable tab', async () => {
    routeMocks.query.providerId = 'codex-provider'
    routeMocks.query.status = 'temporary_unavailable'
    const cooledKey = createPoolKey('codex', {
      key_id: 'codex-cooldown-a',
      key_name: 'cooldown a',
      cooldown_reason: 'rate_limited_429',
      cooldown_ttl_seconds: 180,
      scheduling_state: 'temporary_unavailable',
      scheduling_status: 'degraded',
      scheduling_label: '暂时不可用',
      scheduling_reason: 'temporary_unavailable',
    })
    endpointMocks.getPoolOverview.mockResolvedValue({
      items: [{ ...createOverview('codex'), total_keys: 1, active_keys: 1 }],
    })
    endpointMocks.listPoolKeys.mockResolvedValue({
      total: 1,
      page: 1,
      page_size: 50,
      keys: [cooledKey],
      summary: {
        total: 1,
        plans: [],
        statuses: [{ code: 'temporary_unavailable', label: '暂时不可用', count: 1 }],
      },
    })
    endpointMocks.getProvider.mockResolvedValue(createProvider('codex'))
    endpointMocks.batchActionPoolKeys.mockResolvedValue({
      affected: 1,
      message: '1 keys cooldown cleared',
    })

    const root = mountPoolManagement()
    await settle()

    expect(root.querySelector('[data-testid="pool-bulk-enable-selected"]')).toBeNull()

    const checkbox = root.querySelector<HTMLInputElement>('[data-testid="pool-key-select-codex-cooldown-a"]')
    expect(checkbox).not.toBeNull()
    checkbox?.click()
    await settle()

    const clearButton = root.querySelector<HTMLButtonElement>('[data-testid="pool-bulk-clear-cooldown-selected"]')
    expect(clearButton).not.toBeNull()
    expect(clearButton?.textContent).toContain('清除冷却 1')

    clearButton?.click()
    await settle()

    expect(endpointMocks.batchActionPoolKeys).toHaveBeenCalledWith('codex-provider', {
      key_ids: ['codex-cooldown-a'],
      action: 'clear_cooldown',
    })
  })

  it('deletes selected pool accounts from the main list', async () => {
    const firstKey = createPoolKey('codex', { key_id: 'codex-key-a', key_name: 'alpha' })
    const secondKey = createPoolKey('codex', { key_id: 'codex-key-b', key_name: 'beta' })
    endpointMocks.getPoolOverview.mockResolvedValue({
      items: [{ ...createOverview('codex'), total_keys: 2, active_keys: 2 }],
    })
    endpointMocks.listPoolKeys
      .mockResolvedValueOnce({
        total: 2,
        page: 1,
        page_size: 50,
        keys: [firstKey, secondKey],
      })
      .mockResolvedValue({
        total: 0,
        page: 1,
        page_size: 50,
        keys: [],
      })
    endpointMocks.getProvider.mockResolvedValue(createProvider('codex'))
    endpointMocks.batchActionPoolKeys.mockResolvedValue({
      affected: 2,
      message: '2 keys deleted',
    })

    const root = mountPoolManagement()
    await settle()

    const firstCheckbox = root.querySelector<HTMLInputElement>('[data-testid="pool-key-select-codex-key-a"]')
    const secondCheckbox = root.querySelector<HTMLInputElement>('[data-testid="pool-key-select-codex-key-b"]')
    expect(firstCheckbox).not.toBeNull()
    expect(secondCheckbox).not.toBeNull()

    firstCheckbox?.click()
    secondCheckbox?.click()
    await settle()

    const deleteButton = root.querySelector<HTMLButtonElement>('[data-testid="pool-bulk-delete-selected"]')
    expect(deleteButton).not.toBeNull()
    expect(deleteButton?.disabled).toBe(false)
    expect(deleteButton?.textContent).toContain('删除已选 2')

    deleteButton?.click()
    await settle()

    expect(endpointMocks.batchActionPoolKeys).toHaveBeenCalledWith('codex-provider', {
      key_ids: ['codex-key-a', 'codex-key-b'],
      action: 'delete',
    })
    expect(endpointMocks.listPoolKeys).toHaveBeenCalledTimes(2)
  })

  it('deletes all filtered pool accounts without selecting each page', async () => {
    routeMocks.query.providerId = 'codex-provider'
    routeMocks.query.status = 'invalid'
    const firstKey = createPoolKey('codex', { key_id: 'codex-invalid-a', key_name: 'invalid a' })
    const secondKey = createPoolKey('codex', { key_id: 'codex-invalid-b', key_name: 'invalid b' })
    endpointMocks.getPoolOverview.mockResolvedValue({
      items: [{ ...createOverview('codex'), total_keys: 5000, active_keys: 0 }],
    })
    endpointMocks.listPoolKeys.mockResolvedValue({
      total: 123,
      page: 1,
      page_size: 50,
      keys: [firstKey, secondKey],
      summary: {
        total: 123,
        plans: [],
        statuses: [{ code: 'invalid', label: '已失效', count: 123 }],
      },
    })
    endpointMocks.getProvider.mockResolvedValue(createProvider('codex'))
    endpointMocks.resolvePoolKeySelection.mockResolvedValue({
      total: 3,
      items: [
        { key_id: 'codex-invalid-a', key_name: 'invalid a', auth_type: 'oauth' },
        { key_id: 'codex-invalid-b', key_name: 'invalid b', auth_type: 'oauth' },
        { key_id: 'codex-invalid-c', key_name: 'invalid c', auth_type: 'oauth' },
      ],
    })
    endpointMocks.batchActionPoolKeys.mockResolvedValue({
      affected: 0,
      message: 'delete task submitted',
      task_id: 'task-large-delete',
    })
    endpointMocks.getPoolBatchDeleteTask.mockResolvedValue({
      task_id: 'task-large-delete',
      status: 'completed',
      total_keys: 3,
      deleted_keys: 3,
      message: '3 keys deleted',
    })

    const root = mountPoolManagement()
    await settle()

    const selectFilteredButton = root.querySelector<HTMLButtonElement>('[data-testid="pool-select-filtered-results"]')
    expect(selectFilteredButton).not.toBeNull()
    expect(selectFilteredButton?.textContent).toContain('选择全部筛选结果 123')

    selectFilteredButton?.click()
    await settle()

    const deleteButton = root.querySelector<HTMLButtonElement>('[data-testid="pool-bulk-delete-selected"]')
    expect(deleteButton?.textContent).toContain('删除已选 123')

    deleteButton?.click()
    await settle()

    expect(endpointMocks.resolvePoolKeySelection).toHaveBeenCalledWith('codex-provider', {
      status: 'invalid',
    })
    expect(endpointMocks.batchActionPoolKeys).toHaveBeenCalledWith('codex-provider', {
      key_ids: ['codex-invalid-a', 'codex-invalid-b', 'codex-invalid-c'],
      action: 'delete',
    })
    expect(endpointMocks.getPoolBatchDeleteTask).toHaveBeenCalledWith('codex-provider', 'task-large-delete')
  })

  it('toggles Codex stats to account totals and persists the choice', async () => {
    const codexKey = createPoolKey('codex')
    endpointMocks.getPoolOverview.mockResolvedValue({ items: [createOverview('codex')] })
    endpointMocks.listPoolKeys.mockResolvedValue(createKeyPage(codexKey))
    endpointMocks.getProvider.mockResolvedValue(createProvider('codex'))

    const root = mountPoolManagement()
    await settle()

    const modeButton = root.querySelector<HTMLButtonElement>('[data-testid="pool-stats-mode-control"]')
    expect(modeButton).not.toBeNull()
    modeButton?.click()
    await settle()

    expect(root.querySelector('[data-testid="pool-stats-account-total"]')).not.toBeNull()
    expect(root.querySelector('[data-testid="pool-stats-account-total"]')?.className).toContain('w-[188px]')
    expect(root.querySelector('[data-testid="pool-stats-account-total"]')?.className).toContain('grid-rows-4')
    expect(root.querySelector('[data-testid="pool-stats-account-total"]')?.className).toContain('min-h-16')
    expect(root.querySelector('[data-testid="pool-stats-cycle-group-5h"]')).toBeNull()
    expect(routeMocks.query.statsMode).toBe('account_total')
    expect(window.sessionStorage.getItem(POOL_MANAGEMENT_VIEW_STORAGE_KEY)).toContain('"statsMode":"account_total"')
    expect(modeButton?.getAttribute('title')).toBe('切换为本地周期用量')
  })

  it('restores stored and query account-total mode for Codex providers', async () => {
    seedStoredStatsMode('account_total')
    routeMocks.query.statsMode = 'account_total'
    const codexKey = createPoolKey('codex')
    endpointMocks.getPoolOverview.mockResolvedValue({ items: [createOverview('codex')] })
    endpointMocks.listPoolKeys.mockResolvedValue(createKeyPage(codexKey))
    endpointMocks.getProvider.mockResolvedValue(createProvider('codex'))

    const root = mountPoolManagement()
    await settle()

    expect(root.querySelector('[data-testid="pool-stats-account-total"]')).not.toBeNull()
    expect(root.querySelector('[data-testid="pool-stats-cycle-group-5h"]')).toBeNull()
    expect(routeMocks.query.statsMode).toBe('account_total')
    expect(window.sessionStorage.getItem(POOL_MANAGEMENT_VIEW_STORAGE_KEY)).toContain('"statsMode":"account_total"')
  })

  it('resets Codex cycle stats from the action column', async () => {
    const codexKey = createPoolKey('codex')
    endpointMocks.getPoolOverview.mockResolvedValue({ items: [createOverview('codex')] })
    endpointMocks.listPoolKeys.mockResolvedValue(createKeyPage(codexKey))
    endpointMocks.getProvider.mockResolvedValue(createProvider('codex'))

    const root = mountPoolManagement()
    await settle()

    const resetButton = root.querySelector<HTMLButtonElement>('[data-testid="pool-reset-cycle-stats"]')
    expect(resetButton).not.toBeNull()

    resetButton?.click()
    await settle()

    expect(endpointMocks.resetProviderKeyCycleStats).toHaveBeenCalledWith(codexKey.key_id)
    expect(endpointMocks.listPoolKeys).toHaveBeenCalledTimes(2)
  })

  it('hides the stats mode switch for non-Codex providers and keeps account totals', async () => {
    const openaiKey = createPoolKey('openai', {
      request_count: 12,
      total_tokens: 3456,
      total_cost_usd: '1.25',
    })
    endpointMocks.getPoolOverview.mockResolvedValue({ items: [createOverview('openai')] })
    endpointMocks.listPoolKeys.mockResolvedValue(createKeyPage(openaiKey))
    endpointMocks.getProvider.mockResolvedValue(createProvider('openai'))

    const root = mountPoolManagement()
    await settle()

    expect(root.querySelector('[data-testid="pool-stats-mode-switch"]')).toBeNull()
    expect(root.querySelector('[data-testid="pool-stats-mode-control"]')).toBeNull()
    expect(root.querySelector('[data-testid="pool-reset-cycle-stats"]')).toBeNull()
    expect(root.querySelector('[data-testid="pool-stats-cycle-group-5h"]')).toBeNull()
    expect(root.querySelector('[data-testid="pool-stats-account-total"]')).not.toBeNull()
    expect(root.textContent).toContain('12')
    expect(root.textContent).toContain('3.5K')
    expect(root.textContent).toContain('$1.25')
  })

  it('shows adaptive hot pool metrics entry only when probing is enabled', async () => {
    endpointMocks.getPoolOverview.mockResolvedValue({
      items: [{ ...createOverview('codex'), provider_desired_hot: 4, provider_in_flight: 2, provider_ema_in_flight: 1.8 }],
    })
    endpointMocks.listPoolKeys.mockResolvedValue(createKeyPage(createPoolKey('codex')))
    endpointMocks.getProvider.mockResolvedValue(createProvider('codex', {
      pool_advanced: {
        probing_enabled: true,
      },
    }))

    const enabledRoot = mountPoolManagement()
    await settle()

    expect(enabledRoot.querySelectorAll('[data-testid="pool-demand-metrics-button"]').length).toBeGreaterThan(0)

    for (const { app, root } of mountedApps.splice(0)) {
      app.unmount()
      root.remove()
    }

    endpointMocks.getProvider.mockResolvedValue(createProvider('codex', {
      pool_advanced: {
        probing_enabled: false,
      },
    }))

    const disabledRoot = mountPoolManagement()
    await settle()

    expect(disabledRoot.querySelector('[data-testid="pool-demand-metrics-button"]')).toBeNull()
  })
})
