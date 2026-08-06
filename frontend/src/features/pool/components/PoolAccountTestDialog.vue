<template>
  <ModelTestDialog
    v-if="provider"
    :open="modelTest.dialogOpen.value"
    :result="modelTest.testResult.value"
    :mode="modelTest.testMode.value"
    :provider-type="provider.provider_type"
    :model-options="testModelOptions"
    :selected-model-value="pendingTestModel?.id || null"
    :selecting-model-name="pendingTestModel ? (pendingTestModel.global_model_display_name || pendingTestModel.provider_model_name) : null"
    :requested-model-name="pendingRequestedModelName"
    :endpoints="testEndpoints"
    :selected-endpoint="selectedTestEndpoint"
    :testing="modelTest.testing.value"
    :trace="modelTest.testTrace.value"
    :request-id="modelTest.requestId.value"
    :request-headers-draft="testRequestHeadersDraft"
    :request-headers-reset-value="testRequestHeadersResetValue"
    :request-headers-error="testRequestHeadersError"
    :request-body-draft="testRequestBodyDraft"
    :request-body-reset-value="testRequestBodyResetValue"
    :request-body-error="testRequestBodyError"
    :model-mapping-available="testModelMappingAvailable"
    :model-mapping-options="testModelMappingOptions"
    :selected-model-mapping="selectedTestMappedModelName"
    :start-disabled="!selectedTestEndpoint || !!testRequestHeadersError || !!testRequestBodyError"
    @close="handleClose"
    @back="handleBack"
    @start="handleStart"
    @select-endpoint="handleSelectEndpoint"
    @select-model="handleSelectModel"
    @select-model-mapping="handleSelectModelMapping"
    @update:request-headers-draft="testRequestHeadersDraft = $event"
    @update:request-body-draft="testRequestBodyDraft = $event"
  />
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { getProviderEndpoints, type Model, type ProviderEndpoint } from '@/api/endpoints'
import { getProviderModels } from '@/api/endpoints/models'
import type { EndpointAPIKey } from '@/api/endpoints/keys'
import type { ProviderWithEndpointsSummary } from '@/api/endpoints/types/provider'
import { useModelTest } from '@/composables/useModelTest'
import { useToast } from '@/composables/useToast'
import { parseApiError } from '@/utils/errorParser'
import { formatApiFormat } from '@/api/endpoints/types/api-format'
import ModelTestDialog from '@/features/providers/components/provider-tabs/ModelTestDialog.vue'
import {
  buildDefaultModelTestRequestBody,
  buildDefaultModelTestRequestHeaders,
  isModelTestableEndpoint,
  listModelTestMappedModelOptions,
  modelTestKeySupportsEndpoint,
  normalizeModelTestMappedModelSelection,
  parseModelTestRequestBodyDraft,
  parseModelTestRequestHeadersDraft,
  selectPreferredModelTestEndpoint,
  syncModelTestRequestBodyDraft,
} from '@/features/providers/components/provider-tabs/model-test-request'

const props = defineProps<{
  provider: ProviderWithEndpointsSummary | null
}>()

const { t } = useI18n()
const { error: showError } = useToast()
const modelTest = useModelTest({ providerId: () => props.provider?.id || '' })

const accountKey = ref<EndpointAPIKey | null>(null)
const models = ref<Model[]>([])
const endpoints = ref<ProviderEndpoint[]>([])
const pendingTestModel = ref<Model | null>(null)
const selectedTestEndpoint = ref<ProviderEndpoint | null>(null)
const selectedTestMappedModelName = ref<string | null>(null)
const testRequestHeadersDraft = ref('')
const testRequestHeadersResetValue = ref('')
const testRequestBodyDraft = ref('')
const testRequestBodyResetValue = ref('')
let accountTestLoadGeneration = 0

const sortedModels = computed(() => [...models.value]
  .sort((a, b) => {
    const nameA = (a.global_model_display_name || a.provider_model_name || '').toLowerCase()
    const nameB = (b.global_model_display_name || b.provider_model_name || '').toLowerCase()
    return nameA.localeCompare(nameB)
  }))
const testModelOptions = computed(() => sortedModels.value
  .filter(model => model.is_active !== false)
  .map(model => ({
    value: model.id,
    label: model.global_model_display_name || model.provider_model_name,
  })))
const testEndpoints = computed(() => {
  const key = accountKey.value
  const provider = props.provider
  if (!key || !provider) return []
  return endpoints.value.filter(endpoint => (
    isModelTestableEndpoint(endpoint, [key], provider.provider_type)
    && modelTestKeySupportsEndpoint(key, endpoint, provider.provider_type)
  ))
})
const parsedTestRequestHeaders = computed(() => (
  parseModelTestRequestHeadersDraft(testRequestHeadersDraft.value)
))
const testRequestHeadersError = computed(() => parsedTestRequestHeaders.value.error)
const parsedTestRequestBody = computed(() => (
  parseModelTestRequestBodyDraft(testRequestBodyDraft.value)
))
const testRequestBodyError = computed(() => parsedTestRequestBody.value.error)
const pendingRequestedModelName = computed(() => (
  pendingTestModel.value?.global_model_name || pendingTestModel.value?.provider_model_name || ''
))
const testModelMappingOptions = computed(() => {
  const requestedModelName = pendingRequestedModelName.value.trim()
  return listModelTestMappedModelOptions(pendingTestModel.value, selectedTestEndpoint.value)
    .filter(option => option.name !== requestedModelName)
})
const mappedTestModelName = computed(() => {
  const selected = selectedTestMappedModelName.value?.trim()
  if (!selected) return null
  return testModelMappingOptions.value.some(option => option.name === selected)
    ? selected
    : null
})
const testModelMappingAvailable = computed(() => testModelMappingOptions.value.length > 0)
const effectiveTestRequestModelName = computed(() => (
  mappedTestModelName.value || pendingRequestedModelName.value
))

async function openAccountTest(key: EndpointAPIKey): Promise<boolean> {
  const provider = props.provider
  if (!provider || modelTest.testing.value) return false
  const loadGeneration = ++accountTestLoadGeneration

  try {
    const [loadedEndpoints, loadedModels] = await Promise.all([
      getProviderEndpoints(provider.id),
      getProviderModels(provider.id),
    ])
    if (
      loadGeneration !== accountTestLoadGeneration
      || props.provider?.id !== provider.id
    ) {
      return false
    }
    accountKey.value = key
    endpoints.value = loadedEndpoints
    models.value = loadedModels
  } catch (error) {
    if (loadGeneration !== accountTestLoadGeneration) return false
    showError(parseApiError(error, t('poolAccountTest.loadFailed')))
    return false
  }

  const model = sortedModels.value.find(item => item.is_active !== false)
  if (!model) {
    showError(t('poolAccountTest.noModels'))
    return false
  }
  if (testEndpoints.value.length === 0) {
    showError(t('poolAccountTest.noEndpoints'))
    return false
  }

  pendingTestModel.value = model
  selectedTestEndpoint.value = selectPreferredModelTestEndpoint(model, testEndpoints.value)
  selectedTestMappedModelName.value = null
  testRequestHeadersResetValue.value = buildDefaultModelTestRequestHeaders()
  testRequestHeadersDraft.value = testRequestHeadersResetValue.value
  testRequestBodyResetValue.value = buildDefaultModelTestRequestBody(
    pendingRequestedModelName.value,
    selectedTestEndpoint.value?.api_format,
    model,
  )
  testRequestBodyDraft.value = testRequestBodyResetValue.value
  modelTest.testResult.value = null
  modelTest.dialogOpen.value = true
  return true
}

function handleClose() {
  modelTest.resetState()
  accountKey.value = null
  pendingTestModel.value = null
  selectedTestEndpoint.value = null
  selectedTestMappedModelName.value = null
  testRequestHeadersDraft.value = ''
  testRequestHeadersResetValue.value = ''
  testRequestBodyDraft.value = ''
  testRequestBodyResetValue.value = ''
}

function handleBack() {
  if (modelTest.testing.value) return
  modelTest.testResult.value = null
  modelTest.stopPolling()
}

function handleSelectEndpoint(endpointId: string) {
  const endpoint = testEndpoints.value.find(item => item.id === endpointId)
  if (!endpoint) return
  selectedTestEndpoint.value = endpoint
  syncSelectedTestModelMapping()
  resetTestRequestBodyForSelectedEndpoint()
}

function handleSelectModel(modelId: string) {
  const model = sortedModels.value.find(item => item.id === modelId)
  if (!model) return
  pendingTestModel.value = model
  selectedTestMappedModelName.value = null
  selectedTestEndpoint.value = selectPreferredModelTestEndpoint(model, testEndpoints.value)
  testRequestBodyResetValue.value = buildDefaultModelTestRequestBody(
    pendingRequestedModelName.value,
    selectedTestEndpoint.value?.api_format,
    model,
  )
  testRequestBodyDraft.value = testRequestBodyResetValue.value
}

function handleSelectModelMapping(modelName: string) {
  selectedTestMappedModelName.value = normalizeModelTestMappedModelSelection(
    testModelMappingOptions.value,
    modelName,
  )
  syncTestRequestBodyModel()
}

async function handleStart() {
  const key = accountKey.value
  const model = pendingTestModel.value
  if (!key || !model || modelTest.testing.value) return
  const endpoint = selectedTestEndpoint.value || testEndpoints.value[0]
  if (!endpoint) {
    showError(t('providerModelsTab.selectTestEndpoint'))
    return
  }

  const { value: requestHeaders, error: headersError } = parsedTestRequestHeaders.value
  if (!requestHeaders || headersError) {
    showError(t('providerModelsTab.invalidHeaders', {
      error: headersError || t('providerModelsTab.invalidJson'),
    }))
    return
  }
  const { value: requestBody, error: bodyError } = parsedTestRequestBody.value
  if (!requestBody || bodyError) {
    showError(t('providerModelsTab.invalidBody', {
      error: bodyError || t('providerModelsTab.invalidJson'),
    }))
    return
  }

  selectedTestEndpoint.value = endpoint
  const modelName = model.global_model_name || model.provider_model_name
  await modelTest.startTest({
    mode: 'direct',
    modelName,
    displayLabel: `[${formatApiFormat(endpoint.api_format)}] ${modelName}`,
    apiFormat: endpoint.api_format,
    endpointId: endpoint.id,
    endpointBaseUrl: endpoint.base_url,
    apiKeyId: key.id,
    applyModelMapping: Boolean(mappedTestModelName.value),
    mappedModelName: mappedTestModelName.value ?? undefined,
    requestHeaders,
    requestBody,
  })
}

function syncSelectedTestModelMapping(preferredName?: string | null) {
  const options = testModelMappingOptions.value
  if (options.length === 0) {
    selectedTestMappedModelName.value = null
    return
  }
  const preferred = preferredName ?? selectedTestMappedModelName.value
  selectedTestMappedModelName.value = normalizeModelTestMappedModelSelection(options, preferred)
}

function syncTestRequestBodyModel() {
  const modelName = effectiveTestRequestModelName.value
  if (!modelName) return
  const resetDraft = testRequestBodyResetValue.value || buildDefaultModelTestRequestBody(
    modelName,
    selectedTestEndpoint.value?.api_format,
    pendingTestModel.value,
  )
  const next = syncModelTestRequestBodyDraft(
    testRequestBodyDraft.value,
    testRequestBodyResetValue.value,
    resetDraft,
    modelName,
  )
  testRequestBodyResetValue.value = next.resetValue
  testRequestBodyDraft.value = next.draft
}

function resetTestRequestBodyForSelectedEndpoint() {
  const modelName = effectiveTestRequestModelName.value
  if (!modelName) return
  const nextResetValue = buildDefaultModelTestRequestBody(
    modelName,
    selectedTestEndpoint.value?.api_format,
    pendingTestModel.value,
  )
  const next = syncModelTestRequestBodyDraft(
    testRequestBodyDraft.value,
    testRequestBodyResetValue.value,
    nextResetValue,
    modelName,
  )
  testRequestBodyResetValue.value = next.resetValue
  testRequestBodyDraft.value = next.draft
}

watch(
  [effectiveTestRequestModelName, () => selectedTestEndpoint.value?.api_format],
  () => syncTestRequestBodyModel(),
)

watch(
  () => props.provider?.id,
  (providerId, previousProviderId) => {
    if (providerId === previousProviderId) return
    accountTestLoadGeneration += 1
    handleClose()
  },
)

defineExpose({ openAccountTest })
</script>
