import { i18n } from '@/i18n'

type Translate = (key: string, params?: Record<string, unknown>) => string

function translated(translate: Translate | undefined, key: string, fallback: string): string {
  return (translate ?? ((key, params) => i18n.global.t(key, params)))(key, {}) || fallback
}

export function walletStatusLabel(status: string | null | undefined, translate?: Translate): string {
  const labels: Record<string, string> = {
    active: translated(translate, 'walletDisplay.statusActive', '正常'),
    suspended: translated(translate, 'walletDisplay.statusSuspended', '已冻结'),
    closed: translated(translate, 'walletDisplay.statusClosed', '已关闭'),
  }
  if (!status) return translated(translate, 'common.unknown', '未知')
  return labels[status] || status
}

export function formatWalletCurrency(
  value: number | null | undefined,
  options?: { decimals?: number }
): string {
  return `$${formatWalletAmount(value, options)}`
}

export function formatWalletAmount(
  value: number | null | undefined,
  options?: { decimals?: number }
): string {
  const amount = Number(value ?? 0)
  if (!Number.isFinite(amount)) return '0.00'
  if (options?.decimals !== undefined) {
    return amount.toFixed(options.decimals)
  }
  return amount
    .toFixed(8)
    .replace(/(\.\d{2,}?)0+$/, '$1')
}

export function walletStatusBadge(status: string | null | undefined): string {
  if (status === 'active') return 'success'
  if (status === 'suspended') return 'warning'
  if (status === 'closed') return 'destructive'
  return 'secondary'
}

export function walletTransactionCategoryLabel(category: string | null | undefined, translate?: Translate): string {
  const labels: Record<string, string> = {
    recharge: translated(translate, 'walletDisplay.categoryRecharge', '充值'),
    gift: translated(translate, 'walletDisplay.categoryGift', '赠款'),
    adjust: translated(translate, 'walletDisplay.categoryAdjust', '调账'),
    refund: translated(translate, 'walletDisplay.categoryRefund', '退款'),
  }
  if (!category) return translated(translate, 'common.unknown', '未知')
  return labels[category] || category
}

export function dailyUsageCategoryLabel(isToday = false, translate?: Translate): string {
  return isToday
    ? translated(translate, 'walletDisplay.todayUsage', '今日用量')
    : translated(translate, 'walletDisplay.dailyUsage', '每日用量')
}

export function formatTokenCount(value: number | null | undefined): string {
  const amount = Number(value ?? 0)
  if (amount >= 1_000_000) {
    return `${(amount / 1_000_000).toFixed(amount >= 10_000_000 ? 0 : 1)}M`
  }
  if (amount >= 1_000) {
    return `${(amount / 1_000).toFixed(amount >= 10_000 ? 0 : 1)}K`
  }
  return `${Math.round(amount)}`
}

export function walletTransactionReasonLabel(reasonCode: string | null | undefined, translate?: Translate): string {
  const labels: Record<string, string> = {
    topup_admin_manual: translated(translate, 'walletDisplay.reasonManualTopup', '人工充值'),
    topup_gateway: translated(translate, 'walletDisplay.reasonGatewayTopup', '支付充值'),
    topup_card_code: translated(translate, 'walletDisplay.reasonCardTopup', '卡密充值'),
    gift_initial: translated(translate, 'walletDisplay.reasonInitialGift', '初始赠款'),
    gift_campaign: translated(translate, 'walletDisplay.reasonCampaignGift', '活动赠款'),
    gift_expire_reclaim: translated(translate, 'walletDisplay.reasonGiftReclaim', '赠款回收'),
    adjust_admin: translated(translate, 'walletDisplay.reasonAdminAdjust', '人工调账'),
    adjust_system: translated(translate, 'walletDisplay.reasonSystemAdjust', '系统调账'),
    refund_out: translated(translate, 'walletDisplay.reasonRefundOut', '退款扣减'),
    refund_revert: translated(translate, 'walletDisplay.reasonRefundRevert', '退款回补'),
  }
  if (!reasonCode) return translated(translate, 'common.unknown', '未知')
  return labels[reasonCode] || reasonCode
}

export function paymentMethodLabel(method: string | null | undefined, translate?: Translate): string {
  const labels: Record<string, string> = {
    alipay: translated(translate, 'walletDisplay.paymentAlipay', '支付宝支付'),
    ali_pay: translated(translate, 'walletDisplay.paymentAlipay', '支付宝支付'),
    wechat: translated(translate, 'walletDisplay.paymentWechat', '微信支付'),
    we_chat_pay: translated(translate, 'walletDisplay.paymentWechat', '微信支付'),
    WECHAT: translated(translate, 'walletDisplay.paymentWechat', '微信支付'),
    wxpay: translated(translate, 'walletDisplay.paymentWechat', '微信支付'),
    epay: translated(translate, 'walletDisplay.paymentEpay', '易支付'),
    dodopay: 'DoDoPay',
    ALIPAY: translated(translate, 'walletDisplay.paymentAlipay', '支付宝支付'),
    admin_manual: translated(translate, 'walletDisplay.paymentManual', '人工充值'),
    card_code: translated(translate, 'walletDisplay.paymentCard', '充值卡'),
    gift_code: translated(translate, 'walletDisplay.paymentGiftCard', '礼品卡'),
    card_recharge: translated(translate, 'walletDisplay.paymentCardRecharge', '卡密充值'),
    bank_transfer: translated(translate, 'walletDisplay.paymentBankTransfer', '银行转账'),
    offline: translated(translate, 'walletDisplay.paymentOffline', '线下转账'),
  }
  const normalized = method?.trim()
  if (!normalized) return '-'
  return labels[normalized] || labels[normalized.toLowerCase()] || normalized
}

export function paymentOrderMethodLabel(order: {
  payment_method?: string | null
  payment_provider?: string | null
  payment_channel?: string | null
}, translate?: Translate): string {
  const channel = order.payment_channel?.trim()
  const method = order.payment_method?.trim()
  const provider = order.payment_provider?.trim()
  const gateway = (provider || method || '').toLowerCase()
  if (channel && (gateway === 'dodopay' || gateway === 'epay')) {
    return paymentMethodLabel(channel, translate)
  }
  return paymentMethodLabel(method || provider, translate)
}

export function paymentOrderKindLabel(kind: string | null | undefined, translate?: Translate): string {
  const labels: Record<string, string> = {
    wallet_recharge: translated(translate, 'walletDisplay.walletRecharge', '钱包充值'),
    plan_purchase: translated(translate, 'walletDisplay.planPurchase', '套餐购买'),
  }
  if (!kind) return translated(translate, 'walletDisplay.walletRecharge', '钱包充值')
  return labels[kind] || kind
}

function stringFromSnapshot(snapshot: Record<string, unknown> | null | undefined, keys: string[]): string | null {
  if (!snapshot) return null
  for (const key of keys) {
    const value = snapshot[key]
    if (typeof value === 'string' && value.trim()) {
      return value.trim()
    }
  }
  return null
}

export function paymentOrderProductName(order: {
  product_id?: string | null
  product_snapshot?: Record<string, unknown> | null
}): string | null {
  return stringFromSnapshot(order.product_snapshot, ['title', 'name', 'display_name', 'plan_name']) || order.product_id || null
}

export function paymentOrderContentLabel(order: {
  order_kind?: string | null
  product_id?: string | null
  product_snapshot?: Record<string, unknown> | null
}, translate?: Translate): string {
  if (order.order_kind === 'plan_purchase') {
    const productName = paymentOrderProductName(order)
    return productName
      ? (translate ? translate('walletDisplay.planPurchaseProduct', { product: productName }) : `套餐购买 · ${productName}`)
      : translated(translate, 'walletDisplay.planPurchase', '套餐购买')
  }
  return paymentOrderKindLabel(order.order_kind, translate)
}

export function paymentStatusLabel(status: string | null | undefined, translate?: Translate): string {
  const labels: Record<string, string> = {
    pending: translated(translate, 'walletDisplay.paymentPending', '待支付'),
    paid: translated(translate, 'walletDisplay.paymentPaid', '已支付'),
    credited: translated(translate, 'walletDisplay.paymentCredited', '已到账'),
    failed: translated(translate, 'walletDisplay.paymentFailed', '支付失败'),
    expired: translated(translate, 'walletDisplay.paymentExpired', '已过期'),
    cancelled: translated(translate, 'walletDisplay.paymentCancelled', '已取消'),
    refunding: translated(translate, 'walletDisplay.paymentRefunding', '退款中'),
    refunded: translated(translate, 'walletDisplay.paymentRefunded', '已退款'),
  }
  if (!status) return translated(translate, 'common.unknown', '未知')
  return labels[status] || status
}

export function walletLinkTypeLabel(type: string | null | undefined, translate?: Translate): string {
  const labels: Record<string, string> = {
    payment_order: translated(translate, 'walletDisplay.linkPaymentOrder', '支付订单'),
    refund_request: translated(translate, 'walletDisplay.linkRefundRequest', '退款申请'),
    admin_action: translated(translate, 'walletDisplay.linkAdminAction', '后台操作'),
    system_task: translated(translate, 'walletDisplay.linkSystemTask', '系统任务'),
    campaign: translated(translate, 'walletDisplay.linkCampaign', '活动批次'),
    usage: translated(translate, 'walletDisplay.linkUsage', '用量记录'),
  }
  if (!type) return '-'
  return labels[type] || translated(translate, 'walletDisplay.other', '其他')
}

export function paymentStatusBadge(status: string | null | undefined): string {
  if (status === 'credited' || status === 'refunded') return 'success'
  if (status === 'paid' || status === 'refunding') return 'outline'
  if (status === 'pending') return 'secondary'
  if (status === 'expired') return 'warning'
  if (status === 'failed' || status === 'cancelled') return 'destructive'
  return 'secondary'
}

export function refundModeLabel(mode: string | null | undefined, translate?: Translate): string {
  const labels: Record<string, string> = {
    original_channel: translated(translate, 'walletDisplay.refundOriginal', '原路退回'),
    offline_payout: translated(translate, 'walletDisplay.refundOffline', '线下打款'),
  }
  if (!mode) return '-'
  return labels[mode] || mode
}

export function refundStatusLabel(status: string | null | undefined, translate?: Translate): string {
  const labels: Record<string, string> = {
    pending_approval: translated(translate, 'walletDisplay.refundPending', '待审批'),
    approved: translated(translate, 'walletDisplay.refundApproved', '已审批'),
    processing: translated(translate, 'walletDisplay.refundProcessing', '处理中'),
    succeeded: translated(translate, 'walletDisplay.refundSucceeded', '已完成'),
    failed: translated(translate, 'walletDisplay.refundFailed', '已失败'),
    cancelled: translated(translate, 'walletDisplay.refundCancelled', '已取消'),
  }
  if (!status) return translated(translate, 'common.unknown', '未知')
  return labels[status] || status
}

export function refundStatusBadge(status: string | null | undefined): string {
  if (status === 'succeeded') return 'success'
  if (status === 'processing') return 'outline'
  if (status === 'pending_approval' || status === 'approved') return 'secondary'
  if (status === 'failed' || status === 'cancelled') return 'destructive'
  return 'secondary'
}

export function callbackStatusLabel(status: string | null | undefined, translate?: Translate): string {
  const labels: Record<string, string> = {
    processed: translated(translate, 'walletDisplay.callbackProcessed', '已处理'),
    duplicate: translated(translate, 'walletDisplay.callbackDuplicate', '重复回调'),
    ignored: translated(translate, 'walletDisplay.callbackIgnored', '已忽略'),
    invalid_signature: translated(translate, 'walletDisplay.callbackInvalidSignature', '验签失败'),
    error: translated(translate, 'walletDisplay.callbackError', '处理失败'),
  }
  if (!status) return translated(translate, 'common.unknown', '未知')
  return labels[status] || status
}

export function callbackStatusBadge(status: string | null | undefined): string {
  if (status === 'processed') return 'success'
  if (status === 'duplicate' || status === 'ignored') return 'secondary'
  if (status === 'invalid_signature' || status === 'error') return 'destructive'
  return 'outline'
}
