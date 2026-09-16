<template>
  <Card
    class="p-5 space-y-4"
    data-testid="request-failover-settings"
  >
    <div class="flex items-start justify-between gap-4">
      <div>
        <h3 class="font-semibold">
          失败自动换号
        </h3><p class="mt-1 text-sm text-muted-foreground">
          容量不足或临时限流时，在开始输出前自动尝试其他可用账号。
        </p>
      </div>
      <Switch
        v-model="form.enabled"
        aria-label="失败自动换号"
        :disabled="loading || saving || !!loadError"
      />
    </div>
    <p
      v-if="loading"
      class="text-sm text-muted-foreground"
    >
      正在读取设置…
    </p>
    <div
      v-if="loadError"
      class="flex items-center gap-3"
    >
      <p
        role="alert"
        class="text-sm text-destructive"
      >
        {{ loadError }}
      </p><Button
        variant="outline"
        size="sm"
        @click="load"
      >
        重试
      </Button>
    </div>
    <details
      v-if="form.enabled && !loading && !loadError"
      class="space-y-3"
    >
      <summary class="cursor-pointer text-sm text-muted-foreground">
        高级设置
      </summary>
      <div class="grid gap-4 sm:grid-cols-2">
        <div
          v-for="field in fields"
          :key="field.key"
          class="space-y-1.5"
        >
          <Label :for="`failover-${field.key}`">{{ field.label }}</Label>
          <Input
            :id="`failover-${field.key}`"
            v-model="form[field.key]"
            type="number"
            :min="field.min"
            :max="field.max"
            :step="field.step"
            :disabled="saving"
          />
        </div>
      </div>
    </details>
    <p
      v-if="validation"
      role="alert"
      class="text-sm text-destructive"
    >
      {{ validation }}
    </p>
    <p
      v-if="saveError"
      role="alert"
      class="text-sm text-destructive"
    >
      {{ saveError }}
    </p>
    <p
      v-if="saved && !changed"
      role="status"
      class="text-sm text-muted-foreground"
    >
      已保存，全站请求使用此设置。
    </p>
    <Button
      :disabled="loading || saving || !!loadError || !!validation || !changed"
      @click="save"
    >
      {{ saving ? '保存中…' : '保存' }}
    </Button>
  </Card>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Card, Button, Input, Label, Switch } from '@/components/ui'
import { adminApi } from '@/api/admin'
import { parseApiError } from '@/utils/errorParser'
const defaults = { enabled: true, max_account_switches: 2, max_wait_ms: 5000, max_buffer_bytes: 65536, cooldown_seconds: 30 }
const form = ref({ enabled: true, maxRetries: '2', maxWaitSeconds: '5', maxBufferKB: '64', cooldownSeconds: '30' })
const original = ref('')
const loading = ref(true), saving = ref(false), loadError = ref(''), saveError = ref(''), saved = ref(false)
const fields = [
  { key: 'maxRetries', label: '失败后换号次数', min: 0, max: 999, step: 1 },
  { key: 'maxWaitSeconds', label: '最长等待（秒）', min: 0.25, max: 30, step: 0.25 },
  { key: 'maxBufferKB', label: '暂存上限（KB）', min: 16, max: 1024, step: 1 },
  { key: 'cooldownSeconds', label: '模型冷却（秒）', min: 1, max: 1920, step: 1 },
] as const
const changed = computed(() => JSON.stringify(form.value) !== original.value)
const validation = computed(() => {
  for (const field of fields) {
    const value = String(form.value[field.key]).trim(), number = Number(value)
    if (!value || !Number.isFinite(number) || number < field.min || number > field.max || (field.step === 1 && !Number.isInteger(number))) return `${field.label}需在 ${field.min}–${field.max} 之间${field.step === 1 ? '，且为整数' : ''}`
  }
  return ''
})
async function load() {
  loading.value = true; loadError.value = ''
  try {
    const result = await adminApi.getSystemConfig('request_failover')
    const config = { ...defaults, ...(result.value as Partial<typeof defaults>) }
    form.value = { enabled: config.enabled, maxRetries: String(config.max_account_switches), maxWaitSeconds: String(config.max_wait_ms / 1000), maxBufferKB: String(config.max_buffer_bytes / 1024), cooldownSeconds: String(config.cooldown_seconds) }
    original.value = JSON.stringify(form.value)
  } catch (error) { loadError.value = parseApiError(error, '无法读取自动换号设置') }
  finally { loading.value = false }
}
async function save() {
  if (validation.value || saving.value) return
  saving.value = true; saveError.value = ''; saved.value = false
  try {
    await adminApi.updateSystemConfig('request_failover', { enabled: form.value.enabled, max_account_switches: Number(form.value.maxRetries), max_wait_ms: Math.round(Number(form.value.maxWaitSeconds) * 1000), max_buffer_bytes: Number(form.value.maxBufferKB) * 1024, cooldown_seconds: Number(form.value.cooldownSeconds) }, '全局失败自动换号')
    original.value = JSON.stringify(form.value); saved.value = true
  } catch (error) { saveError.value = parseApiError(error, '保存失败，请重试') }
  finally { saving.value = false }
}
onMounted(load)
</script>
