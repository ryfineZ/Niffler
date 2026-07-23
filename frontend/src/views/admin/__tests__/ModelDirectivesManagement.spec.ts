import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, h, nextTick, type App } from 'vue'

import ModelDirectivesManagement from '../ModelDirectivesManagement.vue'

const apiMocks = vi.hoisted(() => ({
  getSystemConfig: vi.fn(),
  updateSystemConfig: vi.fn(),
}))
const moduleStoreMocks = vi.hoisted(() => ({
  modules: {} as Record<string, { enabled: boolean }>,
  fetchModules: vi.fn(),
  setEnabled: vi.fn(),
}))
const toastMocks = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }))

vi.mock('@/api/admin', () => ({
  adminApi: {
    getSystemConfig: apiMocks.getSystemConfig,
    updateSystemConfig: apiMocks.updateSystemConfig,
  },
}))

vi.mock('@/stores/modules', () => ({
  useModuleStore: () => moduleStoreMocks,
}))

vi.mock('@/composables/useToast', () => ({ useToast: () => toastMocks }))
vi.mock('@/utils/logger', () => ({ log: { error: vi.fn() } }))

vi.mock('@/components/layout', async () => {
  const { defineComponent, h } = await import('vue')
  const component = (tag: string) => defineComponent({
    setup(_, { slots }) {
      return () => h(tag, [slots.default?.(), slots.actions?.()])
    },
  })
  return { PageContainer: component('main'), PageHeader: component('header') }
})

vi.mock('@/components/ui/button.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return { default: defineComponent({
    inheritAttrs: false,
    setup(_, { attrs, slots }) {
      return () => h('button', attrs, slots.default?.())
    },
  }) }
})

vi.mock('@/components/ui/card.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return { default: defineComponent({ setup(_, { slots }) {
    return () => h('section', slots.default?.())
  } }) }
})

vi.mock('@/components/ui/switch.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return { default: defineComponent({
    inheritAttrs: false,
    props: { modelValue: Boolean, disabled: Boolean },
    emits: ['update:modelValue'],
    setup(props, { attrs, emit }) {
      return () => h('button', {
        ...attrs,
        disabled: props.disabled,
        'aria-pressed': props.modelValue,
        onClick: () => emit('update:modelValue', !props.modelValue),
      })
    },
  }) }
})

vi.mock('@/views/admin/module-management/ModelDirectivesPanel.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return { default: defineComponent({
    props: { config: { type: Object, required: true }, loading: Boolean },
    emits: ['save'],
    setup(props, { emit }) {
      return () => h('div', [
        h('div', {
          'data-testid': 'directive-panel',
          'data-reasoning-enabled': String(
            (props.config as { reasoning_effort?: { enabled?: boolean } }).reasoning_effort?.enabled,
          ),
        }),
        h('button', {
          'data-testid': 'save-config',
          onClick: () => emit('save', {
            ...(props.config as Record<string, unknown>),
            reasoning_effort: {
              ...((props.config as { reasoning_effort: Record<string, unknown> }).reasoning_effort),
              enabled: false,
            },
          }),
        }),
      ])
    },
  }) }
})

let app: App | undefined
let root: HTMLElement | undefined

async function flushPromises() {
  await Promise.resolve()
  await Promise.resolve()
  await nextTick()
}

beforeEach(() => {
  vi.clearAllMocks()
  apiMocks.getSystemConfig.mockResolvedValue({
    key: 'model_directives',
    value: { reasoning_effort: { enabled: true, api_formats: {} } },
  })
  apiMocks.updateSystemConfig.mockResolvedValue(undefined)
  moduleStoreMocks.modules = { model_directives: { enabled: false } }
  moduleStoreMocks.fetchModules.mockResolvedValue(moduleStoreMocks.modules)
  moduleStoreMocks.setEnabled.mockImplementation(async (_name: string, enabled: boolean) => {
    moduleStoreMocks.modules = { model_directives: { enabled } }
    return true
  })
})

afterEach(() => {
  app?.unmount()
  root?.remove()
  app = undefined
  root = undefined
})

describe('ModelDirectivesManagement', () => {
  it('binds the page switch to the real module status, not the reasoning sub-setting', async () => {
    root = document.createElement('div')
    document.body.appendChild(root)
    app = createApp(defineComponent({
      setup: () => () => h(ModelDirectivesManagement),
    }))
    app.mount(root)
    await flushPromises()

    const moduleSwitch = root.querySelector(
      'button[aria-label="启用模型后缀参数模块"]',
    ) as HTMLButtonElement
    expect(moduleSwitch.getAttribute('aria-pressed')).toBe('false')
    expect(root.querySelector('[data-testid="directive-panel"]')
      ?.getAttribute('data-reasoning-enabled')).toBe('true')

    moduleSwitch.click()
    await flushPromises()

    expect(moduleStoreMocks.setEnabled).toHaveBeenCalledWith('model_directives', true)
    expect(moduleSwitch.getAttribute('aria-pressed')).toBe('true')
  })

  it('keeps the configuration visible when only the module status request fails', async () => {
    moduleStoreMocks.fetchModules.mockRejectedValueOnce(new Error('module status unavailable'))
    root = document.createElement('div')
    document.body.appendChild(root)
    app = createApp(defineComponent({
      setup: () => () => h(ModelDirectivesManagement),
    }))
    app.mount(root)
    await flushPromises()

    expect(root.querySelector('[data-testid="directive-panel"]')
      ?.getAttribute('data-reasoning-enabled')).toBe('true')
    expect(toastMocks.error).toHaveBeenCalledWith('获取模型后缀参数模块状态失败')
  })

  it('applies the module status even when the configuration request fails', async () => {
    apiMocks.getSystemConfig.mockRejectedValueOnce(new Error('configuration unavailable'))
    moduleStoreMocks.modules = { model_directives: { enabled: true } }
    moduleStoreMocks.fetchModules.mockResolvedValueOnce(moduleStoreMocks.modules)
    root = document.createElement('div')
    document.body.appendChild(root)
    app = createApp(defineComponent({
      setup: () => () => h(ModelDirectivesManagement),
    }))
    app.mount(root)
    await flushPromises()

    const moduleSwitch = root.querySelector(
      'button[aria-label="启用模型后缀参数模块"]',
    ) as HTMLButtonElement
    expect(moduleSwitch.getAttribute('aria-pressed')).toBe('true')
    expect(toastMocks.error).toHaveBeenCalledWith('获取模型后缀参数配置失败')
  })
})
