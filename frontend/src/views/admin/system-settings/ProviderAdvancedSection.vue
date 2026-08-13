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

    <div class="max-w-2xl">
      <div class="flex items-start justify-between gap-6">
        <div>
          <Label
            id="codex-oauth-identity-convergence-label"
            for="codex-oauth-identity-convergence"
            :class="loading || saving || loadError ? 'cursor-not-allowed' : 'cursor-pointer'"
          >
            {{ t('providerAdvanced.codexIdentityConvergence') }}
          </Label>
          <p id="codex-oauth-identity-convergence-hint" class="mt-1 text-sm text-muted-foreground">
            {{ t('providerAdvanced.codexIdentityConvergenceHint') }}
          </p>
          <p v-if="loadError" class="mt-2 text-sm text-destructive" role="alert">
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
    </div>
  </CardSection>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { CardSection } from '@/components/layout'
import Button from '@/components/ui/button.vue'
import Label from '@/components/ui/label.vue'
import Switch from '@/components/ui/switch.vue'

const { t } = useI18n()

defineProps<{
  enabled: boolean
  loading: boolean
  saving: boolean
  loadError: boolean
  hasChanges: boolean
}>()

defineEmits<{
  save: []
  'update:enabled': [value: boolean]
}>()
</script>
