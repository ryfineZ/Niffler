<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { RefreshCw, Search } from 'lucide-vue-next'
import { Badge, Button, Card, Input, Pagination } from '@/components/ui'
import { getCodexStateOverview, type CodexStateAccount, type CodexStateOverview } from '@/api/endpoints/codex-state'
import CodexStateDialog from './CodexStateDialog.vue'
import { summarizeCodexState } from './codex-state-summary'

const { t, locale } = useI18n()
const data = ref<CodexStateOverview | null>(null)
const loading = ref(false)
const error = ref(false)
const search = ref('')
const query = ref('')
const page = ref(1)
const selected = ref<CodexStateAccount | null>(null)
const hasAccounts = ref(false)
const receivedAt = ref<number | null>(null)
const rows = computed(() => data.value?.accounts.map(account => ({ account, ...summarizeCodexState(account) })) ?? [])
let request = 0
let timer: ReturnType<typeof setTimeout> | undefined

async function refresh() {
  const id = ++request
  loading.value = true
  error.value = false
  data.value = null
  try {
    const result = await getCodexStateOverview({ page: page.value, page_size: 10, search: query.value })
    if (id !== request) return
    data.value = result
    if (result.total > 0) hasAccounts.value = true
    receivedAt.value = Date.now() / 1000
  } catch {
    if (id === request) error.value = true
  } finally {
    if (id === request) loading.value = false
  }
}

watch(search, value => {
  clearTimeout(timer)
  // 搜索输入变化时立即废弃旧请求，避免防抖期间旧账号结果闪回。
  request++
  loading.value = true
  data.value = null
  timer = setTimeout(() => {
    query.value = value.trim()
    page.value = 1
    void refresh()
  }, 300)
})
void refresh()
onUnmounted(() => { request++; clearTimeout(timer) })

function changePage(value: number) {
  page.value = value
  void refresh()
}
function date(value?: number | null) {
  return value ? new Date(value * 1000).toLocaleString(locale.value) : t('codexState.none')
}
function statusText(status: string) {
  return ['read_failed', 'unobserved'].includes(status)
    ? t(`codexState.overview.${status}`) : t(`codexState.status.${status}`)
}
function probeText(reason?: string | null) {
  return t(`codexState.probe.${reason && ['accepted', 'missing_state', 'invalid_state', 'upstream_error', 'transport_error'].includes(reason) ? reason : 'unknown'}`)
}
function variant(status: string) {
  return status === 'ready' ? 'success' : ['auth_rejected', 'read_failed'].includes(status) ? 'destructive' : 'secondary'
}
</script>

<template>
  <Card
    v-if="loading || error || hasAccounts || query"
    aria-labelledby="codex-state-heading"
  >
    <div class="space-y-3 border-b border-border px-4 py-4 sm:px-6">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2
            id="codex-state-heading"
            class="text-base font-semibold"
          >
            {{ t('codexState.overview.title') }}
          </h2>
          <p class="mt-1 text-sm text-muted-foreground">
            {{ t('codexState.overview.description') }}
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          :disabled="loading"
          @click="refresh"
        >
          <RefreshCw
            class="mr-2 h-4 w-4"
            :class="{ 'animate-spin': loading }"
            aria-hidden="true"
          />
          {{ loading ? t('codexState.loading') : t('codexState.overview.refresh') }}
        </Button>
      </div>
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div class="relative w-full sm:w-72">
          <Search
            class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            v-model="search"
            class="pl-9"
            :placeholder="t('codexState.overview.search')"
            :aria-label="t('codexState.overview.search')"
          />
        </div>
        <span
          v-if="receivedAt && !loading && !error"
          class="text-xs text-muted-foreground"
        >{{ t('codexState.overview.snapshot', { time: date(receivedAt) }) }}</span>
      </div>
    </div>
    <div
      v-if="loading"
      class="px-6 py-8 text-sm text-muted-foreground"
      role="status"
    >
      {{ t('codexState.loading') }}
    </div>
    <p
      v-else-if="error"
      role="alert"
      class="px-6 py-8 text-sm text-destructive"
    >
      {{ t('codexState.retryRead') }}
    </p>
    <template v-else-if="data">
      <p
        v-if="!rows.length"
        class="px-6 py-8 text-sm text-muted-foreground"
      >
        {{ t('codexState.overview.noMatches') }}
      </p>
      <div v-else>
        <div
          class="hidden grid-cols-12 gap-4 border-b border-border bg-muted/30 px-6 py-3 text-xs font-medium text-muted-foreground lg:grid"
          aria-hidden="true"
        >
          <span class="col-span-3">{{ t('codexState.overview.account') }}</span>
          <span class="col-span-3">{{ t('codexState.overview.state') }}</span>
          <span class="col-span-3">{{ t('codexState.lastProbe') }}</span>
          <span class="col-span-3">{{ t('codexState.lastUse') }}</span>
        </div>
        <ul class="divide-y divide-border">
          <li
            v-for="row in rows"
            :key="row.account.key_id"
            class="grid grid-cols-2 gap-4 px-4 py-4 sm:px-6 lg:grid-cols-12"
          >
            <div class="min-w-0 lg:col-span-3">
              <button
                class="text-left text-sm font-medium text-primary underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring break-all"
                :aria-label="t('codexState.overview.details', { name: row.account.key_name })"
                @click="selected = row.account"
              >
                {{ row.account.key_name }}
              </button>
              <p class="mt-1 break-all text-xs text-muted-foreground">
                {{ row.account.provider_name }}<span v-if="!row.account.active"> · {{ t('codexState.overview.inactive') }}</span>
              </p>
            </div>
            <div class="space-y-1 lg:col-span-3">
              <p class="text-xs text-muted-foreground lg:sr-only">
                {{ t('codexState.overview.state') }}
              </p>
              <Badge :variant="variant(row.status)">
                {{ row.status === 'ready' ? t('codexState.overview.ready', { ready: row.ready, total: row.total }) : statusText(row.status) }}
              </Badge>
              <p
                v-for="warning in row.warnings"
                :key="warning"
                class="text-xs text-destructive"
              >
                {{ statusText(warning) }}
              </p>
              <p
                v-if="row.status === 'unobserved'"
                class="text-xs text-muted-foreground"
              >
                {{ t('codexState.overview.waiting') }}
              </p>
            </div>
            <div class="min-w-0 space-y-1 lg:col-span-3">
              <p class="text-xs text-muted-foreground lg:sr-only">
                {{ t('codexState.lastProbe') }}
              </p>
              <p class="text-sm">
                {{ row.probe ? probeText(row.probe.last_probe?.reason) : row.status === 'read_failed' ? '—' : t('codexState.none') }}
              </p>
              <template v-if="row.probe">
                <p class="break-all text-xs text-muted-foreground">
                  {{ row.probe.model }}
                </p>
                <p class="text-xs text-muted-foreground">
                  {{ date(row.probe.last_probe?.at) }}
                </p>
              </template>
            </div>
            <div class="min-w-0 space-y-1 lg:col-span-3">
              <p class="text-xs text-muted-foreground lg:sr-only">
                {{ t('codexState.lastUse') }}
              </p>
              <p
                class="text-sm"
                :class="{ 'text-destructive': row.use?.last_use?.mode === 'invalidated' }"
              >
                {{ row.use?.last_use ? t(`codexState.use.${row.use.last_use.mode}`) : row.status === 'read_failed' ? '—' : t('codexState.none') }}
              </p>
              <template v-if="row.use">
                <p class="break-all text-xs text-muted-foreground">
                  {{ row.use.model }}
                </p>
                <p class="text-xs text-muted-foreground">
                  {{ date(row.use.last_use?.at) }}
                </p>
              </template>
            </div>
          </li>
        </ul>
      </div>
      <p class="border-t border-border px-4 py-3 text-xs text-muted-foreground sm:px-6">
        {{ t('codexState.explanation') }}
      </p>
      <Pagination
        v-if="data.total > 10"
        :current="data.page"
        :total="data.total"
        :page-size="10"
        :show-page-size-selector="false"
        @update:current="changePage"
      />
    </template>
  </Card>
  <CodexStateDialog
    v-if="selected"
    :open="true"
    :provider-id="selected.provider_id"
    :key-id="selected.key_id"
    :key-name="selected.key_name"
    @update:open="value => { if (!value) selected = null }"
  />
</template>
