<template>
  <Teleport to="body">
    <Transition name="fade">
      <div
        v-if="isOpen"
        class="fixed inset-0 z-[60] flex items-center justify-center"
        role="dialog"
        aria-modal="true"
        aria-labelledby="replay-dialog-title"
        @click.self="handleClose"
      >
        <div
          class="absolute inset-0 bg-black/30 backdrop-blur-sm"
          @click="handleClose"
        />
        <Card class="relative w-full max-w-6xl max-h-[85vh] min-h-[60vh] mx-4 shadow-2xl flex flex-col">
          <!-- 头部：标题 + 提供商/Key 选择 + 发送 -->
          <div class="px-4 py-2.5 border-b flex items-center gap-3 shrink-0 flex-wrap">
            <h3
              id="replay-dialog-title"
              class="text-sm font-semibold shrink-0"
            >
              {{ t('replayDialog.title') }}
            </h3>
            <Separator
              orientation="vertical"
              class="h-4"
            />

            <!-- 提供商选择 -->
            <div class="flex items-center gap-1.5 min-w-0">
              <label class="text-xs text-muted-foreground shrink-0">{{ t('replayDialog.provider') }}</label>
              <select
                v-model="selectedProviderId"
                class="h-7 rounded-md border border-input bg-background px-2 text-xs min-w-[140px]"
                :disabled="replaying"
                @change="onProviderChange"
              >
                <option value="">
                  {{ t('replayDialog.originalProvider', { provider: detail?.provider || '-' }) }}
                </option>
                <option
                  v-for="p in providers"
                  :key="p.id"
                  :value="p.id"
                >
                  {{ p.name }}
                </option>
              </select>
            </div>

            <!-- Key 选择 -->
            <div class="flex items-center gap-1.5 min-w-0">
              <label class="text-xs text-muted-foreground shrink-0">Key</label>
              <select
                v-model="selectedKeyId"
                class="h-7 rounded-md border border-input bg-background px-2 text-xs min-w-[140px]"
                :disabled="replaying || loadingKeys"
              >
                <option value="">
                  {{ loadingKeys ? t('replayDialog.loading') : selectedProviderId ? t('replayDialog.autoSelect') : t('replayDialog.originalKey') }}
                </option>
                <option
                  v-for="k in keys"
                  :key="k.id"
                  :value="k.id"
                >
                  {{ k.name }} ({{ k.api_key_masked }})
                </option>
              </select>
            </div>

            <!-- 右侧：发送 + 关闭 -->
            <div class="flex items-center gap-1 ml-auto shrink-0">
              <Button
                size="sm"
                :disabled="!canReplay"
                class="gap-1.5 h-7 text-xs"
                @click="doReplay"
              >
                <Loader2
                  v-if="replaying"
                  class="w-3.5 h-3.5 animate-spin"
                />
                <Play
                  v-else
                  class="w-3.5 h-3.5"
                />
                {{ replaying ? t('replayDialog.requesting') : t('replayDialog.send') }}
              </Button>
              <Button
                variant="ghost"
                size="icon"
                class="h-7 w-7"
                :aria-label="t('common.close')"
                @click="handleClose"
              >
                <X class="w-4 h-4" />
              </Button>
            </div>
          </div>

          <p class="px-4 py-2 text-xs text-muted-foreground border-b">
            {{ t('replayDialog.billingNote') }}
          </p>
          <p
            v-if="optionsError"
            role="alert"
            class="px-4 py-2 text-xs text-destructive border-b"
          >
            {{ optionsError }}
          </p>
          <!-- 双栏内容区 -->
          <div class="flex-1 min-h-0 flex">
            <!-- ===== 左栏：请求 ===== -->
            <div class="w-1/2 flex flex-col min-h-0 border-r">
              <!-- 左栏头 -->
              <div class="px-4 py-1.5 border-b bg-muted/30 flex items-center justify-between shrink-0">
                <div class="flex items-center gap-2 min-w-0">
                  <span class="text-xs font-medium text-muted-foreground">{{ t('replayDialog.request') }}</span>
                  <span
                    v-if="detail?.model"
                    class="text-[11px] text-muted-foreground/60 font-mono truncate"
                  >{{ detail.model }}</span>
                </div>
                <button
                  class="p-1 rounded transition-colors text-muted-foreground hover:bg-muted shrink-0"
                  :disabled="bodyLoading || Boolean(bodyProblem)"
                  :title="requestCopied ? t('replayDialog.copied') : t('replayDialog.copyRequestBody')"
                  @click="copyRequestBody"
                >
                  <Check
                    v-if="requestCopied"
                    class="w-3 h-3 text-green-500"
                  />
                  <Copy
                    v-else
                    class="w-3 h-3"
                  />
                </button>
              </div>
              <!-- 左栏内容 -->
              <div class="flex-1 overflow-y-auto scrollbar-stable">
                <!-- 请求头（可折叠） -->
                <div class="border-b">
                  <button
                    class="w-full px-4 py-1.5 flex items-center gap-1.5 text-xs text-muted-foreground hover:bg-muted/30 transition-colors"
                    @click="showRequestHeaders = !showRequestHeaders"
                  >
                    <ChevronRight
                      class="w-3 h-3 transition-transform shrink-0"
                      :class="{ 'rotate-90': showRequestHeaders }"
                    />
                    <span class="font-medium">Headers</span>
                    <span class="text-muted-foreground/60 ml-0.5">({{ requestHeaderCount }})</span>
                  </button>
                  <div
                    v-if="showRequestHeaders && hasRequestHeaders"
                    class="px-4 pb-2.5"
                  >
                    <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-[11px] font-mono">
                      <template
                        v-for="(value, key) in displayRequestHeaders"
                        :key="key"
                      >
                        <span class="text-muted-foreground/70 text-right select-none">{{ key }}</span>
                        <span class="break-all">{{ value }}</span>
                      </template>
                    </div>
                  </div>
                </div>
                <!-- 请求体 -->
                <div class="border-b">
                  <div class="px-4 py-1.5 flex items-center gap-1.5 text-xs text-muted-foreground">
                    <span class="w-3 h-3 shrink-0" />
                    <span class="font-medium">Body</span>
                  </div>
                </div>
                <div class="px-4 py-3">
                  <pre
                    v-if="formattedRequestBody && !bodyProblem && !bodyLoading"
                    class="text-xs font-mono whitespace-pre-wrap break-all leading-relaxed"
                  >{{ formattedRequestBody }}</pre>
                  <div
                    v-else
                    class="text-xs text-muted-foreground"
                  >
                    <span
                      v-if="bodyLoading"
                      role="status"
                    >{{ t('replayDialog.loadingBody') }}</span>
                    <span
                      v-else
                      role="alert"
                    >{{ bodyProblem }}</span>
                    <Button
                      v-if="bodyLoadError"
                      variant="outline"
                      size="sm"
                      class="mt-2 block"
                      @click="loadRequestBody"
                    >
                      {{ t('replayDialog.retryLoad') }}
                    </Button>
                  </div>
                </div>
              </div>
            </div>

            <!-- ===== 右栏：响应 ===== -->
            <div class="w-1/2 flex flex-col min-h-0">
              <!-- 右栏头 -->
              <div class="px-4 py-1.5 border-b bg-muted/30 flex items-center justify-between shrink-0">
                <div class="flex items-center gap-2 min-w-0">
                  <span class="text-xs font-medium text-muted-foreground">{{ t('replayDialog.response') }}</span>
                  <template v-if="replayResult">
                    <Badge
                      :variant="replayResult.status === 'success' ? 'success' : 'destructive'"
                      class="text-[10px] px-1.5 py-0 h-4"
                    >
                      {{ t(replayResult.status === 'success' ? 'replayDialog.succeeded' : 'replayDialog.failed') }}
                    </Badge>
                    <span
                      v-if="replayResult.status_code != null"
                      class="text-[11px] text-muted-foreground"
                    >HTTP {{ replayResult.status_code }}</span>
                    <span class="text-[11px] text-muted-foreground/60">{{ replayResult.response_time_ms }}ms</span>
                    <span class="text-[11px] text-muted-foreground/60 font-mono truncate">{{ replayResult.provider }}</span>
                  </template>
                </div>
                <button
                  v-if="replayResult"
                  class="p-1 rounded transition-colors text-muted-foreground hover:bg-muted shrink-0"
                  :title="responseCopied ? t('replayDialog.copied') : t('replayDialog.copyResponseBody')"
                  @click="copyResponseBody"
                >
                  <Check
                    v-if="responseCopied"
                    class="w-3 h-3 text-green-500"
                  />
                  <Copy
                    v-else
                    class="w-3 h-3"
                  />
                </button>
              </div>
              <!-- 右栏内容 -->
              <div class="flex-1 overflow-y-auto scrollbar-stable">
                <!-- 空状态 -->
                <div
                  v-if="!replayResult && !replayError && !replaying"
                  class="flex flex-col items-center justify-center h-full text-muted-foreground/40 gap-2"
                >
                  <Play class="w-8 h-8" />
                  <span class="text-xs">{{ t('replayDialog.sendToViewResponse') }}</span>
                </div>

                <!-- Loading -->
                <div
                  v-else-if="replaying && !replayResult"
                  class="flex flex-col gap-3 items-center justify-center h-full p-4 text-center"
                  role="status"
                  aria-live="polite"
                >
                  <Loader2 class="w-6 h-6 animate-spin text-muted-foreground" />
                  <p class="text-sm">
                    {{ t('replayDialog.waiting', { seconds: elapsedSeconds }) }}
                  </p>
                  <p class="text-xs text-muted-foreground">
                    {{ t('replayDialog.waitingHint') }}
                  </p>
                </div>

                <!-- 错误 -->
                <div
                  v-else-if="replayError"
                  class="px-4 py-4"
                >
                  <div
                    class="rounded-lg bg-destructive/10 border border-destructive/30 p-3"
                    role="alert"
                  >
                    <p class="text-sm text-destructive">
                      {{ replayError }}
                    </p>
                  </div>
                </div>

                <!-- 响应结果 -->
                <template v-if="replayResult">
                  <div
                    class="px-4 py-3 border-b space-y-2"
                    role="status"
                    aria-live="polite"
                  >
                    <p
                      v-if="replayResult.error_message"
                      class="text-xs font-medium text-destructive"
                    >
                      {{ replayResult.error_message }}
                    </p>
                    <p
                      v-if="replayResult.record_warning"
                      role="alert"
                      class="text-xs text-destructive"
                    >
                      {{ replayResult.record_warning }}
                    </p>
                    <p class="text-xs text-muted-foreground break-all">
                      {{ t('replayDialog.replayId') }}：{{ replayResult.replay_id }}
                    </p>
                  </div>
                  <div
                    v-if="replayResult.mapping"
                    class="border-b"
                  >
                    <div class="px-4 py-2 text-[11px] text-muted-foreground flex flex-col gap-1">
                      <div class="flex flex-wrap items-center gap-2">
                        <span class="font-mono truncate">{{ replayResult.mapping.source_model }}</span>
                        <span class="text-muted-foreground/50">→</span>
                        <span class="font-mono truncate">{{ replayResult.mapping.resolved_model }}</span>
                        <Badge
                          variant="outline"
                          class="text-[10px] px-1.5 py-0 h-4"
                        >
                          {{ formatReplayMode(replayResult.mapping.replay_mode) }}
                        </Badge>
                      </div>
                      <div class="flex flex-wrap gap-2 text-muted-foreground/60">
                        <span>Provider: {{ replayResult.mapping.target_provider }}</span>
                        <span>Endpoint: {{ replayResult.mapping.target_endpoint_id }}</span>
                        <span>Format: {{ replayResult.mapping.target_api_format || '-' }}</span>
                      </div>
                      <div
                        v-if="replayResult.mapping.mapping_source && replayResult.mapping.mapping_source !== 'none'"
                        class="text-muted-foreground/50"
                      >
                        {{ formatMappingSource(replayResult.mapping.mapping_source) }}
                      </div>
                    </div>
                  </div>
                  <!-- 响应头（可折叠） -->
                  <div class="border-b">
                    <button
                      class="w-full px-4 py-1.5 flex items-center gap-1.5 text-xs text-muted-foreground hover:bg-muted/30 transition-colors"
                      @click="showResponseHeaders = !showResponseHeaders"
                    >
                      <ChevronRight
                        class="w-3 h-3 transition-transform shrink-0"
                        :class="{ 'rotate-90': showResponseHeaders }"
                      />
                      <span class="font-medium">Headers</span>
                      <span class="text-muted-foreground/60 ml-0.5">({{ responseHeaderCount }})</span>
                    </button>
                    <div
                      v-if="showResponseHeaders"
                      class="px-4 pb-2.5"
                    >
                      <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-[11px] font-mono">
                        <template
                          v-for="(value, key) in replayResult.response_headers"
                          :key="key"
                        >
                          <span class="text-muted-foreground/70 text-right select-none">{{ key }}</span>
                          <span class="break-all">{{ value }}</span>
                        </template>
                      </div>
                    </div>
                  </div>
                  <!-- 响应体 -->
                  <div class="border-b">
                    <div class="px-4 py-1.5 flex items-center gap-1.5 text-xs text-muted-foreground">
                      <span class="w-3 h-3 shrink-0" />
                      <span class="font-medium">Body</span>
                      <span
                        v-if="replayResult.url"
                        class="text-muted-foreground/40 font-mono text-[10px] truncate ml-1"
                      >{{ replayResult.url }}</span>
                    </div>
                  </div>
                  <div class="px-4 py-3">
                    <pre
                      v-if="formattedResponseBody"
                      class="text-xs font-mono whitespace-pre-wrap break-all leading-relaxed"
                    >{{ formattedResponseBody }}</pre>
                    <p
                      v-else
                      class="text-xs text-muted-foreground"
                    >
                      {{ t('replayDialog.emptyResponse') }}
                    </p>
                  </div>
                </template>
              </div>
            </div>
          </div>
        </Card>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, computed, watch, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { dashboardApi, type ReplayResponse, type RequestDetail } from '@/api/dashboard'
import { getProvidersSummary } from '@/api/endpoints/providers'
import { getProviderKeys } from '@/api/endpoints/keys'
import type { EndpointAPIKey } from '@/api/endpoints/types'
import { useClipboard } from '@/composables/useClipboard'
import { useEscapeKey } from '@/composables/useEscapeKey'
import Card from '@/components/ui/card.vue'
import Badge from '@/components/ui/badge.vue'
import Button from '@/components/ui/button.vue'
import Separator from '@/components/ui/separator.vue'
import { X, Play, Loader2, ChevronRight, Copy, Check } from 'lucide-vue-next'
import { log } from '@/utils/logger'

interface ProviderOption {
  id: string
  name: string
}

const props = defineProps<{
  isOpen: boolean
  requestId: string | null
  detail: RequestDetail | null
}>()
const emit = defineEmits<{
  close: []
}>()

const { t } = useI18n()

const selectedProviderId = ref('')
const selectedKeyId = ref('')
const providers = ref<ProviderOption[]>([])
const keys = ref<EndpointAPIKey[]>([])
const loadingKeys = ref(false)
const replaying = ref(false)
const replayResult = ref<ReplayResponse | null>(null)
const replayError = ref<string | null>(null)
const showRequestHeaders = ref(false)
const showResponseHeaders = ref(false)
const requestCopied = ref(false)
const responseCopied = ref(false)
const { copyToClipboard } = useClipboard()
const loadedDetail = ref<RequestDetail | null>(null)
const bodyLoading = ref(false)
const bodyLoadError = ref<string | null>(null)
const optionsError = ref<string | null>(null)
const elapsedSeconds = ref(0)
let generation = 0
let bodyGeneration = 0
let keyGeneration = 0
let elapsedTimer: ReturnType<typeof setInterval> | undefined
const copyTimers = new Set<ReturnType<typeof setTimeout>>()

function stopElapsedTimer() {
  if (elapsedTimer) clearInterval(elapsedTimer)
  elapsedTimer = undefined
}
const bodyProblem = computed(() => {
  if (bodyLoadError.value) return bodyLoadError.value
  const body = loadedDetail.value?.request_body
  const capture = loadedDetail.value?.body_capture?.request as { storage?: string, state?: string } | undefined
  if (body?.truncated === true || capture?.storage === 'truncated' || capture?.state === 'truncated') return t('replayDialog.incompleteBody')
  if (!body || typeof body !== 'object' || Array.isArray(body) || Object.keys(body).length === 0) return t('replayDialog.missingBody')
  return null
})
const canReplay = computed(() => Boolean(props.requestId) && !bodyLoading.value && !bodyProblem.value && !replaying.value && !loadingKeys.value)


// ---- 请求侧数据 ----

const displayRequestHeaders = computed(() => {
  return loadedDetail.value?.request_headers || props.detail?.request_headers || {}
})

const hasRequestHeaders = computed(() => {
  return Object.keys(displayRequestHeaders.value).length > 0
})

const requestHeaderCount = computed(() => {
  return Object.keys(displayRequestHeaders.value).length
})

const formattedRequestBody = computed(() => {
  return formatBody(loadedDetail.value?.request_body)
})

// ---- 响应侧数据 ----

const formattedResponseBody = computed(() => {
  return formatBody(replayResult.value?.response_body)
})

const responseHeaderCount = computed(() => {
  if (!replayResult.value?.response_headers) return 0
  return Object.keys(replayResult.value.response_headers).length
})

function formatReplayMode(mode?: string) {
  switch (mode) {
    case 'same_endpoint_reuse':
      return t('replayDialog.sameEndpointReuse')
    case 'same_provider_remap':
      return t('replayDialog.sameProviderRemap')
    case 'cross_provider_remap':
      return t('replayDialog.crossProviderRemap')
    default:
      return mode || '-'
  }
}

function formatMappingSource(source?: string) {
  switch (source) {
    case 'original_target_model':
      return t('replayDialog.mappingOriginalTarget')
    case 'model_mapping':
      return t('replayDialog.mappingModelMapping')
    case 'none':
      return t('replayDialog.mappingNone')
    default:
      return source ? t('replayDialog.mappingSource', { source }) : ''
  }
}

// ---- 生命周期 ----

function formatBody(body: unknown): string {
  if (body === undefined || body === null) return ''
  return typeof body === 'string' ? body : JSON.stringify(body, null, 2)
}

async function loadRequestBody() {
  if (!props.requestId) return
  const current = ++bodyGeneration
  const id = props.requestId
  bodyLoading.value = true
  bodyLoadError.value = null
  loadedDetail.value = null
  try {
    const detail = await dashboardApi.getRequestDetail(id, { includeBodies: true, cacheTtlMs: 0 })
    if (current !== bodyGeneration) return
    loadedDetail.value = detail
  } catch (error) {
    if (current !== bodyGeneration) return
    bodyLoadError.value = t('replayDialog.bodyLoadFailed')
    log.error('Failed to load replay body:', error)
  } finally {
    if (current === bodyGeneration) bodyLoading.value = false
  }
}

watch(() => [props.isOpen, props.requestId] as const, async ([isOpen]) => {
  const current = ++generation
  ++bodyGeneration
  ++keyGeneration
  stopElapsedTimer()
  replaying.value = false
  bodyLoading.value = false
  loadingKeys.value = false
  if (!isOpen) return
  loadedDetail.value = null
  replayResult.value = null
  replayError.value = null
  optionsError.value = null
  selectedProviderId.value = ''
  selectedKeyId.value = ''
  providers.value = []
  keys.value = []
  showRequestHeaders.value = false
  showResponseHeaders.value = false
  requestCopied.value = false
  responseCopied.value = false
  void loadRequestBody()
  try {
    const response = await getProvidersSummary({ page_size: 9999 })
    if (current !== generation) return
    providers.value = response.items.filter(p => p.is_active && p.active_endpoints > 0)
      .map(p => ({ id: p.id, name: p.name }))
  } catch (error) {
    if (current !== generation) return
    optionsError.value = t('replayDialog.providersLoadFailed')
    log.error('Failed to load providers:', error)
  }
}, { immediate: true })

async function onProviderChange() {
  const current = ++keyGeneration
  selectedKeyId.value = ''
  keys.value = []
  loadingKeys.value = false
  optionsError.value = null
  if (!selectedProviderId.value) return
  loadingKeys.value = true
  try {
    const allKeys = await getProviderKeys(selectedProviderId.value)
    if (current !== keyGeneration) return
    keys.value = allKeys.filter(k => k.is_active)
  } catch (error) {
    if (current !== keyGeneration) return
    optionsError.value = t('replayDialog.keysLoadFailed')
    log.error('Failed to load keys:', error)
  } finally {
    if (current === keyGeneration) loadingKeys.value = false
  }
}

async function doReplay() {
  if (!props.requestId || !canReplay.value) return

  const current = generation
  replaying.value = true
  elapsedSeconds.value = 0
  const start = Date.now()
  stopElapsedTimer()
  elapsedTimer = setInterval(() => { elapsedSeconds.value = Math.floor((Date.now() - start) / 1000) }, 1000)
  replayError.value = null
  replayResult.value = null

  try {
    const params: Record<string, string> = {}
    if (selectedProviderId.value) params.provider_id = selectedProviderId.value
    if (selectedKeyId.value) params.api_key_id = selectedKeyId.value

    const result = await dashboardApi.replayRequest(
      props.requestId,
      Object.keys(params).length > 0 ? params : undefined,
    )
    if (current !== generation) return
    if (!result || typeof result !== 'object') {
      replayError.value = t('replayDialog.invalidResult')
    } else if (result.dry_run) {
      replayError.value = t('replayDialog.dryRun')
    } else if (!result.replay_id || !['success', 'failed'].includes(result.status)) {
      replayError.value = t('replayDialog.invalidResult')
    } else {
      replayResult.value = result
    }
  } catch (e: unknown) {
    if (current !== generation) return
    const err = e as { response?: { data?: { detail?: string } }; message?: string }
    replayError.value = err.response?.data?.detail || t('replayDialog.connectionLost')
    log.error('Replay failed:', e)
  } finally {
    if (current === generation) {
      replaying.value = false
      stopElapsedTimer()
    }
  }
}

async function copyBody(value: string, target: typeof requestCopied) {
  if (!value || !await copyToClipboard(value, false)) return
  target.value = true
  const timer = setTimeout(() => { target.value = false; copyTimers.delete(timer) }, 2000)
  copyTimers.add(timer)
}
function copyRequestBody() { void copyBody(formattedRequestBody.value, requestCopied) }
function copyResponseBody() { void copyBody(formattedResponseBody.value, responseCopied) }

onUnmounted(() => {
  ++generation
  ++bodyGeneration
  ++keyGeneration
  stopElapsedTimer()
  copyTimers.forEach(clearTimeout)
})

function handleClose() {
  emit('close')
}

useEscapeKey(() => {
  if (props.isOpen) handleClose()
}, { disableOnInput: true, once: false })
</script>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s ease;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
