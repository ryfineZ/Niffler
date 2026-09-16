import { describe, expect, it } from 'vitest'

import {
  formatWalletAmount,
  formatWalletCurrency,
  paymentOrderContentLabel,
  paymentOrderKindLabel,
  paymentOrderMethodLabel,
  paymentStatusLabel,
} from '../walletDisplay'

describe('wallet amount formatting', () => {
  it('keeps every stored decimal without hiding the settled amount', () => {
    expect(formatWalletAmount(10)).toBe('10.00')
    expect(formatWalletAmount(10.5)).toBe('10.50')
    expect(formatWalletAmount(10.001_388_89)).toBe('10.00138889')
    expect(formatWalletCurrency(-626.710_000_01)).toBe('$-626.71000001')
  })
})

describe('paymentOrderMethodLabel', () => {
  it('shows the real DoDoPay channel when callback records it', () => {
    expect(paymentOrderMethodLabel({
      payment_method: 'dodopay',
      payment_provider: 'dodopay',
      payment_channel: 'WECHAT',
    })).toBe('微信支付')
    expect(paymentOrderMethodLabel({
      payment_method: 'dodopay',
      payment_provider: 'dodopay',
      payment_channel: 'ALIPAY',
    })).toBe('支付宝支付')
    expect(paymentOrderMethodLabel({
      payment_method: 'dodopay',
      payment_provider: 'dodopay',
      payment_channel: 'we_chat_pay',
    })).toBe('微信支付')
    expect(paymentOrderMethodLabel({
      payment_method: 'dodopay',
      payment_provider: 'dodopay',
      payment_channel: 'ali_pay',
    })).toBe('支付宝支付')
  })

  it('does not treat non-gateway channel as payment channel', () => {
    expect(paymentOrderMethodLabel({
      payment_method: 'gift_code',
      payment_provider: 'redeem_code',
      payment_channel: 'gift',
    })).toBe('礼品卡')
  })
})

describe('paymentStatusLabel', () => {
  it('shows cancelled payment orders as cancelled', () => {
    expect(paymentStatusLabel('cancelled')).toBe('已取消')
  })
})

describe('paymentOrderKindLabel', () => {
  it('shows readable order kinds', () => {
    expect(paymentOrderKindLabel('wallet_recharge')).toBe('钱包充值')
    expect(paymentOrderKindLabel('plan_purchase')).toBe('套餐购买')
  })
})

describe('paymentOrderContentLabel', () => {
  it('shows plan purchase title from product snapshot', () => {
    expect(paymentOrderContentLabel({
      order_kind: 'plan_purchase',
      product_id: 'plan-4-9',
      product_snapshot: {
        title: 'Plus 限购套餐',
      },
    })).toBe('套餐购买 · Plus 限购套餐')
  })

  it('falls back to product id when plan title is missing', () => {
    expect(paymentOrderContentLabel({
      order_kind: 'plan_purchase',
      product_id: 'plan-legacy',
      product_snapshot: {},
    })).toBe('套餐购买 · plan-legacy')
  })
})
