/* eslint-disable vue/one-component-per-file */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, type App } from '@/test/vue'

const getManagedInstructionProfiles = vi.hoisted(() => vi.fn())

vi.mock('@/api/users', () => ({
  usersApi: {
    getManagedInstructionProfiles,
  },
}))

vi.mock('@/components/ui', async () => {
  const { defineComponent, h } = await import('vue')
  const passthrough = (name: string, tag = 'div') => defineComponent({
    name,
    setup(_, { attrs, slots }) {
      return () => h(tag, attrs, slots.default?.())
    },
  })
  return {
    Badge: passthrough('BadgeStub', 'span'),
    Button: passthrough('ButtonStub', 'button'),
    Label: passthrough('LabelStub', 'label'),
    Select: defineComponent({
      name: 'SelectStub',
      props: {
        modelValue: {
          type: String,
          default: '',
        },
        disabled: Boolean,
      },
      emits: ['update:modelValue'],
      setup(props, { attrs, emit, slots }) {
        return () => h('button', {
          ...attrs,
          type: 'button',
          disabled: props.disabled,
          'data-model-value': props.modelValue,
          onClick: () => emit('update:modelValue', 'adult_fiction_v1'),
        }, slots.default?.())
      },
    }),
    SelectContent: passthrough('SelectContentStub'),
    SelectItem: passthrough('SelectItemStub', 'span'),
    SelectTrigger: passthrough('SelectTriggerStub'),
    SelectValue: passthrough('SelectValueStub', 'span'),
    Skeleton: passthrough('SkeletonStub'),
    Switch: defineComponent({
      name: 'SwitchStub',
      props: {
        modelValue: Boolean,
        disabled: Boolean,
      },
      emits: ['update:modelValue'],
      setup(props, { attrs, emit }) {
        return () => h('button', {
          ...attrs,
          type: 'button',
          role: 'switch',
          disabled: props.disabled,
          'aria-checked': String(props.modelValue),
          onClick: () => emit('update:modelValue', !props.modelValue),
        })
      },
    }),
  }
})

import ManagedInstructionsConfigSection from '../ManagedInstructionsConfigSection.vue'

const registry = {
  profiles: [
    {
      profile_id: 'security_research_v1',
      display_name: 'Security & Reverse Engineering',
      description: 'Security and reverse engineering',
      core_version: 'core_v2',
      domain_version: 'security_research_v2',
      profile_sha256: 'a'.repeat(64),
    },
    {
      profile_id: 'adult_fiction_v1',
      display_name: 'Adult Fiction',
      description: 'Adult fiction',
      core_version: 'core_v2',
      domain_version: 'adult_fiction_v1',
      profile_sha256: 'b'.repeat(64),
    },
  ],
  merge_modes: ['prepend', 'if_missing'],
  supported_provider_api_formats: ['openai:responses', 'openai:chat', 'claude:messages'],
  composition_order: ['managed_instructions', 'client_instructions', 'image_generation_bridge'],
}

const mountedApps: Array<{ app: App, root: HTMLElement }> = []

function mountSection(
  modelValue: {
    enabled: boolean
    profile_id: string
    merge_mode: 'prepend' | 'if_missing'
  } | null = null,
  onUpdate = vi.fn()
) {
  const root = document.createElement('div')
  document.body.appendChild(root)
  const app = createApp(ManagedInstructionsConfigSection, {
    modelValue,
    'onUpdate:modelValue': onUpdate,
  })
  app.mount(root)
  mountedApps.push({ app, root })
  return { root, onUpdate }
}

async function flushUi() {
  await Promise.resolve()
  await Promise.resolve()
  await nextTick()
}

beforeEach(() => {
  getManagedInstructionProfiles.mockReset()
})

afterEach(() => {
  for (const { app, root } of mountedApps.splice(0)) {
    app.unmount()
    root.remove()
  }
})

describe('ManagedInstructionsConfigSection', () => {
  it('shows loading state, then renders the server version and hash', async () => {
    let resolveRequest: (value: typeof registry) => void = () => undefined
    getManagedInstructionProfiles.mockReturnValue(new Promise((resolve) => {
      resolveRequest = resolve
    }))
    const { root } = mountSection({
      enabled: false,
      profile_id: 'security_research_v1',
      merge_mode: 'prepend',
    })

    expect(root.querySelector('[data-testid="managed-instructions-loading"]')).not.toBeNull()
    resolveRequest(registry)
    await flushUi()

    const summary = root.querySelector('[data-testid="managed-instructions-summary"]')
    expect(summary?.textContent).toContain('core_v2')
    expect(summary?.textContent).toContain(`SHA-256 ${'a'.repeat(64)}`)
  })

  it('enables the default profile only after the registry has loaded', async () => {
    getManagedInstructionProfiles.mockResolvedValue(registry)
    const { root, onUpdate } = mountSection()
    await flushUi()

    root.querySelector<HTMLButtonElement>('[role="switch"]')?.click()
    await nextTick()

    expect(onUpdate).toHaveBeenCalledWith({
      enabled: true,
      profile_id: 'security_research_v1',
      merge_mode: 'prepend',
    })
  })

  it('keeps an unconfigured group distinct from a disabled configured group', async () => {
    getManagedInstructionProfiles.mockResolvedValue(registry)
    const { root } = mountSection()
    await flushUi()

    expect(root.querySelector('[data-testid="managed-instructions-summary"]')).toBeNull()
    expect(root.querySelector('[data-testid="managed-profile-select"]')?.getAttribute('data-model-value'))
      .toBe('')
    expect(root.querySelector('[data-testid="managed-instructions-unconfigured"]')?.textContent)
      .toContain('未配置')
  })

  it('shows a readable load error and can retry', async () => {
    getManagedInstructionProfiles
      .mockRejectedValueOnce(new Error('network unavailable'))
      .mockResolvedValueOnce(registry)
    const { root } = mountSection()
    await flushUi()

    expect(root.querySelector('[data-testid="managed-instructions-error"]')?.textContent)
      .toContain('network unavailable')
    root.querySelector<HTMLButtonElement>('[data-testid="managed-instructions-error"] button')?.click()
    await flushUi()

    expect(root.querySelector('[data-testid="managed-instructions-error"]')).toBeNull()
    expect(root.querySelector('[data-testid="managed-instructions-unconfigured"]')).not.toBeNull()
  })

  it('shows and repairs a removed profile while the feature stays disabled', async () => {
    getManagedInstructionProfiles.mockResolvedValue(registry)
    const onUpdate = vi.fn()
    const { root } = mountSection({
      enabled: false,
      profile_id: 'direct_v1',
      merge_mode: 'prepend',
    }, onUpdate)
    await flushUi()

    expect(root.textContent).toContain('direct_v1')
    const profileSelect = root.querySelector<HTMLButtonElement>(
      '[data-testid="managed-profile-select"]'
    )
    expect(profileSelect?.disabled).toBe(false)

    profileSelect?.click()
    await nextTick()

    expect(onUpdate).toHaveBeenCalledWith({
      enabled: false,
      profile_id: 'adult_fiction_v1',
      merge_mode: 'prepend',
    })
  })
})
