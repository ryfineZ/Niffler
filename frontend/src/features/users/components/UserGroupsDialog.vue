<template>
  <Dialog
    :model-value="open"
    :title="t('userGroups.title')"
    :description="t('userGroups.description')"
    size="4xl"
    persistent
    @update:model-value="handleDialogUpdate"
  >
    <div class="grid gap-4 lg:min-h-[560px] lg:grid-cols-[17rem_minmax(0,1fr)]">
      <div class="rounded-xl border border-border/70 bg-muted/20 p-3">
        <div class="mb-3 flex justify-end">
          <Button
            variant="ghost"
            size="icon"
            class="nav-action h-8 w-8"
            :title="t('userGroups.create')"
            @click="startCreate"
          >
            <Plus class="h-4 w-4" />
          </Button>
        </div>

        <div
          v-if="loading"
          class="rounded-lg border border-dashed border-border/70 px-3 py-8 text-center text-xs text-muted-foreground"
        >
          {{ t('userGroups.loading') }}
        </div>
        <div
          v-else-if="groups.length === 0"
          class="rounded-lg border border-dashed border-border/70 px-3 py-8 text-center text-xs text-muted-foreground"
        >
          {{ t('userGroups.empty') }}
        </div>
        <div
          v-else
          class="max-h-60 space-y-1.5 overflow-y-auto lg:max-h-none lg:overflow-visible"
        >
          <button
            v-for="group in groups"
            :key="group.id"
            type="button"
            role="tab"
            :aria-selected="editingGroupId === group.id"
            :class="groupButtonClass(group.id)"
            @click="selectGroup(group.id)"
          >
            <span class="min-w-0 flex-1 text-left">
              <span class="flex items-center gap-1.5">
                <span class="truncate text-sm font-medium">{{ group.name }}</span>
                <Badge
                  v-if="group.is_default"
                  variant="secondary"
                  class="h-5 px-1.5 py-0 text-[10px]"
                >
                  {{ t('userGroups.default') }}
                </Badge>
                <Badge
                  v-if="group.legacy_read_only"
                  variant="outline"
                  class="h-5 px-1.5 py-0 text-[10px]"
                  :title="group.legacy_read_only_reason"
                >
                  Niffler Core
                </Badge>
              </span>
            </span>
            <ChevronRight class="h-4 w-4 shrink-0 text-muted-foreground" />
          </button>
        </div>
      </div>

      <div class="min-w-0 rounded-xl border border-border/70 bg-background p-3 sm:p-4">
        <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
          <div class="min-w-0">
            <h4 class="truncate text-base font-semibold text-foreground">
              {{ editingGroupId ? t('userGroups.edit') : t('userGroups.create') }}
            </h4>
            <p class="text-xs text-muted-foreground">
              {{ selectedGroupReadOnly ? t('userGroups.readOnlyHint') : selectedGroup?.is_default ? t('userGroups.defaultHint') : t('userGroups.accessHint') }}
            </p>
          </div>
          <div
            v-if="editingGroupId"
            class="flex items-center gap-1"
          >
            <Button
              variant="ghost"
              size="icon"
              class="nav-action h-8 w-8"
              :class="selectedGroup?.is_default ? 'text-emerald-500 hover:text-emerald-500' : ''"
              :disabled="saving || selectedGroup?.is_default || selectedGroupReadOnly"
              :title="selectedGroupReadOnly ? t('userGroups.editInCore') : selectedGroup?.is_default ? t('userGroups.defaultGroup') : t('userGroups.setDefault')"
              @click="toggleDefault"
            >
              <BadgeCheck class="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              class="nav-action h-8 w-8"
              :disabled="saving || selectedGroup?.is_default || selectedGroupReadOnly"
              :title="selectedGroupReadOnly ? t('userGroups.editInCore') : t('userGroups.delete')"
              @click="deleteSelectedGroup"
            >
              <Trash2 class="h-4 w-4" />
            </Button>
          </div>
        </div>

        <div
          v-if="selectedGroupReadOnly"
          class="mb-4 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-200"
        >
          {{ selectedGroup?.legacy_read_only_reason || t('userGroups.readOnlyMessage') }}
        </div>

        <fieldset
          class="space-y-5"
          :disabled="selectedGroupReadOnly"
          :class="selectedGroupReadOnly ? 'opacity-75' : ''"
        >
          <div class="space-y-4">
            <div class="space-y-2">
              <Label class="text-sm font-medium">{{ t('userGroups.name') }}</Label>
              <Input
                v-model="form.name"
                class="h-10"
                :placeholder="t('userGroups.namePlaceholder')"
              />
            </div>

            <div class="grid gap-3 sm:grid-cols-2">
              <div class="space-y-2">
                <Label class="text-sm font-medium">{{ t('userGroups.visibility') }}</Label>
                <select
                  v-model="form.visibility"
                  class="h-10 w-full rounded-md border border-input bg-background px-3 text-sm"
                >
                  <option value="public">
                    {{ t('userGroups.publicVisibility') }}
                  </option>
                  <option value="internal">
                    {{ t('userGroups.internalVisibility') }}
                  </option>
                </select>
              </div>

              <div class="space-y-2">
                <Label class="text-sm font-medium">{{ t('userGroupDiscount.defaultDiscount') }}</Label>
                <Input
                  :model-value="form.sales_multiplier"
                  type="number"
                  min="0"
                  step="0.01"
                  class="h-10"
                  :placeholder="t('userGroupDiscount.discountPlaceholder')"
                  @update:model-value="(value) => form.sales_multiplier = parseNumberInput(value, { allowFloat: true, min: 0, max: 100 }) ?? 1"
                />
              </div>
            </div>

            <ManagedInstructionsConfigSection
              :model-value="form.managed_instructions"
              @update:model-value="(value) => form.managed_instructions = value"
            />

            <div class="space-y-2">
              <div class="flex flex-wrap items-center justify-between gap-2">
                <Label class="text-sm font-medium">{{ t('userGroupDiscount.modelDiscount') }}</Label>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  class="h-8"
                  :disabled="globalModelSelectOptions.length === 0"
                  @click="addModelSalesMultiplierRow"
                >
                  <Plus class="mr-1.5 h-3.5 w-3.5" />
                  {{ t('userGroups.addModel') }}
                </Button>
              </div>
              <div class="rounded-lg border border-border/70">
                <div
                  v-if="modelSalesMultiplierRows.length === 0"
                  class="px-3 py-4 text-sm text-muted-foreground"
                >
                  {{ t('userGroupDiscount.modelDiscountHint') }}
                </div>
                <div
                  v-for="row in modelSalesMultiplierRows"
                  :key="row.id"
                  class="grid gap-2 border-b border-border/60 p-2 last:border-b-0 sm:grid-cols-[minmax(0,1fr)_8rem_2.25rem]"
                >
                  <select
                    v-model="row.modelId"
                    class="h-9 min-w-0 rounded-md border border-input bg-background px-3 text-sm"
                  >
                    <option value="">
                      {{ t('userGroups.chooseModel') }}
                    </option>
                    <option
                      v-for="option in globalModelSelectOptions"
                      :key="option.value"
                      :value="option.value"
                    >
                      {{ option.label }}
                    </option>
                  </select>
                  <Input
                    :model-value="row.multiplier ?? ''"
                    type="number"
                    min="0"
                    step="0.01"
                    class="h-9"
                    :placeholder="t('userGroupDiscount.discount')"
                    @update:model-value="(value) => row.multiplier = parseNumberInput(value, { allowFloat: true, min: 0, max: 100 }) ?? undefined"
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    class="filter-action h-9 w-9"
                    :title="t('userGroups.delete')"
                    @click="removeModelSalesMultiplierRow(row.id)"
                  >
                    <Trash2 class="h-4 w-4" />
                  </Button>
                </div>
              </div>
              <div
                v-if="providerModelMultiplierSourceOptions.length > 0"
                class="rounded-lg border border-border/70 bg-muted/20 p-3"
              >
                <div class="mb-2 text-xs font-medium text-muted-foreground">
                  {{ t('userGroups.batchByProvider') }}
                </div>
                <div class="grid gap-2 sm:grid-cols-2">
                  <div
                    v-for="provider in providerModelMultiplierSourceOptions"
                    :key="provider.id"
                    class="rounded-lg border border-border/60 bg-background/70 p-2"
                  >
                    <div class="mb-2 truncate text-xs font-medium">
                      {{ provider.name }} · {{ provider.modelIds.length }} {{ t('userGroups.models') }}
                    </div>
                    <div class="flex gap-2">
                      <Input
                        :model-value="getProviderBatchSalesMultiplier(provider.id) ?? ''"
                        type="number"
                        min="0"
                        step="0.01"
                        class="h-8 min-w-0"
                        :placeholder="t('userGroups.providerMultiplierPlaceholder')"
                        @update:model-value="(value) => setProviderBatchSalesMultiplier(provider.id, value)"
                      />
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        class="h-8 shrink-0"
                        @click="applyProviderSalesMultiplier(provider)"
                      >
                        {{ t('userGroups.apply') }}
                      </Button>
                    </div>
                  </div>
                </div>
              </div>
              <p class="text-xs text-muted-foreground">
                {{ t('userGroupDiscount.modelDiscountBatchHint') }}
              </p>
            </div>

            <div class="space-y-2">
              <Label class="text-sm font-medium">{{ t('userGroups.members') }}</Label>
              <MultiSelect
                v-model="memberUserIds"
                :options="userOptions"
                :search-threshold="0"
                :disabled="selectedGroup?.is_default"
                :placeholder="t('userGroups.chooseUser')"
                :empty-text="t('userGroups.noUsers')"
                :no-results-text="t('userGroups.noMatchingUsers')"
              />
            </div>
          </div>

          <div class="space-y-4 border-t border-border/60 pt-5">
            <div class="flex flex-wrap items-baseline justify-between gap-x-2 gap-y-1 pb-2 border-b border-border/60">
              <span class="text-sm font-medium">{{ t('userGroups.permissions') }}</span>
              <span class="text-[11px] text-muted-foreground">
                {{ t('userGroups.permissionsHint') }}
              </span>
            </div>

            <div class="space-y-2">
              <Label class="text-sm font-medium">{{ t('userGroups.allowedProviders') }}</Label>
              <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
                <div class="flex w-full items-center sm:w-auto sm:shrink-0">
                  <Switch
                    :model-value="form.allowed_providers_mode === 'unrestricted'"
                    @update:model-value="(v) => (form.allowed_providers_mode = v ? 'unrestricted' : 'specific')"
                  />
                </div>
                <div class="min-w-0 flex-1">
                  <MultiSelect
                    v-model="form.allowed_providers"
                    :options="providerOptions"
                    :search-threshold="0"
                    :disabled="form.allowed_providers_mode === 'unrestricted'"
                    :placeholder="form.allowed_providers_mode === 'unrestricted' ? t('userGroups.unrestricted') : t('userGroups.chooseProvider')"
                    :empty-text="t('userGroups.noOptions')"
                  />
                </div>
              </div>
            </div>

            <div class="space-y-2">
              <Label class="text-sm font-medium">{{ t('userGroups.allowedEndpoints') }}</Label>
              <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
                <div class="flex w-full items-center sm:w-auto sm:shrink-0">
                  <Switch
                    :model-value="form.allowed_api_formats_mode === 'unrestricted'"
                    @update:model-value="(v) => (form.allowed_api_formats_mode = v ? 'unrestricted' : 'specific')"
                  />
                </div>
                <div class="min-w-0 flex-1">
                  <MultiSelect
                    v-model="form.allowed_api_formats"
                    :options="apiFormatOptions"
                    :search-threshold="0"
                    :disabled="form.allowed_api_formats_mode === 'unrestricted'"
                    :placeholder="form.allowed_api_formats_mode === 'unrestricted' ? t('userGroups.unrestricted') : t('userGroups.chooseEndpoint')"
                    :empty-text="t('userGroups.noOptions')"
                  />
                </div>
              </div>
            </div>

            <div class="space-y-2">
              <Label class="text-sm font-medium">{{ t('userGroups.allowedModels') }}</Label>
              <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
                <div class="flex w-full items-center sm:w-auto sm:shrink-0">
                  <Switch
                    :model-value="form.allowed_models_mode === 'unrestricted'"
                    @update:model-value="(v) => (form.allowed_models_mode = v ? 'unrestricted' : 'specific')"
                  />
                </div>
                <div class="min-w-0 flex-1">
                  <MultiSelect
                    v-model="form.allowed_models"
                    :options="modelOptions"
                    :search-threshold="0"
                    :disabled="form.allowed_models_mode === 'unrestricted'"
                    :placeholder="form.allowed_models_mode === 'unrestricted' ? t('userGroups.unrestricted') : t('userGroups.chooseModel')"
                    :empty-text="t('userGroups.noOptions')"
                  />
                </div>
              </div>
              <div
                v-if="form.allowed_models_mode === 'specific' && providerModelNameSourceOptions.length > 0"
                class="rounded-lg border border-border/70 bg-muted/20 p-3"
              >
                <div class="mb-2 text-xs font-medium text-muted-foreground">
                  {{ t('userGroups.quickByProvider') }}
                </div>
                <div class="flex flex-wrap gap-2">
                  <button
                    v-for="provider in providerModelNameSourceOptions"
                    :key="provider.id"
                    type="button"
                    class="filter-chip rounded-full border px-3 py-1.5 text-xs font-medium transition-colors"
                    :class="[
                      provider.allSelected
                        ? 'border-primary bg-primary text-primary-foreground'
                        : provider.someSelected
                          ? 'border-primary/60 bg-primary/10 text-primary'
                          : 'border-border/60 bg-background text-muted-foreground hover:border-border hover:bg-muted/40'
                    ]"
                    @click="toggleProviderAllowedModels(provider)"
                  >
                    {{ provider.name }} · {{ provider.selectedCount }}/{{ provider.modelNames.length }}
                  </button>
                </div>
              </div>
            </div>

            <div class="space-y-2">
              <Label class="text-sm font-medium">{{ t('userGroups.rateLimit') }}</Label>
              <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
                <div class="flex w-full items-center sm:w-auto sm:shrink-0">
                  <Switch
                    :model-value="form.rate_limit_mode === 'system'"
                    @update:model-value="(v) => (form.rate_limit_mode = v ? 'system' : 'custom')"
                  />
                </div>
                <div class="min-w-0 flex-1">
                  <Input
                    :model-value="form.rate_limit ?? ''"
                    type="number"
                    min="0"
                    max="10000"
                    class="h-10"
                    :disabled="form.rate_limit_mode === 'system'"
                    :placeholder="form.rate_limit_mode === 'system' ? t('userGroups.systemDefault') : t('userGroups.noRateLimit')"
                    @update:model-value="(value) => form.rate_limit = parseNumberInput(value, { min: 0, max: 10000 })"
                  />
                </div>
              </div>
            </div>

            <div class="space-y-2">
              <Label class="text-sm font-medium">{{ t('userGroups.concurrentLimit') }}</Label>
              <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
                <div class="flex w-full items-center sm:w-auto sm:shrink-0">
                  <Switch
                    :model-value="form.concurrent_limit_mode === 'system'"
                    @update:model-value="(v) => (form.concurrent_limit_mode = v ? 'system' : 'custom')"
                  />
                </div>
                <div class="min-w-0 flex-1">
                  <Input
                    :model-value="form.concurrent_limit ?? ''"
                    type="number"
                    min="0"
                    max="10000"
                    class="h-10"
                    :disabled="form.concurrent_limit_mode === 'system'"
                    :placeholder="form.concurrent_limit_mode === 'system' ? t('userGroups.systemDefault') : t('userGroups.noLimit')"
                    @update:model-value="(value) => form.concurrent_limit = parseNumberInput(value, { min: 0, max: 10000 })"
                  />
                </div>
              </div>
              <p class="text-xs text-muted-foreground">
                {{ t('userGroups.concurrentHint') }}
              </p>
            </div>
          </div>
        </fieldset>
      </div>
    </div>

    <template #footer>
      <Button
        variant="outline"
        :disabled="saving"
        @click="emit('close')"
      >
        {{ t('userGroups.close') }}
      </Button>
      <Button
        :disabled="saving || !form.name.trim() || selectedGroupReadOnly"
        :title="selectedGroupReadOnly ? t('userGroups.editInCore') : t('userGroups.save')"
        @click="saveGroup"
      >
        {{ t('userGroups.save') }}
      </Button>
    </template>
  </Dialog>

  <Dialog
    :model-value="deleteReplacementDialogOpen"
    :title="t('userGroupActions.replaceTitle')"
    :description="t('userGroupActions.replaceDescription')"
    size="md"
    :z-index="90"
    persistent
    @update:model-value="handleDeleteReplacementDialogUpdate"
  >
    <div class="space-y-4">
      <div class="rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-900 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-100">
        <p>
          {{ t('userGroupActions.keysInUse', { name: deleteConflictGroup?.name || t('userGroupActions.currentGroup'), count: deleteConflictApiKeyCount }) }}
        </p>
        <p
          v-if="deleteConflictExampleText"
          class="mt-1 text-xs opacity-90"
        >
          {{ t('userGroupActions.currentUsage') }}{{ deleteConflictExampleText }}
        </p>
      </div>

      <div class="space-y-2">
        <Label class="text-sm font-medium">{{ t('userGroupActions.replaceWith') }}</Label>
        <select
          v-model="replacementGroupId"
          class="h-10 w-full rounded-md border border-input bg-background px-3 text-sm"
          :disabled="replacingAndDeleting || replacementGroupOptions.length === 0"
        >
          <option
            v-for="group in replacementGroupOptions"
            :key="group.id"
            :value="group.id"
          >
            {{ group.name }}{{ group.is_default ? t('userGroupActions.defaultSuffix') : '' }}
          </option>
        </select>
        <p
          v-if="replacementGroupOptions.length === 0"
          class="text-xs text-destructive"
        >
          {{ t('userGroupActions.noReplacement') }}
        </p>
        <p
          v-else
          class="text-xs text-muted-foreground"
        >
          {{ t('userGroupActions.replaceHint') }}
        </p>
      </div>
    </div>

    <template #footer>
      <Button
        variant="ghost"
        :disabled="replacingAndDeleting"
        @click="inspectDeleteConflictApiKeys"
      >
        {{ t('userGroupActions.inspectKeys') }}
      </Button>
      <Button
        variant="outline"
        :disabled="replacingAndDeleting"
        @click="resetDeleteReplacementDialog"
      >
        {{ t('userGroupActions.cancel') }}
      </Button>
      <Button
        :disabled="replacingAndDeleting || !replacementGroupId || replacementGroupOptions.length === 0"
        @click="replaceAndDeleteGroup"
      >
        {{ replacingAndDeleting ? t('userGroupActions.processing') : t('userGroupActions.migrateDelete') }}
      </Button>
    </template>
  </Dialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { BadgeCheck, ChevronRight, Plus, Trash2 } from 'lucide-vue-next'
import {
  Badge,
  Button,
  Dialog,
  Input,
  Label,
  Switch,
} from '@/components/ui'
import { MultiSelect } from '@/components/common'
import { useUsersStore } from '@/stores/users'
import { useToast } from '@/composables/useToast'
import { useConfirm } from '@/composables/useConfirm'
import { parseApiError } from '@/utils/errorParser'
import { isApiError } from '@/types/api-error'
import { parseNumberInput } from '@/utils/form'
import { cn } from '@/lib/utils'
import { useUserAccessControlOptions } from '@/features/users/composables/useUserAccessControlOptions'
import ManagedInstructionsConfigSection from './ManagedInstructionsConfigSection.vue'
import type {
  ListPolicyMode,
  ManagedInstructionsConfig,
  RateLimitPolicyMode,
  UpsertUserGroupRequest,
  User,
  UserGroup,
} from '@/api/users'

const props = defineProps<{
  open: boolean
  users: User[]
}>()

const emit = defineEmits<{
  close: []
  changed: []
  inspectApiKeyGroup: [groupId: string]
}>()

const { t } = useI18n()

const usersStore = useUsersStore()
const { success, error, warning } = useToast()
const { confirmDanger, confirmInfo } = useConfirm()
const {
  providers,
  globalModels,
  providerOptions,
  apiFormatOptions,
  modelOptions,
  loadAccessControlOptions,
} = useUserAccessControlOptions()

const loading = ref(false)
const saving = ref(false)
const groups = ref<UserGroup[]>([])
const editingGroupId = ref<string | null>(null)
const memberUserIds = ref<string[]>([])
const modelSalesMultiplierRows = ref<ModelSalesMultiplierRow[]>([])
const providerBatchSalesMultipliers = ref<Record<string, number | undefined>>({})
const deleteReplacementDialogOpen = ref(false)
const deleteConflictGroup = ref<UserGroup | null>(null)
const deleteConflictPayload = ref<UserGroupApiKeyConflictPayload | null>(null)
const replacementGroupId = ref('')
const replacingAndDeleting = ref(false)
let modelSalesMultiplierRowSequence = 0

const form = ref({
  name: '',
  visibility: 'public' as 'public' | 'internal',
  sales_multiplier: 1,
  managed_instructions: null as ManagedInstructionsConfig | null,
  allowed_providers_mode: 'unrestricted' as ListPolicyMode,
  allowed_api_formats_mode: 'unrestricted' as ListPolicyMode,
  allowed_models_mode: 'unrestricted' as ListPolicyMode,
  allowed_providers: [] as string[],
  allowed_api_formats: [] as string[],
  allowed_models: [] as string[],
  rate_limit_mode: 'system' as RateLimitPolicyMode,
  rate_limit: undefined as number | undefined,
  concurrent_limit_mode: 'system' as RateLimitPolicyMode,
  concurrent_limit: undefined as number | undefined,
})

const selectedGroup = computed(() => groups.value.find((group) => group.id === editingGroupId.value) ?? null)
const selectedGroupReadOnly = computed(() => selectedGroup.value?.legacy_read_only === true)
const replacementGroupOptions = computed(() =>
  groups.value.filter((group) =>
    group.id !== deleteConflictGroup.value?.id
    && group.legacy_read_only !== true
  ),
)
const deleteConflictApiKeyCount = computed(() => Number(deleteConflictPayload.value?.api_key_count ?? 0))
const deleteConflictExampleText = computed(() => formatConflictApiKeyExamples(deleteConflictPayload.value?.api_keys))

function showSelectedGroupReadOnly(): void {
  warning(selectedGroup.value?.legacy_read_only_reason || t('userGroups.readOnlyMessage'))
}
const userOptions = computed(() => props.users.map((user) => ({
  label: `${user.username}${user.email ? ` (${user.email})` : ''}`,
  value: user.id,
})))

interface ModelSalesMultiplierRow {
  id: string
  modelId: string
  multiplier?: number
}

interface ProviderModelNameSource {
  id: string
  name: string
  modelNames: string[]
  selectedCount: number
  allSelected: boolean
  someSelected: boolean
}

interface ProviderModelMultiplierSource {
  id: string
  name: string
  modelIds: string[]
}

interface UserGroupApiKeyConflictItem {
  id?: string
  name?: string | null
  user_id?: string
  username?: string
  email?: string | null
}

interface UserGroupApiKeyConflictPayload {
  detail?: string
  api_key_count?: number
  api_keys?: UserGroupApiKeyConflictItem[]
}

const globalModelById = computed(() => {
  const map = new Map<string, { id: string; name: string; display_name?: string | null }>()
  for (const model of globalModels.value) {
    map.set(model.id, model)
  }
  return map
})

const providerNamesByGlobalModelId = computed(() => {
  const map = new Map<string, string[]>()
  for (const provider of providers.value) {
    for (const modelId of provider.global_model_ids || []) {
      const names = map.get(modelId) ?? []
      names.push(provider.name)
      map.set(modelId, names)
    }
  }
  return map
})

const globalModelSelectOptions = computed(() => {
  const knownModelIds = new Set(globalModels.value.map((model) => model.id))
  const loadedOptions = globalModels.value.map((model) => {
    const providerNames = providerNamesByGlobalModelId.value.get(model.id) ?? []
    const providerText = providerNames.length ? ` · ${providerNames.join(' / ')}` : ''
    const modelText = model.display_name && model.display_name !== model.name
      ? `${model.display_name} · ${model.name}`
      : (model.name || model.id)
    return {
      value: model.id,
      label: `${modelText}${providerText}`,
    }
  })
  const missingModelIds = Array.from(new Set(
    modelSalesMultiplierRows.value
      .map((row) => row.modelId)
      .filter((modelId) => modelId && !knownModelIds.has(modelId)),
  ))
  const missingOptions = missingModelIds
    .map((modelId) => ({
      value: modelId,
      label: `${modelId} · ${t('userGroupActions.invalid')}`,
    }))
  return [...loadedOptions, ...missingOptions]
})

const providerModelNameSourceOptions = computed<ProviderModelNameSource[]>(() => {
  const selectedModelNames = new Set(form.value.allowed_models)
  return providers.value
    .map((provider) => {
      const modelNames = Array.from(new Set(
        (provider.global_model_ids || [])
          .map((modelId) => globalModelById.value.get(modelId)?.name)
          .filter((name): name is string => !!name),
      ))
      const selectedCount = modelNames.filter((name) => selectedModelNames.has(name)).length
      return {
        id: provider.id,
        name: provider.name,
        modelNames,
        selectedCount,
        allSelected: modelNames.length > 0 && selectedCount === modelNames.length,
        someSelected: selectedCount > 0,
      }
    })
    .filter((provider) => provider.modelNames.length > 0)
})

const providerModelMultiplierSourceOptions = computed<ProviderModelMultiplierSource[]>(() =>
  providers.value
    .map((provider) => {
      const modelIds = Array.from(new Set(
        (provider.global_model_ids || []).filter((modelId) => globalModelById.value.has(modelId)),
      ))
      return {
        id: provider.id,
        name: provider.name,
        modelIds,
      }
    })
    .filter((provider) => provider.modelIds.length > 0),
)

watch(
  () => props.open,
  (open) => {
    if (!open) {
      resetDeleteReplacementDialog()
      return
    }
    void loadDialogData()
    void loadAccessControlOptions().catch((err) => {
      error(parseApiError(err, t('userGroupActions.loadAccessFailed')))
    })
  },
)

function handleDialogUpdate(value: boolean): void {
  if (!value) emit('close')
}

function handleDeleteReplacementDialogUpdate(value: boolean): void {
  if (value) return
  resetDeleteReplacementDialog()
}

async function loadDialogData(): Promise<void> {
  loading.value = true
  try {
    const response = await usersStore.listUserGroups()
    groups.value = response.items
    if (editingGroupId.value && !groups.value.some((group) => group.id === editingGroupId.value)) {
      editingGroupId.value = null
    }
    const nextGroup = editingGroupId.value
      ? groups.value.find((group) => group.id === editingGroupId.value) ?? null
      : groups.value[0] ?? null
    if (nextGroup) {
      await selectGroup(nextGroup.id)
    } else {
      startCreate()
    }
  } catch (err) {
    error(parseApiError(err, t('userGroupActions.loadGroupsFailed')))
  } finally {
    loading.value = false
  }
}

async function selectGroup(groupId: string): Promise<void> {
  const group = groups.value.find((item) => item.id === groupId)
  if (!group) return
  editingGroupId.value = group.id
  form.value = {
    name: group.name,
    visibility: group.visibility === 'internal' ? 'internal' : 'public',
    sales_multiplier: group.sales_multiplier ?? 1,
    managed_instructions: group.managed_instructions
      ? { ...group.managed_instructions }
      : null,
    allowed_providers_mode: normalizeListMode(group.allowed_providers_mode),
    allowed_api_formats_mode: normalizeListMode(group.allowed_api_formats_mode),
    allowed_models_mode: normalizeListMode(group.allowed_models_mode),
    allowed_providers: group.allowed_providers ? [...group.allowed_providers] : [],
    allowed_api_formats: group.allowed_api_formats ? [...group.allowed_api_formats] : [],
    allowed_models: group.allowed_models ? [...group.allowed_models] : [],
    rate_limit_mode: normalizeRateMode(group.rate_limit_mode),
    rate_limit: group.rate_limit ?? undefined,
    concurrent_limit_mode: normalizeRateMode(group.concurrent_limit_mode),
    concurrent_limit: group.concurrent_limit ?? undefined,
  }
  modelSalesMultiplierRows.value = rowsFromModelSalesMultipliers(group.model_sales_multipliers)
  providerBatchSalesMultipliers.value = {}
  try {
    const members = await usersStore.listUserGroupMembers(group.id)
    memberUserIds.value = members.map((member) => member.user_id)
  } catch (err) {
    memberUserIds.value = []
    error(parseApiError(err, t('userGroupActions.loadMembersFailed')))
  }
}

function normalizeListMode(mode: ListPolicyMode): ListPolicyMode {
  return mode === 'specific' ? 'specific' : 'unrestricted'
}

function normalizeRateMode(mode: RateLimitPolicyMode): RateLimitPolicyMode {
  return mode === 'custom' ? 'custom' : 'system'
}

function startCreate(): void {
  editingGroupId.value = null
  form.value = {
    name: '',
    visibility: 'public',
    sales_multiplier: 1,
    managed_instructions: null,
    allowed_providers_mode: 'unrestricted',
    allowed_api_formats_mode: 'unrestricted',
    allowed_models_mode: 'unrestricted',
    allowed_providers: [],
    allowed_api_formats: [],
    allowed_models: [],
    rate_limit_mode: 'system',
    rate_limit: undefined,
    concurrent_limit_mode: 'system',
    concurrent_limit: undefined,
  }
  modelSalesMultiplierRows.value = []
  providerBatchSalesMultipliers.value = {}
  memberUserIds.value = []
}

function groupButtonClass(groupId: string): string {
  return cn(
    'flex w-full items-center gap-2 rounded-lg border px-3 py-2 transition-colors',
    editingGroupId.value === groupId
      ? 'border-primary/50 bg-primary/10'
      : 'border-transparent hover:border-border hover:bg-background',
  )
}

async function toggleDefault(): Promise<void> {
  const group = selectedGroup.value
  if (!group || group.is_default) return
  if (selectedGroupReadOnly.value) {
    showSelectedGroupReadOnly()
    return
  }
  const confirmed = await confirmInfo(
    t('userGroupActions.setDefaultConfirm', { name: group.name }),
    t('userGroups.setDefault'),
  )
  if (!confirmed) return
  saving.value = true
  try {
    await usersStore.setDefaultUserGroup(group.id)
    success(t('userGroupActions.defaultUpdated'))
    emit('changed')
    await loadDialogData()
  } catch (err) {
    error(parseApiError(err, t('userGroupActions.setDefaultFailed')))
  } finally {
    saving.value = false
  }
}

function buildPayload(): UpsertUserGroupRequest {
  return {
    name: form.value.name.trim(),
    visibility: form.value.visibility,
    sales_multiplier: form.value.sales_multiplier,
    model_sales_multipliers: parseModelSalesMultipliers(),
    managed_instructions: form.value.managed_instructions
      ? { ...form.value.managed_instructions }
      : null,
    allowed_providers_mode: form.value.allowed_providers_mode,
    allowed_api_formats_mode: form.value.allowed_api_formats_mode,
    allowed_models_mode: form.value.allowed_models_mode,
    allowed_providers: form.value.allowed_providers_mode === 'specific'
      ? [...form.value.allowed_providers]
      : null,
    allowed_api_formats: form.value.allowed_api_formats_mode === 'specific'
      ? [...form.value.allowed_api_formats]
      : null,
    allowed_models: form.value.allowed_models_mode === 'specific'
      ? [...form.value.allowed_models]
      : null,
    rate_limit_mode: form.value.rate_limit_mode,
    rate_limit: form.value.rate_limit_mode === 'custom'
      ? (form.value.rate_limit ?? 0)
      : null,
    concurrent_limit_mode: form.value.concurrent_limit_mode,
    concurrent_limit: form.value.concurrent_limit_mode === 'custom'
      ? (form.value.concurrent_limit ?? 0)
      : null,
  }
}

function nextModelSalesMultiplierRowId(): string {
  modelSalesMultiplierRowSequence += 1
  return `model-sales-${modelSalesMultiplierRowSequence}`
}

function rowsFromModelSalesMultipliers(value: unknown): ModelSalesMultiplierRow[] {
  if (!value || Array.isArray(value) || typeof value !== 'object') return []
  return Object.entries(value as Record<string, unknown>)
    .filter(([modelId, multiplier]) =>
      modelId.trim()
      && typeof multiplier === 'number'
      && Number.isFinite(multiplier)
      && multiplier >= 0,
    )
    .map(([modelId, multiplier]) => ({
      id: nextModelSalesMultiplierRowId(),
      modelId,
      multiplier: multiplier as number,
    }))
}

function addModelSalesMultiplierRow(): void {
  const usedModelIds = new Set(modelSalesMultiplierRows.value.map((row) => row.modelId).filter(Boolean))
  const firstAvailableModel = globalModelSelectOptions.value.find((option) => !usedModelIds.has(option.value))
  modelSalesMultiplierRows.value.push({
    id: nextModelSalesMultiplierRowId(),
    modelId: firstAvailableModel?.value ?? '',
    multiplier: form.value.sales_multiplier,
  })
}

function removeModelSalesMultiplierRow(rowId: string): void {
  modelSalesMultiplierRows.value = modelSalesMultiplierRows.value.filter((row) => row.id !== rowId)
}

function parseModelSalesMultipliers(): Record<string, number> | null {
  const result: Record<string, number> = {}
  const seenModelIds = new Set<string>()
  for (const row of modelSalesMultiplierRows.value) {
    const modelId = row.modelId.trim()
    if (!modelId && row.multiplier === undefined) continue
    if (!modelId) throw new Error(t('userGroupDiscount.modelRequired'))
    if (seenModelIds.has(modelId)) throw new Error(t('userGroupDiscount.duplicateModel'))
    if (row.multiplier === undefined || !Number.isFinite(row.multiplier) || row.multiplier < 0) {
      throw new Error(t('userGroupDiscount.invalidDiscount'))
    }
    seenModelIds.add(modelId)
    result[modelId] = row.multiplier
  }
  return Object.keys(result).length ? result : null
}

function toggleProviderAllowedModels(provider: ProviderModelNameSource): void {
  const nextModelNames = new Set(form.value.allowed_models)
  if (provider.allSelected) {
    for (const modelName of provider.modelNames) {
      nextModelNames.delete(modelName)
    }
  } else {
    for (const modelName of provider.modelNames) {
      nextModelNames.add(modelName)
    }
  }
  form.value.allowed_models = Array.from(nextModelNames)
}

function getProviderBatchSalesMultiplier(providerId: string): number | undefined {
  return providerBatchSalesMultipliers.value[providerId]
}

function setProviderBatchSalesMultiplier(providerId: string, value: string | number | null | undefined): void {
  providerBatchSalesMultipliers.value = {
    ...providerBatchSalesMultipliers.value,
    [providerId]: parseNumberInput(value, { allowFloat: true, min: 0, max: 100 }),
  }
}

function applyProviderSalesMultiplier(provider: ProviderModelMultiplierSource): void {
  const multiplier = getProviderBatchSalesMultiplier(provider.id)
  if (multiplier === undefined) {
    error(t('userGroupDiscount.providerDiscountRequired'))
    return
  }
  const nextByModelId = new Map<string, number>()
  for (const row of modelSalesMultiplierRows.value) {
    if (row.modelId && row.multiplier !== undefined) {
      nextByModelId.set(row.modelId, row.multiplier)
    }
  }
  for (const modelId of provider.modelIds) {
    nextByModelId.set(modelId, multiplier)
  }
  modelSalesMultiplierRows.value = Array.from(nextByModelId.entries()).map(([modelId, multiplier]) => ({
    id: nextModelSalesMultiplierRowId(),
    modelId,
    multiplier,
  }))
}

async function saveGroup(): Promise<void> {
  if (!form.value.name.trim()) return
  if (selectedGroupReadOnly.value) {
    showSelectedGroupReadOnly()
    return
  }
  saving.value = true
  try {
    const saved = editingGroupId.value
      ? await usersStore.updateUserGroup(editingGroupId.value, buildPayload())
      : await usersStore.createUserGroup(buildPayload())
    if (!saved.is_default) {
      await usersStore.replaceUserGroupMembers(saved.id, memberUserIds.value)
    }
    success(t('userGroupActions.saved'))
    emit('changed')
    editingGroupId.value = saved.id
    await loadDialogData()
  } catch (err) {
    error(parseApiError(err, t('userGroupActions.saveFailed')))
  } finally {
    saving.value = false
  }
}

function readUserGroupApiKeyConflictPayload(err: unknown): UserGroupApiKeyConflictPayload | null {
  if (!isApiError(err) || err.response?.status !== 409) return null
  const data = err.response.data as UserGroupApiKeyConflictPayload | undefined
  if (!data || typeof data !== 'object') return null
  const count = Number(data.api_key_count ?? 0)
  if (!Number.isFinite(count) || count <= 0) return null
  return data
}

function formatConflictApiKeyExamples(items: UserGroupApiKeyConflictItem[] | undefined): string {
  const examples = (items ?? [])
    .slice(0, 5)
    .map((item) => {
      const keyName = String(item.name || item.id || t('userGroupActions.unnamedKey')).trim()
      const userName = String(item.username || item.user_id || t('userGroupActions.unknownUser')).trim()
      const email = String(item.email || '').trim()
      return email && email !== userName ? `${keyName}（${userName} / ${email}）` : `${keyName}（${userName}）`
    })
    .filter(Boolean)
  return examples.length ? examples.join('、') : ''
}

function showUserGroupApiKeyConflict(
  group: UserGroup,
  payload: UserGroupApiKeyConflictPayload,
) {
  deleteConflictGroup.value = group
  deleteConflictPayload.value = payload
  replacementGroupId.value = replacementGroupOptions.value[0]?.id ?? ''
  deleteReplacementDialogOpen.value = true
}

function resetDeleteReplacementDialog(): void {
  if (replacingAndDeleting.value) return
  deleteReplacementDialogOpen.value = false
  deleteConflictGroup.value = null
  deleteConflictPayload.value = null
  replacementGroupId.value = ''
}

function inspectDeleteConflictApiKeys(): void {
  const groupId = deleteConflictGroup.value?.id
  if (!groupId) return
  resetDeleteReplacementDialog()
  emit('inspectApiKeyGroup', groupId)
}

async function replaceAndDeleteGroup(): Promise<void> {
  const group = deleteConflictGroup.value
  const targetGroupId = replacementGroupId.value
  if (!group || !targetGroupId) return
  replacingAndDeleting.value = true
  try {
    const result = await usersStore.deleteUserGroupWithReplacement(group.id, targetGroupId)
    success(t('userGroupActions.migrated', { count: result.migrated_api_key_count }))
    deleteReplacementDialogOpen.value = false
    deleteConflictGroup.value = null
    deleteConflictPayload.value = null
    replacementGroupId.value = ''
    emit('changed')
    editingGroupId.value = null
    await loadDialogData()
  } catch (err) {
    error(parseApiError(err, t('userGroupActions.replaceFailed')))
  } finally {
    replacingAndDeleting.value = false
  }
}

async function deleteSelectedGroup(): Promise<void> {
  if (!selectedGroup.value) return
  const group = selectedGroup.value
  if (selectedGroupReadOnly.value) {
    showSelectedGroupReadOnly()
    return
  }
  const confirmed = await confirmDanger(
    t('userGroupActions.deleteConfirm', { name: group.name }),
    t('userGroupActions.deleteTitle'),
  )
  if (!confirmed) return
  saving.value = true
  try {
    await usersStore.deleteUserGroup(group.id)
    success(t('userGroupActions.deleted'))
    emit('changed')
    editingGroupId.value = null
    await loadDialogData()
  } catch (err) {
    const conflict = readUserGroupApiKeyConflictPayload(err)
    if (conflict) {
      showUserGroupApiKeyConflict(group, conflict)
      return
    }
    error(parseApiError(err, t('userGroupActions.deleteFailed')))
  } finally {
    saving.value = false
  }
}
</script>
