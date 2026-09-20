import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, type App } from '@/test/vue'

import ProviderAdvancedSection from '../ProviderAdvancedSection.vue'

const mountedApps: Array<{ app: App, root: HTMLElement }> = []

function mountSection(options: {
  attempts?: number
  enabled?: boolean
  loading?: boolean
  saving?: boolean
  loadError?: boolean
  hasChanges?: boolean
} = {}) {
  const root = document.createElement('div')
  document.body.appendChild(root)
  const onAttemptsUpdate = vi.fn()
  const onCooldownUpdate = vi.fn()
  const onSave = vi.fn()
  const onUpdate = vi.fn()
  const onTelemetryUpdate = vi.fn()
  const onStateUpdate = vi.fn()
  const onFallbackUpdate = vi.fn()
  const app = createApp(ProviderAdvancedSection, {
    turnStateEnabled: false,
    turnStateProbeAttempts: options.attempts ?? 3,
    turnStateProbeCooldownSeconds: 30,
    turnStateFallback: 'passthrough',
    telemetryEnabled: false,
    enabled: options.enabled ?? false,
    loading: options.loading ?? false,
    saving: options.saving ?? false,
    loadError: options.loadError ?? false,
    hasChanges: options.hasChanges ?? true,
    onSave,
    'onUpdate:turnStateProbeAttempts': onAttemptsUpdate,
    'onUpdate:turnStateProbeCooldownSeconds': onCooldownUpdate,
    'onUpdate:enabled': onUpdate,
    'onUpdate:telemetryEnabled': onTelemetryUpdate,
    'onUpdate:turnStateEnabled': onStateUpdate,
    'onUpdate:turnStateFallback': onFallbackUpdate,
  })
  app.mount(root)
  mountedApps.push({ app, root })
  return { root, onAttemptsUpdate, onCooldownUpdate, onSave, onUpdate, onTelemetryUpdate, onStateUpdate, onFallbackUpdate }
}

afterEach(() => {
  for (const { app, root } of mountedApps.splice(0)) {
    app.unmount()
    root.remove()
  }
  document.body.innerHTML = ''
})

describe('ProviderAdvancedSection', () => {
  it('shows short collection defaults and emits numeric settings', async () => {
    const { root, onAttemptsUpdate, onCooldownUpdate } = mountSection()
    await nextTick()
    const attempts = root.querySelector<HTMLInputElement>('#codex-state-attempts')!
    const cooldown = root.querySelector<HTMLInputElement>('#codex-state-cooldown')!
    expect(attempts.value).toBe('3')
    expect(cooldown.value).toBe('30')
    attempts.value = '2'
    attempts.dispatchEvent(new Event('input'))
    cooldown.value = '60'
    cooldown.dispatchEvent(new Event('input'))
    expect(onAttemptsUpdate).toHaveBeenCalledWith(2)
    expect(onCooldownUpdate).toHaveBeenCalledWith(60)
  })
  it('prevents saving an invalid collection limit', async () => {
    const { root } = mountSection({ attempts: 0 })
    await nextTick()
    expect(root.querySelector<HTMLButtonElement>('button')?.disabled).toBe(true)
    expect(root.querySelector('[role="alert"]')?.textContent).toContain('采集次数须为')
  })

  it('keeps state collection opt-in and separate from identity and telemetry', async () => {
    const { root, onStateUpdate, onUpdate, onTelemetryUpdate } = mountSection()
    await nextTick()
    const toggle = root.querySelector<HTMLButtonElement>('#codex-turn-state')
    if (!toggle) throw new Error('state switch missing')
    expect(toggle.getAttribute('aria-checked')).toBe('false')
    expect(root.textContent).toContain('采集会消耗上游额度')
    toggle.click()
    expect(onStateUpdate).toHaveBeenCalledWith(true)
    expect(onUpdate).not.toHaveBeenCalled()
    expect(onTelemetryUpdate).not.toHaveBeenCalled()
  })
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

  it('defaults to passthrough and allows explicitly selecting strict mode', async () => {
    const { root, onFallbackUpdate } = mountSection()
    await nextTick()
    const select = root.querySelector<HTMLSelectElement>('#codex-turn-state-fallback')
    if (!select) throw new Error('state fallback selector missing')
    expect(select.value).toBe('passthrough')
    select.value = 'strict'
    select.dispatchEvent(new Event('change'))
    expect(onFallbackUpdate).toHaveBeenCalledWith('strict')
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
