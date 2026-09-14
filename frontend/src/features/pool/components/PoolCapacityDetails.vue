<template>
  <Dialog
    :model-value="!!account"
    title="容量错误记录"
    size="lg"
    @update:model-value="(open: boolean) => { if (!open) $emit('close') }"
  >
    <div
      v-if="account"
      class="space-y-5"
    >
      <p class="text-sm break-words">
        {{ account.oauth_email || account.key_name }} <span class="text-muted-foreground">· {{ account.provider_name }}</span>
      </p>
      <p class="text-sm text-muted-foreground">
        近 24 小时 {{ account.capacity?.count_24h || 0 }} 次。每个模型展示最近 20 条记录。
      </p>
      <section
        v-for="model in account.capacity?.models"
        :key="model.model"
        class="space-y-2 border-t pt-3"
      >
        <div class="flex flex-wrap justify-between gap-2 text-sm">
          <span class="font-medium break-all">{{ model.model === 'unknown' ? '历史记录未注明模型' : model.model }} · {{ model.count_24h }} 次</span>
          <span class="text-muted-foreground">{{ capacityStateLabel(model) }}</span>
        </div>
        <ul class="space-y-2 text-xs">
          <li
            v-for="event in model.recent"
            :key="event.candidate_id"
            class="flex flex-wrap justify-between gap-2"
          >
            <time>{{ new Date(event.occurred_at_ms).toLocaleString() }}</time>
            <span class="text-muted-foreground break-all">请求 {{ event.request_id }}</span>
          </li>
        </ul>
      </section>
    </div>
    <template #footer>
      <Button
        variant="outline"
        @click="$emit('close')"
      >
        关闭
      </Button>
    </template>
  </Dialog>
</template>

<script setup lang="ts">
import { useNow } from '@vueuse/core'
import { capacityModelState, capacityCooldownSeconds } from '../utils/poolCapacity'
import { Dialog, Button } from '@/components/ui'
import type { PoolKeyDetail, PoolCapacityModel } from '@/api/endpoints/pool'
defineProps<{ account: PoolKeyDetail | null }>()
defineEmits<{ close: [] }>()
const now = useNow({ interval: 1000 })
function capacityStateLabel(model: PoolCapacityModel) {
  const timestamp = now.value.getTime()
  const state = capacityModelState(model, timestamp)
  return state === 'recovered' ? '后续请求已成功'
    : state === 'cooldown' ? `冷却剩余 ${capacityCooldownSeconds(model, timestamp)} 秒`
      : '等待后续请求验证'
}
</script>
