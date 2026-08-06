/* eslint-disable vue/one-component-per-file */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, h, nextTick, ref, type App } from '@/test/vue'
import { i18n } from '@/i18n'
import PoolAccountTestDialog from '@/features/pool/components/PoolAccountTestDialog.vue'

const endpointMocks = vi.hoisted(() => ({
  getProviderEndpoints: vi.fn(),
  getProviderModels: vi.fn(),
}))

const toastMocks = vi.hoisted(() => ({
  error: vi.fn(),
}))

const modelTestMocks = vi.hoisted(() => ({
  resetState: vi.fn(),
  startTest: vi.fn(),
  stopPolling: vi.fn(),
}))

vi.mock('@/api/endpoints', () => ({
  getProviderEndpoints: endpointMocks.getProviderEndpoints,
}))

vi.mock('@/api/endpoints/models', () => ({
  getProviderModels: endpointMocks.getProviderModels,
}))

vi.mock('@/composables/useToast', () => ({
  useToast: () => ({ error: toastMocks.error }),
}))

vi.mock('@/composables/useModelTest', async () => {
  const { ref } = await import('vue')
  return {
    useModelTest: () => {
      const dialogOpen = ref(false)
      const testResult = ref(null)
      const testing = ref(false)
      const testMode = ref('direct')
      const testTrace = ref(null)
      const requestId = ref(null)
      return {
        dialogOpen,
        testResult,
        testing,
        testMode,
        testTrace,
        requestId,
        resetState: () => {
          dialogOpen.value = false
          testResult.value = null
          modelTestMocks.resetState()
        },
        startTest: modelTestMocks.startTest,
        stopPolling: modelTestMocks.stopPolling,
      }
    },
  }
})

vi.mock('@/features/providers/components/provider-tabs/ModelTestDialog.vue', async () => {
  const { defineComponent } = await import('vue')
  return {
    default: defineComponent({
      name: 'ModelTestDialogStub',
      setup() {
        return () => null
      },
    }),
  }
})

interface Deferred<T> {
  promise: Promise<T>
  resolve: (value: T) => void
  reject: (reason?: unknown) => void
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

const providerA = {
  id: 'provider-a',
  name: 'Provider A',
  provider_type: 'codex',
  is_active: true,
}

const providerB = {
  id: 'provider-b',
  name: 'Provider B',
  provider_type: 'codex',
  is_active: true,
}

const accountKey = {
  id: 'key-a',
  provider_id: 'provider-a',
  name: 'Account A',
  api_formats: ['openai:responses'],
  auth_type: 'oauth',
  is_active: true,
}

const mountedApps: Array<{ app: App, root: HTMLElement }> = []

function mountDialog() {
  const provider = ref<Record<string, unknown> | null>(providerA)
  const dialog = ref<InstanceType<typeof PoolAccountTestDialog> | null>(null)
  const Harness = defineComponent({
    setup() {
      return () => h(PoolAccountTestDialog, {
        ref: dialog,
        provider: provider.value,
      })
    },
  })
  const root = document.createElement('div')
  document.body.appendChild(root)
  const app = createApp(Harness)
  app.mount(root)
  mountedApps.push({ app, root })
  return { provider, dialog, root }
}

beforeEach(() => {
  endpointMocks.getProviderEndpoints.mockReset()
  endpointMocks.getProviderModels.mockReset()
  toastMocks.error.mockReset()
  modelTestMocks.resetState.mockReset()
  modelTestMocks.startTest.mockReset()
  modelTestMocks.stopPolling.mockReset()
  i18n.global.locale.value = 'zh-CN'
})

afterEach(() => {
  for (const { app, root } of mountedApps.splice(0)) {
    app.unmount()
    root.remove()
  }
  i18n.global.locale.value = 'zh-CN'
})

describe('PoolAccountTestDialog', () => {
  it('discards loaded data when the selected provider changes', async () => {
    const endpoints = deferred<unknown[]>()
    const models = deferred<unknown[]>()
    endpointMocks.getProviderEndpoints.mockReturnValue(endpoints.promise)
    endpointMocks.getProviderModels.mockReturnValue(models.promise)
    const { provider, dialog } = mountDialog()
    await nextTick()

    const openResult = dialog.value?.openAccountTest(accountKey as never)
    provider.value = providerB
    await nextTick()
    endpoints.resolve([])
    models.resolve([])

    await expect(openResult).resolves.toBe(false)
    expect(modelTestMocks.resetState).toHaveBeenCalledTimes(1)
    expect(toastMocks.error).not.toHaveBeenCalled()
  })

  it('uses the English fallback when test data cannot be loaded', async () => {
    i18n.global.locale.value = 'en-US'
    endpointMocks.getProviderEndpoints.mockRejectedValue(undefined)
    endpointMocks.getProviderModels.mockResolvedValue([])
    const { dialog } = mountDialog()
    await nextTick()

    await expect(dialog.value?.openAccountTest(accountKey as never)).resolves.toBe(false)
    expect(toastMocks.error).toHaveBeenCalledWith('Failed to load test data')
  })
})
