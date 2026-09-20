<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Badge, Button, Dialog } from '@/components/ui'
import { isCurrentCodexState } from './codex-state-summary'
import { getCodexStateDiagnostics, type CodexStateDiagnostics, type CodexStateObservation } from '@/api/endpoints/codex-state'

const props = defineProps<{ open: boolean; providerId: string; keyId: string; keyName: string }>()
defineEmits<{ 'update:open': [value: boolean] }>()
const { t, locale } = useI18n()
const data = ref<CodexStateDiagnostics | null>(null)
const loading = ref(false)
const error = ref(false)
let request = 0
let controller: AbortController | undefined
let timer: ReturnType<typeof setTimeout> | undefined
const showHistory = ref(false)
const history = computed(() => data.value?.items.filter(item => !isCurrentCodexState(item)) ?? [])
const visibleItems = computed(() => [
  ...(data.value?.items.filter(isCurrentCodexState) ?? []),
  ...(showHistory.value ? history.value : []),
])
const summary = computed(() => {
  if (!data.value?.enabled) return t('codexState.disabled')
  const ready = data.value.items.filter(item => isCurrentCodexState(item) && item.status === 'ready').length
  return ready ? t('codexState.readyCount', { count: ready }) : t('codexState.noReady')
})

async function refresh() {
  if (!props.open || loading.value) return
  clearTimeout(timer)
  controller?.abort()
  controller = new AbortController()
  const id = ++request
  loading.value = true
  error.value = false
  try {
    const result = await getCodexStateDiagnostics(props.providerId, props.keyId, { signal: controller.signal })
    if (request === id) data.value = result
  } catch {
    if (request === id) error.value = true
  } finally {
    if (request === id) {
      loading.value = false
      if (props.open && !document.hidden) timer = setTimeout(() => { void refresh() }, 10000)
    }
  }
}

watch(() => [props.open, props.providerId, props.keyId] as const, ([open]) => {
  request++
  controller?.abort()
  clearTimeout(timer)
  loading.value = false
  data.value = null
  error.value = false
  showHistory.value = false
  if (open) void refresh()
}, { immediate: true })
function onVisibility() {
  if (document.hidden) clearTimeout(timer)
  else if (props.open) void refresh()
}
document.addEventListener('visibilitychange', onVisibility)
onUnmounted(() => { request++; controller?.abort(); clearTimeout(timer); document.removeEventListener('visibilitychange', onVisibility) })

function date(value?: number | null) {
  return value ? new Date(value * 1000).toLocaleString(locale.value) : t('codexState.none')
}
function variant(item: CodexStateObservation) {
  if (item.status === 'ready') return 'success'
  if (item.status === 'auth_rejected') return 'destructive'
  return 'secondary'
}
function probeReason(item: CodexStateObservation) {
  const reason = item.last_probe?.reason
  const known = ['accepted', 'missing_state', 'invalid_state', 'upstream_error', 'transport_error', 'timeout', 'collecting', 'interrupted', 'dispatch_error', 'read_error', 'missing_completion', 'invalid_response', 'body_too_large', 'configuration_changed', 'revalidation_error']
  return t(`codexState.probe.${reason && known.includes(reason) ? reason : 'unknown'}`)
}
</script>

<template>
  <Dialog :model-value="open" :title="t('codexState.title', { name: keyName })" size="2xl" :z-index="70" @update:model-value="$emit('update:open', $event)">
    <div class="space-y-4">
      <div class="flex items-center justify-between gap-3">
        <p class="text-sm font-medium" aria-live="polite">{{ loading && !data ? t('codexState.loading') : error ? t('codexState.readFailed') : summary }}</p>
        <Button variant="outline" size="sm" :disabled="loading" @click="refresh">{{ t('codexState.refresh') }}</Button>
      </div>
      <p v-if="error" role="alert" class="text-sm text-destructive">{{ t('codexState.retryRead') }}</p>
      <template v-if="data">
        <p class="text-xs text-muted-foreground">{{ t('codexState.explanation') }}</p>
        <p v-if="!data.items.length" class="rounded-md bg-muted p-4 text-sm text-muted-foreground">{{ t('codexState.empty') }}</p>
        <p v-else-if="!data.items.some(isCurrentCodexState)" class="text-sm text-muted-foreground">{{ t('codexState.status.unobserved_current') }}</p>
        <Button v-if="history.length" variant="ghost" size="sm" :aria-expanded="showHistory" @click="showHistory = !showHistory">{{ t(showHistory ? 'codexState.hideHistory' : 'codexState.showHistory', { count: history.length }) }}</Button>
        <div v-if="visibleItems.length" class="divide-y divide-border">
          <section v-for="(item, index) in visibleItems" :key="item.id ?? index" class="space-y-3 py-4 first:pt-0">
            <p v-if="!isCurrentCodexState(item)" class="text-xs text-muted-foreground">{{ t('codexState.history') }}</p>
            <div class="flex flex-wrap items-center justify-between gap-2">
              <h3 class="text-sm font-medium break-all">{{ item.model }}</h3>
              <Badge :variant="variant(item)">{{ t(`codexState.status.${item.status}`) }}</Badge>
            </div>
            <p class="text-xs text-muted-foreground break-all">
              {{ t(`codexState.egress.${item.egress}`) }}<span v-if="item.node_id"> · {{ item.node_id }}</span><span v-if="item.instance"> · {{ item.instance }}</span>
            </p>
            <dl class="grid grid-cols-1 gap-3 text-sm sm:grid-cols-2">
              <div><dt class="text-xs text-muted-foreground">{{ t('codexState.lastProbe') }}</dt><dd>{{ item.last_probe ? probeReason(item) : t('codexState.none') }}</dd><dd class="text-xs text-muted-foreground">{{ date(item.last_probe?.at) }}<span v-if="item.last_probe?.status"> · HTTP {{ item.last_probe.status }}</span></dd></div>
              <div v-if="item.last_probe?.attempt"><dt class="text-xs text-muted-foreground">{{ t('codexState.attemptTitle') }}</dt><dd>{{ t('codexState.attempt', { count: item.last_probe.attempt, limit: item.last_probe.attempt_limit }) }}</dd><dd v-if="item.last_probe.observation?.elapsed_ms" class="text-xs text-muted-foreground">{{ t('codexState.elapsed', { seconds: (item.last_probe.observation.elapsed_ms / 1000).toFixed(1) }) }}</dd></div>
              <div v-if="item.last_probe?.observation?.phase"><dt class="text-xs text-muted-foreground">{{ t('codexState.phaseTitle') }}</dt><dd>{{ t(`codexState.phase.${item.last_probe.observation.phase}`) }}</dd></div>
              <div v-if="item.last_probe?.observation?.returned_state"><dt class="text-xs text-muted-foreground">{{ t('codexState.returnedTitle') }}</dt><dd>{{ t(`codexState.returned.${item.last_probe.observation.returned_state}`) }}</dd><dd v-if="item.last_probe.observation.observed_blocks != null && item.last_probe.observation.expected_blocks != null" class="text-xs text-muted-foreground">{{ t('codexState.blocks', { observed: item.last_probe.observation.observed_blocks, expected: item.last_probe.observation.expected_blocks }) }}</dd></div>
              <div><dt class="text-xs text-muted-foreground">{{ t('codexState.lastUse') }}</dt><dd>{{ item.last_use ? t(`codexState.use.${item.last_use.mode}`) : t('codexState.none') }}</dd><dd class="text-xs text-muted-foreground">{{ date(item.last_use?.at) }}</dd></div>
              <div v-if="item.last_use?.returned_state"><dt class="text-xs text-muted-foreground">{{ t('codexState.formalReturnedTitle') }}</dt><dd>{{ t(`codexState.returned.${item.last_use.returned_state}`) }}</dd><dd v-if="item.last_use.observed_blocks != null && item.last_use.expected_blocks != null" class="text-xs text-muted-foreground">{{ t('codexState.blocks', { observed: item.last_use.observed_blocks, expected: item.last_use.expected_blocks }) }}</dd></div>
              <div v-if="item.status === 'ready' && item.source"><dt class="text-xs text-muted-foreground">{{ t('codexState.sourceTitle') }}</dt><dd>{{ t(`codexState.source.${item.source}`) }}</dd></div>
              <div v-if="item.status === 'ready'"><dt class="text-xs text-muted-foreground">{{ t('codexState.expires') }}</dt><dd>{{ date(item.expires_at) }}</dd></div>
              <div v-if="item.status === 'cooldown'"><dt class="text-xs text-muted-foreground">{{ t('codexState.cooldown') }}</dt><dd>{{ t('codexState.seconds', { count: item.cooldown_seconds }) }}</dd></div>
              <div v-if="item.status === 'rate_limited'"><dt class="text-xs text-muted-foreground">{{ t('codexState.retryAt') }}</dt><dd>{{ date(item.retry_until) }}</dd></div>
              <div v-if="item.status === 'auth_rejected'"><dt class="text-xs text-muted-foreground">{{ t('codexState.authRejected') }}</dt><dd>HTTP {{ item.auth_status }}</dd></div>
            </dl>
          </section>
        </div>
        <p class="text-xs text-muted-foreground">{{ t('codexState.snapshot', { time: date(data.observed_at) }) }}</p>
      </template>
    </div>
    <template #footer>
      <Button variant="outline" @click="$emit('update:open', false)">{{ t('common.close') }}</Button>
    </template>
  </Dialog>
</template>
