<template>
  <PageContainer padding="lg">
    <PageHeader
      :title="t('billingPlansManagement.title')"
      :description="t('billingPlansManagement.description')"
    >
      <template #actions>
        <Button
          size="sm"
          @click="openCreateDialog"
        >
          <Plus class="mr-2 h-4 w-4" />
          {{ t('billingPlansManagement.create') }}
        </Button>
      </template>
    </PageHeader>

    <div class="mt-6 space-y-6">
      <div
        v-if="loading"
        class="py-16"
      >
        <LoadingState :message="t('billingPlansManagement.loading')" />
      </div>

      <CardSection
        v-else
        :title="t('billingPlansManagement.list')"
        :description="t('billingPlansManagement.listHint')"
      >
        <div
          v-if="plans.length === 0"
          class="py-12"
        >
          <EmptyState
            :title="t('billingPlansManagement.empty')"
            :description="t('billingPlansManagement.emptyHint')"
          />
        </div>

        <div
          v-else
          class="overflow-x-auto"
        >
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead class="w-[24%]">
                  {{ t('billingPlansManagement.plan') }}
                </TableHead>
                <TableHead class="w-[10%] whitespace-nowrap">
                  {{ t('billingPlansManagement.price') }}
                </TableHead>
                <TableHead class="w-[18%] whitespace-nowrap">
                  {{ t('billingPlansManagement.duration') }}
                </TableHead>
                <TableHead class="w-[20%]">
                  {{ t('billingPlansManagement.benefits') }}
                </TableHead>
                <TableHead class="w-[6%] whitespace-nowrap text-center">
                  {{ t('billingPlansManagement.order') }}
                </TableHead>
                <TableHead class="w-[7%] whitespace-nowrap text-center">
                  {{ t('billingPlansManagement.status') }}
                </TableHead>
                <TableHead class="w-[15%] whitespace-nowrap text-right">
                  {{ t('billingPlansManagement.actions') }}
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableRow
                v-for="plan in plans"
                :key="plan.id"
              >
                <TableCell>
                  <div class="text-sm font-medium">
                    {{ plan.title }}
                  </div>
                  <div
                    v-if="plan.description"
                    class="mt-0.5 max-w-[280px] truncate text-xs text-muted-foreground"
                  >
                    {{ plan.description }}
                  </div>
                </TableCell>
                <TableCell class="whitespace-nowrap text-sm tabular-nums">
                  {{ formatPlanPriceAmount(plan) }} {{ plan.price_currency }}
                </TableCell>
                <TableCell>
                  <div class="whitespace-nowrap text-sm">
                    {{ formatPlanPeriod(plan) }}
                  </div>
                  <div class="mt-0.5 text-xs text-muted-foreground">
                    {{ planDurationHint(plan) }}
                  </div>
                </TableCell>
                <TableCell>
                  <div class="flex flex-wrap items-center gap-1">
                    <span
                      v-for="item in entitlementBadges(plan)"
                      :key="item"
                      class="inline-flex whitespace-nowrap rounded-md border border-border/60 bg-muted/40 px-2 py-0.5 text-xs"
                    >
                      {{ item }}
                    </span>
                  </div>
                </TableCell>
                <TableCell class="text-center text-sm tabular-nums text-muted-foreground">
                  {{ plan.sort_order }}
                </TableCell>
                <TableCell class="text-center">
                  <span
                    class="inline-flex items-center gap-1.5 text-xs"
                    :class="plan.enabled ? 'text-emerald-500' : 'text-muted-foreground'"
                  >
                    <span
                      class="h-1.5 w-1.5 rounded-full"
                      :class="plan.enabled ? 'bg-emerald-500' : 'bg-muted-foreground/40'"
                    />
                    {{ plan.enabled ? t('billingPlansManagement.enabled') : t('billingPlansManagement.disabled') }}
                  </span>
                </TableCell>
                <TableCell class="whitespace-nowrap text-right">
                  <div class="inline-flex items-center">
                    <Button
                      variant="ghost"
                      size="sm"
                      :disabled="deletingPlanId === plan.id"
                      @click="togglePlanStatus(plan)"
                    >
                      {{ plan.enabled ? t('billingPlansManagement.disable') : t('billingPlansManagement.enable') }}
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      :disabled="deletingPlanId === plan.id"
                      @click="openEditDialog(plan)"
                    >
                      {{ t('billingPlansManagement.edit') }}
                    </Button>
                    <DropdownMenu>
                      <DropdownMenuTrigger as-child>
                        <Button
                          variant="ghost"
                          size="sm"
                          class="h-9 w-9 p-0"
                          :disabled="deletingPlanId === plan.id"
                        >
                          <MoreHorizontal class="h-4 w-4" />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuItem
                          class="text-destructive focus:text-destructive"
                          :disabled="deletingPlanId === plan.id"
                          @select="deletePlan(plan)"
                        >
                          <Trash2 class="mr-2 h-4 w-4" />
                          {{ deletingPlanId === plan.id ? t('billingPlansManagement.deleting') : t('billingPlansManagement.delete') }}
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </div>
      </CardSection>
    </div>

    <Dialog
      v-model:open="dialogOpen"
      size="4xl"
      :title="editingPlan ? t('billingPlansManagement.editTitle') : t('billingPlansManagement.createTitle')"
      :description="t('billingPlansManagement.formHint')"
      no-padding
    >
      <div class="max-h-[calc(100vh-193px)] space-y-4 overflow-y-auto px-6 py-4">
        <div class="grid grid-cols-1 gap-2 md:grid-cols-3">
          <Button
            variant="outline"
            size="sm"
            class="h-12 justify-start rounded-xl px-3 text-left"
            @click="applyTemplate('daily')"
          >
            <span>
              <span class="block text-sm font-medium leading-5">{{ t('billingPlansManagement.quotaPlan') }}</span>
              <span class="block text-xs font-normal leading-4 text-muted-foreground">{{ t('billingPlansManagement.quotaPlanHint') }}</span>
            </span>
          </Button>
          <Button
            variant="outline"
            size="sm"
            class="h-12 justify-start rounded-xl px-3 text-left"
            @click="applyTemplate('membership')"
          >
            <span>
              <span class="block text-sm font-medium leading-5">{{ t('billingPlansManagement.membershipPlan') }}</span>
              <span class="block text-xs font-normal leading-4 text-muted-foreground">{{ t('billingPlansManagement.membershipPlanHint') }}</span>
            </span>
          </Button>
          <Button
            variant="outline"
            size="sm"
            class="h-12 justify-start rounded-xl px-3 text-left"
            @click="applyTemplate('mixed')"
          >
            <span>
              <span class="block text-sm font-medium leading-5">{{ t('billingPlansManagement.hybridPlan') }}</span>
              <span class="block text-xs font-normal leading-4 text-muted-foreground">{{ t('billingPlansManagement.hybridPlanHint') }}</span>
            </span>
          </Button>
        </div>

        <div class="rounded-xl border border-border/60 bg-muted/20 px-3 py-2">
          <div class="grid grid-cols-1 gap-2 lg:grid-cols-[1fr_auto]">
            <div class="min-w-0 space-y-1.5">
              <div class="flex flex-wrap items-center gap-2">
                <div class="text-sm font-medium">
                  {{ planModeGuide.title }}
                </div>
                <Badge variant="outline">
                  {{ planModeGuide.badge }}
                </Badge>
              </div>
              <p class="text-xs leading-5 text-muted-foreground">
                {{ planModeGuide.description }}
              </p>
            </div>
            <div class="flex flex-wrap items-center gap-1.5 lg:justify-end">
              <span
                v-for="note in planModeGuide.notes"
                :key="note"
                class="rounded-full border border-border/60 bg-card/60 px-2.5 py-1 text-xs leading-4 text-muted-foreground"
              >
                {{ note }}
              </span>
            </div>
          </div>
        </div>

        <div
          v-if="planMode !== 'empty'"
          class="mx-auto w-full max-w-[880px] rounded-2xl border border-border/60 bg-muted/10 p-6"
        >
          <div class="grid grid-cols-1 gap-x-4 gap-y-3 xl:grid-cols-12">
            <div class="border-b border-border/70 pb-2 xl:col-span-12">
              <h3 class="text-sm font-semibold leading-5">
                {{ t('billingPlansManagement.basicInfo') }}
              </h3>
            </div>

            <div class="space-y-1.5 xl:col-span-8">
              <Label
                for="plan-title"
                class="inline-flex items-center gap-1.5 text-sm font-medium"
              >
                <span>{{ t('billingPlansManagement.planName') }}</span>
                <span class="text-destructive">*</span>
                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger as-child>
                      <button
                        type="button"
                        class="text-muted-foreground/60 hover:text-muted-foreground"
                        :aria-label="t('billingPlansManagement.planNameAria')"
                      >
                        <CircleHelp class="h-3.5 w-3.5" />
                      </button>
                    </TooltipTrigger>
                    <TooltipContent
                      side="top"
                      class="max-w-64 text-xs"
                    >
                      {{ t('billingPlansManagement.planNameHint') }}
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              </Label>
              <Input
                id="plan-title"
                v-model="form.title"
                class="h-9 rounded-xl bg-muted/70"
                :placeholder="t('billingPlansManagement.planNamePlaceholder')"
              />
            </div>

            <div class="space-y-1.5 xl:col-span-4">
              <Label
                for="plan-price"
                class="inline-flex items-center gap-1.5 text-sm font-medium"
              >
                <span>{{ t('billingPlansManagement.price') }}</span>
                <span class="text-destructive">*</span>
                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger as-child>
                      <button
                        type="button"
                        class="text-muted-foreground/60 hover:text-muted-foreground"
                        :aria-label="t('billingPlansManagement.priceAria')"
                      >
                        <CircleHelp class="h-3.5 w-3.5" />
                      </button>
                    </TooltipTrigger>
                    <TooltipContent
                      side="top"
                      class="max-w-72 text-xs"
                    >
                      {{ t('billingPlansManagement.priceHint') }}
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              </Label>
              <div class="grid grid-cols-[minmax(0,1fr)_88px]">
                <Input
                  id="plan-price"
                  v-model.number="form.price_amount"
                  class="h-9 rounded-l-xl rounded-r-none border-r-0 bg-muted/70 focus-visible:z-10"
                  type="number"
                  inputmode="decimal"
                  min="0.01"
                  step="0.01"
                  @blur="normalizePriceAmount"
                />
                <Select v-model="form.price_currency">
                  <SelectTrigger class="h-9 rounded-l-none rounded-r-xl border-l-0 bg-muted/70 px-3 focus:z-10">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem
                      v-for="currency in priceCurrencyOptions"
                      :key="currency"
                      :value="currency"
                    >
                      {{ currency }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </div>

            <div class="space-y-1.5 xl:col-span-12">
              <Label
                for="plan-description"
                class="inline-flex items-center gap-1.5 text-sm font-medium"
              >
                <span>{{ t('billingPlansManagement.summary') }}</span>
                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger as-child>
                      <button
                        type="button"
                        class="text-muted-foreground/60 hover:text-muted-foreground"
                        :aria-label="t('billingPlansManagement.summaryAria')"
                      >
                        <CircleHelp class="h-3.5 w-3.5" />
                      </button>
                    </TooltipTrigger>
                    <TooltipContent
                      side="top"
                      class="max-w-72 text-xs"
                    >
                      {{ t('billingPlansManagement.summaryHint') }}
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              </Label>
              <Textarea
                id="plan-description"
                v-model="form.description"
                class="min-h-[72px] resize-y rounded-2xl bg-muted/70"
                rows="2"
                :placeholder="t('billingPlansManagement.summaryPlaceholder')"
              />
            </div>
          </div>

          <section class="mt-5 space-y-3">
            <div class="border-b border-border/70 pb-2">
              <h3 class="text-sm font-semibold leading-5">
                {{ t('billingPlansManagement.durationAndLimits') }}
              </h3>
            </div>
            <div class="grid grid-cols-1 gap-x-4 gap-y-3 xl:grid-cols-12">
              <div
                class="space-y-1.5"
                :class="purchaseLimitFieldSpanClass"
              >
                <Label
                  for="plan-purchase-limit-scope"
                  class="inline-flex items-center gap-1.5 text-sm font-medium"
                >
                  <span>{{ t('billingPlansManagement.purchaseLimit') }}</span>
                  <TooltipProvider>
                    <Tooltip>
                      <TooltipTrigger as-child>
                        <button
                          type="button"
                          class="text-muted-foreground/60 hover:text-muted-foreground"
                          :aria-label="t('billingPlansManagement.purchaseLimitAria')"
                        >
                          <CircleHelp class="h-3.5 w-3.5" />
                        </button>
                      </TooltipTrigger>
                      <TooltipContent
                        side="top"
                        class="max-w-72 text-xs"
                      >
                        {{ t('billingPlansManagement.purchaseLimitHint') }}
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                </Label>
                <Select v-model="form.purchase_limit_scope">
                  <SelectTrigger
                    id="plan-purchase-limit-scope"
                    class="h-9 rounded-xl bg-muted/70 px-3"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="active_period">
                      {{ t('billingPlansManagement.periodLimit') }}
                    </SelectItem>
                    <SelectItem value="lifetime">
                      {{ t('billingPlansManagement.lifetimeLimit') }}
                    </SelectItem>
                    <SelectItem value="unlimited">
                      {{ t('billingPlansManagement.unlimitedPurchase') }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div
                class="space-y-1.5 xl:col-span-4"
              >
                <Label
                  for="plan-duration"
                  class="inline-flex items-center gap-1.5 text-sm font-medium"
                >
                  <span>{{ durationFieldLabel }}</span>
                  <span class="text-destructive">*</span>
                  <TooltipProvider>
                    <Tooltip>
                      <TooltipTrigger as-child>
                        <button
                          type="button"
                          class="text-muted-foreground/60 hover:text-muted-foreground"
                          :aria-label="t('billingPlansManagement.durationAria')"
                        >
                          <CircleHelp class="h-3.5 w-3.5" />
                        </button>
                      </TooltipTrigger>
                      <TooltipContent
                        side="top"
                        class="max-w-72 text-xs"
                      >
                        {{ durationTooltipText }}
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                </Label>
                <div class="grid grid-cols-[minmax(0,1fr)_88px]">
                  <Input
                    id="plan-duration"
                    v-model.number="form.duration_value"
                    class="h-9 rounded-l-xl rounded-r-none border-r-0 bg-muted/70 focus-visible:z-10"
                    type="number"
                    inputmode="numeric"
                    min="1"
                    step="1"
                    @blur="normalizeDurationValue"
                  />
                  <Select v-model="form.duration_unit">
                    <SelectTrigger class="h-9 rounded-l-none rounded-r-xl border-l-0 bg-muted/70 px-3 focus:z-10">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="day">
                        {{ t('billingPlansManagement.day') }}
                      </SelectItem>
                      <SelectItem value="month">
                        {{ t('billingPlansManagement.month') }}
                      </SelectItem>
                      <SelectItem value="year">
                        {{ t('billingPlansManagement.year') }}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </div>

              <div
                v-if="showPurchaseLimitCount"
                class="space-y-1.5"
                :class="purchaseLimitFieldSpanClass"
              >
                <Label
                  for="plan-max-active"
                  class="inline-flex items-center gap-1.5 text-sm font-medium"
                >
                  <span>{{ activeLimitFieldLabel }}</span>
                  <TooltipProvider>
                    <Tooltip>
                      <TooltipTrigger as-child>
                        <button
                          type="button"
                          class="text-muted-foreground/60 hover:text-muted-foreground"
                          :aria-label="t('billingPlansManagement.maxActiveAria')"
                        >
                          <CircleHelp class="h-3.5 w-3.5" />
                        </button>
                      </TooltipTrigger>
                      <TooltipContent
                        side="top"
                        class="max-w-72 text-xs"
                      >
                        {{ activeLimitTooltipText }}
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                </Label>
                <Input
                  id="plan-max-active"
                  v-model.number="form.max_active_per_user"
                  class="h-9 rounded-xl bg-muted/70"
                  type="number"
                  inputmode="numeric"
                  min="1"
                  step="1"
                  @blur="normalizeActiveLimit"
                />
              </div>
              <div class="xl:col-span-12 rounded-xl border border-border/60 bg-muted/20 px-3 py-2 text-xs leading-5 text-muted-foreground">
                <span class="font-medium text-foreground/80">{{ t('billingPlansManagement.currentLogic') }}：</span>
                {{ purchaseLimitSummaryText }}
              </div>
              <div class="xl:col-span-12 rounded-xl border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs leading-5 text-amber-200">
                {{ t('billingPlansManagement.renewalLogic') }}
              </div>
            </div>
          </section>

          <section class="mt-5 space-y-3">
            <div class="border-b border-border/70 pb-2">
              <h3 class="text-sm font-semibold leading-5">
                {{ t('billingPlansManagement.displayAndListing') }}
              </h3>
            </div>
            <div class="grid grid-cols-1 gap-x-4 gap-y-3 xl:grid-cols-12">
              <div class="space-y-1.5 xl:col-span-6">
                <Label
                  for="plan-sort"
                  class="inline-flex items-center gap-1.5 text-sm font-medium"
                >
                  <span>{{ t('billingPlansManagement.displayOrder') }}</span>
                  <TooltipProvider>
                    <Tooltip>
                      <TooltipTrigger as-child>
                        <button
                          type="button"
                          class="text-muted-foreground/60 hover:text-muted-foreground"
                          :aria-label="t('billingPlansManagement.displayOrderAria')"
                        >
                          <CircleHelp class="h-3.5 w-3.5" />
                        </button>
                      </TooltipTrigger>
                      <TooltipContent
                        side="top"
                        class="max-w-64 text-xs"
                      >
                        {{ t('billingPlansManagement.displayOrderHint') }}
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                </Label>
                <Input
                  id="plan-sort"
                  v-model.number="form.sort_order"
                  class="h-9 rounded-xl bg-muted/70"
                  type="number"
                  step="1"
                />
              </div>
              <div class="space-y-1.5 xl:col-span-6">
                <Label class="inline-flex items-center gap-1.5 text-sm font-medium">
                  <span>{{ t('billingPlansManagement.listingStatus') }}</span>
                  <TooltipProvider>
                    <Tooltip>
                      <TooltipTrigger as-child>
                        <button
                          type="button"
                          class="text-muted-foreground/60 hover:text-muted-foreground"
                          :aria-label="t('billingPlansManagement.listingStatusAria')"
                        >
                          <CircleHelp class="h-3.5 w-3.5" />
                        </button>
                      </TooltipTrigger>
                      <TooltipContent
                        side="top"
                        class="max-w-64 text-xs"
                      >
                        {{ t('billingPlansManagement.listingStatusHint') }}
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                </Label>
                <div class="flex h-9 items-center justify-between rounded-xl border border-border/60 bg-muted/70 px-3">
                  <span class="text-sm text-muted-foreground">
                    {{ form.enabled ? t('billingPlansManagement.listed') : t('billingPlansManagement.unlisted') }}
                  </span>
                  <Switch v-model="form.enabled" />
                </div>
              </div>
            </div>
          </section>
        </div>

        <section
          v-if="planMode !== 'empty'"
          class="space-y-4"
        >
          <h3 class="text-sm font-semibold">
            {{ t('billingPlansManagement.entitlements') }}
          </h3>

          <div
            v-if="showWalletCreditConfig"
            class="space-y-3 rounded-2xl border border-border/60 bg-muted/20 p-4"
          >
            <div class="flex items-center justify-between gap-3">
              <div>
                <Label class="text-sm font-medium">{{ t('billingPlansManagement.bonusBalance') }}</Label>
                <p class="mt-1 text-xs text-muted-foreground">
                  {{ walletCreditSummaryText }}
                </p>
              </div>
              <Switch v-model="form.wallet_credit_enabled" />
            </div>
            <div
              v-if="form.wallet_credit_enabled"
              class="grid grid-cols-1 gap-3 md:grid-cols-2"
            >
              <div class="space-y-1.5">
                <Label>{{ t('billingPlansManagement.grantAmount') }}</Label>
                <Input
                  v-model.number="form.wallet_credit_amount_usd"
                  type="number"
                  min="0.01"
                  step="0.01"
                />
              </div>
              <div class="space-y-1.5">
                <Label>{{ t('billingPlansManagement.balanceType') }}</Label>
                <Select v-model="form.wallet_credit_balance_bucket">
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="recharge">
                      {{ t('billingPlansManagement.rechargeBalance') }}
                    </SelectItem>
                    <SelectItem value="gift">
                      {{ t('billingPlansManagement.giftBalance') }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <p class="rounded-xl border border-border/50 bg-card/60 px-3 py-2 text-xs leading-5 text-muted-foreground md:col-span-2">
                {{ walletCreditDetailText }}
              </p>
            </div>
          </div>

          <div
            v-if="showDailyQuotaConfig"
            class="space-y-3 rounded-2xl border border-border/60 bg-muted/20 p-4"
          >
            <div class="flex items-center justify-between gap-3">
              <div>
                <Label class="text-sm font-medium">{{ t('billingPlansManagement.usageQuota') }}</Label>
                <p class="mt-1 text-xs text-muted-foreground">
                  {{ dailyQuotaSummaryText }}
                </p>
              </div>
              <Switch v-model="form.daily_quota_enabled" />
            </div>
            <div
              v-if="form.daily_quota_enabled"
              class="grid grid-cols-1 gap-3 md:grid-cols-2"
            >
              <div class="space-y-1.5">
                <Label>{{ t('billingPlansManagement.dailyQuota') }}</Label>
                <Input
                  v-model.number="form.daily_quota_usd"
                  type="number"
                  min="0"
                  step="0.01"
                />
              </div>
              <div class="space-y-1.5">
                <Label>{{ t('billingPlansManagement.fiveHourQuota') }}</Label>
                <Input
                  v-model.number="form.five_hour_quota_usd"
                  type="number"
                  min="0"
                  step="0.01"
                />
              </div>
              <div class="space-y-1.5">
                <Label>{{ t('billingPlansManagement.weeklyQuota') }}</Label>
                <Input
                  v-model.number="form.weekly_quota_usd"
                  type="number"
                  min="0"
                  step="0.01"
                />
              </div>
              <div class="space-y-1.5">
                <Label>{{ t('billingPlansManagement.monthlyQuota') }}</Label>
                <Input
                  v-model.number="form.monthly_quota_usd"
                  type="number"
                  min="0"
                  step="0.01"
                />
              </div>
              <div class="space-y-1.5">
                <Label>{{ t('billingPlansManagement.consumptionMultiplier') }}</Label>
                <Input
                  v-model.number="form.quota_multiplier"
                  type="number"
                  min="0.0001"
                  step="0.0001"
                />
                <p class="text-xs leading-5 text-muted-foreground">
                  {{ t('billingPlansManagement.multiplierHint') }}
                </p>
              </div>
              <div class="space-y-1.5 md:col-span-2">
                <Label class="inline-flex items-center gap-1.5">
                  <span>{{ t('billingPlansManagement.allowedProviders') }}</span>
                  <span class="text-destructive">*</span>
                </Label>
                <MultiSelect
                  v-model="form.allowed_provider_ids"
                  :options="providerOptions"
                  :placeholder="loadingProviders ? t('billingPlansManagement.loadingProviders') : t('billingPlansManagement.chooseProviders')"
                  :empty-text="t('billingPlansManagement.noProviders')"
                />
                <p class="text-xs leading-5 text-muted-foreground">
                  {{ t('billingPlansManagement.allowedProvidersHint') }}
                </p>
                <div class="flex min-h-9 items-center rounded-md border border-border/60 bg-muted/20 px-3 py-2 text-xs leading-5 text-muted-foreground">
                  <span class="font-medium text-foreground">{{ t('billingPlansManagement.derivedModels') }}：</span>
                  <span class="ml-1">{{ derivedModelSummary }}</span>
                </div>
              </div>
              <div class="space-y-1.5">
                <Label>{{ t('billingPlansManagement.resetTimezone') }}</Label>
                <Input
                  v-model="form.reset_timezone"
                  placeholder="Asia/Shanghai"
                  disabled
                />
              </div>
              <div class="flex items-center justify-between rounded-xl border border-border/60 bg-card/50 p-3 opacity-70">
                <div>
                  <Label>{{ t('billingPlansManagement.rollover') }}</Label>
                  <p class="mt-1 text-xs text-muted-foreground">
                    {{ t('billingPlansManagement.rolloverUnsupported') }}
                  </p>
                </div>
                <Switch
                  v-model="form.carry_over"
                  disabled
                />
              </div>
              <p class="rounded-xl border border-border/50 bg-card/60 px-3 py-2 text-xs leading-5 text-muted-foreground md:col-span-2">
                {{ dailyQuotaDetailText }}
              </p>
            </div>
          </div>

          <div
            v-if="showMembershipGroupConfig"
            class="space-y-3 rounded-2xl border border-border/60 bg-muted/20 p-4"
          >
            <div class="flex items-center justify-between gap-3">
              <div>
                <Label class="text-sm font-medium">{{ t('billingPlansManagement.memberGroups') }}</Label>
                <p class="mt-1 text-xs text-muted-foreground">
                  {{ membershipSummaryText }}
                </p>
              </div>
              <Switch v-model="form.membership_group_enabled" />
            </div>
            <div
              v-if="form.membership_group_enabled"
              class="space-y-3"
            >
              <p class="rounded-xl border border-border/50 bg-card/60 px-3 py-2 text-xs leading-5 text-muted-foreground">
                {{ membershipDetailText }}
              </p>
              <MultiSelect
                v-model="form.grant_user_groups"
                :options="userGroupOptions"
                :placeholder="t('billingPlansManagement.chooseGroups')"
                :empty-text="t('billingPlansManagement.noGroups')"
              />
              <div class="grid grid-cols-1 gap-2 md:grid-cols-[1fr_auto]">
                <Input
                  v-model="manualGroupId"
                  :placeholder="t('billingPlansManagement.groupIdPlaceholder')"
                  @keyup.enter="addManualGroup"
                />
                <Button
                  variant="outline"
                  @click="addManualGroup"
                >
                  {{ t('billingPlansManagement.add') }}
                </Button>
              </div>
            </div>
          </div>
        </section>
      </div>

      <div class="flex h-14 items-center justify-end gap-3 border-t border-border bg-muted/10 px-6">
        <Button
          variant="outline"
          class="h-9"
          :disabled="saving"
          @click="dialogOpen = false"
        >
          {{ t('billingPlansManagement.cancel') }}
        </Button>
        <Button
          variant="default"
          class="h-9"
          :disabled="saving || isSaveDisabled"
          @click="savePlan"
        >
          {{ saving ? t('billingPlansManagement.saving') : t('billingPlansManagement.savePlan') }}
        </Button>
      </div>
    </Dialog>
  </PageContainer>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
import { CircleHelp, MoreHorizontal, Plus, Trash2 } from 'lucide-vue-next'
import {
  adminBillingPlansApi,
  type BillingDurationUnit,
  type BillingEntitlement,
  type BillingPlan,
  type BillingPurchaseLimitScope,
  type BillingPlanWriteRequest,
  type DailyQuotaEntitlement,
  type MembershipGroupEntitlement,
  type WalletCreditBucket,
  type WalletCreditEntitlement,
} from '@/api/billing'
import { getProvidersSummary } from '@/api/endpoints/providers'
import type { ProviderWithEndpointsSummary } from '@/api/endpoints/types'
import { getGlobalModels, type GlobalModelResponse } from '@/api/global-models'
import { usersApi, type UserGroup } from '@/api/users'
import {
  Badge,
  Button,
  Dialog,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  Input,
  Label,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Switch,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  Textarea,
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui'
import { EmptyState, LoadingState, MultiSelect } from '@/components/common'
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

type TemplateKey = 'daily' | 'membership' | 'mixed'
type PlanMode = 'empty' | 'wallet' | 'daily' | 'membership' | 'mixed'

interface PlanModeGuide {
  badge: string
  title: string
  description: string
  notes: string[]
}

interface PlanFormState {
  title: string
  description: string
  price_amount: number
  price_currency: string
  duration_unit: BillingDurationUnit
  duration_value: number
  enabled: boolean
  sort_order: number
  max_active_per_user: number
  purchase_limit_scope: BillingPurchaseLimitScope
  wallet_credit_enabled: boolean
  wallet_credit_amount_usd: number
  wallet_credit_balance_bucket: WalletCreditBucket
  daily_quota_enabled: boolean
  daily_quota_usd: number
  five_hour_quota_usd: number
  weekly_quota_usd: number
  monthly_quota_usd: number
  quota_multiplier: number
  reset_timezone: string
  carry_over: boolean
  allowed_provider_ids: string[]
  membership_group_enabled: boolean
  grant_user_groups: string[]
}

const { success, error: showError } = useToast()

const loading = ref(true)
const saving = ref(false)
const deletingPlanId = ref<string | null>(null)
const dialogOpen = ref(false)
const plans = ref<BillingPlan[]>([])
const editingPlan = ref<BillingPlan | null>(null)
const userGroups = ref<UserGroup[]>([])
const globalModels = ref<GlobalModelResponse[]>([])
const providers = ref<ProviderWithEndpointsSummary[]>([])
const loadingGlobalModels = ref(false)
const loadingProviders = ref(false)
const manualGroupId = ref('')

const form = reactive<PlanFormState>(buildDefaultForm())

const userGroupOptions = computed(() =>
  userGroups.value.map((group) => ({
    value: group.id,
    label: group.name,
  }))
)

const providerOptions = computed(() => {
  const knownIds = new Set(providers.value.map((provider) => provider.id))
  const loadedOptions = providers.value
    .filter((provider) => provider.is_active || form.allowed_provider_ids.includes(provider.id))
    .map((provider) => ({
      value: provider.id,
      label: provider.is_active
        ? provider.name
        : `${provider.name} (${t('billingPlansManagement.providerDisabled')})`,
    }))
  const missingOptions = form.allowed_provider_ids
    .filter((id) => id && !knownIds.has(id))
    .map((id) => ({
      value: id,
      label: id,
    }))
  return [...loadedOptions, ...missingOptions]
})

const derivedGlobalModels = computed(() => {
  const activeModelIds = new Set(globalModels.value.map((model) => model.id))
  const selectedProviderIds = new Set(form.allowed_provider_ids)
  const modelIds = new Set(
    providers.value
      .filter((provider) => provider.is_active && selectedProviderIds.has(provider.id))
      .flatMap((provider) => provider.global_model_ids)
      .filter((modelId) => activeModelIds.has(modelId)),
  )
  return globalModels.value.filter((model) => modelIds.has(model.id))
})

const derivedModelSummary = computed(() => {
  if (loadingProviders.value || loadingGlobalModels.value) {
    return t('billingPlansManagement.loadingModels')
  }
  if (form.allowed_provider_ids.length === 0) {
    return t('billingPlansManagement.chooseProvidersFirst')
  }
  if (derivedGlobalModels.value.length === 0) {
    return t('billingPlansManagement.noDerivedModels')
  }
  const names = derivedGlobalModels.value
    .slice(0, 3)
    .map((model) => model.display_name || model.name || model.id)
    .join('、')
  return derivedGlobalModels.value.length <= 3
    ? names
    : t('billingPlansManagement.derivedModelCount', {
      names,
      count: derivedGlobalModels.value.length,
    })
})

const priceCurrencyOptions = computed(() => {
  const normalized = form.price_currency.trim().toUpperCase()
  const options = ['CNY', 'USD']
  return normalized && !options.includes(normalized) ? [...options, normalized] : options
})

const isEditingExistingZeroPricePlan = computed(() =>
  editingPlan.value !== null && Number(editingPlan.value.price_amount) === 0
)

const hasValidPricePrecision = computed(() =>
  /^\d+(\.\d{1,2})?$/.test(String(form.price_amount))
)

const hasValidPriceAmount = computed(() => {
  const value = Number(form.price_amount)
  if (!Number.isFinite(value) || value < 0) return false
  if (value === 0 && !isEditingExistingZeroPricePlan.value) return false
  return hasValidPricePrecision.value
})

const hasValidDuration = computed(() =>
  Number.isInteger(Number(form.duration_value)) && Number(form.duration_value) > 0
)

const hasValidDurationUnit = computed(() =>
  ['day', 'month', 'year'].includes(form.duration_unit)
)

const hasValidActiveLimit = computed(() =>
  Number.isInteger(Number(form.max_active_per_user)) && Number(form.max_active_per_user) > 0
)

const hasValidPurchaseLimitScope = computed(() =>
  ['active_period', 'lifetime', 'unlimited'].includes(form.purchase_limit_scope)
)

const hasSelectedPackageEntitlement = computed(() =>
  form.daily_quota_enabled || form.membership_group_enabled
)

const showPurchaseLimitCount = computed(() =>
  form.purchase_limit_scope !== 'unlimited'
)

const purchaseLimitFieldSpanClass = computed(() =>
  'xl:col-span-4'
)

const isSaveDisabled = computed(() =>
  !form.title.trim()
  || !form.price_currency.trim()
  || !hasValidPriceAmount.value
  || !hasValidDuration.value
  || !hasValidDurationUnit.value
  || (showPurchaseLimitCount.value && !hasValidActiveLimit.value)
  || !hasValidPurchaseLimitScope.value
  || !hasSelectedPackageEntitlement.value
  || (form.daily_quota_enabled && form.allowed_provider_ids.length === 0)
)

const planMode = computed<PlanMode>(() => {
  const enabledCount = [
    form.wallet_credit_enabled,
    form.daily_quota_enabled,
    form.membership_group_enabled,
  ].filter(Boolean).length

  if (enabledCount === 0) return 'empty'
  if (enabledCount > 1) return 'mixed'
  if (form.wallet_credit_enabled) return 'wallet'
  if (form.daily_quota_enabled) return 'daily'
  return 'membership'
})

const planModeGuide = computed<PlanModeGuide>(() => {
  switch (planMode.value) {
    case 'wallet':
      return {
        badge: t('billingPlansManagement.legacyBalanceBadge'),
        title: t('billingPlansManagement.legacyBalancePlan'),
        description: t('billingPlansManagement.legacyBalanceDescription'),
        notes: [
          t('billingPlansManagement.legacyBalanceNote1'),
          t('billingPlansManagement.legacyBalanceNote2'),
          t('billingPlansManagement.legacyBalanceNote3'),
        ],
      }
    case 'daily':
      return {
        badge: t('billingPlansManagement.periodQuotaBadge'),
        title: t('billingPlansManagement.quotaPlan'),
        description: t('billingPlansManagement.periodQuotaDescription'),
        notes: [
          t('billingPlansManagement.periodQuotaNote1'),
          t('billingPlansManagement.periodQuotaNote2'),
          t('billingPlansManagement.periodQuotaNote3'),
        ],
      }
    case 'membership':
      return {
        badge: t('billingPlansManagement.membershipBadge'),
        title: t('billingPlansManagement.membershipPlan'),
        description: t('billingPlansManagement.membershipDescription'),
        notes: [
          t('billingPlansManagement.membershipNote1'),
          t('billingPlansManagement.membershipNote2'),
          t('billingPlansManagement.membershipNote3'),
        ],
      }
    case 'mixed':
      return {
        badge: t('billingPlansManagement.hybridBadge'),
        title: t('billingPlansManagement.hybridBenefitsPlan'),
        description: t('billingPlansManagement.hybridDescription'),
        notes: [
          t('billingPlansManagement.hybridNote1'),
          t('billingPlansManagement.hybridNote2'),
          t('billingPlansManagement.hybridNote3'),
        ],
      }
    default:
      return {
        badge: t('billingPlansManagement.pendingConfig'),
        title: t('billingPlansManagement.chooseTemplate'),
        description: t('billingPlansManagement.chooseTemplateDescription'),
        notes: [
          t('billingPlansManagement.chooseTemplateNote1'),
          t('billingPlansManagement.chooseTemplateNote2'),
          t('billingPlansManagement.chooseTemplateNote3'),
        ],
      }
  }
})

const durationFieldLabel = computed(() => t('billingPlansManagement.planValidity'))

const durationTooltipText = computed(() => {
  if (planMode.value === 'wallet') {
    return t('billingPlansManagement.legacyDurationHint')
  }
  return t('billingPlansManagement.durationHint')
})

const activeLimitFieldLabel = computed(() =>
  form.purchase_limit_scope === 'lifetime' ? t('billingPlansManagement.maxPurchasesPerUser') : t('billingPlansManagement.maxPendingOrdersPerUser')
)

const activeLimitTooltipText = computed(() =>
  form.purchase_limit_scope === 'lifetime'
    ? t('billingPlansManagement.lifetimeLimitHint')
    : t('billingPlansManagement.pendingOrderLimitHint')
)

const purchaseLimitSummaryText = computed(() => {
  if (form.purchase_limit_scope === 'unlimited') {
    return t('billingPlansManagement.unlimitedSummary', { duration: form.duration_value || 1, unit: durationUnitLabel(form.duration_unit) })
  }
  if (form.purchase_limit_scope === 'lifetime') {
    return t('billingPlansManagement.lifetimeSummary', { duration: form.duration_value || 1, unit: durationUnitLabel(form.duration_unit), count: form.max_active_per_user || 1 })
  }
  return t('billingPlansManagement.activePeriodSummary', { duration: form.duration_value || 1, unit: durationUnitLabel(form.duration_unit), count: form.max_active_per_user || 1 })
})

const showWalletCreditConfig = computed(() =>
  planMode.value === 'mixed' || form.wallet_credit_enabled
)

const showDailyQuotaConfig = computed(() =>
  planMode.value === 'mixed' || form.daily_quota_enabled
)

const showMembershipGroupConfig = computed(() =>
  planMode.value === 'mixed' || form.membership_group_enabled
)

const walletCreditSummaryText = computed(() =>
  planMode.value === 'mixed'
    ? t('billingPlansManagement.bonusBalanceSummary')
    : t('billingPlansManagement.legacyBalanceSummary')
)

const walletCreditDetailText = computed(() => {
  const bucket = form.wallet_credit_balance_bucket === 'recharge' ? t('billingPlansManagement.rechargeBalance') : t('billingPlansManagement.giftBalance')
  return t('billingPlansManagement.balanceGrantDetail', { bucket })
})

const dailyQuotaSummaryText = computed(() =>
  planMode.value === 'mixed'
    ? t('billingPlansManagement.hybridQuotaSummary')
    : t('billingPlansManagement.quotaWindowSummary')
)

const dailyQuotaDetailText = computed(() =>
  t('billingPlansManagement.walletOverageDetail')
)

const membershipSummaryText = computed(() =>
  planMode.value === 'mixed'
    ? t('billingPlansManagement.hybridMembershipSummary')
    : t('billingPlansManagement.membershipGrantSummary')
)

const membershipDetailText = computed(() =>
  t('billingPlansManagement.membershipGrantDetail')
)

onMounted(() => {
  void Promise.all([loadPlans(), loadUserGroups(), loadGlobalModels(), loadProviders()]).finally(() => {
    loading.value = false
  })
})

function buildDefaultForm(): PlanFormState {
  return {
    title: '',
    description: '',
    price_amount: 100,
    price_currency: 'CNY',
    duration_unit: 'month',
    duration_value: 1,
    enabled: true,
    sort_order: 0,
    max_active_per_user: 1,
    purchase_limit_scope: 'active_period',
    wallet_credit_enabled: false,
    wallet_credit_amount_usd: 10,
    wallet_credit_balance_bucket: 'recharge',
    daily_quota_enabled: false,
    daily_quota_usd: 50,
    five_hour_quota_usd: 0,
    weekly_quota_usd: 0,
    monthly_quota_usd: 0,
    quota_multiplier: 1,
    reset_timezone: 'Asia/Shanghai',
    carry_over: false,
    allowed_provider_ids: [],
    membership_group_enabled: false,
    grant_user_groups: [],
  }
}

function assignForm(next: PlanFormState) {
  Object.assign(form, next)
}

async function loadPlans() {
  try {
    const response = await adminBillingPlansApi.list()
    plans.value = [...response.items].sort((left, right) =>
      left.sort_order === right.sort_order
        ? left.price_amount - right.price_amount
        : left.sort_order - right.sort_order
    )
  } catch (err) {
    log.error(t('billingPlansManagement.loadPlansFailed'), err)
    showError(parseApiError(err, t('billingPlansManagement.loadPlansFailed')))
  }
}

async function loadUserGroups() {
  try {
    const response = await usersApi.listUserGroups()
    userGroups.value = response.items
  } catch (err) {
    log.error(t('billingPlansManagement.loadGroupsFailed'), err)
    showError(parseApiError(err, t('billingPlansManagement.loadGroupsFailed')))
  }
}

async function loadGlobalModels() {
  loadingGlobalModels.value = true
  try {
    const response = await getGlobalModels({ skip: 0, limit: 1000, is_active: true })
    globalModels.value = response.models
  } catch (err) {
    log.error(t('billingPlansManagement.loadModelsFailed'), err)
    showError(parseApiError(err, t('billingPlansManagement.loadModelsFailed')))
  } finally {
    loadingGlobalModels.value = false
  }
}

async function loadProviders() {
  loadingProviders.value = true
  try {
    const response = await getProvidersSummary(
      { page: 1, page_size: 500 },
      { cacheTtlMs: 10 * 1000 },
    )
    providers.value = response.items
  } catch (err) {
    log.error(t('billingPlansManagement.loadProvidersFailed'), err)
    showError(parseApiError(err, t('billingPlansManagement.loadProvidersFailed')))
  } finally {
    loadingProviders.value = false
  }
}

function openCreateDialog() {
  editingPlan.value = null
  assignForm(buildDefaultForm())
  manualGroupId.value = ''
  dialogOpen.value = true
}

function openEditDialog(plan: BillingPlan) {
  editingPlan.value = plan
  assignForm(formFromPlan(plan))
  manualGroupId.value = ''
  dialogOpen.value = true
}

function formFromPlan(plan: BillingPlan): PlanFormState {
  const next = buildDefaultForm()
  next.title = plan.title
  next.description = plan.description || ''
  next.price_amount = plan.price_amount
  next.price_currency = plan.price_currency.toUpperCase()
  next.duration_unit = plan.duration_unit
  next.duration_value = plan.duration_value
  next.enabled = plan.enabled
  next.sort_order = plan.sort_order
  next.max_active_per_user = plan.max_active_per_user
  next.purchase_limit_scope = plan.purchase_limit_scope || 'active_period'
  next.allowed_provider_ids = Array.isArray(plan.allowed_provider_ids)
    ? [...plan.allowed_provider_ids]
    : []

  for (const entitlement of normalizeBillingEntitlements(plan.entitlements)) {
    if (entitlement.type === 'wallet_credit') {
      const wallet = entitlement as WalletCreditEntitlement
      next.wallet_credit_enabled = true
      next.wallet_credit_amount_usd = Number(wallet.amount_usd || next.wallet_credit_amount_usd)
      next.wallet_credit_balance_bucket = wallet.balance_bucket || 'recharge'
    } else if (isDailyQuotaEntitlement(entitlement)) {
      const quota = entitlement
      const limits = quota.limits || {}
      next.daily_quota_enabled = true
      next.daily_quota_usd = Number(quota.daily_quota_usd ?? limits.daily_limit_usd ?? 0)
      next.five_hour_quota_usd = Number(quota.five_hour_quota_usd ?? limits.five_hour_limit_usd ?? 0)
      next.weekly_quota_usd = Number(quota.weekly_quota_usd ?? limits.weekly_limit_usd ?? 0)
      next.monthly_quota_usd = Number(quota.monthly_quota_usd ?? limits.monthly_limit_usd ?? 0)
      next.quota_multiplier = Number(quota.quota_multiplier ?? 1)
      next.reset_timezone = quota.reset_timezone || 'Asia/Shanghai'
      next.carry_over = Boolean(quota.carry_over)
    } else if (entitlement.type === 'membership_group') {
      const membership = entitlement as MembershipGroupEntitlement
      next.membership_group_enabled = true
      next.grant_user_groups = Array.isArray(membership.grant_user_groups)
        ? [...membership.grant_user_groups]
        : []
    }
  }
  return next
}

function applyTemplate(template: TemplateKey) {
  const next = buildDefaultForm()
  if (template === 'daily') {
    next.title = t('billingPlansManagement.defaultQuotaTitle')
    next.description = t('billingPlansManagement.defaultQuotaDescription')
    next.daily_quota_enabled = true
    next.duration_unit = 'day'
    next.duration_value = 7
    next.daily_quota_usd = 0
    next.weekly_quota_usd = 50
    next.max_active_per_user = 1
  } else if (template === 'membership') {
    next.title = t('billingPlansManagement.defaultMembershipTitle')
    next.description = t('billingPlansManagement.defaultMembershipDescription')
    next.membership_group_enabled = true
    next.max_active_per_user = 1
  } else {
    next.title = t('billingPlansManagement.defaultHybridTitle')
    next.description = t('billingPlansManagement.defaultHybridDescription')
    next.daily_quota_enabled = true
    next.duration_unit = 'day'
    next.duration_value = 7
    next.daily_quota_usd = 0
    next.weekly_quota_usd = 50
    next.membership_group_enabled = true
    next.max_active_per_user = 1
  }
  assignForm(next)
}

function buildEntitlements(): BillingEntitlement[] {
  const entitlements: BillingEntitlement[] = []
  if (form.wallet_credit_enabled) {
    entitlements.push({
      type: 'wallet_credit',
      amount_usd: Number(form.wallet_credit_amount_usd),
      balance_bucket: form.wallet_credit_balance_bucket,
    })
  }
  if (form.daily_quota_enabled) {
    const quota: DailyQuotaEntitlement = {
      type: 'daily_quota',
      daily_quota_usd: Number(form.daily_quota_usd),
      five_hour_quota_usd: Number(form.five_hour_quota_usd),
      weekly_quota_usd: Number(form.weekly_quota_usd),
      monthly_quota_usd: Number(form.monthly_quota_usd),
      quota_multiplier: Number(form.quota_multiplier),
      reset_timezone: form.reset_timezone.trim() || 'Asia/Shanghai',
      carry_over: false,
      allow_wallet_overage: true,
    }
    entitlements.push(quota)
  }
  if (form.membership_group_enabled) {
    entitlements.push({
      type: 'membership_group',
      grant_user_groups: form.grant_user_groups.map((value) => value.trim()).filter(Boolean),
    })
  }
  return entitlements
}

function normalizePriceAmount() {
  const value = Number(form.price_amount)
  if (!Number.isFinite(value) || value <= 0) return
  form.price_amount = Number(value.toFixed(2))
}

function normalizeDurationValue() {
  const value = Number(form.duration_value)
  if (!Number.isFinite(value) || value <= 0) return
  form.duration_value = Math.floor(value)
}

function normalizeActiveLimit() {
  const value = Number(form.max_active_per_user)
  if (!Number.isFinite(value) || value <= 0) return
  form.max_active_per_user = Math.floor(value)
}

function validatePlan(entitlements: BillingEntitlement[]): string | null {
  if (!form.title.trim()) return t('billingPlansManagement.planNameRequired')
  if (!hasValidPricePrecision.value) return t('billingPlansManagement.pricePrecision')
  if (!hasValidPriceAmount.value) return t('billingPlansManagement.pricePositive')
  if (!form.price_currency.trim()) return t('billingPlansManagement.currencyRequired')
  if (!hasValidPurchaseLimitScope.value) return t('billingPlansManagement.purchaseLimitInvalid')
  if (!hasValidDurationUnit.value) return t('billingPlansManagement.durationUnitInvalid')
  if (!hasValidDuration.value) return t('billingPlansManagement.durationInvalid')
  if (showPurchaseLimitCount.value && !hasValidActiveLimit.value) {
    return t('billingPlansManagement.positiveIntegerRequired', { field: activeLimitFieldLabel.value })
  }
  if (entitlements.length === 0) return t('billingPlansManagement.entitlementRequired')
  if (!hasPackageEntitlement(entitlements)) return t('billingPlansManagement.packageEntitlementRequired')
  if (form.wallet_credit_enabled && Number(form.wallet_credit_amount_usd) <= 0) return t('billingPlansManagement.bonusBalancePositive')
  if (form.daily_quota_enabled && !hasAnyUsageQuota()) return t('billingPlansManagement.quotaAmountRequired')
  if (form.daily_quota_enabled && (!Number.isFinite(Number(form.quota_multiplier)) || Number(form.quota_multiplier) <= 0)) {
    return t('billingPlansManagement.multiplierPositive')
  }
  if (form.daily_quota_enabled && form.allowed_provider_ids.length === 0) return t('billingPlansManagement.providersRequired')
  if (form.membership_group_enabled && form.grant_user_groups.length === 0) return t('billingPlansManagement.groupRequired')
  return null
}

function hasAnyUsageQuota(): boolean {
  return [
    form.daily_quota_usd,
    form.five_hour_quota_usd,
    form.weekly_quota_usd,
    form.monthly_quota_usd,
  ].some((value) => Number(value) > 0)
}

function buildPlanPayload(): BillingPlanWriteRequest | null {
  const entitlements = buildEntitlements()
  const validationError = validatePlan(entitlements)
  if (validationError) {
    showError(validationError)
    return null
  }
  return {
    title: form.title.trim(),
    description: form.description.trim() || null,
    price_amount: Number(Number(form.price_amount).toFixed(2)),
    price_currency: form.price_currency.trim().toUpperCase(),
    duration_unit: hasValidDurationUnit.value ? form.duration_unit : 'month',
    duration_value: hasValidDuration.value ? Number(form.duration_value) : 1,
    enabled: form.enabled,
    sort_order: Number(form.sort_order),
    max_active_per_user: showPurchaseLimitCount.value ? Number(form.max_active_per_user) : 1,
    purchase_limit_scope: form.purchase_limit_scope,
    allowed_provider_ids: form.allowed_provider_ids
      .map((value) => value.trim())
      .filter(Boolean),
    entitlements,
  }
}

async function savePlan() {
  const payload = buildPlanPayload()
  if (!payload) return

  saving.value = true
  try {
    if (editingPlan.value) {
      await adminBillingPlansApi.update(editingPlan.value.id, payload)
      success(t('billingPlansManagement.updated'))
    } else {
      await adminBillingPlansApi.create(payload)
      success(t('billingPlansManagement.created'))
    }
    dialogOpen.value = false
    await loadPlans()
  } catch (err) {
    log.error(t('billingPlansManagement.saveFailed'), err)
    showError(parseApiError(err, t('billingPlansManagement.saveFailed')))
  } finally {
    saving.value = false
  }
}

async function togglePlanStatus(plan: BillingPlan) {
  try {
    await adminBillingPlansApi.setStatus(plan.id, !plan.enabled)
    success(plan.enabled ? t('billingPlansManagement.disabledSuccess') : t('billingPlansManagement.enabledSuccess'))
    await loadPlans()
  } catch (err) {
    log.error(t('billingPlansManagement.updateStatusFailed'), err)
    showError(parseApiError(err, t('billingPlansManagement.updateStatusFailed')))
  }
}

async function deletePlan(plan: BillingPlan) {
  if (deletingPlanId.value) return
  const confirmed = window.confirm(
    t('billingPlansManagement.deleteConfirm', { title: plan.title })
  )
  if (!confirmed) return

  deletingPlanId.value = plan.id
  try {
    await adminBillingPlansApi.delete(plan.id)
    success(t('billingPlansManagement.deleted'))
    await loadPlans()
  } catch (err) {
    log.error(t('billingPlansManagement.deleteFailed'), err)
    showError(parseApiError(err, t('billingPlansManagement.deleteFailed')))
  } finally {
    deletingPlanId.value = null
  }
}

function addManualGroup() {
  const value = manualGroupId.value.trim()
  if (!value) return
  if (!form.grant_user_groups.includes(value)) {
    form.grant_user_groups = [...form.grant_user_groups, value]
  }
  manualGroupId.value = ''
}

function formatPlanPriceAmount(plan: BillingPlan): string {
  return Number(plan.price_amount || 0).toFixed(2)
}

function durationUnitLabel(unit: BillingDurationUnit): string {
  const labels: Record<BillingDurationUnit, string> = {
    day: t('billingPlansManagement.durationDays'),
    month: t('billingPlansManagement.durationMonths'),
    year: t('billingPlansManagement.durationYears'),
    custom: t('billingPlansManagement.durationCustom'),
  }
  return labels[unit] || labels.month
}

function formatDuration(unit: BillingDurationUnit, value: number): string {
  return `${value}${durationUnitLabel(unit)}`
}

function formatPlanPeriod(plan: BillingPlan): string {
  return formatDuration(plan.duration_unit, plan.duration_value)
}

function resolvePlanModeFromEntitlements(entitlements: BillingEntitlementsInput): PlanMode {
  const items = normalizeBillingEntitlements(entitlements)
  const hasWallet = items.some((entitlement) => entitlement.type === 'wallet_credit')
  const hasDaily = items.some(isDailyQuotaEntitlement)
  const hasMembership = items.some((entitlement) => entitlement.type === 'membership_group')
  const enabledCount = [hasWallet, hasDaily, hasMembership].filter(Boolean).length

  if (enabledCount === 0) return 'empty'
  if (enabledCount > 1) return 'mixed'
  if (hasWallet) return 'wallet'
  if (hasDaily) return 'daily'
  return 'membership'
}

function planDurationHint(plan: BillingPlan): string {
  const mode = resolvePlanModeFromEntitlements(plan.entitlements)
  const modeText = (() => {
    if (mode === 'wallet') return t('billingPlansManagement.legacyPlanDisable')
    if (mode === 'daily') return t('billingPlansManagement.periodQuotaBadge')
    if (mode === 'membership') return t('billingPlansManagement.membershipBadge')
    if (mode === 'mixed') return t('billingPlansManagement.hybridBenefits')
    return t('billingPlansManagement.noBenefitsConfigured')
  })()
  if (plan.purchase_limit_scope === 'unlimited') return t('billingPlansManagement.modeUnlimited', { mode: modeText })
  if (plan.purchase_limit_scope === 'lifetime') return t('billingPlansManagement.modeLifetime', { mode: modeText })
  return t('billingPlansManagement.modeActivePeriod', { mode: modeText })
}

function groupName(groupId: string): string {
  return userGroups.value.find((group) => group.id === groupId)?.name || groupId
}

function providerName(providerId: string): string {
  return providers.value.find((provider) => provider.id === providerId)?.name || providerId
}

function formatAllowedProviders(plan: BillingPlan): string {
  if (Array.isArray(plan.allowed_provider_ids) && plan.allowed_provider_ids.length > 0) {
    const names = plan.allowed_provider_ids.map(providerName)
    return names.length <= 2
      ? names.join('、')
      : t('billingPlansManagement.providerListSummary', {
        names: names.slice(0, 2).join('、'),
        count: names.length,
      })
  }
  return t('billingPlansManagement.noProviders')
}

function entitlementBadges(plan: BillingPlan): string[] {
  return normalizeBillingEntitlements(plan.entitlements).map((entitlement) => {
    if (entitlement.type === 'wallet_credit') {
      return t('billingPlansManagement.bonusBalanceAmount', { amount: Number(entitlement.amount_usd || 0).toFixed(2) })
    }
    if (entitlement.type === 'daily_quota') {
      const parts = []
      if (Number(entitlement.daily_quota_usd || 0) > 0) {
        parts.push(t('billingPlansManagement.quota24hAmount', { amount: Number(entitlement.daily_quota_usd || 0).toFixed(2) }))
      }
      if (Number(entitlement.five_hour_quota_usd || 0) > 0) {
        parts.push(`5H $${Number(entitlement.five_hour_quota_usd || 0).toFixed(2)}`)
      }
      if (Number(entitlement.weekly_quota_usd || 0) > 0) {
        parts.push(t('billingPlansManagement.quota7dAmount', { amount: Number(entitlement.weekly_quota_usd || 0).toFixed(2) }))
      }
      if (Number(entitlement.monthly_quota_usd || 0) > 0) {
        parts.push(t('billingPlansManagement.quota30dAmount', { amount: Number(entitlement.monthly_quota_usd || 0).toFixed(2) }))
      }
      const quotaText = parts.join(' / ') || t('billingPlansManagement.usageQuota')
      const labels = [formatAllowedProviders(plan)]
      const multiplierLabel = quotaConsumptionMultiplierLabel(entitlement)
      if (multiplierLabel) labels.push(multiplierLabel)
      return `${quotaText} · ${labels.join(' · ')}`
    }
    if (entitlement.type === 'membership_group') {
      const groups = entitlement.grant_user_groups.map(groupName).join(', ')
      return t('billingPlansManagement.membershipGroupsSummary', { groups })
    }
    return t('billingPlansManagement.unknownEntitlement')
  })
}

function isDailyQuotaEntitlement(entitlement: BillingEntitlement): entitlement is DailyQuotaEntitlement {
  return entitlement.type === 'daily_quota'
    || Boolean((entitlement as unknown as DailyQuotaEntitlement).limits)
}

function hasPackageEntitlement(entitlements: BillingEntitlementsInput): boolean {
  return hasPackageBillingEntitlement(entitlements)
}
</script>
