<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { UsageRecord } from '../types'
const props = defineProps<{ value?: UsageRecord['codex_turn_state'] }>()
const { t } = useI18n()
const mode = computed(() => {
  const value = props.value?.mode
  return value && ['injected', 'passthrough', 'not_applicable', 'failed', 'invalidated'].includes(value) ? value : 'unknown'
})
</script>

<template>
  <span
    class="block whitespace-nowrap text-xs text-muted-foreground"
    data-testid="usage-state"
  >{{ t(`codexState.request.${mode}`) }}</span>
</template>
