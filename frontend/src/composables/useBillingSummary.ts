import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { billingApi, type UserPlanEntitlement } from '@/api/billing'
import { walletApi, type WalletBalanceResponse } from '@/api/wallet'
import { useAuthStore } from '@/stores/auth'
import { hasPackageBillingEntitlement } from '@/utils/billingEntitlements'
import { formatWalletCurrency as formatCurrency } from '@/utils/walletDisplay'
import { log } from '@/utils/logger'

export const BILLING_SUMMARY_REFRESH_EVENT = 'aether:billing-summary-refresh'

export function useBillingSummary() {
  const authStore = useAuthStore()
  const { t, locale } = useI18n()
  const walletBalance = ref<WalletBalanceResponse | null>(null)
  const planEntitlements = ref<UserPlanEntitlement[]>([])
  const loading = ref(false)
  const walletError = ref(false)
  const planError = ref(false)
  let requestId = 0

  const walletAmount = computed(() => finiteAmount(
    walletBalance.value?.wallet_balance ?? walletBalance.value?.balance,
  ))
  const packageAmount = computed(() => finiteAmount(walletBalance.value?.package_balance))
  const totalAmount = computed(() => {
    const total = walletBalance.value?.total_available_balance
    return typeof total === 'number' && Number.isFinite(total)
      ? total
      : walletAmount.value + packageAmount.value
  })
  const hasError = computed(() => walletError.value || planError.value)
  const statusLabel = computed(() => {
    if (loading.value) return t('console.loading')
    if (walletError.value && planError.value) return t('console.loadFailed')
    if (walletError.value) return t('console.balanceLoadFailed')
    if (planError.value) return t('console.planLoadFailed')
    return ''
  })
  const totalLabel = computed(() => {
    if (!walletBalance.value) return loading.value ? t('console.loading') : '-'
    if (walletBalance.value.unlimited) return t('console.unlimited')
    return formatCurrency(totalAmount.value)
  })
  const walletLabel = computed(() => walletBalance.value ? formatCurrency(walletAmount.value) : '-')
  const packageLabel = computed(() => walletBalance.value ? formatCurrency(packageAmount.value) : '-')
  const nearestPlanExpiryLabel = computed(() => {
    if (loading.value && planEntitlements.value.length === 0) return t('console.loading')
    if (planError.value && planEntitlements.value.length === 0) return '-'

    const activePlans = planEntitlements.value.filter(item =>
      item.active !== false
      && item.status === 'active'
      && hasPackageBillingEntitlement(item.entitlements),
    )
    if (activePlans.length === 0) return t('console.notEnabled')

    const now = Date.now()
    const futureExpiries = activePlans
      .map(item => item.expires_at)
      .filter((value): value is string => Boolean(value))
      .map(value => new Date(value))
      .filter(value => Number.isFinite(value.getTime()) && value.getTime() > now)
      .sort((a, b) => a.getTime() - b.getTime())

    if (futureExpiries[0]) {
      return futureExpiries[0].toLocaleString(locale.value, {
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
      })
    }
    if (activePlans.some(item => !item.expires_at)) return t('console.longTerm')
    return t('console.notEnabled')
  })

  async function refresh() {
    if (!authStore.user || !authStore.token) {
      reset()
      return
    }

    const currentRequestId = ++requestId
    loading.value = true
    const [balanceResult, entitlementResult] = await Promise.allSettled([
      walletApi.getBalance(),
      billingApi.listEntitlements(),
    ])
    if (currentRequestId !== requestId) return

    if (balanceResult.status === 'fulfilled') {
      walletBalance.value = balanceResult.value
      walletError.value = false
    } else {
      walletError.value = true
      log.error('加载账户余额汇总失败:', balanceResult.reason)
    }

    if (entitlementResult.status === 'fulfilled') {
      planEntitlements.value = entitlementResult.value.items
      planError.value = false
    } else {
      planError.value = true
      log.error('加载套餐权益汇总失败:', entitlementResult.reason)
    }

    loading.value = false
  }

  function reset() {
    requestId += 1
    walletBalance.value = null
    planEntitlements.value = []
    loading.value = false
    walletError.value = false
    planError.value = false
  }

  function handleVisibilityChange() {
    if (!document.hidden) void refresh()
  }

  watch(
    () => [authStore.user, authStore.token] as const,
    () => { void refresh() },
    { immediate: true },
  )

  onMounted(() => {
    window.addEventListener(BILLING_SUMMARY_REFRESH_EVENT, refresh)
    document.addEventListener('visibilitychange', handleVisibilityChange)
  })
  onUnmounted(() => {
    window.removeEventListener(BILLING_SUMMARY_REFRESH_EVENT, refresh)
    document.removeEventListener('visibilitychange', handleVisibilityChange)
    requestId += 1
  })

  return {
    walletBalance,
    planEntitlements,
    loading,
    walletError,
    planError,
    hasError,
    statusLabel,
    walletAmount,
    packageAmount,
    totalAmount,
    totalLabel,
    walletLabel,
    packageLabel,
    nearestPlanExpiryLabel,
    refresh,
  }
}

function finiteAmount(value: number | null | undefined): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0
}
