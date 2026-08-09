<template>
  <Teleport to="body">
    <Transition name="drawer">
      <div
        v-if="open && localWallet"
        class="fixed inset-0 z-[80] flex justify-end"
      >
        <div
          class="absolute inset-0 bg-black/35 backdrop-blur-sm"
          @click="handleClose"
        />

        <div class="drawer-panel relative h-full w-full sm:w-[760px] lg:w-[860px] sm:max-w-[95vw] border-l border-border bg-background shadow-2xl overflow-y-auto">
          <div class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur px-4 py-3 sm:px-6 sm:py-4">
            <div class="flex items-start justify-between gap-3">
              <div class="flex items-center gap-3 min-w-0">
                <div
                  class="flex h-10 w-10 items-center justify-center rounded-xl shrink-0"
                  :class="accentClasses"
                >
                  <Wallet class="h-5 w-5" />
                </div>
                <div class="min-w-0">
                  <div class="flex items-center gap-1.5">
                    <h3 class="text-lg font-semibold text-foreground leading-tight">
                      {{ contextLabel || t('walletOps.title') }}
                    </h3>
                    <Badge
                      :variant="walletStatusBadge(localWallet.status)"
                      class="w-fit px-2 py-0.5 text-[11px] leading-none"
                    >
                      {{ walletStatusLabel(localWallet.status) }}
                    </Badge>
                  </div>
                  <p class="text-xs text-muted-foreground">
                    {{ ownerName || '-' }} <span v-if="ownerSubtitle">· {{ ownerSubtitle }}</span>
                  </p>
                </div>
              </div>
              <Button
                variant="ghost"
                size="icon"
                class="h-9 w-9 shrink-0"
                :title="t('walletOps.close')"
                @click="handleClose"
              >
                <X class="h-4 w-4" />
              </Button>
            </div>
          </div>

          <div class="p-4 sm:p-6 space-y-5">
            <div class="rounded-2xl border border-border/60 bg-muted/30 p-4">
              <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                <div class="rounded-xl bg-background/80 p-3">
                  <div class="text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
                    {{ t('walletOps.totalAvailable') }}
                  </div>
                  <div
                    class="mt-1 text-lg font-semibold"
                    :class="totalAvailableAmount !== null && totalAvailableAmount < 0 ? 'text-rose-600' : 'text-foreground'"
                  >
                    {{ totalAvailableAmount === null ? t('walletOps.unlimited') : `$${formatFixed(totalAvailableAmount, 2)}` }}
                  </div>
                </div>
                <div class="rounded-xl bg-background/80 p-3">
                  <div class="text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
                    {{ t('walletOps.todayQuota') }}
                  </div>
                  <div class="mt-1 text-lg font-semibold text-foreground">
                    {{ isApiKeyWallet ? t('walletOps.notApplicable') : `$${formatFixed(packageBalanceAmount, 2)}` }}
                  </div>
                  <div
                    v-if="dailyQuota?.has_active"
                    class="mt-1 text-[11px] text-muted-foreground"
                  >
                    {{ t('walletOps.used') }} ${{ formatFixed(dailyQuota.used_usd, 2) }} / ${{ formatFixed(dailyQuota.total_usd, 2) }}
                  </div>
                </div>
                <div class="rounded-xl bg-background/80 p-3">
                  <div class="text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
                    {{ t('walletOps.walletBalance') }}
                  </div>
                  <div
                    class="mt-1 text-lg font-semibold"
                    :class="walletBalanceAmount < 0 ? 'text-rose-600' : 'text-foreground'"
                  >
                    ${{ formatFixed(walletBalanceAmount, 2) }}
                  </div>
                </div>
                <div class="rounded-xl bg-background/80 p-3">
                  <div class="text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
                    {{ t('walletOps.rechargeBalance') }}
                  </div>
                  <div class="mt-1 text-lg font-semibold text-foreground">
                    ${{ formatFixed(localWallet.recharge_balance, 2) }}
                  </div>
                </div>
                <div class="rounded-xl bg-background/80 p-3">
                  <div class="text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
                    {{ t('walletOps.giftBalance') }}
                  </div>
                  <div class="mt-1 text-lg font-semibold text-foreground">
                    {{ isApiKeyWallet ? t('walletOps.unsupported') : `$${formatFixed(localWallet.gift_balance, 2)}` }}
                  </div>
                </div>
                <div class="rounded-xl bg-background/80 p-3">
                  <div class="text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
                    {{ t('walletOps.totalConsumed') }}
                  </div>
                  <div class="mt-1 text-lg font-semibold text-foreground">
                    ${{ formatFixed(localWallet.total_consumed, 2) }}
                  </div>
                </div>
              </div>
              <p
                v-if="!isApiKeyWallet"
                class="mt-3 text-xs text-muted-foreground"
              >
                {{ t('walletOps.deductionHint') }}
              </p>
            </div>

            <Tabs v-model="activeTab">
              <TabsList :class="tabsListClass">
                <TabsTrigger value="actions">
                  {{ t('walletOps.actionsTab') }}
                </TabsTrigger>
                <TabsTrigger value="transactions">
                  {{ t('walletOps.transactionsTab') }}
                </TabsTrigger>
                <TabsTrigger
                  v-if="showUsageRecords"
                  value="usage"
                >
                  {{ t('walletOps.usageTab') }}
                </TabsTrigger>
                <TabsTrigger
                  v-if="showPlanRecords"
                  value="plans"
                >
                  {{ t('walletOps.plansTab') }}
                </TabsTrigger>
                <TabsTrigger
                  v-if="showRefunds"
                  value="refunds"
                >
                  {{ t('walletOps.refundsTab') }}
                </TabsTrigger>
              </TabsList>

              <TabsContent
                value="actions"
                class="mt-4 space-y-4"
              >
                <div
                  v-if="!isApiKeyWallet"
                  class="space-y-2"
                >
                  <Label class="text-sm font-medium">
                    {{ t('walletOps.actionType') }}
                  </Label>
                  <Select v-model="moneyActionType">
                    <SelectTrigger class="h-11">
                      <SelectValue :placeholder="t('walletOps.chooseAction')" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="recharge">
                        {{ t('walletOps.manualRecharge') }}
                      </SelectItem>
                      <SelectItem value="adjust">
                        {{ t('walletOps.manualAdjustment') }}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>

                <div class="space-y-2">
                  <Label
                    for="wallet-action-amount"
                    class="text-sm font-medium"
                  >{{ t('walletOps.amount') }}</Label>
                  <Input
                    id="wallet-action-amount"
                    :model-value="actionAmount ?? ''"
                    type="number"
                    step="0.01"
                    :placeholder="isApiKeyWallet || moneyActionType === 'adjust' ? t('walletOps.adjustAmountPlaceholder') : t('walletOps.rechargeAmountPlaceholder')"
                    class="h-11"
                    @update:model-value="(value) => actionAmount = parseNumberInput(value, { allowFloat: true })"
                  />
                  <p class="text-xs text-muted-foreground">
                    {{
                      isApiKeyWallet || moneyActionType === 'adjust'
                        ? t('walletOps.adjustAmountHint')
                        : t('walletOps.rechargeAmountHint')
                    }}
                  </p>
                </div>

                <div class="space-y-2">
                  <Label
                    for="wallet-action-description"
                    class="text-sm font-medium"
                  >{{ t('walletOps.description') }}</Label>
                  <Input
                    id="wallet-action-description"
                    v-model="actionDescription"
                    type="text"
                    :placeholder="t('walletOps.descriptionPlaceholder')"
                    class="h-11"
                  />
                </div>

                <div
                  v-if="moneyActionType === 'adjust' && !isApiKeyWallet"
                  class="space-y-2"
                >
                  <Label class="text-sm font-medium">
                    {{ t('walletOps.adjustAccount') }}
                  </Label>
                  <Select v-model="adjustBalanceType">
                    <SelectTrigger class="h-11">
                      <SelectValue :placeholder="t('walletOps.chooseAdjustAccount')" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="recharge">
                        {{ t('walletOps.refundableBalance') }}
                      </SelectItem>
                      <SelectItem
                        v-if="!isApiKeyWallet"
                        value="gift"
                      >
                        {{ t('walletOps.nonRefundableGift') }}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>

                <div
                  v-if="!isApiKeyWallet"
                  class="rounded-xl border border-border/60 p-3 text-xs text-muted-foreground"
                >
                  {{ t('walletOps.actionNotice') }}
                </div>

                <div class="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
                  <Button
                    variant="outline"
                    class="h-10 px-5"
                    @click="handleClose"
                  >
                    {{ t('walletOps.close') }}
                  </Button>
                  <Button
                    class="h-10 px-5"
                    :disabled="submitMoneyDisabled"
                    @click="submitMoneyAction"
                  >
                    {{ submittingMoneyAction ? t('walletOps.processing') : submitMoneyLabel }}
                  </Button>
                </div>
              </TabsContent>

              <TabsContent
                value="transactions"
                class="mt-4 space-y-3"
              >
                <div class="flex items-center justify-between gap-3">
                  <div class="text-sm text-muted-foreground">
                    {{ t('walletOps.totalRecords', { count: txTotal }) }}
                  </div>
                  <RefreshButton
                    :loading="loadingTx"
                    @click="loadTransactions"
                  />
                </div>

                <div class="rounded-2xl border border-border/60 overflow-hidden bg-background">
                  <div class="overflow-x-auto">
                    <Table class="w-full min-w-[890px] table-fixed">
                      <colgroup>
                        <col :style="{ width: walletTxColumnWidths.time }">
                        <col :style="{ width: walletTxColumnWidths.type }">
                        <col :style="{ width: walletTxColumnWidths.amount }">
                        <col :style="{ width: walletTxColumnWidths.balance }">
                        <col :style="{ width: walletTxColumnWidths.description }">
                      </colgroup>
                      <TableHeader>
                        <TableRow>
                          <SortableTableHead :sortable="false" resize-column-key="time" :resizable="true" @resize-start="handleWalletTxColumnResizeStart">
                            {{ t('walletOps.time') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="type" :resizable="true" @resize-start="handleWalletTxColumnResizeStart">
                            {{ t('walletOps.type') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="amount" :resizable="true" @resize-start="handleWalletTxColumnResizeStart">
                            {{ t('walletOps.amountColumn') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="balance" :resizable="true" @resize-start="handleWalletTxColumnResizeStart">
                            {{ t('walletOps.balanceChange') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="description" :resizable="true" @resize-start="handleWalletTxColumnResizeStart">
                            {{ t('walletOps.description') }}
                          </SortableTableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        <TableRow
                          v-for="tx in txItems"
                          :key="tx.id"
                        >
                          <TableCell class="text-xs text-muted-foreground whitespace-nowrap">
                            {{ formatDateTime(tx.created_at) }}
                          </TableCell>
                          <TableCell>
                            <div class="space-y-1">
                              <Badge
                                variant="outline"
                                class="font-mono"
                              >
                                {{ walletTransactionCategoryLabel(tx.category) }}
                              </Badge>
                              <div class="text-[11px] text-muted-foreground">
                                {{ walletTransactionReasonLabel(tx.reason_code) }}
                              </div>
                            </div>
                          </TableCell>
                          <TableCell
                            class="tabular-nums"
                            :class="toFiniteNumber(tx.amount) >= 0 ? 'text-emerald-600' : 'text-rose-600'"
                          >
                            {{ toFiniteNumber(tx.amount) >= 0 ? '+' : '' }}{{ formatFixed(tx.amount, 4) }}
                          </TableCell>
                          <TableCell class="text-xs tabular-nums whitespace-nowrap">
                            <div>{{ formatFixed(tx.balance_before, 4) }} → {{ formatFixed(tx.balance_after, 4) }}</div>
                            <div
                              v-if="tx.recharge_balance_before !== null && tx.recharge_balance_before !== undefined && tx.gift_balance_before !== null && tx.gift_balance_before !== undefined"
                              class="text-[11px] text-muted-foreground mt-0.5"
                            >
                              {{ t('walletOps.rechargeShort') }} {{ formatFixed(tx.recharge_balance_before, 4) }}→{{ formatFixed(tx.recharge_balance_after, 4) }}
                              · {{ t('walletOps.giftShort') }} {{ formatFixed(tx.gift_balance_before, 4) }}→{{ formatFixed(tx.gift_balance_after, 4) }}
                            </div>
                          </TableCell>
                          <TableCell
                            class="text-xs text-muted-foreground whitespace-pre-wrap break-words"
                            :title="tx.description || '-'"
                          >
                            {{ tx.description || '-' }}
                          </TableCell>
                        </TableRow>
                        <TableRow v-if="!loadingTx && txItems.length === 0">
                          <TableCell
                            colspan="5"
                            class="py-10"
                          >
                            <EmptyState
                              :title="t('walletOps.noTransactions')"
                              :description="t('walletOps.noTransactionsHint')"
                            />
                          </TableCell>
                        </TableRow>
                      </TableBody>
                    </Table>
                  </div>
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
                v-if="showUsageRecords"
                value="usage"
                class="mt-4 space-y-3"
              >
                <div class="flex items-center justify-between gap-3">
                  <div class="text-sm text-muted-foreground">
                    {{ t('walletOps.recentUsage', { count: usageTotal }) }}
                  </div>
                  <RefreshButton
                    :loading="loadingUsage"
                    @click="loadUsageRecords"
                  />
                </div>

                <div class="rounded-2xl border border-border/60 overflow-hidden bg-background">
                  <div class="overflow-x-auto">
                    <Table class="w-full min-w-[1120px] table-fixed">
                      <colgroup>
                        <col :style="{ width: walletUsageColumnWidths.time }">
                        <col :style="{ width: walletUsageColumnWidths.model }">
                        <col :style="{ width: walletUsageColumnWidths.status }">
                        <col :style="{ width: walletUsageColumnWidths.official }">
                        <col :style="{ width: walletUsageColumnWidths.packageDebit }">
                        <col :style="{ width: walletUsageColumnWidths.walletDebit }">
                        <col :style="{ width: walletUsageColumnWidths.platformCost }">
                        <col :style="{ width: walletUsageColumnWidths.costMultiplier }">
                      </colgroup>
                      <TableHeader>
                        <TableRow>
                          <SortableTableHead :sortable="false" resize-column-key="time" :resizable="true" @resize-start="handleWalletUsageColumnResizeStart">
                            {{ t('walletOps.time') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="model" :resizable="true" @resize-start="handleWalletUsageColumnResizeStart">
                            {{ t('walletOps.model') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="status" :resizable="true" @resize-start="handleWalletUsageColumnResizeStart">
                            {{ t('walletOps.status') }}
                          </SortableTableHead>
                          <SortableTableHead class="text-right" :sortable="false" align="right" resize-column-key="official" :resizable="true" @resize-start="handleWalletUsageColumnResizeStart">
                            {{ t('walletOps.officialPrice') }}
                          </SortableTableHead>
                          <SortableTableHead class="text-right" :sortable="false" align="right" resize-column-key="packageDebit" :resizable="true" @resize-start="handleWalletUsageColumnResizeStart">
                            {{ t('walletOps.packageDebit') }}
                          </SortableTableHead>
                          <SortableTableHead class="text-right" :sortable="false" align="right" resize-column-key="walletDebit" :resizable="true" @resize-start="handleWalletUsageColumnResizeStart">
                            {{ t('walletOps.walletDebit') }}
                          </SortableTableHead>
                          <SortableTableHead class="text-right" :sortable="false" align="right" resize-column-key="platformCost" :resizable="true" @resize-start="handleWalletUsageColumnResizeStart">
                            {{ t('walletOps.platformCost') }}
                          </SortableTableHead>
                          <SortableTableHead class="text-right" :sortable="false" align="right" resize-column-key="costMultiplier" :resizable="true" @resize-start="handleWalletUsageColumnResizeStart">
                            {{ t('walletOps.costMultiplier') }}
                          </SortableTableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        <TableRow
                          v-for="record in usageItems"
                          :key="record.id"
                          :title="usageCostTitle(record)"
                        >
                          <TableCell class="text-xs text-muted-foreground whitespace-nowrap">
                            {{ formatDateTime(record.created_at) }}
                          </TableCell>
                          <TableCell>
                            <div class="break-all text-sm font-medium text-foreground">
                              {{ record.model || '-' }}
                            </div>
                            <div class="mt-1 break-words text-[11px] text-muted-foreground">
                              {{ record.api_key?.name || record.api_key_name || t('walletOps.unnamedKey') }}
                            </div>
                          </TableCell>
                          <TableCell>
                            <Badge :variant="usageStatusBadge(record)">
                              {{ usageStatusLabel(record) }}
                            </Badge>
                          </TableCell>
                          <TableCell class="text-right text-xs tabular-nums text-primary">
                            {{ formatUsageCurrency(usageOfficialCost(record)) }}
                          </TableCell>
                          <TableCell class="text-right text-xs tabular-nums">
                            {{ formatUsageDebitWithMultiplier(usagePackageDebit(record), usagePackageMultiplier(record)) }}
                          </TableCell>
                          <TableCell class="text-right text-xs tabular-nums text-muted-foreground">
                            {{ formatUsageDebitWithMultiplier(usageWalletDebit(record), usageWalletMultiplier(record)) }}
                          </TableCell>
                          <TableCell class="text-right text-xs tabular-nums">
                            {{ hasUsagePlatformCost(record) ? formatUsageCurrency(usagePlatformCost(record)) : '-' }}
                          </TableCell>
                          <TableCell class="text-right text-xs tabular-nums text-muted-foreground">
                            {{ formatUsageCostMultiplier(record) }}
                          </TableCell>
                        </TableRow>
                        <TableRow v-if="!loadingUsage && usageItems.length === 0">
                          <TableCell
                            colspan="8"
                            class="py-10"
                          >
                            <EmptyState
                              :title="t('walletOps.noUsage')"
                              :description="t('walletOps.noUsageHint')"
                            />
                          </TableCell>
                        </TableRow>
                      </TableBody>
                    </Table>
                  </div>
                </div>

                <Pagination
                  :current="usagePage"
                  :total="usageTotal"
                  :page-size="usagePageSize"
                  @update:current="handleUsagePageChange"
                  @update:page-size="handleUsagePageSizeChange"
                />
              </TabsContent>

              <TabsContent
                v-if="showPlanRecords"
                value="plans"
                class="mt-4 space-y-3"
              >
                <div class="flex items-center justify-between gap-3">
                  <div class="text-sm text-muted-foreground">
                    {{ t('walletOps.totalRecords', { count: planItems.length }) }}
                  </div>
                  <RefreshButton
                    :loading="loadingPlans"
                    @click="loadPlans"
                  />
                </div>

                <div class="rounded-2xl border border-border/60 overflow-hidden bg-background">
                  <div class="overflow-x-auto">
                    <Table class="w-full min-w-[1170px] table-fixed">
                      <colgroup>
                        <col :style="{ width: walletPlanColumnWidths.plan }">
                        <col :style="{ width: walletPlanColumnWidths.status }">
                        <col :style="{ width: walletPlanColumnWidths.entitlements }">
                        <col :style="{ width: walletPlanColumnWidths.price }">
                        <col :style="{ width: walletPlanColumnWidths.created }">
                        <col :style="{ width: walletPlanColumnWidths.starts }">
                        <col :style="{ width: walletPlanColumnWidths.expires }">
                      </colgroup>
                      <TableHeader>
                        <TableRow>
                          <SortableTableHead :sortable="false" resize-column-key="plan" :resizable="true" @resize-start="handleWalletPlanColumnResizeStart">
                            {{ t('walletOps.plan') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="status" :resizable="true" @resize-start="handleWalletPlanColumnResizeStart">
                            {{ t('walletOps.status') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="entitlements" :resizable="true" @resize-start="handleWalletPlanColumnResizeStart">
                            {{ t('walletOps.benefits') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="price" :resizable="true" @resize-start="handleWalletPlanColumnResizeStart">
                            {{ t('walletOps.priceQuota') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="created" :resizable="true" @resize-start="handleWalletPlanColumnResizeStart">
                            {{ t('walletOps.obtainedAt') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="starts" :resizable="true" @resize-start="handleWalletPlanColumnResizeStart">
                            {{ t('walletOps.start') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="expires" :resizable="true" @resize-start="handleWalletPlanColumnResizeStart">
                            {{ t('walletOps.expiry') }}
                          </SortableTableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        <TableRow
                          v-for="plan in planItems"
                          :key="plan.id"
                        >
                          <TableCell>
                            <div class="break-words font-medium text-foreground">
                              {{ plan.plan_title || plan.plan?.title || plan.plan_id }}
                            </div>
                            <div class="mt-1 break-all text-[11px] text-muted-foreground">
                              {{ plan.payment_order_id || '-' }}
                            </div>
                          </TableCell>
                          <TableCell>
                            <Badge
                              :variant="plan.active ? 'success' : 'secondary'"
                              class="whitespace-nowrap"
                            >
                              {{ plan.active ? t('walletOps.active') : planStatusLabel(plan.status) }}
                            </Badge>
                          </TableCell>
                          <TableCell>
                            <div class="flex flex-wrap gap-1.5">
                              <Badge
                                v-for="label in entitlementLabels(plan.entitlements)"
                                :key="label"
                                variant="outline"
                                class="h-5 px-1.5 py-0 text-[10px]"
                              >
                                {{ label }}
                              </Badge>
                            </div>
                          </TableCell>
                          <TableCell class="text-xs text-muted-foreground whitespace-nowrap">
                            <div class="text-foreground">
                              {{ formatPlanPrice(plan.plan) }} / {{ formatPlanQuota(plan.plan) }}
                            </div>
                            <div class="mt-1">
                              {{ formatPlanEquivalentMultiplier(plan.plan) }}
                            </div>
                          </TableCell>
                          <TableCell class="text-xs text-muted-foreground whitespace-nowrap">
                            {{ formatDateTime(plan.created_at) }}
                          </TableCell>
                          <TableCell class="text-xs text-muted-foreground whitespace-nowrap">
                            {{ formatDateTime(plan.starts_at) }}
                          </TableCell>
                          <TableCell class="text-xs text-muted-foreground whitespace-nowrap">
                            {{ formatDateTime(plan.expires_at) }}
                          </TableCell>
                        </TableRow>
                        <TableRow v-if="!loadingPlans && planItems.length === 0">
                          <TableCell
                            colspan="7"
                            class="py-10"
                          >
                            <EmptyState
                              :title="t('walletOps.noPlans')"
                              :description="t('walletOps.noPlansHint')"
                            />
                          </TableCell>
                        </TableRow>
                      </TableBody>
                    </Table>
                  </div>
                </div>
              </TabsContent>

              <TabsContent
                v-if="showRefunds"
                value="refunds"
                class="mt-4 space-y-3"
              >
                <div class="flex items-center justify-between gap-3">
                  <div class="text-sm text-muted-foreground">
                    {{ t('walletOps.totalRecords', { count: refundTotal }) }}
                  </div>
                  <RefreshButton
                    :loading="loadingRefunds"
                    @click="loadRefunds"
                  />
                </div>

                <div
                  v-if="refundActionType && actionRefund"
                  class="rounded-xl border border-border/60 p-4 space-y-3"
                >
                  <div class="text-sm font-semibold">
                    {{ refundActionType === 'fail' ? t('walletOps.rejectRefund') : t('walletOps.completeRefund') }} - {{ actionRefund.refund_no }}
                  </div>
                  <template v-if="refundActionType === 'fail'">
                    <div class="space-y-1.5">
                      <Label>{{ t('walletOps.rejectReason') }}</Label>
                      <Input
                        v-model="refundFailReason"
                        :placeholder="t('walletOps.rejectReasonPlaceholder')"
                      />
                    </div>
                  </template>
                  <template v-else>
                    <div class="space-y-1.5">
                      <Label>{{ t('walletOps.gatewayRefundId') }}</Label>
                      <Input v-model="refundGatewayRefundId" />
                    </div>
                    <div class="space-y-1.5">
                      <Label>{{ t('walletOps.payoutReference') }}</Label>
                      <Input v-model="refundPayoutReference" />
                    </div>
                  </template>
                  <div class="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
                    <Button
                      variant="outline"
                      @click="resetRefundActionForm"
                    >
                      {{ t('walletOps.cancel') }}
                    </Button>
                    <Button
                      v-if="refundActionType === 'fail'"
                      variant="destructive"
                      :disabled="submittingRefundAction"
                      @click="submitFailRefund"
                    >
                      {{ submittingRefundAction ? t('walletOps.submitting') : t('walletOps.confirmReject') }}
                    </Button>
                    <Button
                      v-else
                      :disabled="submittingRefundAction"
                      @click="submitCompleteRefund"
                    >
                      {{ submittingRefundAction ? t('walletOps.submitting') : t('walletOps.confirmComplete') }}
                    </Button>
                  </div>
                </div>

                <div class="rounded-2xl border border-border/60 overflow-hidden bg-background">
                  <div class="overflow-x-auto">
                    <Table class="w-full min-w-[930px] table-fixed">
                      <colgroup>
                        <col :style="{ width: walletRefundColumnWidths.refundNo }">
                        <col :style="{ width: walletRefundColumnWidths.amount }">
                        <col :style="{ width: walletRefundColumnWidths.mode }">
                        <col :style="{ width: walletRefundColumnWidths.status }">
                        <col :style="{ width: walletRefundColumnWidths.reason }">
                        <col :style="{ width: walletRefundColumnWidths.actions }">
                      </colgroup>
                      <TableHeader>
                        <TableRow>
                          <SortableTableHead :sortable="false" resize-column-key="refundNo" :resizable="true" @resize-start="handleWalletRefundColumnResizeStart">
                            {{ t('walletOps.refundNo') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="amount" :resizable="true" @resize-start="handleWalletRefundColumnResizeStart">
                            {{ t('walletOps.amountColumn') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="mode" :resizable="true" @resize-start="handleWalletRefundColumnResizeStart">
                            {{ t('walletOps.mode') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="status" :resizable="true" @resize-start="handleWalletRefundColumnResizeStart">
                            {{ t('walletOps.status') }}
                          </SortableTableHead>
                          <SortableTableHead :sortable="false" resize-column-key="reason" :resizable="true" @resize-start="handleWalletRefundColumnResizeStart">
                            {{ t('walletOps.reason') }}
                          </SortableTableHead>
                          <SortableTableHead class="text-right" :sortable="false" align="right" resize-column-key="actions" :resizable="true" @resize-start="handleWalletRefundColumnResizeStart">
                            {{ t('walletOps.actions') }}
                          </SortableTableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        <TableRow
                          v-for="refund in refundItems"
                          :key="refund.id"
                        >
                          <TableCell
                            class="font-mono text-xs break-all"
                            :title="refund.refund_no"
                          >
                            {{ refund.refund_no }}
                          </TableCell>
                          <TableCell class="tabular-nums whitespace-nowrap">
                            ${{ formatFixed(refund.amount_usd, 4) }}
                          </TableCell>
                          <TableCell>
                            {{ refundModeLabel(refund.refund_mode) }}
                          </TableCell>
                          <TableCell>
                            <Badge :variant="refundStatusBadge(refund.status)">
                              {{ refundStatusLabel(refund.status) }}
                            </Badge>
                          </TableCell>
                          <TableCell
                            class="text-xs text-muted-foreground whitespace-pre-wrap break-words"
                            :title="refund.reason || refund.failure_reason || '-'"
                          >
                            {{ refund.reason || refund.failure_reason || '-' }}
                          </TableCell>
                          <TableCell class="text-right">
                            <div class="flex justify-end gap-2">
                              <Button
                                v-if="canProcessRefund(refund.status)"
                                size="sm"
                                variant="outline"
                                :disabled="submittingRefundAction"
                                @click="processRefund(refund)"
                              >
                                {{ t('walletOps.process') }}
                              </Button>
                              <Button
                                v-if="canCompleteRefund(refund.status)"
                                size="sm"
                                :disabled="submittingRefundAction"
                                @click="openCompleteRefund(refund)"
                              >
                                {{ t('walletOps.complete') }}
                              </Button>
                              <Button
                                v-if="canFailRefund(refund.status)"
                                size="sm"
                                variant="destructive"
                                :disabled="submittingRefundAction"
                                @click="openFailRefund(refund)"
                              >
                                {{ t('walletOps.reject') }}
                              </Button>
                            </div>
                          </TableCell>
                        </TableRow>
                        <TableRow v-if="!loadingRefunds && refundItems.length === 0">
                          <TableCell
                            colspan="6"
                            class="py-10"
                          >
                            <EmptyState
                              :title="t('walletOps.noRefunds')"
                              :description="t('walletOps.noRefundsHint')"
                            />
                          </TableCell>
                        </TableRow>
                      </TableBody>
                    </Table>
                  </div>
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
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  Badge,
  Button,
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
  TableHeader,
  TableRow,
  SortableTableHead,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from '@/components/ui'
import { EmptyState } from '@/components/common'
import {
  adminWalletApi,
  type AdminWallet,
} from '@/api/admin-wallets'
import { usageApi } from '@/api/usage'
import { usersApi, type AdminUserPlanEntitlement } from '@/api/users'
import type { BillingPlan, DailyQuotaEntitlement } from '@/api/billing'
import type { RefundRequest, WalletTransaction } from '@/api/wallet'
import type { UsageRecord } from '@/features/usage/types'
import {
  normalizeBillingEntitlements,
  quotaConsumptionMultiplierLabel,
  type BillingEntitlementsInput,
} from '@/utils/billingEntitlements'
import { parseApiError } from '@/utils/errorParser'
import { parseNumberInput } from '@/utils/form'
import { formatCurrency } from '@/utils/format'
import {
  refundModeLabel,
  refundStatusBadge,
  refundStatusLabel,
  walletStatusBadge,
  walletStatusLabel,
  walletTransactionCategoryLabel,
  walletTransactionReasonLabel,
} from '@/utils/walletDisplay'
import { useToast } from '@/composables/useToast'
import { useConfirm } from '@/composables/useConfirm'
import { useResizableTableColumns, type ResizableTableColumn } from '@/composables/useResizableTableColumns'
import { Wallet, X } from 'lucide-vue-next'
import { log } from '@/utils/logger'

const props = withDefaults(
  defineProps<{
    open: boolean
    wallet: AdminWallet | null
    ownerName?: string
    ownerSubtitle?: string
    contextLabel?: string
    userId?: string | null
    accent?: 'emerald' | 'blue'
    showRefunds?: boolean
  }>(),
  {
    ownerName: '',
    ownerSubtitle: '',
    contextLabel: '',
    userId: null,
    accent: 'emerald',
    showRefunds: true,
  }
)
const { t } = useI18n()

const emit = defineEmits<{
  close: []
  changed: []
}>()

const { success, error } = useToast()
const { confirm } = useConfirm()

const activeTab = ref<'actions' | 'transactions' | 'usage' | 'plans' | 'refunds'>('actions')
const localWallet = ref<AdminWallet | null>(null)

const moneyActionType = ref<'recharge' | 'adjust'>('adjust')
const actionAmount = ref<number | undefined>(undefined)
const actionDescription = ref('')
const adjustBalanceType = ref<'recharge' | 'gift'>('recharge')
const submittingMoneyAction = ref(false)

const loadingTx = ref(false)
const txItems = ref<WalletTransaction[]>([])
const txTotal = ref(0)
const txPage = ref(1)
const txPageSize = ref(20)
const loadingUsage = ref(false)
const usageItems = ref<UsageRecord[]>([])
const usageTotal = ref(0)
const usagePage = ref(1)
const usagePageSize = ref(20)

const loadingRefunds = ref(false)
const refundItems = ref<RefundRequest[]>([])
const refundTotal = ref(0)
const refundPage = ref(1)
const refundPageSize = ref(20)
const submittingRefundAction = ref(false)
const loadingPlans = ref(false)
const planItems = ref<AdminUserPlanEntitlement[]>([])

type WalletTxColumnKey = 'time' | 'type' | 'amount' | 'balance' | 'description'
const walletTxColumns: ResizableTableColumn<WalletTxColumnKey>[] = [
  { key: 'time', width: '150px', minWidth: 140 },
  { key: 'type', width: '150px', minWidth: 130 },
  { key: 'amount', width: '110px', minWidth: 100 },
  { key: 'balance', width: '220px', minWidth: 190 },
  { key: 'description', width: '260px', minWidth: 210 },
]
const {
  columnWidths: walletTxColumnWidths,
  startResize: handleWalletTxColumnResizeStart,
} = useResizableTableColumns<WalletTxColumnKey>({
  storageKey: 'wallet-drawer-transactions-table-column-widths',
  columns: walletTxColumns,
  defaultMinWidth: 90,
})

type WalletUsageColumnKey = 'time' | 'model' | 'status' | 'official' | 'packageDebit' | 'walletDebit' | 'platformCost' | 'costMultiplier'
const walletUsageColumns: ResizableTableColumn<WalletUsageColumnKey>[] = [
  { key: 'time', width: '150px', minWidth: 140 },
  { key: 'model', width: '240px', minWidth: 200 },
  { key: 'status', width: '110px', minWidth: 100 },
  { key: 'official', width: '120px', minWidth: 110 },
  { key: 'packageDebit', width: '130px', minWidth: 120 },
  { key: 'walletDebit', width: '130px', minWidth: 120 },
  { key: 'platformCost', width: '120px', minWidth: 110 },
  { key: 'costMultiplier', width: '120px', minWidth: 110 },
]
const {
  columnWidths: walletUsageColumnWidths,
  startResize: handleWalletUsageColumnResizeStart,
} = useResizableTableColumns<WalletUsageColumnKey>({
  storageKey: 'wallet-drawer-usage-table-column-widths',
  columns: walletUsageColumns,
  defaultMinWidth: 90,
})

type WalletPlanColumnKey = 'plan' | 'status' | 'entitlements' | 'price' | 'created' | 'starts' | 'expires'
const walletPlanColumns: ResizableTableColumn<WalletPlanColumnKey>[] = [
  { key: 'plan', width: '200px', minWidth: 170 },
  { key: 'status', width: '110px', minWidth: 100 },
  { key: 'entitlements', width: '260px', minWidth: 220 },
  { key: 'price', width: '150px', minWidth: 130 },
  { key: 'created', width: '150px', minWidth: 140 },
  { key: 'starts', width: '150px', minWidth: 140 },
  { key: 'expires', width: '150px', minWidth: 140 },
]
const {
  columnWidths: walletPlanColumnWidths,
  startResize: handleWalletPlanColumnResizeStart,
} = useResizableTableColumns<WalletPlanColumnKey>({
  storageKey: 'wallet-drawer-plans-table-column-widths',
  columns: walletPlanColumns,
  defaultMinWidth: 90,
})

type WalletRefundColumnKey = 'refundNo' | 'amount' | 'mode' | 'status' | 'reason' | 'actions'
const walletRefundColumns: ResizableTableColumn<WalletRefundColumnKey>[] = [
  { key: 'refundNo', width: '190px', minWidth: 160 },
  { key: 'amount', width: '110px', minWidth: 100 },
  { key: 'mode', width: '110px', minWidth: 100 },
  { key: 'status', width: '110px', minWidth: 100 },
  { key: 'reason', width: '240px', minWidth: 190 },
  { key: 'actions', width: '170px', minWidth: 150 },
]
const {
  columnWidths: walletRefundColumnWidths,
  startResize: handleWalletRefundColumnResizeStart,
} = useResizableTableColumns<WalletRefundColumnKey>({
  storageKey: 'wallet-drawer-refunds-table-column-widths',
  columns: walletRefundColumns,
  defaultMinWidth: 90,
})

const refundActionType = ref<'fail' | 'complete' | null>(null)
const actionRefund = ref<RefundRequest | null>(null)
const refundFailReason = ref('')
const refundGatewayRefundId = ref('')
const refundPayoutReference = ref('')

const accentClasses = computed(() => {
  return props.accent === 'blue' ? 'bg-blue-500/10 text-blue-600' : 'bg-emerald-500/10 text-emerald-600'
})
const isApiKeyWallet = computed(() => localWallet.value?.owner_type === 'api_key')
const dailyQuota = computed(() => localWallet.value?.daily_quota ?? null)
const packageBalanceAmount = computed(() => toFiniteNumber(localWallet.value?.package_balance, 0))
const walletBalanceAmount = computed(() => toFiniteNumber(
  localWallet.value?.actual_wallet_balance
    ?? localWallet.value?.wallet_balance
    ?? localWallet.value?.balance,
  0,
))
const totalAvailableAmount = computed(() => {
  if (!localWallet.value) return 0
  if (localWallet.value.unlimited || localWallet.value.total_available_balance === null) return null
  return toFiniteNumber(
    localWallet.value.total_available_balance,
    walletBalanceAmount.value + packageBalanceAmount.value
  )
})
const showRefunds = computed(() => props.showRefunds)
const showPlanRecords = computed(() => Boolean(props.userId && !isApiKeyWallet.value))
const showUsageRecords = computed(() => Boolean(props.userId && !isApiKeyWallet.value))
const tabsListClass = computed(() => {
  const columnCount = 2 + (showUsageRecords.value ? 1 : 0) + (showPlanRecords.value ? 1 : 0) + (showRefunds.value ? 1 : 0)
  return [
    'tabs-button-list',
    'grid',
    'w-full',
    'grid-cols-2',
    columnCount === 5 ? 'sm:grid-cols-5' : columnCount === 4 ? 'sm:grid-cols-4' : columnCount === 3 ? 'sm:grid-cols-3' : 'sm:grid-cols-2',
  ]
})
const submitMoneyDisabled = computed(() => {
  if (submittingMoneyAction.value) return true
  if (!actionAmount.value) return true
  if (moneyActionType.value === 'recharge') {
    return actionAmount.value <= 0
  }
  return actionAmount.value === 0
})
const submitMoneyLabel = computed(() => {
  if (isApiKeyWallet.value) return t('walletOps.confirmAdjustment')
  return moneyActionType.value === 'recharge' ? t('walletOps.confirmRecharge') : t('walletOps.confirmAdjustment')
})

watch(
  () => [props.open, props.wallet?.id, props.userId] as const,
  async ([open]) => {
    if (!open || !props.wallet) {
      return
    }
    localWallet.value = { ...props.wallet }
    resetActionForm()
    resetRefundActionForm()
    activeTab.value = 'actions'
    txPage.value = 1
    usagePage.value = 1
    refundPage.value = 1
    usageItems.value = []
    usageTotal.value = 0
    planItems.value = []
    await refreshDrawerData()
  },
  { immediate: true }
)

function handleClose() {
  emit('close')
}

function resetActionForm() {
  moneyActionType.value = isApiKeyWallet.value ? 'adjust' : 'recharge'
  actionAmount.value = undefined
  actionDescription.value = ''
  adjustBalanceType.value = 'recharge'
}

watch(
  () => [moneyActionType.value, isApiKeyWallet.value] as const,
  () => {
    if (isApiKeyWallet.value && moneyActionType.value !== 'adjust') {
      moneyActionType.value = 'adjust'
      return
    }
    if (moneyActionType.value !== 'adjust') {
      adjustBalanceType.value = 'recharge'
      return
    }
    if (isApiKeyWallet.value && adjustBalanceType.value === 'gift') {
      adjustBalanceType.value = 'recharge'
    }
  }
)

function resetRefundActionForm() {
  refundActionType.value = null
  actionRefund.value = null
  refundFailReason.value = ''
  refundGatewayRefundId.value = ''
  refundPayoutReference.value = ''
}

async function loadTransactions() {
  if (!localWallet.value) return
  loadingTx.value = true
  try {
    const offset = (txPage.value - 1) * txPageSize.value
    const resp = await adminWalletApi.getWalletTransactions(localWallet.value.id, {
      limit: txPageSize.value,
      offset,
    })
    localWallet.value = resp.wallet
    txItems.value = resp.items
    txTotal.value = resp.total
  } catch (err) {
    log.error('加载钱包流水失败:', err)
    error(parseApiError(err, t('walletOps.loadTransactionsFailed')))
  } finally {
    loadingTx.value = false
  }
}

function usageDateRangeParams(): { preset: string; timezone: string; tz_offset_minutes: number } {
  return {
    preset: 'last30days',
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    tz_offset_minutes: -new Date().getTimezoneOffset(),
  }
}

async function loadUsageRecords() {
  if (!props.userId || !showUsageRecords.value) {
    usageItems.value = []
    usageTotal.value = 0
    return
  }
  loadingUsage.value = true
  try {
    const offset = (usagePage.value - 1) * usagePageSize.value
    const resp = await usageApi.getAllUsageRecords({
      user_id: props.userId,
      limit: usagePageSize.value,
      offset,
      ...usageDateRangeParams(),
    })
    usageItems.value = (resp.records || []) as unknown as UsageRecord[]
    usageTotal.value = resp.total || 0
  } catch (err) {
    log.error('加载消费记录失败:', err)
    error(parseApiError(err, t('walletOps.loadUsageFailed')))
    usageItems.value = []
    usageTotal.value = 0
  } finally {
    loadingUsage.value = false
  }
}

async function loadPlans() {
  if (!props.userId || !showPlanRecords.value) {
    planItems.value = []
    return
  }
  loadingPlans.value = true
  try {
    const resp = await usersApi.listUserPlanEntitlements(props.userId)
    planItems.value = resp.items
  } catch (err) {
    log.error('加载套餐记录失败:', err)
    error(parseApiError(err, t('walletOps.loadPlansFailed')))
    planItems.value = []
  } finally {
    loadingPlans.value = false
  }
}

async function loadRefunds() {
  if (!showRefunds.value || !localWallet.value) {
    refundItems.value = []
    refundTotal.value = 0
    return
  }
  loadingRefunds.value = true
  try {
    const offset = (refundPage.value - 1) * refundPageSize.value
    const resp = await adminWalletApi.getWalletRefunds(localWallet.value.id, {
      limit: refundPageSize.value,
      offset,
    })
    localWallet.value = resp.wallet
    refundItems.value = resp.items
    refundTotal.value = resp.total
    if (actionRefund.value) {
      const latest = refundItems.value.find((item) => item.id === actionRefund.value?.id)
      if (latest) actionRefund.value = latest
    }
  } catch (err) {
    log.error('加载钱包退款失败:', err)
    error(parseApiError(err, t('walletOps.loadRefundsFailed')))
  } finally {
    loadingRefunds.value = false
  }
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

function handleUsagePageChange(page: number) {
  if (!showUsageRecords.value) return
  usagePage.value = page
  void loadUsageRecords()
}

function handleUsagePageSizeChange(size: number) {
  if (!showUsageRecords.value) return
  usagePageSize.value = size
  usagePage.value = 1
  void loadUsageRecords()
}

function handleRefundPageChange(page: number) {
  if (!showRefunds.value) return
  refundPage.value = page
  void loadRefunds()
}

function handleRefundPageSizeChange(size: number) {
  if (!showRefunds.value) return
  refundPageSize.value = size
  refundPage.value = 1
  void loadRefunds()
}

async function refreshDrawerData() {
  const tasks: Array<Promise<void>> = [loadTransactions()]
  if (showUsageRecords.value) {
    tasks.push(loadUsageRecords())
  }
  if (showPlanRecords.value) {
    tasks.push(loadPlans())
  }
  if (showRefunds.value) {
    tasks.push(loadRefunds())
  }
  await Promise.all(tasks)
}

async function submitRecharge() {
  if (!localWallet.value) return
  if (!actionAmount.value || actionAmount.value <= 0) {
    error(t('walletOps.rechargePositive'))
    return
  }

  const rechargeBefore = localWallet.value.recharge_balance
  const rechargeAfter = rechargeBefore + actionAmount.value
  const totalBefore = localWallet.value.balance
  const totalAfter = totalBefore + actionAmount.value
  const confirmed = await confirm({
    title: t('walletOps.confirmManualRecharge'),
    message: t('walletOps.rechargeConfirmMessage', { owner: props.ownerName || t('walletOps.thisWallet'), amount: formatFixed(actionAmount.value, 4), rechargeBefore: formatFixed(rechargeBefore, 4), rechargeAfter: formatFixed(rechargeAfter, 4), totalBefore: formatFixed(totalBefore, 4), totalAfter: formatFixed(totalAfter, 4) }),
    confirmText: t('walletOps.confirmRecharge'),
    variant: 'warning',
  })
  if (!confirmed) return

  submittingMoneyAction.value = true
  try {
    const response = await adminWalletApi.rechargeWallet(localWallet.value.id, {
      amount_usd: actionAmount.value,
      payment_method: 'admin_manual',
      description: actionDescription.value || t('walletOps.defaultRechargeDescription', { owner: props.ownerName || t('walletOps.wallet') }),
    })
    localWallet.value = response.wallet
    success(t('walletOps.rechargeSuccess'))
    resetActionForm()
    await refreshDrawerData()
    emit('changed')
  } catch (err) {
    log.error('钱包人工充值失败:', err)
    error(parseApiError(err, t('walletOps.rechargeFailed')))
  } finally {
    submittingMoneyAction.value = false
  }
}

function previewAdjustResult(
  rechargeBefore: number,
  giftBefore: number,
  amount: number,
  balanceType: 'recharge' | 'gift'
) {
  let rechargeAfter = rechargeBefore
  let giftAfter = giftBefore

  if (amount > 0) {
    if (balanceType === 'gift') {
      giftAfter += amount
    } else {
      rechargeAfter += amount
    }
    return {
      rechargeAfter,
      giftAfter,
      totalAfter: rechargeAfter + giftAfter,
    }
  }

  let remaining = Math.abs(amount)
  const consumePositiveBucket = (value: number) => {
    const available = Math.max(value, 0)
    const used = Math.min(available, remaining)
    remaining -= used
    return value - used
  }

  if (balanceType === 'gift') {
    giftAfter = consumePositiveBucket(giftAfter)
    rechargeAfter = consumePositiveBucket(rechargeAfter)
  } else {
    rechargeAfter = consumePositiveBucket(rechargeAfter)
    giftAfter = consumePositiveBucket(giftAfter)
  }

  if (remaining > 0) {
    rechargeAfter -= remaining
  }

  return {
    rechargeAfter,
    giftAfter,
    totalAfter: rechargeAfter + giftAfter,
  }
}

async function submitAdjust() {
  if (!localWallet.value) return
  if (!actionAmount.value || actionAmount.value === 0) {
    error(t('walletOps.adjustNonZero'))
    return
  }

  if (isApiKeyWallet.value && adjustBalanceType.value === 'gift') {
    error(t('walletOps.keyWalletNoGift'))
    return
  }

  const rechargeBefore = localWallet.value.recharge_balance
  const giftBefore = localWallet.value.gift_balance
  const currentBucketBalance = adjustBalanceType.value === 'gift' ? giftBefore : rechargeBefore
  const preview = previewAdjustResult(
    rechargeBefore,
    giftBefore,
    actionAmount.value,
    adjustBalanceType.value
  )
  const afterBalance = adjustBalanceType.value === 'gift' ? preview.giftAfter : preview.rechargeAfter
  const totalBefore = localWallet.value.balance
  const totalAfter = preview.totalAfter
  const balanceTypeLabel = adjustBalanceType.value === 'gift' ? t('walletOps.giftBalance') : t('walletOps.rechargeBalance')
  const isDeduct = actionAmount.value < 0
  const detailLine = isDeduct
    ? t('walletOps.adjustDeductDetail', { rechargeBefore: formatFixed(rechargeBefore, 4), rechargeAfter: formatFixed(preview.rechargeAfter, 4), giftBefore: formatFixed(giftBefore, 4), giftAfter: formatFixed(preview.giftAfter, 4) })
    : t('walletOps.adjustAddDetail', { type: balanceTypeLabel, before: formatFixed(currentBucketBalance, 4), after: formatFixed(afterBalance, 4) })
  const confirmed = await confirm({
    title: t('walletOps.confirmWalletAdjustment'),
    message: t('walletOps.adjustConfirmMessage', { owner: props.ownerName || t('walletOps.thisWallet'), type: balanceTypeLabel, action: actionAmount.value > 0 ? t('walletOps.increase') : t('walletOps.decrease'), amount: formatFixed(Math.abs(actionAmount.value), 4), detail: detailLine, totalBefore: formatFixed(totalBefore, 4), totalAfter: formatFixed(totalAfter, 4) }),
    confirmText: t('walletOps.confirmAdjustment'),
    variant: 'warning',
  })
  if (!confirmed) return

  submittingMoneyAction.value = true
  try {
    const response = await adminWalletApi.adjustWallet(localWallet.value.id, {
      amount_usd: actionAmount.value,
      balance_type: adjustBalanceType.value,
      description: actionDescription.value || t('walletOps.defaultAdjustDescription', { owner: props.ownerName || t('walletOps.wallet') }),
    })
    localWallet.value = response.wallet
    success(t('walletOps.adjustSuccess'))
    resetActionForm()
    await refreshDrawerData()
    emit('changed')
  } catch (err) {
    log.error('钱包调账失败:', err)
    error(parseApiError(err, t('walletOps.adjustFailed')))
  } finally {
    submittingMoneyAction.value = false
  }
}

async function submitMoneyAction() {
  if (!isApiKeyWallet.value && moneyActionType.value === 'recharge') {
    await submitRecharge()
    return
  }
  await submitAdjust()
}

function canProcessRefund(status: string) {
  return status === 'pending_approval' || status === 'approved'
}

function canFailRefund(status: string) {
  return status === 'processing' || status === 'pending_approval' || status === 'approved'
}

function canCompleteRefund(status: string) {
  return status === 'processing'
}

async function processRefund(refund: RefundRequest) {
  if (!localWallet.value) return
  submittingRefundAction.value = true
  try {
    const resp = await adminWalletApi.processRefund(localWallet.value.id, refund.id)
    localWallet.value = resp.wallet
    success(t('walletOps.refundProcessing'))
    await refreshDrawerData()
    emit('changed')
  } catch (err) {
    log.error('处理退款失败:', err)
    error(parseApiError(err, t('walletOps.processRefundFailed')))
  } finally {
    submittingRefundAction.value = false
  }
}

function openFailRefund(refund: RefundRequest) {
  refundActionType.value = 'fail'
  actionRefund.value = refund
  refundFailReason.value = ''
}

function openCompleteRefund(refund: RefundRequest) {
  refundActionType.value = 'complete'
  actionRefund.value = refund
  refundGatewayRefundId.value = ''
  refundPayoutReference.value = ''
}

async function submitFailRefund() {
  if (!localWallet.value || !actionRefund.value) return
  if (!refundFailReason.value.trim()) {
    error(t('walletOps.rejectReasonRequired'))
    return
  }

  submittingRefundAction.value = true
  try {
    const resp = await adminWalletApi.failRefund(localWallet.value.id, actionRefund.value.id, {
      reason: refundFailReason.value.trim(),
    })
    localWallet.value = resp.wallet
    success(t('walletOps.refundRejected'))
    resetRefundActionForm()
    await refreshDrawerData()
    emit('changed')
  } catch (err) {
    log.error('驳回退款失败:', err)
    error(parseApiError(err, t('walletOps.rejectRefundFailed')))
  } finally {
    submittingRefundAction.value = false
  }
}

async function submitCompleteRefund() {
  if (!localWallet.value || !actionRefund.value) return

  submittingRefundAction.value = true
  try {
    await adminWalletApi.completeRefund(localWallet.value.id, actionRefund.value.id, {
      gateway_refund_id: refundGatewayRefundId.value || undefined,
      payout_reference: refundPayoutReference.value || undefined,
    })
    success(t('walletOps.refundCompleted'))
    resetRefundActionForm()
    await refreshDrawerData()
    emit('changed')
  } catch (err) {
    log.error('完成退款失败:', err)
    error(parseApiError(err, t('walletOps.completeRefundFailed')))
  } finally {
    submittingRefundAction.value = false
  }
}

function finiteNumber(value: number | null | undefined): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

const USAGE_COST_EPSILON = 0.0000001

interface UsageChargeBreakdownView {
  officialCost: number
  packageDebit: number
  packageMultiplier: number | null
  walletDebit: number
  walletMultiplier: number | null
  userDebit: number
}

function usageSalesMultiplier(record: UsageRecord): number | null {
  return finiteNumber(record.sales_multiplier)
}

function resolveUsageChargeBreakdown(record: UsageRecord): UsageChargeBreakdownView {
  const rawBreakdown = record.charge_breakdown
  const salesMultiplier = usageSalesMultiplier(record)
  const recordCost = finiteNumber(record.cost) ?? 0
  let officialCost = finiteNumber(rawBreakdown?.official_cost) ?? finiteNumber(record.official_cost)
  if (officialCost === null && salesMultiplier !== null && salesMultiplier > 0) {
    officialCost = recordCost / salesMultiplier
  } else if (officialCost === null) {
    officialCost = recordCost
  }
  const resolvedOfficialCost = officialCost ?? recordCost
  const packageDebit = Math.max(finiteNumber(rawBreakdown?.package_debit) ?? 0, 0)
  const hasBreakdown = rawBreakdown !== null && rawBreakdown !== undefined
  const walletDebit = Math.max(
    finiteNumber(rawBreakdown?.wallet_debit) ?? (hasBreakdown ? 0 : recordCost),
    0,
  )
  const userDebit = Math.max(
    finiteNumber(rawBreakdown?.user_debit) ?? (packageDebit + walletDebit),
    0,
  )
  const packageMultiplier = finiteNumber(rawBreakdown?.package_multiplier)
    ?? (packageDebit > USAGE_COST_EPSILON ? 1 : null)
  const walletMultiplier = finiteNumber(rawBreakdown?.wallet_multiplier)
    ?? salesMultiplier
    ?? (resolvedOfficialCost > USAGE_COST_EPSILON && walletDebit > USAGE_COST_EPSILON
      ? walletDebit / resolvedOfficialCost
      : null)

  return {
    officialCost: resolvedOfficialCost,
    packageDebit,
    packageMultiplier,
    walletDebit,
    walletMultiplier,
    userDebit,
  }
}

function usageUserCharge(record: UsageRecord): number {
  return resolveUsageChargeBreakdown(record).userDebit
}

function usageOfficialCost(record: UsageRecord): number {
  return resolveUsageChargeBreakdown(record).officialCost
}

function usagePackageDebit(record: UsageRecord): number {
  return resolveUsageChargeBreakdown(record).packageDebit
}

function usagePackageMultiplier(record: UsageRecord): number | null {
  return resolveUsageChargeBreakdown(record).packageMultiplier
}

function usageWalletDebit(record: UsageRecord): number {
  return resolveUsageChargeBreakdown(record).walletDebit
}

function usageWalletMultiplier(record: UsageRecord): number | null {
  return resolveUsageChargeBreakdown(record).walletMultiplier
}

function hasUsagePlatformCost(record: UsageRecord): boolean {
  return finiteNumber(record.actual_cost) !== null
}

function usagePlatformCost(record: UsageRecord): number {
  return finiteNumber(record.actual_cost) ?? usageOfficialCost(record)
}

function usageCostMultiplier(record: UsageRecord): number | null {
  const saved = finiteNumber(record.rate_multiplier)
  if (saved !== null) return saved
  const official = usageOfficialCost(record)
  if (official <= 0 || !hasUsagePlatformCost(record)) return null
  return usagePlatformCost(record) / official
}

function formatMultiplier(value: number | null): string {
  if (value === null) return '-'
  return `${value.toFixed(4).replace(/0+$/, '').replace(/\.$/, '')}x`
}

function formatUsageCostMultiplier(record: UsageRecord): string {
  return formatMultiplier(usageCostMultiplier(record))
}

function formatUsageCurrency(value: number): string {
  return formatCurrency(value)
}

function formatUsageDebitWithMultiplier(amount: number, multiplier: number | null): string {
  if (amount <= USAGE_COST_EPSILON) return '-'
  return `${formatUsageCurrency(amount)} · ${formatMultiplier(multiplier)}`
}

function usageCostTitle(record: UsageRecord): string {
  const packageDebit = usagePackageDebit(record)
  const walletDebit = usageWalletDebit(record)
  const lines = [
    `${t('walletOps.officialPrice')}: ${formatUsageCurrency(usageOfficialCost(record))}`,
  ]
  if (packageDebit > USAGE_COST_EPSILON) {
    lines.push(`${t('walletOps.packageDebit')}: ${formatUsageDebitWithMultiplier(packageDebit, usagePackageMultiplier(record))}`)
  }
  if (walletDebit > USAGE_COST_EPSILON) {
    lines.push(`${t('walletOps.walletDebit')}: ${formatUsageDebitWithMultiplier(walletDebit, usageWalletMultiplier(record))}`)
  }
  if (packageDebit <= USAGE_COST_EPSILON && walletDebit <= USAGE_COST_EPSILON) {
    lines.push(t('walletOps.noUserCharge'))
  }
  if (hasUsagePlatformCost(record)) {
    lines.push(`${t('walletOps.platformCost')}: ${formatUsageCurrency(usagePlatformCost(record))}`)
    lines.push(`${t('walletOps.costMultiplier')}: ${formatUsageCostMultiplier(record)}`)
  }
  return lines.join('\n')
}

function usageStatusLabel(record: UsageRecord): string {
  if (record.status === 'failed' || (record.status_code ?? 0) >= 400) return t('walletOps.failed')
  if (record.status === 'cancelled') return t('walletOps.cancelled')
  if (record.status === 'pending') return t('walletOps.pending')
  if (record.status === 'streaming') return t('walletOps.streaming')
  return t('walletOps.complete')
}

function usageStatusBadge(record: UsageRecord): 'default' | 'secondary' | 'outline' | 'destructive' | 'success' {
  if (record.status === 'failed' || (record.status_code ?? 0) >= 400) return 'destructive'
  if (record.status === 'cancelled') return 'outline'
  if (record.status === 'pending' || record.status === 'streaming') return 'secondary'
  return 'success'
}

function formatDateTime(value: string | null | undefined) {
  if (!value) return '-'
  return new Date(value).toLocaleString('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function planStatusLabel(status: string | null | undefined): string {
  const labels: Record<string, string> = {
    active: t('walletOps.granted'),
    cancelled: t('walletOps.cancelled'),
    expired: t('walletOps.expired'),
  }
  return labels[String(status || '')] || String(status || '-')
}

function planQuotaValues(plan: BillingPlan | null | undefined): number[] {
  if (!plan) return []
  return normalizeBillingEntitlements(plan.entitlements)
    .filter((item): item is DailyQuotaEntitlement => item.type === 'daily_quota')
    .flatMap((item) => {
      const limits = item.limits || {}
      return [
        item.daily_quota_usd ?? limits.daily_limit_usd,
        item.five_hour_quota_usd ?? limits.five_hour_limit_usd,
        item.weekly_quota_usd ?? limits.weekly_limit_usd,
        item.monthly_quota_usd ?? limits.monthly_limit_usd,
      ]
    })
    .map((value) => Number(value))
    .filter((value) => Number.isFinite(value) && value > 0)
}

function planQuotaUsd(plan: BillingPlan | null | undefined): number | null {
  const values = planQuotaValues(plan)
  if (values.length === 0) return null
  return Math.max(...values)
}

function formatPlanPrice(plan: BillingPlan | null | undefined): string {
  if (!plan) return '-'
  return `${Number(plan.price_amount || 0).toFixed(2)} ${plan.price_currency || 'CNY'}`
}

function formatPlanQuota(plan: BillingPlan | null | undefined): string {
  const quota = planQuotaUsd(plan)
  return quota === null ? t('walletOps.noQuota') : `$${quota.toFixed(2)}`
}

function formatPlanEquivalentMultiplier(plan: BillingPlan | null | undefined): string {
  const quota = planQuotaUsd(plan)
  if (!plan || quota === null || quota <= 0) return t('walletOps.equivalentMultiplierEmpty')
  const price = Number(plan.price_amount || 0)
  return t('walletOps.equivalentMultiplier', { value: formatMultiplier(price / quota) })
}

function entitlementLabels(items: BillingEntitlementsInput): string[] {
  return normalizeBillingEntitlements(items).map((item) => {
    if (item.type === 'wallet_credit') {
      return t('walletOps.bonusBalance', { amount: Number(item.amount_usd || 0).toFixed(2) })
    }
    if (item.type === 'daily_quota') {
      return quotaEntitlementLabel(item)
    }
    if (item.type === 'membership_group') {
      return t('walletOps.membershipBenefits')
    }
    return t('walletOps.unknownBenefit')
  })
}

function quotaEntitlementLabel(item: DailyQuotaEntitlement): string {
  const limits = item.limits || {}
  const parts = []
  const daily = Number(item.daily_quota_usd ?? limits.daily_limit_usd ?? 0)
  const fiveHour = Number(item.five_hour_quota_usd ?? limits.five_hour_limit_usd ?? 0)
  const weekly = Number(item.weekly_quota_usd ?? limits.weekly_limit_usd ?? 0)
  const monthly = Number(item.monthly_quota_usd ?? limits.monthly_limit_usd ?? 0)
  if (daily > 0) parts.push(t('walletOps.dailyQuota', { amount: daily.toFixed(2) }))
  if (fiveHour > 0) parts.push(t('walletOps.fiveHourQuota', { amount: fiveHour.toFixed(2) }))
  if (weekly > 0) parts.push(t('walletOps.weeklyQuota', { amount: weekly.toFixed(2) }))
  if (monthly > 0) parts.push(t('walletOps.monthlyQuota', { amount: monthly.toFixed(2) }))
  const quotaText = parts.join(' / ') || t('walletOps.usageQuota')
  const modelIds = item.allowed_global_model_ids || []
  const labels = [modelIds.length > 0 ? t('walletOps.modelCount', { count: modelIds.length }) : t('walletOps.allModels')]
  const multiplierLabel = quotaConsumptionMultiplierLabel(item)
  if (multiplierLabel) labels.push(multiplierLabel)
  return `${quotaText} · ${labels.join(' · ')}`
}

function toFiniteNumber(value: unknown, fallback = 0): number {
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed : fallback
}

function formatFixed(value: unknown, digits: number): string {
  return toFiniteNumber(value).toFixed(digits)
}
</script>

<style scoped>
.drawer-enter-active,
.drawer-leave-active {
  transition: opacity 0.3s ease;
}

.drawer-enter-active .drawer-panel,
.drawer-leave-active .drawer-panel {
  transition: transform 0.3s ease;
}

.drawer-enter-from,
.drawer-leave-to {
  opacity: 0;
}

.drawer-enter-from .drawer-panel,
.drawer-leave-to .drawer-panel {
  transform: translateX(100%);
}
</style>
