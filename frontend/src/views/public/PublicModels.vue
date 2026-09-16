<template>
  <div class="min-h-screen bg-background text-foreground literary-grid literary-paper">
    <main class="relative z-10 mx-auto max-w-[1480px] px-4 pb-16 pt-12 sm:px-6 lg:px-8 lg:pt-16">
      <section class="border-l-4 border-primary pl-5 sm:pl-8">
        <div class="text-xs font-bold tracking-[0.2em] text-primary">{{ t('models.eyebrow') }}</div>
        <div class="mt-4 flex flex-col justify-between gap-5 lg:flex-row lg:items-end">
          <div>
            <h1 class="font-serif text-4xl font-semibold tracking-tight sm:text-6xl">{{ t('models.title') }}</h1>
            <p class="mt-4 max-w-2xl text-sm leading-7 text-muted-foreground sm:text-base">{{ t('models.subtitle') }}</p>
          </div>
          <div class="shrink-0 rounded-full border border-border bg-background/70 px-4 py-2 text-sm font-medium shadow-sm">
            {{ t('models.total', { count: models.length }) }}
          </div>
        </div>
      </section>

      <section class="mt-8 flex items-start gap-3 border border-primary/25 bg-primary/[0.06] px-4 py-4 sm:px-5" aria-labelledby="pricing-note-title">
        <CircleDollarSign class="mt-0.5 h-5 w-5 shrink-0 text-primary" aria-hidden="true" />
        <div class="min-w-0">
          <h2 id="pricing-note-title" class="text-sm font-semibold text-foreground">{{ t('models.pricingNoteTitle') }}</h2>
          <p class="mt-1 text-sm leading-6 text-muted-foreground">{{ t(usesDedicatedDiscountPricing ? 'models.pricingNoteOfficialUsd' : 'models.pricingNote') }}</p>
        </div>
      </section>

      <section class="mt-6 grid min-w-0 grid-cols-[minmax(0,1fr)] gap-5 lg:grid-cols-[260px_minmax(0,1fr)]">
        <aside class="h-fit border border-border/70 bg-background/70 p-5 backdrop-blur-sm lg:sticky lg:top-24">
          <div class="relative">
            <Search class="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <input
              v-model="searchQuery"
              class="h-11 w-full border border-border bg-background pl-10 pr-3 text-sm outline-none transition focus:border-primary"
              :placeholder="t('models.searchPlaceholder')"
            >
          </div>

          <div class="mt-6">
            <div class="text-xs font-bold uppercase tracking-[0.14em] text-muted-foreground">{{ t('models.manufacturer') }}</div>
            <div class="mt-3 flex flex-wrap gap-2 lg:flex-col">
              <button
                v-for="manufacturer in manufacturers"
                :key="manufacturer.id"
                class="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 border px-3 py-2 text-left text-sm transition"
                :class="selectedManufacturer === manufacturer.id ? 'border-primary bg-primary/10 text-primary' : 'border-border bg-background/60 hover:border-primary/40'"
                @click="selectedManufacturer = manufacturer.id"
              >
                <span class="min-w-0 truncate">{{ manufacturer.id === ALL_MANUFACTURERS ? t('common.all') : manufacturer.label }}</span>
                <span class="shrink-0 text-xs tabular-nums text-muted-foreground">{{ manufacturerCount(manufacturer.id) }}</span>
              </button>
            </div>
          </div>

          <div class="mt-6">
            <div class="text-xs font-bold uppercase tracking-[0.14em] text-muted-foreground">{{ t('models.group') }}</div>
            <div class="mt-3 flex flex-wrap gap-2 lg:flex-col">
              <button
                v-for="group in groups"
                :key="group.id"
                class="grid min-w-0 grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-2 border px-3 py-2 text-left text-sm transition"
                :class="selectedGroup === group.id ? 'border-primary bg-primary/10 text-primary' : 'border-border bg-background/60 hover:border-primary/40'"
                @click="selectedGroup = group.id"
              >
                <span class="min-w-0 truncate">{{ group.id === ALL_GROUP ? t('common.all') : group.name }}</span>
                <span v-if="group.id !== ALL_GROUP" class="shrink-0 text-xs font-mono text-muted-foreground">{{ groupPriceLabel(group.id) }}</span>
                <span v-else aria-hidden="true"></span>
                <span class="shrink-0 text-xs tabular-nums text-muted-foreground">{{ groupCount(group.id) }}</span>
              </button>
            </div>
          </div>

        </aside>

        <div class="min-w-0">
          <div v-if="loading" class="flex min-h-80 items-center justify-center border border-border/70 bg-background/60">
            <div class="text-center text-sm text-muted-foreground">
              <RefreshCw class="mx-auto mb-3 h-5 w-5 animate-spin" />
              {{ t('models.loading') }}
            </div>
          </div>

          <div v-else-if="loadError" class="flex min-h-80 items-center justify-center border border-border/70 bg-background/60 p-8 text-center">
            <div>
              <p class="font-medium">{{ t('models.loadError') }}</p>
              <button class="mt-4 border border-primary px-4 py-2 text-sm font-semibold text-primary" @click="loadModels">
                {{ t('common.retry') }}
              </button>
            </div>
          </div>

          <div v-else-if="filteredModels.length === 0" class="flex min-h-80 items-center justify-center border border-border/70 bg-background/60 p-8 text-center">
            <div>
              <Boxes class="mx-auto h-8 w-8 text-muted-foreground" />
              <p class="mt-4 font-medium">{{ t('models.empty') }}</p>
              <p class="mt-1 text-sm text-muted-foreground">{{ t('models.emptyHint') }}</p>
            </div>
          </div>

          <div v-else class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-4 md:grid-cols-2 xl:grid-cols-3">
            <article
              v-for="model in filteredModels"
              :key="model.id"
              class="group flex min-h-[280px] min-w-0 max-w-full flex-col overflow-hidden border border-border/80 bg-background/75 p-4 shadow-sm backdrop-blur-sm transition hover:-translate-y-0.5 hover:border-primary/50 hover:shadow-md sm:p-5"
            >
              <div class="flex items-start justify-between gap-3">
                <div class="flex min-w-0 items-center gap-3">
                  <div
                    class="flex h-10 w-10 shrink-0 items-center justify-center rounded-full border bg-background font-serif text-lg font-semibold"
                    :class="modelBadgeClass(model.name)"
                    :aria-label="`${modelFamily(model.name)} model`"
                  >
                    <img
                      v-if="modelIcon(model.name)"
                      :src="modelIcon(model.name) || undefined"
                      :alt="`${modelFamily(model.name)} icon`"
                      class="h-5 w-5 object-contain"
                    >
                    <span v-else>{{ modelInitial(model.name) }}</span>
                  </div>
                  <div class="min-w-0">
                    <h2 class="truncate font-semibold" :title="model.display_name || model.name">{{ model.display_name || model.name }}</h2>
                    <div class="mt-1 truncate font-mono text-[11px] text-muted-foreground">{{ model.name }}</div>
                  </div>
                </div>
                <button
                  class="flex h-8 w-8 shrink-0 items-center justify-center border border-border text-muted-foreground transition hover:border-primary hover:text-primary"
                  :title="t('common.copy')"
                  @click="copyToClipboard(model.name)"
                >
                  <Copy class="h-3.5 w-3.5" />
                </button>
              </div>

              <div class="mt-5 grid min-w-0 grid-cols-2 gap-2 border-y border-border/60 py-4 text-sm">
                <template v-if="hasTokenPricing(model)">
                  <div class="min-w-0">
                    <div class="text-xs text-muted-foreground">{{ t('models.input') }}</div>
                    <div class="mt-1 break-all font-mono font-semibold">{{ formatPrice(firstTierPrice(model, 'input')) }}</div>
                    <div v-if="originalTierPrice(model, 'input') !== null" class="mt-1 flex items-baseline gap-1 text-[11px] text-muted-foreground">
                      <span>{{ t('models.original') }}</span>
                      <span class="line-through">{{ formatPrice(originalTierPrice(model, 'input')) }}</span>
                    </div>
                    <span v-if="discountPercent(model, 'input') !== null" class="mt-1 inline-flex rounded-full bg-emerald-500/10 px-1.5 py-0.5 text-[10px] font-semibold text-emerald-600 dark:text-emerald-400">{{ discountLabel(model, 'input') }}</span>
                  </div>
                  <div class="min-w-0">
                    <div class="text-xs text-muted-foreground">{{ t('models.output') }}</div>
                    <div class="mt-1 break-all font-mono font-semibold">{{ formatPrice(firstTierPrice(model, 'output')) }}</div>
                    <div v-if="originalTierPrice(model, 'output') !== null" class="mt-1 flex items-baseline gap-1 text-[11px] text-muted-foreground">
                      <span>{{ t('models.original') }}</span>
                      <span class="line-through">{{ formatPrice(originalTierPrice(model, 'output')) }}</span>
                    </div>
                    <span v-if="discountPercent(model, 'output') !== null" class="mt-1 inline-flex rounded-full bg-emerald-500/10 px-1.5 py-0.5 text-[10px] font-semibold text-emerald-600 dark:text-emerald-400">{{ discountLabel(model, 'output') }}</span>
                  </div>
                  <div class="col-span-2 flex items-center justify-between text-[10px] uppercase tracking-wider text-muted-foreground">
                    <span>{{ t('models.perMillion') }}</span>
                    <span v-if="modelDiscount(model) > 0 && modelDiscount(model) < 1" class="font-semibold text-emerald-600 dark:text-emerald-400">{{ t('models.discountSavings', { percent: Math.round((1 - modelDiscount(model)) * 100) }) }}</span>
                  </div>
                </template>
                <div v-else-if="model.default_price_per_request" class="col-span-2 font-mono font-semibold">
                  {{ t('models.perRequest', { price: formatPrice(model.default_price_per_request * modelDiscount(model)) }) }}
                </div>
                <div v-else class="col-span-2 text-xs text-muted-foreground">{{ t('models.noPricing') }}</div>
              </div>

              <div class="mt-4 flex flex-wrap gap-1.5">
                <span class="border border-foreground/15 bg-foreground/[0.04] px-2 py-1 text-[10px] font-semibold">
                  {{ modelManufacturerLabel(model, t('models.otherManufacturer')) }}
                </span>
                <span
                  v-for="group in modelGroups(model)"
                  :key="group.id"
                  class="border border-primary/25 bg-primary/5 px-2 py-1 text-[10px] font-semibold text-primary"
                >
                  {{ group.name }} <span class="font-mono text-muted-foreground">{{ groupModelPriceLabel(group, model) }}</span>
                </span>
                <span
                  v-for="provider in modelProviderNames(model)"
                  :key="provider"
                  class="max-w-full break-all border border-border bg-muted/35 px-2 py-1 text-[10px] font-medium"
                >
                  {{ provider }}
                </span>
                <span v-if="modelHealth(model)" class="border px-2 py-1 text-[10px] font-semibold" :class="modelHealth(model)?.status === 'healthy' ? 'border-emerald-500/30 bg-emerald-500/10 text-emerald-600' : modelHealth(model)?.status === 'degraded' ? 'border-amber-500/30 bg-amber-500/10 text-amber-600' : 'border-red-500/30 bg-red-500/10 text-red-600'">
                  {{ modelHealth(model)?.status === 'healthy' ? t('models.healthHealthy') : modelHealth(model)?.status === 'degraded' ? t('models.healthDegraded') : t('models.healthUnavailable') }}
                  <span v-if="modelHealth(model)?.score !== null"> {{ Math.round((modelHealth(model)?.score || 0) * 100) }}%</span>
                </span>
                <span
                  v-for="capability in capabilities(model).slice(0, 4)"
                  :key="capability"
                  class="max-w-full break-all border border-border bg-muted/35 px-2 py-1 text-[10px] font-medium"
                >
                  {{ capability }}
                </span>
              </div>

              <div v-if="(model.usage_count || 0) > 0" class="mt-auto flex min-w-0 flex-wrap items-center justify-between gap-2 pt-5 text-xs text-muted-foreground">
                <span v-if="(model.usage_count || 0) > 0">{{ t('models.calls', { count: model.usage_count.toLocaleString(locale) }) }}</span>
              </div>
            </article>
          </div>
        </div>
      </section>
    </main>

  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Boxes, CircleDollarSign, Copy, RefreshCw, Search } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { useClipboard } from '@/composables/useClipboard'
import { getPublicModelGroupCatalog, type PublicGlobalModel, type PublicModelGroup, type PublicModelGroupCatalog } from '@/api/public-models'
import { MODEL_MANUFACTURERS, modelManufacturerId, modelManufacturerLabel } from './model-manufacturers'

const ALL_MANUFACTURERS = '__all__'
const ALL_GROUP = '__all__'
const { t, locale } = useI18n()
const { copyToClipboard } = useClipboard()

const models = ref<PublicGlobalModel[]>([])
const groupCatalog = ref<PublicModelGroupCatalog[]>([])
const loading = ref(true)
const loadError = ref(false)
const searchQuery = ref('')
const selectedManufacturer = ref(ALL_MANUFACTURERS)
const selectedGroup = ref(ALL_GROUP)

const manufacturers = computed(() => {
  const present = new Set(models.value.map(modelManufacturerId))
  return [
    { id: ALL_MANUFACTURERS, label: '' },
    ...MODEL_MANUFACTURERS.filter(manufacturer => present.has(manufacturer.id)),
    ...(present.has('other') ? [{ id: 'other', label: t('models.otherManufacturer') }] : []),
  ]
})

const groups = computed(() => [
  { id: ALL_GROUP, name: '' },
  ...groupCatalog.value.map(group => ({ id: group.id, name: group.name })),
])

const usesDedicatedDiscountPricing = computed(() => groupCatalog.value.some(groupUsesDiscountTerms))

const filteredModels = computed(() => {
  const query = searchQuery.value.trim().toLocaleLowerCase()
  const result = models.value.filter(model => {
    if (selectedManufacturer.value !== ALL_MANUFACTURERS && modelManufacturerId(model) !== selectedManufacturer.value) return false
    if (selectedGroup.value !== ALL_GROUP && !groupAllowsModel(selectedGroup.value, model)) return false
    if (!query) return true
    const haystack = [
      model.name,
      model.display_name,
      modelManufacturerLabel(model, t('models.otherManufacturer')),
      ...modelProviderNames(model),
      ...capabilities(model),
    ]
      .filter(Boolean)
      .join(' ')
      .toLocaleLowerCase()
    return query.split(/\s+/).every(part => haystack.includes(part))
  })
  return [...result].sort((a, b) => (a.display_name || a.name).localeCompare(b.display_name || b.name))
})

function modelProviderNames(model: PublicGlobalModel): string[] {
  return Array.isArray(model.health?.providers)
    ? model.health.providers.map(String).filter(Boolean)
    : []
}

function modelGroups(model: PublicGlobalModel): PublicModelGroupCatalog[] {
  return groupCatalog.value.filter(group => group.models.some(candidate => candidate.id === model.id || candidate.name === model.name))
}

function groupUsesDiscountTerms(group: PublicModelGroup): boolean {
  return Object.prototype.hasOwnProperty.call(group, 'discount')
    || Object.prototype.hasOwnProperty.call(group, 'model_discounts')
}

function groupModelDiscount(group: PublicModelGroup, model: PublicGlobalModel): number {
  const overrides = group.model_discounts || group.model_sales_multipliers || {}
  const override = overrides[model.id] ?? overrides[model.name]
  if (typeof override === 'number' && Number.isFinite(override)) return override
  if (typeof group.discount === 'number' && Number.isFinite(group.discount)) return group.discount
  return typeof group.sales_multiplier === 'number' && Number.isFinite(group.sales_multiplier)
    ? group.sales_multiplier
    : 1
}

function modelDiscount(model: PublicGlobalModel): number {
  const memberships = modelGroups(model)
  if (selectedGroup.value !== ALL_GROUP) {
    const selectedMembership = memberships.find(group => group.id === selectedGroup.value)
    if (selectedMembership) return groupModelDiscount(selectedMembership, model)
  }
  // 未指定分组时展示所有公开方案中的最低可用价格。
  if (memberships.length) return Math.min(...memberships.map(group => groupModelDiscount(group, model)))
  const config = model.config || {}
  const billing = config.billing
  const candidates = [
    config.sales_multiplier,
    config.price_multiplier,
    config.multiplier,
    config.cost_multiplier,
    billing && typeof billing === 'object' ? (billing as Record<string, unknown>).cost_multiplier : null,
  ]
  const value = candidates.find(candidate => typeof candidate === 'number' && Number.isFinite(candidate) && candidate >= 0)
  return typeof value === 'number' ? value : 1
}

function groupDiscount(groupId: string): number {
  const group = groupCatalog.value.find(group => group.id === groupId)
  if (!group) return 1
  if (typeof group.discount === 'number' && Number.isFinite(group.discount)) return group.discount
  return typeof group.sales_multiplier === 'number' && Number.isFinite(group.sales_multiplier)
    ? group.sales_multiplier
    : 1
}

function groupPriceLabel(groupId: string): string {
  const group = groupCatalog.value.find(group => group.id === groupId)
  if (!group) return ''
  const value = formatDiscount(groupDiscount(groupId))
  return groupUsesDiscountTerms(group) ? t('models.discountFactor', { value }) : `×${value}`
}

function groupModelPriceLabel(group: PublicModelGroup, model: PublicGlobalModel): string {
  const value = formatDiscount(groupModelDiscount(group, model))
  return groupUsesDiscountTerms(group) ? t('models.discountFactor', { value }) : `×${value}`
}

function modelHealth(model: PublicGlobalModel) {
  const item = modelGroups(model)
    .map(group => group.models.find(candidate => candidate.id === model.id || candidate.name === model.name))
    .find(Boolean)
  return item?.health || null
}

function groupAllowsModel(groupId: string, model: PublicGlobalModel): boolean {
  const group = groupCatalog.value.find(item => item.id === groupId)
  return group?.models.some(candidate => candidate.id === model.id || candidate.name === model.name) ?? false
}

function formatDiscount(value: number): string {
  return new Intl.NumberFormat(locale.value, { maximumFractionDigits: 3 }).format(value)
}

function modelFamily(name: string): string {
  const normalized = name.toLowerCase()
  if (normalized.startsWith('claude')) return 'claude'
  if (normalized.startsWith('codex')) return 'codex'
  if (normalized.startsWith('gpt-image')) return 'image'
  if (normalized.startsWith('gpt') || normalized.startsWith('o1') || normalized.startsWith('o3')) return 'gpt'
  if (normalized.startsWith('gemini')) return 'gemini'
  if (normalized.startsWith('deepseek')) return 'deepseek'
  if (normalized.startsWith('doubao')) return 'doubao'
  if (normalized.startsWith('glm') || normalized.startsWith('chatglm') || normalized.startsWith('zhipu')) return 'glm'
  if (normalized.startsWith('grok')) return 'grok'
  if (normalized.startsWith('kimi') || normalized.startsWith('moonshot')) return 'kimi'
  if (normalized.startsWith('mimo') || normalized.startsWith('xiaomi')) return 'mimo'
  if (normalized.startsWith('minimax')) return 'minimax'
  if (normalized.startsWith('qwen')) return 'qwen'
  if (normalized.startsWith('wenxin') || normalized.startsWith('ernie') || normalized.startsWith('baidu')) return 'wenxin'
  if (normalized.includes('embedding')) return 'embedding'
  if (normalized.includes('rerank') || normalized.startsWith('bge')) return 'rerank'
  return normalized.split(/[-/:]/)[0] || normalized
}

function modelInitial(name: string) {
  const family = modelFamily(name)
  if (family === 'claude' || family === 'codex') return 'C'
  if (family === 'gpt' || family === 'image') return 'G'
  if (family === 'gemini') return '✦'
  if (family === 'deepseek') return 'D'
  if (family === 'qwen') return 'Q'
  if (family === 'embedding') return 'E'
  if (family === 'rerank') return 'R'
  return name.slice(0, 1).toUpperCase()
}

function modelBadgeClass(name: string) {
  const family = modelFamily(name)
  if (family === 'claude' || family === 'codex') return 'border-[#d97757]/35 bg-[#d97757]/10 text-[#c65f3d]'
  if (family === 'gpt' || family === 'image') return 'border-[#10a37f]/35 bg-[#10a37f]/10 text-[#087f63]'
  if (family === 'gemini') return 'border-[#4285f4]/35 bg-[#4285f4]/10 text-[#3574d3]'
  if (family === 'deepseek') return 'border-[#4b8bea]/35 bg-[#4b8bea]/10 text-[#3675c9]'
  if (family === 'qwen') return 'border-[#6155d9]/35 bg-[#6155d9]/10 text-[#5145bf]'
  return 'border-primary/25 bg-primary/10 text-primary'
}

function modelIcon(name: string): string | null {
  const family = modelFamily(name)
  if (family === 'claude') return '/claude-color.svg'
  if (family === 'gemini') return '/gemini-color.svg'
  if (family === 'gpt' || family === 'image' || family === 'codex') return '/openai.svg'
  if (family === 'deepseek') return '/deepseek.svg'
  if (family === 'doubao') return '/doubao.svg'
  if (family === 'glm') return '/glm.svg'
  if (family === 'grok') return '/grok.svg'
  if (family === 'kimi') return '/kimi.svg'
  if (family === 'mimo') return '/mimo.svg'
  if (family === 'minimax') return '/minimax.svg'
  if (family === 'qwen') return '/qwen.svg'
  if (family === 'wenxin') return '/wenxin.svg'
  return null
}

function capabilities(model: PublicGlobalModel): string[] {
  const value = model.supported_capabilities
  if (Array.isArray(value)) return value.map(String)
  if (value && typeof value === 'object') return Object.entries(value).filter(([, enabled]) => Boolean(enabled)).map(([key]) => key)
  return model.supports_embedding ? ['embedding'] : []
}

function firstTierPrice(model: PublicGlobalModel, type: 'input' | 'output'): number | null {
  const base = baseTierPrice(model, type)
  return base === null ? null : base * modelDiscount(model)
}

function baseTierPrice(model: PublicGlobalModel, type: 'input' | 'output'): number | null {
  const tier = model.default_tiered_pricing?.tiers?.[0]
  if (!tier) return null
  return type === 'input' ? tier.input_price_per_1m ?? null : tier.output_price_per_1m ?? null
}

function originalTierPrice(model: PublicGlobalModel, type: 'input' | 'output'): number | null {
  const current = firstTierPrice(model, type)
  if (current === null) return null
  const config = model.config || {}
  const configured = config[`official_${type}_price_per_1m`]
  const original = typeof configured === 'number' && Number.isFinite(configured)
    ? configured
    : baseTierPrice(model, type)
  return original !== null && original > current ? original : null
}

function discountPercent(model: PublicGlobalModel, type: 'input' | 'output'): number | null {
  const current = firstTierPrice(model, type)
  const original = originalTierPrice(model, type)
  if (current === null || original === null || original <= 0) return null
  return Math.round((1 - current / original) * 100)
}

function discountLabel(model: PublicGlobalModel, type: 'input' | 'output'): string {
  const percent = discountPercent(model, type)
  return percent === null ? '' : t('models.discount', { percent })
}

function groupCount(groupId: string) {
  return groupId === ALL_GROUP
    ? models.value.length
    : models.value.filter(model => groupAllowsModel(groupId, model)).length
}

function manufacturerCount(manufacturerId: string) {
  return manufacturerId === ALL_MANUFACTURERS
    ? models.value.length
    : models.value.filter(model => modelManufacturerId(model) === manufacturerId).length
}

function hasTokenPricing(model: PublicGlobalModel) {
  return firstTierPrice(model, 'input') !== null || firstTierPrice(model, 'output') !== null
}

function formatPrice(value: number | null) {
  if (value === null) return '—'
  return new Intl.NumberFormat(locale.value, { style: 'currency', currency: 'USD', maximumFractionDigits: 6 }).format(value)
}

async function loadModels() {
  loading.value = true
  loadError.value = false
  try {
    const catalogResponse = await getPublicModelGroupCatalog()
    groupCatalog.value = catalogResponse.groups || []
    const modelsById = new Map<string, PublicGlobalModel>()
    for (const group of groupCatalog.value) {
      for (const model of group.models) {
        if (!modelsById.has(model.id)) modelsById.set(model.id, model as PublicGlobalModel)
      }
    }
    models.value = Array.from(modelsById.values())
  } catch {
    loadError.value = true
  } finally {
    loading.value = false
  }
}

onMounted(loadModels)
</script>

<style scoped>
.nav-link {
  border-radius: 0.5rem;
  padding: 0.5rem 0.75rem;
  color: hsl(var(--muted-foreground));
  font-size: 0.875rem;
  font-weight: 500;
  transition: color 150ms ease, background-color 150ms ease;
}
.nav-link:hover { color: hsl(var(--foreground)); background: hsl(var(--muted) / 0.5); }
.nav-link-active { color: hsl(var(--primary)); background: hsl(var(--primary) / 0.1); }
.filter-button {
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background) / 0.6);
  padding: 0.5rem 0.75rem;
  font-size: 0.875rem;
  transition: border-color 150ms ease, color 150ms ease, background-color 150ms ease;
}
.filter-button:hover { border-color: hsl(var(--primary) / 0.4); }
.filter-button-active { border-color: hsl(var(--primary)); color: hsl(var(--primary)); background: hsl(var(--primary) / 0.1); }
</style>
