<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { Badge } from '@/components/ui'
import type { PoolCodexState } from '../composables/usePoolCodexState'
import { summarizeCodexState } from '@/features/providers/components/codex-state-summary'

const props = defineProps<{ state?: PoolCodexState; name: string; eligible: boolean }>()
defineEmits<{ details: [] }>()
const { t, locale } = useI18n()
const summary = computed(() => props.state ? summarizeCodexState(props.state) : null)
const label = computed(() => {
  if (!props.state || props.state.loading) return t('codexState.loading')
  if (summary.value?.status === 'read_failed') return t('codexState.readFailed')
  if (summary.value?.status === 'unobserved') return t('codexState.pool.unobserved')
  if (summary.value?.status === 'ready') return t('codexState.pool.preferred')
  return t(`codexState.status.${summary.value?.status}`)
})
const variant = computed(() => summary.value?.status === 'ready' ? 'success'
  : ['auth_rejected', 'read_failed'].includes(summary.value?.status ?? '') ? 'destructive' : 'secondary')
const detailTitle = computed(() => {
  const use = summary.value?.use
  const at = use?.last_use?.at
  const timestamp = at ? new Date(at * 1000).toLocaleString(locale.value) : ''
  return [t('codexState.overview.details', { name: props.name }), use?.model, timestamp].filter(Boolean).join(' · ')
})
</script>

<template>
  <span
    v-if="!eligible"
    class="text-xs text-muted-foreground"
  >—</span>
  <span
    v-else-if="!state || state.loading"
    class="text-xs text-muted-foreground"
    role="status"
  >{{ t('codexState.loading') }}</span>
  <button
    v-else
    type="button"
    class="inline-flex flex-col items-start gap-1 rounded-md text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    :title="detailTitle"
    :aria-label="t('codexState.overview.details', { name })"
    data-testid="pool-codex-state"
    @click.stop="$emit('details')"
  >
    <Badge
      :variant="variant"
      class="whitespace-nowrap"
    >
      {{ label }}
    </Badge>
    <span v-if="summary?.readyModels.length" class="text-xs text-muted-foreground">{{ summary.readyModels.join(' · ') }}</span>
    <span
      v-for="warning in summary?.warnings"
      :key="warning"
      class="text-xs text-destructive"
    >{{ t(`codexState.status.${warning}`) }}</span>
    <span v-if="summary?.probe?.last_probe?.attempt" class="text-xs text-muted-foreground whitespace-nowrap">{{ t('codexState.attempt', { count: summary.probe.last_probe.attempt, limit: summary.probe.last_probe.attempt_limit }) }}</span>
    <span
      v-if="summary?.use?.last_use"
      class="text-xs text-muted-foreground whitespace-nowrap"
    >{{ t(`codexState.pool.use.${summary.use.last_use.mode}`) }}</span>
  </button>
</template>
