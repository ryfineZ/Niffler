<template>
  <div
    v-if="capacity?.count_24h"
    class="text-xs space-y-1"
  >
    <button
      type="button"
      class="text-primary underline-offset-4 hover:underline text-left"
      @click="$emit('details')"
    >
      {{ stateLabel }} · 24小时 {{ capacity.count_24h }} 次
    </button>
    <p class="text-muted-foreground">
      最近 {{ latestTime }}
    </p>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useNow } from '@vueuse/core'
import { capacityModelState } from '../utils/poolCapacity'
import type { PoolCapacity } from '@/api/endpoints/pool'
const props = defineProps<{ capacity?: PoolCapacity }>()
defineEmits<{ details: [] }>()
const now = useNow({ interval: 1000 })
const stateLabel = computed(() => props.capacity?.models.some(m => capacityModelState(m, now.value.getTime()) === 'cooldown') ? '容量冷却中'
  : props.capacity?.models.some(m => capacityModelState(m, now.value.getTime()) === 'pending') ? '容量不足 · 待验证' : '容量已恢复')
const latestTime = computed(() => {
  const latest = Math.max(0, ...(props.capacity?.models.map(m => m.last_occurred_at_ms) ?? []))
  return latest ? new Date(latest).toLocaleString() : '—'
})
</script>
