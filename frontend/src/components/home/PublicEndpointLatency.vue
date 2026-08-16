<template>
  <section
    class="mt-7 border-t border-border/70 pt-5"
    :aria-label="t('home.endpointLatencyTitle')"
  >
    <div class="flex items-center justify-between gap-4">
      <div>
        <div class="flex items-center gap-2 text-[10px] font-bold uppercase tracking-[0.18em] text-primary">
          <Gauge class="h-4 w-4" />
          {{ t('home.endpointLatencyTitle') }}
        </div>
        <p class="mt-1 text-xs leading-5 text-muted-foreground">
          {{ t('home.endpointLatencyDescription') }}
        </p>
      </div>
      <button
        type="button"
        class="inline-flex h-9 shrink-0 items-center gap-2 border border-border bg-background/75 px-3 text-xs font-semibold transition hover:border-primary/45 hover:text-primary active:translate-y-px disabled:cursor-wait disabled:opacity-60"
        :disabled="isMeasuring"
        :aria-label="t('home.endpointLatencyRefresh')"
        :aria-busy="isMeasuring"
        :data-state="isMeasuring ? 'checking' : 'idle'"
        data-refresh-endpoint-latency
        @click="measureEndpoints"
      >
        <RefreshCw
          class="h-3.5 w-3.5"
          :class="{ 'animate-spin': isMeasuring }"
        />
        {{ t('home.endpointLatencyRefresh') }}
      </button>
    </div>

    <div
      class="mt-3 grid gap-2 sm:grid-cols-3 lg:grid-cols-1 min-[1280px]:grid-cols-3"
      aria-live="polite"
    >
      <article
        v-for="endpoint in endpoints"
        :key="endpoint.id"
        class="flex min-w-0 items-center justify-between gap-4 border border-border/70 bg-background/65 px-4 py-3"
        :data-endpoint-latency="endpoint.id"
        :data-status="results[endpoint.id].status"
      >
        <div class="flex min-w-0 items-center gap-3">
          <span
            class="h-2.5 w-2.5 shrink-0 rounded-full"
            :class="statusDotClass(results[endpoint.id].status)"
          />
          <div class="min-w-0">
            <div class="truncate text-xs font-semibold text-foreground">
              {{ t(endpoint.labelKey) }}
            </div>
            <div class="truncate font-mono text-[10px] text-muted-foreground">
              {{ endpoint.host }}
            </div>
          </div>
        </div>
        <strong
          class="shrink-0 font-mono text-sm tabular-nums"
          :class="results[endpoint.id].status === 'error' ? 'text-destructive' : 'text-foreground'"
        >
          {{ latencyText(results[endpoint.id]) }}
        </strong>
      </article>
    </div>

  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive } from 'vue'
import { useI18n } from 'vue-i18n'
import { Gauge, RefreshCw } from 'lucide-vue-next'

type EndpointId = 'us1' | 'us2' | 'cn'
type MeasurementStatus = 'checking' | 'ready' | 'error'

interface EndpointDefinition {
  id: EndpointId
  host: string
  labelKey: string
  probeUrl: string
}

interface MeasurementResult {
  status: MeasurementStatus
  latencyMs: number | null
}

const SAMPLE_COUNT = 3
const PROBE_TIMEOUT_MS = 5000

const { t } = useI18n()
const endpoints: EndpointDefinition[] = [
  {
    id: 'us1',
    host: 'us1.niffler.org',
    labelKey: 'home.endpointLatencyUs1',
    probeUrl: 'https://us1.niffler.org/__niffler_latency',
  },
  {
    id: 'us2',
    host: 'us2.niffler.org',
    labelKey: 'home.endpointLatencyUs2',
    probeUrl: 'https://us2.niffler.org/__niffler_latency',
  },
  {
    id: 'cn',
    host: 'cn.niffler.org',
    labelKey: 'home.endpointLatencyCn',
    probeUrl: 'https://cn.niffler.org/__niffler_latency',
  },
]
const results = reactive<Record<EndpointId, MeasurementResult>>({
  us1: { status: 'checking', latencyMs: null },
  us2: { status: 'checking', latencyMs: null },
  cn: { status: 'checking', latencyMs: null },
})
const isMeasuring = computed(() => endpoints.some(endpoint => results[endpoint.id].status === 'checking'))
let activeRun = 0
let activeController: AbortController | null = null

function latencyText(result: MeasurementResult) {
  if (result.status === 'checking') return t('home.endpointLatencyChecking')
  if (result.status === 'error' || result.latencyMs == null) return t('home.endpointLatencyUnavailable')
  return `${result.latencyMs} ms`
}

function statusDotClass(status: MeasurementStatus) {
  if (status === 'ready') return 'bg-emerald-500'
  if (status === 'error') return 'bg-destructive'
  return 'animate-pulse bg-amber-500'
}

async function timedProbe(endpoint: EndpointDefinition, signal: AbortSignal) {
  const controller = new AbortController()
  const abortProbe = () => controller.abort()
  signal.addEventListener('abort', abortProbe, { once: true })
  const timeoutId = window.setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS)
  const startedAt = performance.now()

  try {
    const response = await fetch(`${endpoint.probeUrl}?probe=${Date.now()}-${Math.random()}`, {
      method: 'GET',
      mode: 'cors',
      cache: 'no-store',
      credentials: 'omit',
      signal: controller.signal,
    })
    if (!response.ok && response.status !== 204) throw new Error(`Unexpected probe status: ${response.status}`)
    return Math.max(1, Math.round(performance.now() - startedAt))
  } finally {
    window.clearTimeout(timeoutId)
    signal.removeEventListener('abort', abortProbe)
  }
}

async function measureEndpoint(endpoint: EndpointDefinition, signal: AbortSignal) {
  const samples: number[] = []
  for (let index = 0; index < SAMPLE_COUNT; index += 1) {
    samples.push(await timedProbe(endpoint, signal))
  }
  samples.sort((left, right) => left - right)
  return samples[Math.floor(samples.length / 2)] ?? null
}

async function measureEndpoints() {
  activeRun += 1
  const run = activeRun
  activeController?.abort()
  const controller = new AbortController()
  activeController = controller

  for (const endpoint of endpoints) {
    results[endpoint.id] = { status: 'checking', latencyMs: null }
  }

  await Promise.all(endpoints.map(async endpoint => {
    try {
      const latencyMs = await measureEndpoint(endpoint, controller.signal)
      if (run !== activeRun) return
      results[endpoint.id] = { status: 'ready', latencyMs }
    } catch {
      if (run !== activeRun) return
      results[endpoint.id] = { status: 'error', latencyMs: null }
    }
  }))
}

onMounted(() => {
  void measureEndpoints()
})

onBeforeUnmount(() => {
  activeRun += 1
  activeController?.abort()
})
</script>
