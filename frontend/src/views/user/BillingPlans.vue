<template>
  <PageContainer>
    <PageHeader
      :title="t('billing.title')"
      :description="t('billing.description')"
    />

    <div class="mt-6 space-y-6">
      <div
        v-if="loading"
        class="py-16"
      >
        <LoadingState :message="t('billing.loading')" />
      </div>

      <template v-else>
        <div
          v-if="walletInDebt"
          class="rounded-lg border border-rose-500/30 bg-rose-500/10 px-4 py-3 text-sm leading-6 text-rose-700 dark:text-rose-300"
          role="status"
        >
          {{ t('billing.walletDebtNotice', { amount: formatDebtAmount(walletDebtUsd) }) }}
        </div>

        <Card
          v-if="latestCheckout"
          class="border-primary/30 p-4"
          aria-live="polite"
        >
          <div class="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
            <div>
              <div class="text-sm font-medium">
                {{ t('billing.latestOrder') }}: <span class="font-mono">{{ latestCheckout.order.order_no }}</span>
              </div>
              <dl
                v-if="latestCheckoutIncludesDebt"
                class="mt-3 grid grid-cols-1 gap-2 text-sm sm:grid-cols-3"
              >
                <div>
                  <dt class="text-xs text-muted-foreground">
                    {{ t('billing.planPrice') }}
                  </dt>
                  <dd class="mt-0.5 tabular-nums">
                    {{ formatUsd(latestPlanAmountUsd) }}
                  </dd>
                </div>
                <div>
                  <dt class="text-xs text-muted-foreground">
                    {{ t('billing.walletDebt') }}
                  </dt>
                  <dd class="mt-0.5 tabular-nums text-rose-600 dark:text-rose-300">
                    {{ formatUsd(latestDebtRepaymentUsd) }}
                  </dd>
                </div>
                <div>
                  <dt class="text-xs font-medium text-foreground">
                    {{ t('billing.totalDue') }}
                  </dt>
                  <dd class="mt-0.5 text-base font-semibold tabular-nums">
                    {{ formatUsd(latestCheckout.order.amount_usd) }}
                  </dd>
                </div>
              </dl>
              <div
                v-if="latestCheckoutIncludesDebt"
                class="mt-2 text-xs leading-5 text-muted-foreground"
              >
                {{ t('billing.debtPaymentNotice') }}
              </div>
              <div class="mt-1 text-xs text-muted-foreground">
                {{ t('billing.amountDue') }} {{ latestCheckout.order.pay_amount ?? '-' }} {{ latestCheckout.order.pay_currency || '' }}
              </div>
            </div>
            <div class="flex flex-wrap gap-2">
              <Button
                v-if="latestPaymentUrl"
                variant="outline"
                class="active:bg-primary/20"
                @click="openPaymentUrl(latestPaymentUrl)"
              >
                {{ latestCheckoutIncludesDebt ? t('billing.proceedToPayment') : t('billing.openPayment') }}
              </Button>
              <Button
                v-if="latestCancelUrl"
                variant="ghost"
                class="active:bg-accent/80"
                @click="cancelLatestCheckout"
              >
                {{ t('billing.cancelPayment') }}
              </Button>
            </div>
          </div>
        </Card>

        <CardSection
          :title="t('billing.current')"
          :description="t('billing.currentHint')"
        >
          <div
            v-if="activeEntitlements.length"
            class="grid grid-cols-1 gap-3 lg:grid-cols-2"
          >
            <div
              v-for="item in activeEntitlements"
              :key="item.id"
              class="rounded-lg border border-border/60 bg-muted/20 p-4"
            >
              <div class="flex items-start justify-between gap-3">
                <div>
                  <div class="font-medium">
                    {{ planTitle(item) }}
                  </div>
                  <div class="mt-1 text-xs text-muted-foreground">
                    {{ formatDate(item.starts_at) }} - {{ formatDate(item.expires_at) }}
                  </div>
                  <div class="mt-1 text-xs text-muted-foreground">
                    {{ t('billing.grantedAt') }}: {{ formatDate(item.created_at) }}
                  </div>
                </div>
                <Badge variant="success">
                  {{ t('billing.active') }}
                </Badge>
              </div>
              <div class="mt-3 flex flex-wrap gap-1.5">
                <Badge
                  v-for="label in entitlementLabels(item.entitlements, item.allowed_provider_ids)"
                  :key="label"
                  variant="outline"
                >
                  {{ label }}
                </Badge>
              </div>
              <div
                v-if="quotaSummaryStatus === 'unavailable'"
                class="mt-3 border-t border-border/60 pt-3 text-xs font-medium text-amber-700 dark:text-amber-300"
              >
                {{ t('planQuota.quotaUnavailable') }}
              </div>
              <dl
                v-else-if="quotaSummaryFor(item)?.daily_total_usd != null"
                class="mt-3 grid grid-cols-1 gap-2 border-t border-border/60 pt-3 text-xs sm:grid-cols-2 lg:grid-cols-4"
              >
                <div>
                  <dt class="text-muted-foreground">
                    {{ t('planQuota.todayRemaining') }}
                  </dt>
                  <dd
                    class="mt-1 font-semibold tabular-nums"
                    :class="dailyRemainingFor(item) <= 0 ? 'text-rose-600 dark:text-rose-300' : 'text-foreground'"
                  >
                    {{ formatUsd(dailyRemainingFor(item)) }} / {{ formatUsd(quotaSummaryFor(item)?.daily_total_usd ?? 0) }}
                  </dd>
                </div>
                <div>
                  <dt class="text-muted-foreground">
                    {{ t('planQuota.todayUsed') }}
                  </dt>
                  <dd class="mt-1 font-medium tabular-nums text-foreground">
                    {{ formatUsd(quotaSummaryFor(item)?.daily_used_usd ?? 0) }}
                  </dd>
                </div>
                <div v-if="hasTighterOverallQuota(item)">
                  <dt class="text-muted-foreground">
                    {{ t('planQuota.currentRemaining') }}
                  </dt>
                  <dd
                    class="mt-1 font-semibold tabular-nums"
                    :class="currentRemainingFor(item) <= 0 ? 'text-rose-600 dark:text-rose-300' : 'text-foreground'"
                  >
                    {{ formatUsd(currentRemainingFor(item)) }}
                  </dd>
                </div>
                <div>
                  <dt class="text-muted-foreground">
                    {{ t('planQuota.nextRefresh') }}
                  </dt>
                  <dd class="mt-1 font-medium tabular-nums text-foreground">
                    {{ formatDateTime(quotaSummaryFor(item)?.daily_window_ends_at) }}
                  </dd>
                </div>
              </dl>
            </div>
          </div>
          <EmptyState
            v-else
            :title="t('billing.empty')"
            :description="t('billing.emptyHint')"
          />
        </CardSection>

        <CardSection
          v-if="!latestCheckout"
          :title="t('billing.available')"
          :description="t('billing.availableHint')"
        >
          <div class="grid grid-cols-1 gap-4 xl:grid-cols-3">
            <Card
              v-for="plan in purchaseablePlans"
              :key="plan.id"
              class="flex flex-col p-5"
            >
              <div class="flex items-start justify-between gap-3">
                <div>
                  <h3 class="text-base font-semibold">
                    {{ hasMatchingActivePlan(plan)
                      ? t('billing.renewPlanTitle', { title: plan.title })
                      : plan.title }}
                  </h3>
                  <p class="mt-1 min-h-[32px] text-xs text-muted-foreground">
                    {{ plan.description || t('billing.standard') }}
                  </p>
                </div>
                <Badge variant="outline">
                  {{ formatDuration(plan.duration_unit, plan.duration_value) }}
                </Badge>
              </div>

              <div class="mt-5">
                <span class="text-3xl font-semibold tabular-nums">
                  {{ Number(plan.price_amount || 0).toFixed(2) }}
                </span>
                <span class="ml-1 text-sm text-muted-foreground">
                  {{ plan.price_currency }}
                </span>
              </div>

              <div class="mt-5 flex flex-wrap gap-1.5">
                <Badge
                  v-for="label in entitlementLabels(plan.entitlements, plan.allowed_provider_ids)"
                  :key="label"
                  variant="outline"
                >
                  {{ label }}
                </Badge>
              </div>

              <div
                v-if="replacementNotice(plan)"
                class="mt-4 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs leading-5 text-amber-200"
              >
                {{ replacementNotice(plan) }}
              </div>

              <div class="mt-5 flex-1" />

              <div class="mt-5 space-y-3">
                <Select
                  v-model="selectedPaymentOptionKey"
                  :disabled="planBlockedByActivePlan(plan)"
                >
                  <SelectTrigger>
                    <SelectValue
                      :placeholder="paymentOptions.length ? t('billing.choosePayment') : t('billing.noPayment')"
                    />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem
                      v-for="option in paymentOptions"
                      :key="option.key"
                      :value="option.key"
                    >
                      {{ option.display_name }}
                    </SelectItem>
                  </SelectContent>
                </Select>
                <Button
                  class="w-full active:bg-primary/80"
                  :disabled="
                    checkoutLoadingPlanId === plan.id
                      || paymentOptions.length === 0
                      || !selectedPaymentOption
                      || planBlockedByActivePlan(plan)
                  "
                  :title="planPurchaseDisabledReason(plan)"
                  :aria-busy="checkoutLoadingPlanId === plan.id"
                  @click="checkoutPlan(plan)"
                >
                  <CreditCard class="mr-2 h-4 w-4" />
                  {{ checkoutLoadingPlanId === plan.id
                    ? t('billing.creating')
                    : planBlockedByActivePlan(plan)
                      ? t('billing.otherPlanBuyDisabled')
                      : t('billing.buy') }}
                </Button>
              </div>
            </Card>

            <div
              v-if="purchaseablePlans.length === 0"
              class="xl:col-span-3"
            >
              <EmptyState
                :title="t('billing.noPlans')"
                :description="t('billing.noPlansHint')"
              />
            </div>
          </div>
        </CardSection>
      </template>
    </div>
  </PageContainer>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { CreditCard } from 'lucide-vue-next'
import {
  billingApi,
  type BillingDurationUnit,
  type BillingCheckoutResponse,
  type DailyQuotaEntitlement,
  type BillingPlan,
  type UserPlanEntitlement,
  type UserPlanQuotaSummary,
} from '@/api/billing'
import { walletApi, type WalletBalanceResponse, type WalletRechargeOption } from '@/api/wallet'
import {
  Badge,
  Button,
  Card,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui'
import { EmptyState, LoadingState } from '@/components/common'
import { CardSection, PageContainer, PageHeader } from '@/components/layout'
import { useToast } from '@/composables/useToast'
import { parseApiError } from '@/utils/errorParser'
import { log } from '@/utils/logger'
import {
  hasPackageBillingEntitlement,
  normalizeBillingEntitlements,
  quotaConsumptionMultiplierLabel,
  type BillingEntitlementsInput,
} from '@/utils/billingEntitlements'

const { t, locale } = useI18n()
const { success, error: showError } = useToast()

const loading = ref(true)
const plans = ref<BillingPlan[]>([])
const entitlements = ref<UserPlanEntitlement[]>([])
const quotaSummary = ref<UserPlanQuotaSummary | null>(null)
const quotaSummaryStatus = ref<'ok' | 'unavailable'>('ok')
const rechargeOptions = ref<WalletRechargeOption[]>([])
const walletBalance = ref<WalletBalanceResponse | null>(null)
const selectedPaymentOptionKey = ref('')
const checkoutLoadingPlanId = ref<string | null>(null)
const latestCheckout = ref<BillingCheckoutResponse | null>(null)
const BILLING_SUMMARY_REFRESH_EVENT = 'aether:billing-summary-refresh'

const actualWalletBalance = computed(() => {
  const value = walletBalance.value?.actual_wallet_balance
    ?? walletBalance.value?.wallet_balance
    ?? walletBalance.value?.balance
    ?? 0
  return typeof value === 'number' && Number.isFinite(value) ? value : 0
})
const walletInDebt = computed(() =>
  walletBalance.value?.billing_state === 'in_debt' || actualWalletBalance.value < 0
)
const walletDebtUsd = computed(() => {
  const explicitDebt = walletBalance.value?.debt_usd
  if (typeof explicitDebt === 'number' && Number.isFinite(explicitDebt)) {
    return Math.max(0, explicitDebt)
  }
  return Math.max(0, -actualWalletBalance.value)
})

const paymentOptions = computed(() =>
  rechargeOptions.value
    .filter((option) => option.payment_provider === 'epay' || option.payment_method === 'epay' || option.payment_provider === 'dodopay' || option.payment_method === 'dodopay')
    .map((option, index) => ({
      ...option,
      key: [
        option.payment_provider || option.provider || option.payment_method,
        option.payment_method,
        option.payment_channel || '',
        index,
      ].join(':'),
    }))
)

const selectedPaymentOption = computed(() => {
  if (paymentOptions.value.length === 0) return null
  return paymentOptions.value.find(option => option.key === selectedPaymentOptionKey.value)
    || paymentOptions.value[0]
})

const activeEntitlements = computed(() =>
  entitlements.value.filter((item) =>
    item.active !== false
    && item.status === 'active'
    && hasPackageEntitlement(item.entitlements)
  )
)

const activePlanId = computed(() => activeEntitlements.value[0]?.plan_id || '')

const purchaseablePlans = computed(() =>
  plans.value.filter((plan) => hasPackageEntitlement(plan.entitlements))
)

const latestPaymentUrl = computed(() => {
  const value = latestCheckout.value?.payment_instructions?.payment_url
  return typeof value === 'string' && value ? value : ''
})

const latestCancelUrl = computed(() => {
  const value = latestCheckout.value?.payment_instructions?.local_cancel_url
  return typeof value === 'string' && value ? value : ''
})

const latestDebtRepaymentUsd = computed(() => {
  const value = latestCheckout.value?.order.debt_repayment_usd
  return typeof value === 'number' && Number.isFinite(value) ? Math.max(0, value) : 0
})

const latestCheckoutIncludesDebt = computed(() => latestDebtRepaymentUsd.value > 0)

const latestPlanAmountUsd = computed(() => {
  const explicit = latestCheckout.value?.order.plan_amount_usd
  if (typeof explicit === 'number' && Number.isFinite(explicit)) {
    return Math.max(0, explicit)
  }
  const total = latestCheckout.value?.order.amount_usd
  return typeof total === 'number' && Number.isFinite(total)
    ? Math.max(0, total - latestDebtRepaymentUsd.value)
    : 0
})

watch(paymentOptions, (options) => {
  const keys = options.map(option => option.key)
  if (!keys.includes(selectedPaymentOptionKey.value)) {
    selectedPaymentOptionKey.value = keys[0] || ''
  }
}, { immediate: true })

onMounted(async () => {
  await Promise.all([
    loadPlans(),
    loadEntitlements(),
    loadWalletBalance(),
    loadRechargeOptions(),
  ])
  loading.value = false
})

async function loadPlans() {
  try {
    const response = await billingApi.listPlans()
    plans.value = response.items
  } catch (err) {
    log.error('加载套餐失败:', err)
    showError(parseApiError(err, t('billing.loadPlansFailed')))
  }
}

async function loadEntitlements() {
  try {
    const response = await billingApi.listEntitlements()
    entitlements.value = response.items
    quotaSummary.value = response.quota_summary ?? null
    quotaSummaryStatus.value = response.quota_summary_status ?? 'ok'
    window.dispatchEvent(new CustomEvent(BILLING_SUMMARY_REFRESH_EVENT))
  } catch (err) {
    log.error('加载套餐权益失败:', err)
    showError(parseApiError(err, t('billing.loadEntitlementsFailed')))
    quotaSummary.value = null
    quotaSummaryStatus.value = 'unavailable'
  }
}

async function loadRechargeOptions() {
  try {
    const response = await walletApi.listRechargeOptions()
    rechargeOptions.value = response.items
    if (!selectedPaymentOptionKey.value && paymentOptions.value.length > 0) {
      selectedPaymentOptionKey.value = paymentOptions.value[0].key
    }
  } catch (err) {
    log.error('加载支付通道失败:', err)
    showError(parseApiError(err, t('billing.loadPaymentMethodsFailed')))
  }
}

async function loadWalletBalance() {
  try {
    walletBalance.value = await walletApi.getBalance()
  } catch (err) {
    log.error('加载钱包余额失败:', err)
    showError(parseApiError(err, t('billing.loadWalletFailed')))
  }
}

async function checkoutPlan(plan: BillingPlan) {
  if (planBlockedByActivePlan(plan)) {
    showError(t('billing.otherPlanBuyDisabled'))
    return
  }
  if (hasMatchingActivePlan(plan)) {
    const confirmed = window.confirm(t('billing.renewConfirm'))
    if (!confirmed) return
  }
  checkoutLoadingPlanId.value = plan.id
  try {
    const option = selectedPaymentOption.value
    if (!option) {
      showError(t('billing.paymentRequired'))
      return
    }
    const response = await billingApi.checkout(plan.id, {
      payment_method: option.payment_method,
      payment_provider: option.payment_provider || option.provider || option.payment_method,
      payment_channel: option.payment_channel,
    })
    latestCheckout.value = response
    success(t('billing.orderCreated'))
    const debtRepaymentUsd = response.order.debt_repayment_usd
    if (!(typeof debtRepaymentUsd === 'number' && debtRepaymentUsd > 0)) {
      submitPaymentInstructions(response.payment_instructions)
    }
  } catch (err) {
    log.error('创建套餐订单失败:', err)
    showError(parseApiError(err, t('billing.createOrderFailed')))
  } finally {
    checkoutLoadingPlanId.value = null
  }
}

function planBlockedByActivePlan(plan: BillingPlan): boolean {
  return Boolean(activePlanId.value && activePlanId.value !== plan.id)
}

function planPurchaseDisabledReason(plan: BillingPlan): string | undefined {
  if (planBlockedByActivePlan(plan)) return t('billing.otherPlanBuyDisabled')
  return undefined
}

function formatDebtAmount(value: number): string {
  return formatUsd(value)
}

function formatUsd(value: number): string {
  return `$${value.toFixed(2)}`
}

function openPaymentUrl(url: string) {
  submitPaymentInstructions(latestCheckout.value?.payment_instructions || { payment_url: url })
}

function submitPaymentInstructions(instructions: Record<string, unknown> | null | undefined) {
  if (!instructions) return
  const paymentUrl = instructions.payment_url
  if (typeof paymentUrl !== 'string' || !paymentUrl) return
  const paymentParams = instructions.payment_params
  if (paymentParams && typeof paymentParams === 'object' && !Array.isArray(paymentParams)) {
    submitPaymentForm(paymentUrl, paymentParams as Record<string, unknown>)
    return
  }
  const opened = window.open(paymentUrl, '_blank', 'noopener,noreferrer')
  if (!opened) {
    showError(t('billing.popupBlocked'))
  }
}

function cancelLatestCheckout() {
  if (!latestCancelUrl.value) return
  const confirmed = window.confirm(t('billing.cancelConfirm'))
  if (!confirmed) return
  window.location.href = latestCancelUrl.value
}

function submitPaymentForm(url: string, params: Record<string, unknown>) {
  const form = document.createElement('form')
  form.action = url
  form.method = 'POST'
  form.target = '_blank'
  Object.entries(params).forEach(([key, value]) => {
    if (value === null || value === undefined) return
    const input = document.createElement('input')
    input.type = 'hidden'
    input.name = key
    input.value = String(value)
    form.appendChild(input)
  })
  document.body.appendChild(form)
  form.submit()
  document.body.removeChild(form)
}

function planTitle(item: UserPlanEntitlement): string {
  return quotaSummaryFor(item)?.plan_title
    || plans.value.find((plan) => plan.id === item.plan_id)?.title
    || item.plan_id
}

function quotaSummaryFor(item: UserPlanEntitlement): UserPlanQuotaSummary | null {
  return quotaSummary.value?.entitlement_id === item.id ? quotaSummary.value : null
}

function dailyRemainingFor(item: UserPlanEntitlement): number {
  const summary = quotaSummaryFor(item)
  if (!summary) return 0
  const value = Number(summary.daily_remaining_usd ?? 0)
  return Number.isFinite(value) ? Math.max(0, value) : 0
}

function currentRemainingFor(item: UserPlanEntitlement): number {
  const value = Number(quotaSummaryFor(item)?.quota_remaining_usd ?? 0)
  return Number.isFinite(value) ? Math.max(0, value) : 0
}

function hasTighterOverallQuota(item: UserPlanEntitlement): boolean {
  return currentRemainingFor(item) + Number.EPSILON < dailyRemainingFor(item)
}

function hasMatchingActivePlan(plan: BillingPlan): boolean {
  return activeEntitlements.value.some((item) => item.plan_id === plan.id)
}

function replacementNotice(plan: BillingPlan): string {
  if (hasMatchingActivePlan(plan)) {
    return t('billing.renewNotice')
  }
  return ''
}

function entitlementLabels(items: BillingEntitlementsInput, providerIds: string[] = []): string[] {
  return normalizeBillingEntitlements(items).map((item) => {
    if (item.type === 'wallet_credit') {
      return t('billing.walletCredit', { amount: Number(item.amount_usd || 0).toFixed(2) })
    }
    if (item.type === 'daily_quota') {
      return quotaEntitlementLabel(item, providerIds)
    }
    if (item.type === 'membership_group') {
      return t('billing.membershipGroups', { groups: item.grant_user_groups.join(', ') })
    }
    return t('billing.unknownEntitlement')
  })
}

function hasPackageEntitlement(items: BillingEntitlementsInput): boolean {
  return hasPackageBillingEntitlement(items)
}

function quotaEntitlementLabel(item: DailyQuotaEntitlement, providerIds: string[]): string {
  const limits = item.limits || {}
  const parts = []
  const daily = Number(item.daily_quota_usd ?? limits.daily_limit_usd ?? 0)
  const fiveHour = Number(item.five_hour_quota_usd ?? limits.five_hour_limit_usd ?? 0)
  const weekly = Number(item.weekly_quota_usd ?? limits.weekly_limit_usd ?? 0)
  const monthly = Number(item.monthly_quota_usd ?? limits.monthly_limit_usd ?? 0)
  if (daily > 0) parts.push(t('billing.quota24Hours', { amount: daily.toFixed(2) }))
  if (fiveHour > 0) parts.push(`5H $${fiveHour.toFixed(2)}`)
  if (weekly > 0) parts.push(t('billing.quota7Days', { amount: weekly.toFixed(2) }))
  if (monthly > 0) parts.push(t('billing.quota30Days', { amount: monthly.toFixed(2) }))
  const quotaText = parts.join(' / ') || t('billing.usageQuota')
  const labels = [quotaModelScopeLabel(providerIds)]
  const multiplierLabel = quotaConsumptionMultiplierLabel(item, t)
  if (multiplierLabel) labels.push(multiplierLabel)
  return `${quotaText} · ${labels.join(' · ')}`
}

function quotaModelScopeLabel(providerIds: string[]): string {
  if (providerIds.length > 0) {
    return t('billing.modelsByProviders')
  }
  return t('billing.noPlanProviders')
}

function formatDuration(unit: BillingDurationUnit, value: number): string {
  const labels: Record<BillingDurationUnit, string> = {
    day: t('billing.durationDay'),
    month: t('billing.durationMonth'),
    year: t('billing.durationYear'),
    custom: t('billing.durationCustom'),
  }
  return unit === 'custom' ? `${value} ${labels[unit]}` : `${value}${labels[unit]}`
}

function formatDate(value: string | null | undefined): string {
  if (!value) return '-'
  return new Date(value).toLocaleDateString(locale.value)
}

function formatDateTime(value: string | null | undefined): string {
  if (!value) return '-'
  return new Date(value).toLocaleString(locale.value, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}
</script>
