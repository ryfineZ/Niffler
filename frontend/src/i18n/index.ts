import { createI18n } from 'vue-i18n'
import zhCN from './locales/zh-CN'
import enUS from './locales/en-US'

export const SUPPORTED_LOCALES = ['zh-CN', 'en-US'] as const
export type AppLocale = (typeof SUPPORTED_LOCALES)[number]

const messages = {
  'zh-CN': {
    ...zhCN,
    userManagement: {
      ...zhCN.userManagement,
      walletBalance: '钱包余额：',
      inDebt: '欠费',
    },
    userPlans: {
      ...zhCN.userPlans,
      editNotice: '可以修改这份套餐的时间、额度上限和提供商范围；切换套餐类型请先取消再重新发放。',
      allowedProviders: '套餐可用提供商',
      loadingProviders: '正在加载提供商...',
      chooseProviders: '选择这个套餐可以使用的提供商',
      noProviders: '暂无可用提供商',
      allowedProvidersHint: '旧套餐可在这里确认提供商；可用模型会自动跟随这些提供商当前启用的模型。',
      providersRequired: '请至少选择一个套餐提供商',
    },
    billing: {
      ...zhCN.billing,
      walletDebtNotice: '钱包当前欠费 {amount}。购买或续费套餐时，本次付款会同时包含套餐价格和下单时的欠款。',
      planPrice: '套餐价格',
      walletDebt: '钱包欠款',
      totalDue: '本次实付',
      debtPaymentNotice: '支付成功后，将结清本订单所列欠款并开通套餐。',
      proceedToPayment: '前往付款',
      otherPlanBuyDisabled: '已有生效套餐，只能续费原套餐',
      renewPlanTitle: '续费 {title}',
      loadWalletFailed: '加载钱包余额失败',
      modelsByProviders: '模型随套餐供应商自动更新',
    },
    wallet: {
      ...zhCN.wallet,
      inDebtPlanStillUsable: '钱包欠费；已有套餐仍可正常使用',
      inDebtNoUsablePlan: '钱包欠费；当前没有可用套餐额度，新请求将被拒绝',
      overage: '套餐不足部分会继续扣钱包余额',
    },
    billingPlansManagement: {
      ...zhCN.billingPlansManagement,
      allowedProviders: '套餐可用提供商',
      chooseProviders: '选择这个套餐可以使用的提供商',
      noProviders: '暂无可用提供商',
      allowedProvidersHint: '提供商新增或移除模型后，套餐可用模型会自动更新。',
      derivedModels: '当前可用模型',
      providerDisabled: '已停用',
      chooseProvidersFirst: '选择提供商后显示',
      noDerivedModels: '所选提供商当前没有可用模型',
      derivedModelCount: '{names} 等 {count} 个模型',
      providersRequired: '请选择套餐可用提供商',
      providerListSummary: '{names} 等 {count} 个提供商',
      renewalLogic: '同一时间只能有一个生效套餐；同套餐续费会从当前到期时间后顺延。',
      periodQuotaNote2: '套餐额度耗尽后，超出部分继续扣钱包余额',
      walletOverageDetail: '套餐额度不足时，超出部分会继续扣钱包余额；本次结算后若钱包欠费，后续钱包付费请求会停止。',
    },
    poolManagement: {
      ...zhCN.poolManagement,
      refresh: '刷新',
      refreshing: '刷新中...',
      refreshData: '刷新数据',
      refreshDataQuota: '刷新数据和额度',
      refreshComplete: '刷新完成：成功 {success}，失败 {failed}，跳过 {skipped}',
      refreshFailedDetail: '刷新失败：{error}，跳过 {skipped}',
      refreshPageFailed: '刷新当前页失败',
      refreshDataCooldown: '{wait} 后可再次刷新',
      refreshEligible: '可刷新 {eligible} / {total}',
      refreshProviderFailed: '刷新提供商失败',
      refreshEndpointsFailed: '刷新端点失败',
      unnamedAccount: '未命名账号',
      selectAccount: '选择账号 {name}',
    },
  },
  'en-US': {
    ...enUS,
    userManagement: {
      ...enUS.userManagement,
      walletBalance: 'Wallet balance:',
      inDebt: 'In debt',
    },
    userPlans: {
      ...enUS.userPlans,
      editNotice: 'Update the dates, quota limit, and provider scope of this plan record. Cancel and grant again to switch plan types.',
      allowedProviders: 'Plan providers',
      loadingProviders: 'Loading providers...',
      chooseProviders: 'Choose providers available to this plan',
      noProviders: 'No providers available',
      allowedProvidersHint: 'Confirm providers for legacy plans here. Available models automatically follow the active models of these providers.',
      providersRequired: 'Choose at least one plan provider',
    },
    billing: {
      ...enUS.billing,
      walletDebtNotice: 'Your wallet owes {amount}. Buying or renewing a plan charges the plan price plus the debt recorded when the order is created.',
      planPrice: 'Plan price',
      walletDebt: 'Wallet debt',
      totalDue: 'Total due',
      debtPaymentNotice: 'After payment, the debt listed on this order is cleared and the plan is activated.',
      proceedToPayment: 'Continue to payment',
      otherPlanBuyDisabled: 'An active plan already exists; only that plan can be renewed',
      renewPlanTitle: 'Renew {title}',
      loadWalletFailed: 'Failed to load wallet balance',
      modelsByProviders: 'Models update with plan providers',
    },
    wallet: {
      ...enUS.wallet,
      inDebtPlanStillUsable: 'Wallet in debt; existing plans remain usable',
      inDebtNoUsablePlan: 'Wallet in debt; no plan quota is available, so new requests are blocked',
      overage: 'Plan overage continues charging the wallet',
    },
    billingPlansManagement: {
      ...enUS.billingPlansManagement,
      allowedProviders: 'Plan providers',
      chooseProviders: 'Choose providers available to this plan',
      noProviders: 'No providers available',
      allowedProvidersHint: 'Available models update automatically when these providers add or remove models.',
      derivedModels: 'Available models now',
      providerDisabled: 'Disabled',
      chooseProvidersFirst: 'Choose providers to preview',
      noDerivedModels: 'The selected providers currently have no available models',
      derivedModelCount: '{names}, {count} models total',
      providersRequired: 'Choose providers available to the plan',
      providerListSummary: '{names} and {count} providers total',
      renewalLogic: 'Only one plan can be active at a time; renewing the same plan extends it after the current expiry.',
      periodQuotaNote2: 'Usage beyond the plan quota continues charging the wallet',
      walletOverageDetail: 'Usage beyond the plan quota continues charging the wallet. If settlement puts the wallet in debt, later wallet-paid requests stop.',
    },
    poolManagement: {
      ...enUS.poolManagement,
      refresh: 'Refresh',
      refreshing: 'Refreshing...',
      refreshData: 'Refresh data',
      refreshDataQuota: 'Refresh data and quota',
      refreshComplete: 'Refresh complete: {success} succeeded, {failed} failed, {skipped} skipped',
      refreshFailedDetail: 'Refresh failed: {error}; {skipped} skipped',
      refreshPageFailed: 'Failed to refresh current page',
      refreshDataCooldown: 'Refresh available again in {wait}',
      refreshEligible: 'Eligible to refresh: {eligible} / {total}',
      refreshProviderFailed: 'Failed to refresh provider',
      refreshEndpointsFailed: 'Failed to refresh endpoints',
      unnamedAccount: 'Unnamed account',
      selectAccount: 'Select account {name}',
    },
  },
}

function resolveInitialLocale(): AppLocale {
  if (import.meta.env.MODE === 'test') return 'zh-CN'
  const saved = localStorage.getItem('niffler-locale')
  if (saved === 'zh-CN' || saved === 'en-US') return saved
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en-US'
}

export const i18n = createI18n({
  legacy: false,
  locale: resolveInitialLocale(),
  fallbackLocale: 'zh-CN',
  messages,
})

export function setAppLocale(locale: AppLocale) {
  i18n.global.locale.value = locale
  localStorage.setItem('niffler-locale', locale)
  document.documentElement.lang = locale
}
