import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, type App } from '@/test/vue'

import ProviderAdvancedSection from '../ProviderAdvancedSection.vue'

const mountedApps: Array<{ app: App, root: HTMLElement }> = []

function mountSection(options: {
  enabled?: boolean
  loading?: boolean
  saving?: boolean
  loadError?: boolean
  hasChanges?: boolean
} = {}) {
  const root = document.createElement('div')
  document.body.appendChild(root)
  const onSave = vi.fn()
  const onUpdate = vi.fn()
  const onTelemetryUpdate = vi.fn()
  const app = createApp(ProviderAdvancedSection, {
    telemetryEnabled: false,
    enabled: options.enabled ?? false,
    loading: options.loading ?? false,
    saving: options.saving ?? false,
    loadError: options.loadError ?? false,
    hasChanges: options.hasChanges ?? true,
    onSave,
    'onUpdate:enabled': onUpdate,
    'onUpdate:telemetryEnabled': onTelemetryUpdate,
  })
  app.mount(root)
  mountedApps.push({ app, root })
  return { root, onSave, onUpdate, onTelemetryUpdate }
}

afterEach(() => {
  for (const { app, root } of mountedApps.splice(0)) {
    app.unmount()
    root.remove()
  }
  document.body.innerHTML = ''
})

describe('ProviderAdvancedSection', () => {
  it('emits the global switch value and save action', async () => {
    const { root, onSave, onUpdate } = mountSection()
    await nextTick()

    const identitySwitch = root.querySelector<HTMLButtonElement>(
      '#codex-oauth-identity-convergence',
    )
    expect(identitySwitch).not.toBeNull()
    identitySwitch!.click()
    root.querySelector<HTMLButtonElement>('button')!.click()

    expect(onUpdate).toHaveBeenCalledWith(true)
    expect(onSave).toHaveBeenCalledOnce()
  })

  it('keeps telemetry off by default and emits only its own change', async () => {
    const { root, onUpdate, onTelemetryUpdate } = mountSection()
    await nextTick()
    const toggle = root.querySelector<HTMLButtonElement>('#codex-telemetry')!
    expect(toggle.getAttribute('aria-checked')).toBe('false')
    toggle.click()
    expect(onTelemetryUpdate).toHaveBeenCalledWith(true)
    expect(onUpdate).not.toHaveBeenCalled()
  })

  it('blocks editing and reports the load error', async () => {
    const { root } = mountSection({ loadError: true })
    await nextTick()

    expect(Array.from(root.querySelectorAll<HTMLButtonElement>('[role="switch"]')).every(button => button.disabled)).toBe(true)
    expect(root.querySelector('[role="alert"]')?.textContent).toContain('配置加载失败')
  })

  it('shows saving feedback and disables controls while saving', async () => {
    const { root } = mountSection({ saving: true })
    await nextTick()

    expect(root.textContent).toContain('保存中')
    expect(root.querySelector<HTMLButtonElement>('button')?.disabled).toBe(true)
    expect(Array.from(root.querySelectorAll<HTMLButtonElement>('[role="switch"]')).every(button => button.disabled)).toBe(true)
  })
})
