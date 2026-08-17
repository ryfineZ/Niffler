<template>
  <div class="space-y-6 pb-8">
    <div
      v-if="loadingInitial"
      class="py-16"
    >
      <LoadingState :message="t('wallet.loading')" />
    </div>

    <template v-else>
      <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 gap-4">
        <Card class="p-5 space-y-2">
          <div class="text-xs uppercase tracking-wider text-muted-foreground">
            {{ t('wallet.total') }}
          </div>
          <div class="text-3xl font-bold tabular-nums">
            {{ walletBalance?.unlimited ? t('wallet.unlimited') : formatCurrency(totalAvailableBalance) }}
          </div>
          <div class="text-xs text-muted-foreground">
            {{ t('wallet.package') }}: {{ formatCurrency(packageBalance) }} · {{ t('wallet.balance') }}: {{ formatCurrency(walletOnlyBalance) }}
          </div>
        </Card>

        <Card class="p-5 space-y-3">
          <div class="text-xs uppercase tracking-wider text-muted-foreground">
            {{ t('wallet.package') }}
          </div>
          <div class="text-2xl font-bold tabular-nums">
            <template v-if="hasActiveDailyQuota">
              {{ formatCurrency(packageBalance) }}
            </template>
            <template v-else>
              {{ t('wallet.notEnabled') }}
            </template>
          </div>
          <div
            v-if="hasActiveDailyQuota"
            class="space-y-1.5"
          >
            <div class="h-1.5 overflow-hidden rounded-full bg-muted">
              <div
                class="h-full rounded-full bg-primary transition-all"
                :style="{ width: `${dailyQuotaRemainingPercent}%` }"
              />
            </div>
            <div class="text-xs text-muted-foreground">
              {{ t('wallet.used') }} {{ formatCurrency(dailyQuotaUsed) }} / {{ t('wallet.daily') }} {{ formatCurrency(dailyQuotaTotal) }}
            </div>
            <div class="text-xs text-muted-foreground">
              {{ t('wallet.overage') }}
            </div>
          </div>
          <div
            v-else
            class="text-xs text-muted-foreground"
          >
            {{ t('wallet.packageHint') }}
          </div>
        </Card>

        <Card class="p-5 space-y-2">
          <div class="text-xs uppercase tracking-wider text-muted-foreground">
            {{ t('wallet.balance') }}
          </div>
          <div
            class="text-2xl font-semibold tabular-nums"
            :class="walletOnlyBalance < 0 ? 'text-rose-600' : undefined"
          >
            {{ formatCurrency(walletOnlyBalance) }}
          </div>
          <div
            v-if="walletOnlyBalance < 0"
            class="text-xs font-medium text-rose-600"
          >
            {{ t(hasUsablePlanBalance ? 'wallet.inDebtPlanStillUsable' : 'wallet.inDebtNoUsablePlan') }}
          </div>
          <div class="text-xs text-muted-foreground">
            {{ t('wallet.rechargeBalance') }}: {{ formatCurrency(walletBalance?.wallet?.recharge_balance) }} · {{ t('wallet.giftBalance') }}: {{ formatCurrency(walletBalance?.wallet?.gift_balance) }}
          </div>
        </Card>

        <Card class="p-5 space-y-2">
          <div class="text-xs uppercase tracking-wider text-muted-foreground">
            {{ t('wallet.status') }}
          </div>
          <div class="flex items-center gap-2">
            <Badge :variant="walletStatusBadge(walletBalance?.wallet?.status)">
              {{ walletStatusLabel(walletBalance?.wallet?.status, t) }}
            </Badge>
          </div>
          <div class="text-xs text-muted-foreground">
            {{ t('wallet.recharge') }} / {{ t('wallet.consume') }}:
            {{ formatCurrency(walletBalance?.wallet?.total_recharged) }}
            <span class="text-muted-foreground font-normal mx-1">/</span>
            {{ formatCurrency(walletBalance?.wallet?.total_consumed) }}
          </div>
          <div class="text-xs text-muted-foreground">
            {{ t('wallet.refunded') }}: {{ formatCurrency(walletBalance?.wallet?.total_refunded) }} · {{ t('wallet.refundable') }}: {{ formatCurrency(walletBalance?.wallet?.refundable_balance) }}
          </div>
          <div
            v-if="walletBalance?.unlimited"
            class="text-xs text-amber-600 dark:text-amber-400"
          >
            {{ t('wallet.unlimitedHint') }}
          </div>
          <div class="text-xs text-muted-foreground">
            {{ t('wallet.pendingRefund') }}: {{ walletBalance?.pending_refund_count || 0 }}
          </div>
        </Card>
      </div>

      <Card class="p-5 space-y-4">
        <div class="flex items-center justify-between">
          <div>
            <h3 class="text-base font-semibold">
              {{ t('wallet.redeem') }}
            </h3>
            <p class="text-xs text-muted-foreground mt-1">
              {{ t('wallet.redeemHint') }}
            </p>
          </div>
          <RefreshButton
            :loading="loadingOrders || loadingTransactions"
            @click="refreshRedeemSection"
          />
        </div>

        <div class="grid grid-cols-1 lg:grid-cols-[1fr_auto] gap-3">
          <Input
            v-model="redeemForm.code"
            :placeholder="t('wallet.redeemPlaceholder')"
            autocomplete="off"
          />
          <Button
            :disabled="submittingRedeem"
            @click="submitRedeem"
          >
            {{ submittingRedeem ? t('wallet.redeeming') : t('wallet.redeemNow') }}
          </Button>
        </div>

        <div
          v-if="latestRedeem"
          class="rounded-xl border border-border/60 bg-muted/20 p-3 text-xs text-muted-foreground space-y-1.5"
        >
          <div>
            {{ t('wallet.redeemedBatch') }}: <span class="font-medium text-foreground">{{ latestRedeem.batch_name }}</span>
          </div>
          <div>
            {{ t('wallet.amount') }}: <span class="font-medium text-foreground">{{ formatCurrency(latestRedeem.amount_usd) }}</span>
          </div>
          <div>
            {{ t('wallet.orderNo') }}: <span class="font-mono text-foreground">{{ latestRedeem.order.order_no }}</span>
          </div>
        </div>
      </Card>

      <!-- TODO(wallet): 充值/退款用户主动操作入口暂未启用，待支付链路联调完成后再开放 -->
      <div
        v-if="ENABLE_WALLET_ACTION_FORMS"
        class="grid grid-cols-1 xl:grid-cols-2 gap-4"
      >
        <Card class="p-5 space-y-4">
          <div class="flex items-center justify-between">
            <h3 class="text-base font-semibold">
              {{ t('wallet.rechargeForm') }}
            </h3>
            <RefreshButton
              :loading="loadingOrders"
              @click="loadOrders"
            />
          </div>

          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div class="space-y-1.5">
              <Label>{{ t('wallet.amount') }}</Label>
              <Input
                v-model.number="rechargeForm.amount_usd"
                type="number"
                min="0.01"
                step="0.01"
                placeholder="10"
              />
            </div>

            <div class="space-y-1.5">
              <Label>{{ t('wallet.payment') }}</Label>
              <Select v-model="rechargeForm.payment_option_key">
                <SelectTrigger>
                  <SelectValue
                    :placeholder="rechargeOptionsWithKey.length ? t('wallet.choosePayment') : t('wallet.noPayment')"
                  />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="option in rechargeOptionsWithKey"
                    :key="option.key"
                    :value="option.key"
                  >
                    {{ option.display_name }}
                    <span
                      v-if="option.pay_currency && option.usd_exchange_rate"
                      class="text-xs text-muted-foreground"
                    >
                      · {{ option.pay_currency }}
                    </span>
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          <div
            v-if="selectedRechargeOption?.usd_exchange_rate"
            class="rounded-xl border border-border/60 bg-muted/20 p-3 text-xs text-muted-foreground"
          >
            <div class="flex flex-wrap items-center justify-between gap-2">
              <span>{{ t('wallet.payable') }}</span>
              <span class="text-sm font-semibold text-foreground">
                {{ estimatedRechargePayAmount }}
                {{ selectedRechargeOption.pay_currency || 'CNY' }}
              </span>
            </div>
            <div class="mt-1">
              {{ t('wallet.recharge') }} {{ rechargeAmountUsdText }} USD，1 USD =
              {{ Number(selectedRechargeOption.usd_exchange_rate).toFixed(4) }}
              {{ selectedRechargeOption.pay_currency || 'CNY' }} {{ t('wallet.conversion') }}
            </div>
          </div>

          <Button
            class="w-full"
            :disabled="submittingRecharge || rechargeOptionsWithKey.length === 0"
            @click="submitRecharge"
          >
            {{ submittingRecharge ? t('wallet.creatingOrder') : t('wallet.createOrder') }}
          </Button>

          <div
            v-if="latestRecharge"
            class="rounded-xl border border-border/60 bg-muted/30 p-3 space-y-1.5"
          >
            <div class="text-xs text-muted-foreground">
              {{ t('wallet.latestOrder') }}: <span class="font-medium text-foreground">{{ latestRecharge.order.order_no }}</span>
            </div>
            <div class="text-xs text-muted-foreground">
              {{ t('wallet.status') }}:
              <Badge
                :variant="paymentStatusBadge(latestRecharge.order.status)"
                class="ml-1"
              >
                {{ paymentStatusLabel(latestRecharge.order.status, t) }}
              </Badge>
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <a
                v-if="latestRecharge.payment_instructions?.payment_url"
                class="inline-flex text-xs text-primary hover:underline"
                :href="String(latestRecharge.payment_instructions.payment_url)"
                target="_blank"
                rel="noopener noreferrer"
                @click.prevent="submitPaymentInstructions(latestRecharge.payment_instructions)"
              >
                {{ t('wallet.openPayment') }}
              </a>
              <Button
                v-if="latestRechargeCancelUrl"
                variant="ghost"
                size="sm"
                class="h-auto px-0 py-0 text-xs text-destructive hover:bg-transparent hover:text-destructive/80"
                @click="cancelLatestRecharge"
              >
                {{ t('wallet.cancelPayment') }}
              </Button>
            </div>
            <div
              v-if="latestRecharge.payment_instructions?.qr_code"
              class="text-xs text-muted-foreground break-all"
            >
              QR: {{ latestRecharge.payment_instructions.qr_code }}
            </div>
          </div>
        </Card>

        <Card class="p-5 space-y-4">
          <div class="flex items-center justify-between">
            <h3 class="text-base font-semibold">
              {{ t('wallet.refundApply') }}
            </h3>
            <RefreshButton
              :loading="loadingRefunds"
              @click="loadRefunds"
            />
          </div>

          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div class="space-y-1.5">
              <Label>{{ t('wallet.refundAmount') }}</Label>
              <Input
                v-model.number="refundForm.amount_usd"
                type="number"
                min="0.01"
                step="0.01"
                placeholder="5"
              />
            </div>

            <div class="space-y-1.5">
              <Label>{{ t('wallet.refundMode') }}</Label>
              <Select v-model="refundForm.refund_mode">
                <SelectTrigger>
                  <SelectValue :placeholder="t('wallet.chooseRefundMode')" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="original_channel">
                    {{ t('wallet.originalRoute') }}
                  </SelectItem>
                  <SelectItem value="offline_payout">
                    {{ t('wallet.offlineTransfer') }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          <div class="space-y-1.5">
            <Label>{{ t('wallet.relatedOrder') }}</Label>
            <Select v-model="refundForm.payment_order_id">
              <SelectTrigger>
                <SelectValue :placeholder="t('wallet.refundUnspecifiedHint')" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__">
                  {{ t('wallet.unspecified') }}
                </SelectItem>
                <SelectItem
                  v-for="order in refundableOrders"
                  :key="order.id"
                  :value="order.id"
                >
                  {{ order.order_no }} ({{ t('wallet.refundableAmount', { amount: formatCurrency(order.refundable_amount_usd) }) }})
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div class="space-y-1.5">
            <Label>{{ t('wallet.refundReason') }}</Label>
            <Textarea
              v-model="refundForm.reason"
              :placeholder="t('wallet.refundReasonHint')"
              rows="3"
            />
          </div>

          <div class="rounded-xl border border-border/60 bg-muted/20 p-3 text-xs text-muted-foreground">
            {{ t('wallet.refundNotice') }}
          </div>

          <Button
            class="w-full"
            variant="outline"
            :disabled="submittingRefund"
            @click="submitRefund"
          >
            {{ submittingRefund ? t('wallet.submitting') : t('wallet.submitRefund') }}
          </Button>
        </Card>
      </div>

      <Card class="overflow-hidden">
        <div class="px-5 pt-5 pb-2">
          <Tabs v-model="activeTab">
            <TabsList class="tabs-button-list grid grid-cols-3 w-full max-w-xl">
              <TabsTrigger value="transactions">
                {{ t('wallet.transactions') }}
              </TabsTrigger>
              <TabsTrigger value="orders">
                {{ t('wallet.orders') }}
              </TabsTrigger>
              <TabsTrigger value="refunds">
                {{ t('wallet.refunds') }}
              </TabsTrigger>
            </TabsList>

            <TabsContent
              value="transactions"
              class="mt-4 space-y-4"
            >
              <div class="px-5 flex items-center justify-between">
                <div class="text-sm text-muted-foreground">
                  {{ t('wallet.transactions') }} {{ txTotal }}
                </div>
                <RefreshButton
                  :loading="loadingTransactions"
                  @click="loadTransactions"
                />
              </div>
              <div class="overflow-x-auto">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{{ t('wallet.time') }}</TableHead>
                      <TableHead>{{ t('wallet.type') }}</TableHead>
                      <TableHead>{{ t('wallet.amountColumn') }}</TableHead>
                      <TableHead>{{ t('wallet.balanceChange') }}</TableHead>
                      <TableHead>{{ t('wallet.note') }}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    <TableRow v-if="todayUsage">
                      <TableCell class="text-xs text-muted-foreground">
                        {{ todayUsage.date || '-' }}
                      </TableCell>
                      <TableCell>
                        <div class="space-y-1">
                          <div class="flex items-center gap-2">
                            <Badge
                              variant="outline"
                              class="font-mono border-amber-500/40 text-amber-700 dark:text-amber-300"
                            >
                              {{ dailyUsageCategoryLabel(true, t) }}
                            </Badge>
                            <span class="inline-flex h-2 w-2 rounded-full bg-emerald-500 animate-pulse" />
                            <span class="text-[11px] text-muted-foreground">
                              Live
                            </span>
                          </div>
                          <div class="text-[11px] text-muted-foreground">
                            {{ todayUsage.timezone || 'UTC' }}
                          </div>
                        </div>
                      </TableCell>
                      <TableCell class="text-muted-foreground">
                        {{ todayUsage.total_cost.toFixed(4) }}
                      </TableCell>
                      <TableCell class="text-xs text-muted-foreground">
                        {{ t('wallet.note') }}
                      </TableCell>
                      <TableCell class="text-xs text-muted-foreground">
                        {{ t('wallet.requestsAndTokens', { count: todayUsage.total_requests, input: formatTokenCount(todayUsage.input_tokens), output: formatTokenCount(todayUsage.output_tokens) }) }}
                      </TableCell>
                    </TableRow>
                    <template
                      v-for="item in flowItems"
                      :key="item.type === 'transaction' ? item.data.id : `daily-${item.data.id || item.data.date}`"
                    >
                      <TableRow v-if="item.type === 'transaction'">
                        <TableCell class="text-xs text-muted-foreground">
                          {{ formatDateTime(item.data.created_at) }}
                        </TableCell>
                        <TableCell>
                          <div class="space-y-1">
                            <Badge
                              variant="outline"
                              class="font-mono"
                            >
                              {{ walletTransactionCategoryLabel(item.data.category, t) }}
                            </Badge>
                            <div class="text-[11px] text-muted-foreground">
                              {{ walletTransactionReasonLabel(item.data.reason_code, t) }}
                            </div>
                          </div>
                        </TableCell>
                        <TableCell
                          :class="item.data.amount >= 0 ? 'text-emerald-600 dark:text-emerald-400' : 'text-rose-600 dark:text-rose-400'"
                        >
                          {{ item.data.amount >= 0 ? '+' : '' }}{{ formatWalletAmount(item.data.amount) }}
                        </TableCell>
                        <TableCell class="text-xs tabular-nums">
                          {{ formatWalletAmount(item.data.balance_before) }} → {{ formatWalletAmount(item.data.balance_after) }}
                        </TableCell>
                        <TableCell class="text-xs text-muted-foreground">
                          {{ item.data.description || '-' }}
                        </TableCell>
                      </TableRow>
                      <TableRow v-else>
                        <TableCell class="text-xs text-muted-foreground">
                          {{ item.data.date || '-' }}
                        </TableCell>
                        <TableCell>
                          <div class="space-y-1">
                            <Badge
                              variant="outline"
                              class="font-mono border-amber-500/40 text-amber-700 dark:text-amber-300"
                            >
                              {{ dailyUsageCategoryLabel(false, t) }}
                            </Badge>
                            <div class="text-[11px] text-muted-foreground">
                              {{ item.data.timezone || '-' }}
                            </div>
                          </div>
                        </TableCell>
                        <TableCell class="text-muted-foreground">
                          {{ item.data.total_cost.toFixed(4) }}
                        </TableCell>
                        <TableCell class="text-xs text-muted-foreground">
                          {{ t('wallet.note') }}
                        </TableCell>
                        <TableCell class="text-xs text-muted-foreground">
                          {{ t('wallet.requestsAndTokens', { count: item.data.total_requests, input: formatTokenCount(item.data.input_tokens), output: formatTokenCount(item.data.output_tokens) }) }}
                        </TableCell>
                      </TableRow>
                    </template>
                    <TableRow v-if="!loadingTransactions && flowItems.length === 0">
                      <TableCell
                        colspan="5"
                        class="py-10"
                      >
                        <EmptyState
                          :title="t('wallet.noFlow')"
                          :description="t('wallet.noFlow')"
                        />
                      </TableCell>
                    </TableRow>
                  </TableBody>
                </Table>
              </div>
              <Pagination
                :current="txPage"
                :total="txTotal"
                :page-size="txPageSize"
                @update:current="handleTxPageChange"
                @update:page-size="handleTxPageSizeChange"
              />
            </TabsContent>

            <TabsContent
              value="orders"
              class="mt-4 space-y-4"
            >
              <div class="px-5 flex items-center justify-between">
                <div class="text-sm text-muted-foreground">
                  {{ t('wallet.orders') }} {{ orderTotal }}
                </div>
                <RefreshButton
                  :loading="loadingOrders"
                  @click="loadOrders"
                />
              </div>
              <div class="overflow-x-auto">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{{ t('wallet.orderNo') }}</TableHead>
                      <TableHead>{{ t('wallet.orderContent') }}</TableHead>
                      <TableHead>{{ t('wallet.amountColumn') }}</TableHead>
                      <TableHead>{{ t('wallet.payment') }}</TableHead>
                      <TableHead>{{ t('apiKeys.status') }}</TableHead>
                      <TableHead>{{ t('wallet.refundable') }}</TableHead>
                      <TableHead>{{ t('wallet.time') }}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    <TableRow
                      v-for="order in rechargeOrders"
                      :key="order.id"
                    >
                      <TableCell class="font-mono text-xs">
                        {{ order.order_no }}
                      </TableCell>
                      <TableCell>{{ paymentOrderContentLabel(order, t) }}</TableCell>
                      <TableCell class="tabular-nums">
                        {{ formatCurrency(order.amount_usd) }}
                      </TableCell>
                      <TableCell>{{ paymentOrderMethodLabel(order, t) }}</TableCell>
                      <TableCell>
                        <Badge :variant="paymentStatusBadge(order.status)">
                          {{ paymentStatusLabel(order.status, t) }}
                        </Badge>
                      </TableCell>
                      <TableCell class="tabular-nums">
                        {{ formatCurrency(order.refundable_amount_usd) }}
                      </TableCell>
                      <TableCell class="text-xs text-muted-foreground">
                        {{ formatDateTime(order.created_at) }}
                      </TableCell>
                    </TableRow>
                    <TableRow v-if="!loadingOrders && rechargeOrders.length === 0">
                      <TableCell
                        colspan="7"
                        class="py-10"
                      >
                        <EmptyState
                          :title="t('wallet.noFlow')"
                          :description="t('wallet.orderContent')"
                        />
                      </TableCell>
                    </TableRow>
                  </TableBody>
                </Table>
              </div>
              <Pagination
                :current="orderPage"
                :total="orderTotal"
                :page-size="orderPageSize"
                @update:current="handleOrderPageChange"
                @update:page-size="handleOrderPageSizeChange"
              />
            </TabsContent>

            <TabsContent
              value="refunds"
              class="mt-4 space-y-4"
            >
              <div class="px-5 flex items-center justify-between">
                <div class="text-sm text-muted-foreground">
                  {{ t('wallet.refunds') }} {{ refundTotal }}
                </div>
                <RefreshButton
                  :loading="loadingRefunds"
                  @click="loadRefunds"
                />
              </div>
              <div class="overflow-x-auto">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{{ t('wallet.orderNo') }}</TableHead>
                      <TableHead>{{ t('wallet.amountColumn') }}</TableHead>
                      <TableHead>{{ t('wallet.refundMode') }}</TableHead>
                      <TableHead>{{ t('wallet.status') }}</TableHead>
                      <TableHead>{{ t('wallet.refundReason') }}</TableHead>
                      <TableHead>{{ t('wallet.time') }}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    <TableRow
                      v-for="refund in refunds"
                      :key="refund.id"
                    >
                      <TableCell class="font-mono text-xs">
                        {{ refund.refund_no }}
                      </TableCell>
                      <TableCell class="tabular-nums">
                        {{ formatCurrency(refund.amount_usd) }}
                      </TableCell>
                      <TableCell>{{ refundModeLabel(refund.refund_mode) }}</TableCell>
                      <TableCell>
                        <Badge :variant="refundStatusBadge(refund.status)">
                          {{ refundStatusLabel(refund.status, t) }}
                        </Badge>
                      </TableCell>
                      <TableCell class="text-xs text-muted-foreground max-w-[220px] truncate">
                        {{ refund.reason || refund.failure_reason || '-' }}
                      </TableCell>
                      <TableCell class="text-xs text-muted-foreground">
                        {{ formatDateTime(refund.created_at) }}
                      </TableCell>
                    </TableRow>
                    <TableRow v-if="!loadingRefunds && refunds.length === 0">
                      <TableCell
                        colspan="6"
                        class="py-10"
                      >
                        <EmptyState
                          :title="t('wallet.refunds')"
                          :description="t('wallet.refundNotice')"
                        />
                      </TableCell>
                    </TableRow>
                  </TableBody>
                </Table>
              </div>
              <Pagination
                :current="refundPage"
                :total="refundTotal"
                :page-size="refundPageSize"
                @update:current="handleRefundPageChange"
                @update:page-size="handleRefundPageSizeChange"
              />
            </TabsContent>
          </Tabs>
        </div>
      </Card>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import {
  Badge,
  Button,
  Card,
  Input,
  Label,
  Pagination,
  RefreshButton,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  Textarea,
} from '@/components/ui'
import { EmptyState, LoadingState } from '@/components/common'
import {
  walletApi,
  type DailyUsageRecord,
  type FlowItem,
  type PaymentOrder,
  type RefundRequest,
  type WalletBalanceResponse,
  type WalletRedeemResponse,
  type WalletRechargeOption,
} from '@/api/wallet'
import { useToast } from '@/composables/useToast'
import { parseApiError } from '@/utils/errorParser'
import { log } from '@/utils/logger'
import {
  dailyUsageCategoryLabel,
  formatTokenCount,
  formatWalletAmount,
  formatWalletCurrency as formatCurrency,
  paymentOrderContentLabel,
  paymentOrderMethodLabel,
  paymentStatusBadge,
  paymentStatusLabel,
  refundModeLabel,
  refundStatusBadge,
  refundStatusLabel,
  walletStatusBadge,
  walletStatusLabel,
  walletTransactionCategoryLabel,
  walletTransactionReasonLabel,
} from '@/utils/walletDisplay'

const route = useRoute()
const router = useRouter()
const { t, locale } = useI18n()
const { success, error: showError, warning } = useToast()

const ENABLE_WALLET_ACTION_FORMS = true

const loadingInitial = ref(true)
const loadingTransactions = ref(false)
const loadingOrders = ref(false)
const loadingRefunds = ref(false)
const submittingRedeem = ref(false)
const submittingRecharge = ref(false)
const submittingRefund = ref(false)

const walletBalance = ref<WalletBalanceResponse | null>(null)
const latestRecharge = ref<{ order: PaymentOrder; payment_instructions: Record<string, unknown> } | null>(null)
const latestRedeem = ref<WalletRedeemResponse | null>(null)
const rechargeOptions = ref<WalletRechargeOption[]>([])

const flowItems = ref<FlowItem[]>([])
const todayUsage = ref<DailyUsageRecord | null>(null)
const txTotal = ref(0)
const txPage = ref(1)
const txPageSize = ref(20)

const rechargeOrders = ref<PaymentOrder[]>([])
const orderTotal = ref(0)
const orderPage = ref(1)
const orderPageSize = ref(20)

const latestRechargeCancelUrl = computed(() => {
  const value = latestRecharge.value?.payment_instructions?.local_cancel_url
  return typeof value === 'string' && value ? value : ''
})

const refunds = ref<RefundRequest[]>([])
const refundTotal = ref(0)
const refundPage = ref(1)
const refundPageSize = ref(20)

const activeTab = ref('transactions')
let todayCostPollTimer: ReturnType<typeof setInterval> | null = null
const BILLING_SUMMARY_REFRESH_EVENT = 'aether:billing-summary-refresh'

const rechargeForm = reactive({
  amount_usd: 10,
  payment_option_key: '',
})

const refundForm = reactive({
  amount_usd: 0,
  payment_order_id: '__none__',
  refund_mode: 'offline_payout',
  reason: '',
})

const redeemForm = reactive({
  code: '',
})

const refundableOrders = computed(() =>
  rechargeOrders.value.filter(o => (o.refundable_amount_usd || 0) > 0)
)

const rechargeOptionsWithKey = computed(() =>
  rechargeOptions.value.map((option, index) => ({
    ...option,
    key: [
      option.payment_provider || option.provider || option.payment_method,
      option.payment_method,
      option.payment_channel || '',
      index,
    ].join(':'),
  }))
)

const selectedRechargeOption = computed(() => {
  if (rechargeOptionsWithKey.value.length === 0) return null
  return rechargeOptionsWithKey.value.find(option => option.key === rechargeForm.payment_option_key)
    || rechargeOptionsWithKey.value[0]
})

const estimatedRechargePayAmount = computed(() => {
  const rate = Number(selectedRechargeOption.value?.usd_exchange_rate || 0)
  if (!Number.isFinite(rate) || rate <= 0) return '-'
  return (Number(rechargeForm.amount_usd || 0) * rate).toFixed(2)
})
const rechargeAmountUsdText = computed(() => Number(rechargeForm.amount_usd || 0).toFixed(2))

const dailyQuota = computed(() => walletBalance.value?.daily_quota ?? null)
const hasActiveDailyQuota = computed(() => Boolean(dailyQuota.value?.has_active))
const walletOnlyBalance = computed(() => {
  const explicitBalance = walletBalance.value?.actual_wallet_balance
    ?? walletBalance.value?.wallet_balance
  if (typeof explicitBalance === 'number' && Number.isFinite(explicitBalance)) {
    return explicitBalance
  }
  return Number(walletBalance.value?.balance ?? 0)
})
const packageBalance = computed(() => {
  const quotaRemaining = dailyQuota.value?.remaining_usd
  if (hasActiveDailyQuota.value && typeof quotaRemaining === 'number' && Number.isFinite(quotaRemaining)) {
    return Math.max(0, quotaRemaining)
  }
  const explicitBalance = walletBalance.value?.package_balance
  if (typeof explicitBalance === 'number' && Number.isFinite(explicitBalance)) {
    return Math.max(0, explicitBalance)
  }
  return 0
})
const hasUsablePlanBalance = computed(() =>
  hasActiveDailyQuota.value && packageBalance.value > 0.000_000_01
)
const totalAvailableBalance = computed(() => {
  const explicitBalance = walletBalance.value?.total_available_balance
  if (typeof explicitBalance === 'number' && Number.isFinite(explicitBalance)) {
    return explicitBalance
  }
  return walletOnlyBalance.value + packageBalance.value
})
const dailyQuotaTotal = computed(() => {
  const value = dailyQuota.value?.total_usd
  return typeof value === 'number' && Number.isFinite(value) ? Math.max(0, value) : 0
})
const dailyQuotaUsed = computed(() => {
  const value = dailyQuota.value?.used_usd
  return typeof value === 'number' && Number.isFinite(value) ? Math.max(0, value) : 0
})
const dailyQuotaRemainingPercent = computed(() => {
  if (!hasActiveDailyQuota.value || dailyQuotaTotal.value <= 0) return 0
  return Math.min(100, Math.max(0, (packageBalance.value / dailyQuotaTotal.value) * 100))
})

onMounted(async () => {
  document.addEventListener('visibilitychange', handleVisibilityChange)
  showPaymentCancelledNotice()
  try {
    await Promise.all([
      loadBalance(),
      loadTransactions(),
      loadTodayCost(),
      loadOrders(),
      loadRefunds(),
      loadRechargeOptions(),
    ])
    syncTodayCostPolling()
  } finally {
    loadingInitial.value = false
  }
})

function showPaymentCancelledNotice() {
  if (route.query.payment_cancelled !== '1' && route.query.payment_cancel_failed !== '1') return
  if (route.query.payment_cancelled === '1') {
    warning(t('wallet.paymentCancelled'))
  } else {
    showError(t('wallet.paymentCancelFailed'))
  }
  const nextQuery = { ...route.query }
  delete nextQuery.payment_cancelled
  delete nextQuery.payment_cancel_failed
  void router.replace({ query: nextQuery })
}

onBeforeUnmount(() => {
  stopTodayCostPolling()
  document.removeEventListener('visibilitychange', handleVisibilityChange)
})

watch(activeTab, () => {
  syncTodayCostPolling()
})

async function loadBalance() {
  walletBalance.value = await walletApi.getBalance()
  window.dispatchEvent(new CustomEvent(BILLING_SUMMARY_REFRESH_EVENT))
}

async function loadRechargeOptions() {
  try {
    const response = await walletApi.listRechargeOptions()
    rechargeOptions.value = response.items
    if (!rechargeForm.payment_option_key && rechargeOptionsWithKey.value.length > 0) {
      const preferred = rechargeOptionsWithKey.value.find(option => option.payment_provider === 'epay')
        || rechargeOptionsWithKey.value[0]
      rechargeForm.payment_option_key = preferred.key
    }
  } catch (error) {
    log.error('加载充值方式失败:', error)
    showError(parseApiError(error, t('wallet.loadRechargeOptionsFailed')))
  }
}

async function loadTransactions() {
  loadingTransactions.value = true
  try {
    const offset = (txPage.value - 1) * txPageSize.value
    const resp = await walletApi.getFlow({ limit: txPageSize.value, offset })
    flowItems.value = resp.items
    txTotal.value = resp.total
    todayUsage.value = resp.today_entry
  } catch (error) {
    log.error('加载资金与用量失败:', error)
    showError(parseApiError(error, t('wallet.loadFundsFailed')))
  } finally {
    loadingTransactions.value = false
  }
}

async function loadTodayCost() {
  try {
    todayUsage.value = await walletApi.getTodayCost()
  } catch (error) {
    log.error('加载今日用量失败:', error)
  }
}

function syncTodayCostPolling() {
  if (activeTab.value === 'transactions' && !document.hidden) {
    startTodayCostPolling()
  } else {
    stopTodayCostPolling()
  }
}

function startTodayCostPolling() {
  if (todayCostPollTimer) return
  todayCostPollTimer = setInterval(() => {
    void loadTodayCost()
  }, 20_000)
}

function stopTodayCostPolling() {
  if (!todayCostPollTimer) return
  clearInterval(todayCostPollTimer)
  todayCostPollTimer = null
}

function handleVisibilityChange() {
  syncTodayCostPolling()
}

async function loadOrders() {
  loadingOrders.value = true
  try {
    const offset = (orderPage.value - 1) * orderPageSize.value
    const resp = await walletApi.listRechargeOrders({ limit: orderPageSize.value, offset })
    rechargeOrders.value = resp.items
    orderTotal.value = resp.total
  } catch (error) {
    log.error('加载支付订单失败:', error)
    showError(parseApiError(error, t('wallet.loadOrdersFailed')))
  } finally {
    loadingOrders.value = false
  }
}

async function loadRefunds() {
  loadingRefunds.value = true
  try {
    const offset = (refundPage.value - 1) * refundPageSize.value
    const resp = await walletApi.listRefunds({ limit: refundPageSize.value, offset })
    refunds.value = resp.items
    refundTotal.value = resp.total
  } catch (error) {
    log.error('加载退款记录失败:', error)
    showError(parseApiError(error, t('wallet.loadRefundsFailed')))
  } finally {
    loadingRefunds.value = false
  }
}

async function refreshRedeemSection() {
  await Promise.all([loadBalance(), loadOrders(), loadTransactions()])
}

async function submitRedeem() {
  if (!redeemForm.code.trim()) {
    showError(t('wallet.redeemRequired'))
    return
  }

  submittingRedeem.value = true
  try {
    latestRedeem.value = await walletApi.redeemCode({
      code: redeemForm.code.trim(),
    })
    redeemForm.code = ''
    success(t('wallet.redeemSuccess'))
    await Promise.all([loadBalance(), loadOrders(), loadTransactions(), loadTodayCost()])
    activeTab.value = 'orders'
  } catch (error) {
    log.error('兑换码充值失败:', error)
    showError(parseApiError(error, t('wallet.redeemFailed')))
  } finally {
    submittingRedeem.value = false
  }
}

async function submitRecharge() {
  if (!rechargeForm.amount_usd || rechargeForm.amount_usd <= 0) {
    showError(t('wallet.invalidRechargeAmount'))
    return
  }
  const option = selectedRechargeOption.value
  if (!option) {
    showError(t('wallet.paymentRequired'))
    return
  }
  if (option.min_recharge_usd && rechargeForm.amount_usd < option.min_recharge_usd) {
    showError(t('wallet.minimumRecharge', { amount: formatCurrency(option.min_recharge_usd) }))
    return
  }

  submittingRecharge.value = true
  try {
    latestRecharge.value = await walletApi.createRechargeOrder({
      amount_usd: rechargeForm.amount_usd,
      payment_method: option.payment_method,
      payment_provider: option.payment_provider,
      payment_channel: option.payment_channel,
    })
    success(t('wallet.rechargeCreated'))
    await Promise.all([loadOrders(), loadBalance()])
    activeTab.value = 'orders'
    submitPaymentInstructions(latestRecharge.value.payment_instructions)
  } catch (error) {
    log.error('创建充值订单失败:', error)
    showError(parseApiError(error, t('wallet.createRechargeFailed')))
  } finally {
    submittingRecharge.value = false
  }
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
    showError(t('wallet.popupBlocked'))
  }
}

function cancelLatestRecharge() {
  if (!latestRechargeCancelUrl.value) return
  const confirmed = window.confirm(t('wallet.cancelConfirm'))
  if (!confirmed) return
  window.location.href = latestRechargeCancelUrl.value
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

async function submitRefund() {
  if (!refundForm.amount_usd || refundForm.amount_usd <= 0) {
    showError(t('wallet.invalidRefundAmount'))
    return
  }
  const refundableBalance =
    walletBalance.value?.wallet?.refundable_balance ?? walletBalance.value?.refundable_balance ?? null
  if (refundableBalance !== null && refundForm.amount_usd > refundableBalance) {
    showError(t('wallet.refundExceeds', { amount: formatCurrency(refundableBalance) }))
    return
  }

  submittingRefund.value = true
  try {
    await walletApi.createRefund({
      amount_usd: refundForm.amount_usd,
      payment_order_id:
        refundForm.payment_order_id && refundForm.payment_order_id !== '__none__'
          ? refundForm.payment_order_id
          : undefined,
      refund_mode: refundForm.refund_mode || undefined,
      reason: refundForm.reason || undefined,
      idempotency_key: `web_refund_${buildRefundIdempotencyKey()}`,
    })
    success(t('wallet.refundSubmitted'))
    refundForm.amount_usd = 0
    refundForm.payment_order_id = '__none__'
    refundForm.reason = ''
    await Promise.all([loadRefunds(), loadBalance(), loadOrders(), loadTransactions(), loadTodayCost()])
    activeTab.value = 'refunds'
  } catch (error) {
    log.error('提交退款申请失败:', error)
    showError(parseApiError(error, t('wallet.submitRefundFailed')))
  } finally {
    submittingRefund.value = false
  }
}

function buildRefundIdempotencyKey(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID().replaceAll('-', '')
  }
  return `${Date.now()}_${Math.random().toString(16).slice(2, 10)}`
}

function handleTxPageChange(page: number) {
  txPage.value = page
  void loadTransactions()
}

function handleTxPageSizeChange(size: number) {
  txPageSize.value = size
  txPage.value = 1
  void loadTransactions()
}

function handleOrderPageChange(page: number) {
  orderPage.value = page
  void loadOrders()
}

function handleOrderPageSizeChange(size: number) {
  orderPageSize.value = size
  orderPage.value = 1
  void loadOrders()
}

function handleRefundPageChange(page: number) {
  refundPage.value = page
  void loadRefunds()
}

function handleRefundPageSizeChange(size: number) {
  refundPageSize.value = size
  refundPage.value = 1
  void loadRefunds()
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
