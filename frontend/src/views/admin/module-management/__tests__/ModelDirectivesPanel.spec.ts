import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, h, nextTick, ref, type App } from 'vue'

import ModelDirectivesPanel from '../ModelDirectivesPanel.vue'
import { createDefaultModelDirectivesConfig } from '../modelDirectivesConfig'

vi.mock('@/components/ui', async () => {
  const { defineComponent, h } = await import('vue')
  const passthrough = (tag: string) => defineComponent({
    inheritAttrs: false,
    setup(_, { attrs, slots }) {
      return () => h(tag, attrs, slots.default?.())
    },
  })
  return {
    Button: passthrough('button'),
    Select: defineComponent({
      inheritAttrs: false,
      props: { modelValue: String, disabled: Boolean },
      emits: ['update:modelValue'],
      setup(props, { emit, slots }) {
        return () => h('select', {
          value: props.modelValue,
          disabled: props.disabled,
          onChange: (event: Event) => emit(
            'update:modelValue',
            (event.target as HTMLSelectElement).value,
          ),
        }, slots.default?.())
      },
    }),
    SelectContent: passthrough('optgroup'),
    SelectItem: defineComponent({
      inheritAttrs: false,
      props: { value: { type: String, required: true } },
      setup(props, { slots }) {
        return () => h('option', { value: props.value }, slots.default?.())
      },
    }),
    SelectTrigger: defineComponent({ setup: () => () => null }),
    SelectValue: defineComponent({ setup: () => () => null }),
  }
})

vi.mock('@/components/ui/switch.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    default: defineComponent({
      inheritAttrs: false,
      props: { modelValue: Boolean, disabled: Boolean },
      emits: ['update:modelValue'],
      setup(props, { attrs, emit }) {
        return () => h('button', {
          ...attrs,
          type: 'button',
          disabled: props.disabled,
          'aria-pressed': props.modelValue,
          onClick: () => emit('update:modelValue', !props.modelValue),
        })
      },
    }),
  }
})

vi.mock('@/components/ui/textarea.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    default: defineComponent({
      inheritAttrs: false,
      props: { modelValue: String, disabled: Boolean },
      emits: ['update:modelValue'],
      setup(props, { attrs, emit }) {
        return () => h('textarea', {
          ...attrs,
          value: props.modelValue,
          disabled: props.disabled,
          onInput: (event: Event) => emit(
            'update:modelValue',
            (event.target as HTMLTextAreaElement).value,
          ),
        })
      },
    }),
  }
})

vi.mock('@/components/ui/input.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    default: defineComponent({
      inheritAttrs: false,
      props: { modelValue: String, disabled: Boolean },
      emits: ['update:modelValue'],
      setup(props, { attrs, emit }) {
        return () => h('input', {
          ...attrs,
          value: props.modelValue,
          disabled: props.disabled,
          onInput: (event: Event) => emit(
            'update:modelValue',
            (event.target as HTMLInputElement).value,
          ),
        })
      },
    }),
  }
})

const mountedApps: Array<{ app: App, root: HTMLElement }> = []

function mountPanel(onSave?: (value: ReturnType<typeof createDefaultModelDirectivesConfig>) => void) {
  const root = document.createElement('div')
  document.body.appendChild(root)
  const config = ref(createDefaultModelDirectivesConfig())
  const app = createApp(defineComponent({
    setup() {
      return () => h(ModelDirectivesPanel, {
        config: config.value,
        loading: false,
        onSave,
      })
    },
  }))
  app.mount(root)
  mountedApps.push({ app, root })
  return { config, root }
}

function selectSuffix(select: HTMLSelectElement, suffix: string) {
  select.value = suffix
  select.dispatchEvent(new Event('change', { bubbles: true }))
}

afterEach(() => {
  for (const { app, root } of mountedApps.splice(0)) {
    app.unmount()
    root.remove()
  }
})

describe('ModelDirectivesPanel', () => {
  it('shows Codex ultra and authoritative custom suffixes in the OpenAI selector', async () => {
    const { config, root } = mountPanel()
    const suffixSelect = root.querySelectorAll('select').item(1) as HTMLSelectElement
    expect(suffixSelect.value).toBe('low')
    expect([...suffixSelect.options].map(option => option.value)).toContain('ultra')
    const searchSuffixSelect = root.querySelectorAll('select').item(3) as HTMLSelectElement
    expect([...searchSuffixSelect.options].map(option => option.value)).not.toContain('fast')

    const builtInMapping = root.querySelector(
      'textarea[aria-label="OpenAI Responses low 映射参数"]',
    ) as HTMLTextAreaElement
    expect(JSON.parse(builtInMapping.value)).toEqual({ reasoning: { effort: 'low' } })
    expect(root.textContent).toContain('内置映射预览（运行时按目标模型调整）')

    const current = config.value.reasoning_effort.api_formats['openai:responses']
    config.value = {
      ...config.value,
      reasoning_effort: {
        ...config.value.reasoning_effort,
        api_formats: {
          ...config.value.reasoning_effort.api_formats,
          'openai:responses': {
            ...current,
            suffixes: [...current.suffixes, 'vendor-future'],
            mappings: {
              ...current.mappings,
              'mapped-future': { vendor_option: true },
            },
          },
        },
      },
    }
    await nextTick()

    expect([...suffixSelect.options].map(option => option.value)).toEqual(expect.arrayContaining([
      'ultra',
      'vendor-future',
      'mapped-future',
    ]))
    selectSuffix(suffixSelect, 'ultra')
    await nextTick()
    expect(root.textContent).toContain('Codex Ultra 推理预设，仅支持兼容的 Codex 模型')
  })

  it('retains per-suffix drafts and validation state while switching suffixes', async () => {
    const { root } = mountPanel()
    const suffixSelect = root.querySelectorAll('select').item(1) as HTMLSelectElement
    const lowMapping = root.querySelector(
      'textarea[aria-label="OpenAI Responses low 映射参数"]',
    ) as HTMLTextAreaElement

    lowMapping.value = '{'
    lowMapping.dispatchEvent(new Event('input', { bubbles: true }))
    await nextTick()
    const saveButton = root.querySelector(
      'button[aria-label="保存 OpenAI Responses 映射参数"]',
    ) as HTMLButtonElement
    saveButton.click()
    await nextTick()
    expect(root.textContent).toContain('JSON 格式无效，请修正后再保存')

    selectSuffix(suffixSelect, 'medium')
    await nextTick()
    selectSuffix(suffixSelect, 'low')
    await nextTick()

    const restoredDraft = root.querySelector(
      'textarea[aria-label="OpenAI Responses low 映射参数"]',
    ) as HTMLTextAreaElement
    expect(restoredDraft.value).toBe('{')
    expect(restoredDraft.getAttribute('aria-invalid')).toBe('true')
    expect(root.textContent).toContain('JSON 格式无效，请修正后再保存')
  })

  it('refreshes cached clean drafts when authoritative config changes', async () => {
    const { config, root } = mountPanel()
    const suffixSelect = root.querySelectorAll('select').item(1) as HTMLSelectElement

    selectSuffix(suffixSelect, 'medium')
    await nextTick()
    selectSuffix(suffixSelect, 'low')
    await nextTick()

    const current = config.value.reasoning_effort.api_formats['openai:responses']
    config.value = {
      ...config.value,
      reasoning_effort: {
        ...config.value.reasoning_effort,
        api_formats: {
          ...config.value.reasoning_effort.api_formats,
          'openai:responses': {
            ...current,
            mappings: {
              ...current.mappings,
              medium: { reasoning: { effort: 'high' } },
            },
          },
        },
      },
    }
    await nextTick()

    selectSuffix(suffixSelect, 'medium')
    await nextTick()
    const refreshedDraft = root.querySelector(
      'textarea[aria-label="OpenAI Responses medium 映射参数"]',
    ) as HTMLTextAreaElement
    expect(JSON.parse(refreshedDraft.value)).toEqual({ reasoning: { effort: 'high' } })
  })

  it('keeps a valid mapping draft retryable until the saved config becomes authoritative', async () => {
    const savedConfigs: Array<ReturnType<typeof createDefaultModelDirectivesConfig>> = []
    const { root } = mountPanel(value => savedConfigs.push(value))
    const mapping = root.querySelector(
      'textarea[aria-label="OpenAI Responses low 映射参数"]',
    ) as HTMLTextAreaElement
    mapping.value = '{"reasoning":{"effort":"medium"}}'
    mapping.dispatchEvent(new Event('input', { bubbles: true }))
    await nextTick()

    const saveButton = root.querySelector(
      'button[aria-label="保存 OpenAI Responses 映射参数"]',
    ) as HTMLButtonElement
    saveButton.click()
    await nextTick()
    expect(savedConfigs).toHaveLength(1)
    expect(savedConfigs[0].reasoning_effort.api_formats['openai:responses'].mappings.low)
      .toEqual({ reasoning: { effort: 'medium' } })
    expect(saveButton.disabled).toBe(false)

    saveButton.click()
    await nextTick()
    expect(savedConfigs).toHaveLength(2)
  })

  it('selects the first enabled suffix when the loaded config disables low', async () => {
    const { config, root } = mountPanel()
    const current = config.value.reasoning_effort.api_formats['openai:responses']
    config.value = {
      ...config.value,
      reasoning_effort: {
        ...config.value.reasoning_effort,
        api_formats: {
          ...config.value.reasoning_effort.api_formats,
          'openai:responses': {
            ...current,
            suffixes: ['high'],
            mappings: {},
          },
        },
      },
    }
    await nextTick()

    const suffixSelect = root.querySelectorAll('select').item(1) as HTMLSelectElement
    expect(suffixSelect.value).toBe('high')
    const mapping = root.querySelector(
      'textarea[aria-label="OpenAI Responses high 映射参数"]',
    ) as HTMLTextAreaElement
    expect(JSON.parse(mapping.value)).toEqual({ reasoning: { effort: 'high' } })
  })

  it('enables a custom suffix atomically with its first mapping', async () => {
    const savedConfigs: Array<ReturnType<typeof createDefaultModelDirectivesConfig>> = []
    const { config, root } = mountPanel(value => savedConfigs.push(value))

    const addButton = root.querySelector(
      'button[aria-label="新增 OpenAI Responses 自定义后缀"]',
    ) as HTMLButtonElement
    addButton.click()
    await nextTick()
    const input = root.querySelector(
      'input[aria-label="OpenAI Responses 自定义后缀名称"]',
    ) as HTMLInputElement
    input.value = 'vendor-depth'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    await nextTick()
    const confirmButton = root.querySelector(
      'button[aria-label="添加 OpenAI Responses 自定义后缀"]',
    ) as HTMLButtonElement
    confirmButton.click()
    await nextTick()

    const suffixSelect = root.querySelectorAll('select').item(1) as HTMLSelectElement
    expect(suffixSelect.value).toBe('vendor-depth')
    expect([...suffixSelect.options].map(option => option.value)).toContain('vendor-depth')
    expect(savedConfigs).toHaveLength(0)
    expect(root.textContent).toContain('保存非空映射后启用此后缀')

    const mapping = root.querySelector(
      'textarea[aria-label="OpenAI Responses vendor-depth 映射参数"]',
    ) as HTMLTextAreaElement
    mapping.value = '{"vendor_option":"deep"}'
    mapping.dispatchEvent(new Event('input', { bubbles: true }))
    await nextTick()
    const saveButton = root.querySelector(
      'button[aria-label="保存 OpenAI Responses 映射参数"]',
    ) as HTMLButtonElement
    saveButton.click()
    await nextTick()

    expect(savedConfigs).toHaveLength(1)
    const saved = savedConfigs[0].reasoning_effort.api_formats['openai:responses']
    expect(saved.suffixes).toContain('vendor-depth')
    expect(saved.mappings['vendor-depth']).toEqual({ vendor_option: 'deep' })

    config.value = savedConfigs[0]
    await nextTick()
    expect(suffixSelect.value).toBe('vendor-depth')
  })

  it('removes one custom override without disabling its suffix', async () => {
    const savedConfigs: Array<ReturnType<typeof createDefaultModelDirectivesConfig>> = []
    const { config, root } = mountPanel(value => savedConfigs.push(value))
    const current = config.value.reasoning_effort.api_formats['openai:responses']
    config.value = {
      ...config.value,
      reasoning_effort: {
        ...config.value.reasoning_effort,
        api_formats: {
          ...config.value.reasoning_effort.api_formats,
          'openai:responses': {
            ...current,
            mappings: {
              low: { reasoning: { effort: 'medium', summary: 'auto' } },
            },
          },
        },
      },
    }
    await nextTick()

    const resetButton = root.querySelector(
      'button[aria-label="恢复 OpenAI Responses 内置映射"]',
    ) as HTMLButtonElement
    resetButton.click()
    await nextTick()

    expect(savedConfigs).toHaveLength(1)
    const saved = savedConfigs[0].reasoning_effort.api_formats['openai:responses']
    expect(saved.mappings).not.toHaveProperty('low')
    expect(saved.suffixes).toContain('low')
  })

  it('disables a persisted custom suffix when its mapping is cleared', async () => {
    const savedConfigs: Array<ReturnType<typeof createDefaultModelDirectivesConfig>> = []
    const { config, root } = mountPanel(value => savedConfigs.push(value))
    const current = config.value.reasoning_effort.api_formats['openai:responses']
    config.value = {
      ...config.value,
      reasoning_effort: {
        ...config.value.reasoning_effort,
        api_formats: {
          ...config.value.reasoning_effort.api_formats,
          'openai:responses': {
            ...current,
            suffixes: [...current.suffixes, 'vendor-depth'],
            mappings: { 'vendor-depth': { vendor_option: 'deep' } },
          },
        },
      },
    }
    await nextTick()

    const suffixSelect = root.querySelectorAll('select').item(1) as HTMLSelectElement
    selectSuffix(suffixSelect, 'vendor-depth')
    await nextTick()
    const mapping = root.querySelector(
      'textarea[aria-label="OpenAI Responses vendor-depth 映射参数"]',
    ) as HTMLTextAreaElement
    mapping.value = '{}'
    mapping.dispatchEvent(new Event('input', { bubbles: true }))
    await nextTick()

    const saveButton = root.querySelector(
      'button[aria-label="保存 OpenAI Responses 映射参数"]',
    ) as HTMLButtonElement
    saveButton.click()
    await nextTick()

    expect(savedConfigs).toHaveLength(1)
    const saved = savedConfigs[0].reasoning_effort.api_formats['openai:responses']
    expect(saved.suffixes).not.toContain('vendor-depth')
    expect(saved.mappings).not.toHaveProperty('vendor-depth')
  })

  it('stores only the custom difference from the visible built-in mapping', async () => {
    const savedConfigs: Array<ReturnType<typeof createDefaultModelDirectivesConfig>> = []
    const { root } = mountPanel(value => savedConfigs.push(value))
    const mapping = root.querySelector(
      'textarea[aria-label="OpenAI Responses low 映射参数"]',
    ) as HTMLTextAreaElement
    mapping.value = JSON.stringify({
      reasoning: { effort: 'low', summary: 'auto' },
      trace: true,
    })
    mapping.dispatchEvent(new Event('input', { bubbles: true }))
    await nextTick()

    const saveButton = root.querySelector(
      'button[aria-label="保存 OpenAI Responses 映射参数"]',
    ) as HTMLButtonElement
    saveButton.click()
    await nextTick()

    expect(savedConfigs).toHaveLength(1)
    expect(savedConfigs[0].reasoning_effort.api_formats['openai:responses'].mappings.low)
      .toEqual({ reasoning: { summary: 'auto' }, trace: true })
    expect(JSON.parse(mapping.value)).toEqual({
      reasoning: { effort: 'low', summary: 'auto' },
      trace: true,
    })
  })
})
