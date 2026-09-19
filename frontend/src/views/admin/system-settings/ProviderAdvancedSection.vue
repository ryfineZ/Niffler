<template>
  <CardSection
    :title="t('providerAdvanced.title')"
    :description="t('providerAdvanced.description')"
  >
    <template #actions>
      <Button
        size="sm"
        :disabled="loading || saving || loadError || !hasChanges"
        @click="$emit('save')"
      >
        {{ saving ? t('providerAdvanced.saving') : t('providerAdvanced.save') }}
      </Button>
    </template>

    <div class="max-w-2xl space-y-6">
      <div class="flex items-start justify-between gap-6">
        <div>
          <Label
            id="codex-oauth-identity-convergence-label"
            for="codex-oauth-identity-convergence"
            :class="loading || saving || loadError ? 'cursor-not-allowed' : 'cursor-pointer'"
          >
            {{ t('providerAdvanced.codexIdentityConvergence') }}
          </Label>
          <p
            id="codex-oauth-identity-convergence-hint"
            class="mt-1 text-sm text-muted-foreground"
          >
            {{ t('providerAdvanced.codexIdentityConvergenceHint') }}
          </p>
          <p
            v-if="loadError"
            class="mt-2 text-sm text-destructive"
            role="alert"
          >
            {{ t('providerAdvanced.loadFailed') }}
          </p>
        </div>
        <Switch
          id="codex-oauth-identity-convergence"
          class="shrink-0"
          :model-value="enabled"
          :disabled="loading || saving || loadError"
          aria-labelledby="codex-oauth-identity-convergence-label"
          aria-describedby="codex-oauth-identity-convergence-hint"
          @update:model-value="$emit('update:enabled', $event)"
        />
      </div>
      <div class="flex items-start justify-between gap-6">
        <div>
          <Label
            id="codex-telemetry-label"
            for="codex-telemetry"
            :class="loading || saving || loadError ? 'cursor-not-allowed' : 'cursor-pointer'"
          >
            {{ t('providerAdvanced.codexTelemetry') }}
          </Label>
          <p
            id="codex-telemetry-hint"
            class="mt-1 text-sm text-muted-foreground"
          >
            {{ t('providerAdvanced.codexTelemetryHint') }}
          </p>
        </div>
        <Switch
          id="codex-telemetry"
          class="shrink-0"
          :model-value="telemetryEnabled"
          :disabled="loading || saving || loadError"
          aria-labelledby="codex-telemetry-label"
          aria-describedby="codex-telemetry-hint"
          @update:model-value="$emit('update:telemetryEnabled', $event)"
        />
      </div>
      <div class="flex items-start justify-between gap-6">
        <div>
          <Label
            id="codex-turn-state-label"
            for="codex-turn-state"
            :class="loading || saving || loadError ? 'cursor-not-allowed' : 'cursor-pointer'"
          >
            {{ t('providerAdvanced.codexTurnState') }}
          </Label>
          <p
            id="codex-turn-state-hint"
            class="mt-1 text-sm text-muted-foreground"
          >
            {{ t('providerAdvanced.codexTurnStateHint') }}
          </p>
        </div>
        <Switch
          id="codex-turn-state"
          class="shrink-0"
          :model-value="turnStateEnabled"
          :disabled="loading || saving || loadError"
          aria-labelledby="codex-turn-state-label"
          aria-describedby="codex-turn-state-hint"
          @update:model-value="$emit('update:turnStateEnabled', $event)"
        />
      </div>
      <div class="flex items-start justify-between gap-6">
        <div>
          <Label
            id="codex-turn-state-fallback-label"
            for="codex-turn-state-fallback"
            :class="loading || saving || loadError ? 'cursor-not-allowed' : 'cursor-pointer'"
          >
            {{ t('providerAdvanced.codexTurnStateFallback') }}
          </Label>
          <p
            id="codex-turn-state-fallback-hint"
            class="mt-1 text-sm text-muted-foreground"
          >
            {{ t('providerAdvanced.codexTurnStateFallbackHint') }}
          </p>
        </div>
        <select
          id="codex-turn-state-fallback"
          class="h-9 shrink-0 rounded-md border border-input bg-background px-3 text-sm"
          :value="turnStateFallback"
          :disabled="loading || saving || loadError"
          aria-labelledby="codex-turn-state-fallback-label"
          aria-describedby="codex-turn-state-fallback-hint"
          @change="onFallbackChange"
        >
          <option value="passthrough">{{ t('providerAdvanced.codexTurnStatePassthrough') }}</option>
          <option value="strict">{{ t('providerAdvanced.codexTurnStateStrict') }}</option>
        </select>
      </div>
    </div>
  </CardSection>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { CardSection } from '@/components/layout'
import Button from '@/components/ui/button.vue'
import Label from '@/components/ui/label.vue'
import Switch from '@/components/ui/switch.vue'

defineProps<{
  turnStateEnabled: boolean
  turnStateFallback: 'passthrough' | 'strict'
  telemetryEnabled: boolean
  enabled: boolean
  loading: boolean
  saving: boolean
  loadError: boolean
  hasChanges: boolean
}>()

const { t } = useI18n()
const emit = defineEmits<{
  save: []
  'update:enabled': [value: boolean]
  'update:telemetryEnabled': [value: boolean]
  'update:turnStateEnabled': [value: boolean]
  'update:turnStateFallback': [value: 'passthrough' | 'strict']
}>()

function onFallbackChange(event: Event) {
  const value = (event.target as HTMLSelectElement).value
  if (value === 'strict' || value === 'passthrough') emit('update:turnStateFallback', value)
}

</script>
