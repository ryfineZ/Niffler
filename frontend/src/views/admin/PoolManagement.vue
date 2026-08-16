<template>
  <div class="space-y-6 pb-8">
    <Card
      variant="default"
      class="overflow-hidden"
    >
      <!-- Header -->
      <div class="px-4 sm:px-6 py-3 sm:py-3.5 border-b border-border/60">
        <!-- Mobile -->
        <div class="flex flex-col gap-3 xl:hidden">
          <div class="min-w-0">
            <h3 class="text-base font-semibold">
              {{ t('poolManagement.title') }}
            </h3>
            <p
              v-if="poolHeaderMetaText"
              class="mt-1 text-xs text-muted-foreground"
            >
              {{ poolHeaderMetaText }}
            </p>
          </div>
          <div
            class="grid grid-cols-3 items-center gap-2"
          >
            <Select
              v-model="selectedProviderIdProxy"
              :disabled="providerSelectDisabled"
            >
              <SelectTrigger
                class="h-9 text-xs border-border/60"
                :disabled="providerSelectDisabled"
              >
                <SelectValue :placeholder="t('poolManagement.selectProvider')" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  v-for="item in poolProviders"
                  :key="item.provider_id"
                  :value="item.provider_id"
                >
                  {{ item.provider_name }}
                  <span class="text-muted-foreground ml-1">({{ item.total_keys }})</span>
                  <span
                    v-if="!item.pool_enabled"
                    class="ml-1 text-[10px] text-amber-600"
                  >{{ t('poolManagement.disabled') }}</span>
                </SelectItem>
              </SelectContent>
            </Select>
            <Select v-model="statusFilter">
              <SelectTrigger class="h-9 w-full text-xs border-border/60">
                <SelectValue :placeholder="t('poolManagement.status')" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">
                  {{ t('poolManagement.all') }}
                </SelectItem>
                <SelectItem
                  v-for="option in poolKeyStatusFilterOptions.filter((item) => item.value !== 'all')"
                  :key="option.value"
                  :value="option.value"
                >
                  {{ option.label }}
                </SelectItem>
              </SelectContent>
            </Select>
            <div class="relative min-w-0">
              <Search class="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground z-10 pointer-events-none" />
              <Input
                v-model="searchQuery"
                type="text"
                :placeholder="t('poolManagement.search')"
                class="w-full pl-8 pr-3 h-9 text-sm bg-background/50 border-border/60"
              />
            </div>
          </div>
          <div
            v-if="selectedProviderId"
            class="flex items-center gap-1"
          >
            <div class="min-w-0 flex-1 flex justify-center">
              <Button
                variant="ghost"
                size="icon"
                class="h-8 w-8 shrink-0"
                :title="t('poolManagement.add')"
                @click="showImportDialog = true"
              >
                <Upload class="w-3.5 h-3.5" />
              </Button>
            </div>
            <div class="min-w-0 flex-1 flex justify-center">
              <ProviderProxyPopover
                :open="providerProxyMobilePopoverOpen"
                :node-id="selectedProviderData?.proxy?.node_id"
                :saving="savingProviderProxy"
                :title="getProviderProxyButtonTitle()"
                @update:open="(open: boolean) => handleProviderProxyPopoverToggle('mobile', open)"
                @select="setProviderProxy"
                @clear="clearProviderProxy"
              />
            </div>
            <div class="min-w-0 flex-1 flex justify-center">
              <Button
                variant="ghost"
                size="icon"
                class="h-8 w-8 shrink-0"
                :title="t('poolManagement.scheduling')"
                @click="openSchedulingDialog()"
              >
                <SlidersHorizontal class="w-3.5 h-3.5" />
              </Button>
            </div>
            <div class="min-w-0 flex-1 flex justify-center">
              <Button
                variant="ghost"
                size="icon"
                class="h-8 w-8 shrink-0"
                :title="t('poolManagement.bulk')"
                @click="showAccountBatchDialog = true"
              >
                <Users class="w-3.5 h-3.5" />
              </Button>
            </div>
            <div class="min-w-0 flex-1 flex justify-center">
              <Button
                variant="ghost"
                size="icon"
                class="h-8 w-8 shrink-0"
                :title="t('poolManagement.editProvider')"
                @click="openProviderEditDialog"
              >
                <Edit class="w-3.5 h-3.5" />
              </Button>
            </div>
            <div class="min-w-0 flex-1 flex justify-center">
              <Button
                variant="ghost"
                size="icon"
                class="h-8 w-8 shrink-0"
                :title="t('poolManagement.editEndpoint')"
                @click="openEndpointEditDialog"
              >
                <Plug class="w-3.5 h-3.5" />
              </Button>
            </div>
            <div
              v-if="showAdaptiveHotPoolMetricsButton"
              class="min-w-0 flex-1 flex justify-center"
            >
              <Button
                variant="ghost"
                size="icon"
                class="h-8 w-8 shrink-0"
                data-testid="pool-demand-metrics-button"
                :title="t('poolManagement.metrics')"
                @click="showDemandMetricsDialog = true"
              >
                <Activity class="w-3.5 h-3.5" />
              </Button>
            </div>
            <div class="min-w-0 flex-1 flex justify-center">
              <Button
                variant="ghost"
                size="icon"
                class="h-8 w-8 shrink-0"
                :title="t('poolManagement.advanced')"
                @click="showAdvancedDialog = true"
              >
                <Settings2 class="w-3.5 h-3.5" />
              </Button>
            </div>
            <div class="min-w-0 flex-1 flex justify-center">
              <Button
                variant="ghost"
                size="icon"
                class="h-8 w-8 shrink-0"
                :class="getProviderToggleButtonClass()"
                :disabled="togglingProviderStatus"
                :title="getProviderToggleButtonTitle()"
                @click="toggleSelectedProviderStatus"
              >
                <Power class="w-3.5 h-3.5" />
              </Button>
            </div>
            <div class="min-w-0 flex-1 flex justify-center">
              <RefreshButton
                :loading="refreshCurrentPageLoading"
                :title="refreshButtonTitle"
                @click="refreshCurrentPage"
              />
            </div>
          </div>
        </div>

        <!-- Desktop -->
        <div class="hidden xl:flex items-center justify-between gap-4">
          <div class="flex items-center gap-2">
            <h3 class="text-base font-semibold">
              {{ t('poolManagement.title') }}
              <span
                v-if="poolHeaderMetaText"
                class="ml-2 text-xs font-normal text-muted-foreground"
              >
                | {{ poolHeaderMetaText }}
              </span>
            </h3>
          </div>
          <div
            class="flex items-center gap-2"
            data-testid="pool-header-actions"
          >
            <Select
              v-model="selectedProviderIdProxy"
              :disabled="providerSelectDisabled"
            >
              <SelectTrigger
                class="w-36 h-8 text-xs border-border/60"
                :disabled="providerSelectDisabled"
              >
                <SelectValue :placeholder="t('poolManagement.selectProvider')" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  v-for="item in poolProviders"
                  :key="item.provider_id"
                  :value="item.provider_id"
                >
                  {{ item.provider_name }}
                  <span class="text-muted-foreground ml-1">({{ item.total_keys }})</span>
                  <span
                    v-if="!item.pool_enabled"
                    class="ml-1 text-[10px] text-amber-600"
                  >{{ t('poolManagement.disabled') }}</span>
                </SelectItem>
              </SelectContent>
            </Select>
            <div class="h-4 w-px bg-border" />
            <div
              v-if="selectedProviderId"
              class="relative"
            >
              <Search class="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground z-10 pointer-events-none" />
              <Input
                v-model="searchQuery"
                type="text"
                :placeholder="t('poolManagement.search')"
                class="w-40 pl-8 pr-2 h-8 text-xs bg-background/50 border-border/60"
              />
            </div>
            <div
              v-if="selectedProviderId"
              class="h-4 w-px bg-border"
            />
            <button
              v-if="selectedProviderId"
              class="group inline-flex items-center gap-1.5 px-2.5 h-8 rounded-md border border-border/50 bg-muted/20 hover:bg-muted/40 hover:border-primary/40 transition-all duration-200 text-xs"
              :title="t('poolManagement.scheduling')"
              @click="openSchedulingDialog()"
            >
              <span class="text-muted-foreground/80 hidden lg:inline">{{ t('poolManagement.schedulingLabel') }}:</span>
              <span class="font-medium text-foreground/90">{{ poolSchedulingLabel }}</span>
              <ChevronDown class="w-3 h-3 text-muted-foreground/70 group-hover:text-foreground transition-colors" />
            </button>
            <div
              v-if="selectedProviderId"
              class="h-4 w-px bg-border"
            />
            <Button
              v-if="selectedProviderId"
              variant="ghost"
              size="icon"
              class="h-8 w-8"
              :title="t('poolManagement.add')"
              @click="showImportDialog = true"
            >
              <Upload class="w-3.5 h-3.5" />
            </Button>
            <ProviderProxyPopover
              v-if="selectedProviderId"
              :open="providerProxyDesktopPopoverOpen"
              :node-id="selectedProviderData?.proxy?.node_id"
              :saving="savingProviderProxy"
              :title="getProviderProxyButtonTitle()"
              @update:open="(open: boolean) => handleProviderProxyPopoverToggle('desktop', open)"
              @select="setProviderProxy"
              @clear="clearProviderProxy"
            />
            <Button
              v-if="selectedProviderId"
              variant="ghost"
              size="icon"
              class="h-8 w-8"
              :title="t('poolManagement.editProvider')"
              @click="openProviderEditDialog"
            >
              <Edit class="w-3.5 h-3.5" />
            </Button>
            <Button
              v-if="selectedProviderId"
              variant="ghost"
              size="icon"
              class="h-8 w-8"
              :title="t('poolManagement.editEndpoint')"
              @click="openEndpointEditDialog"
            >
              <Plug class="w-3.5 h-3.5" />
            </Button>
            <Button
              v-if="showAdaptiveHotPoolMetricsButton"
              variant="ghost"
              size="icon"
              class="h-8 w-8"
              data-testid="pool-demand-metrics-button"
              :title="t('poolManagement.metrics')"
              @click="showDemandMetricsDialog = true"
            >
              <Activity class="w-3.5 h-3.5" />
            </Button>
            <Button
              v-if="selectedProviderId"
              variant="ghost"
              size="icon"
              class="h-8 w-8"
              :title="t('poolManagement.advanced')"
              @click="showAdvancedDialog = true"
            >
              <Settings2 class="w-3.5 h-3.5" />
            </Button>
            <Button
              v-if="selectedProviderId"
              variant="ghost"
              size="icon"
              class="h-8 w-8"
              :title="t('poolManagement.bulk')"
              @click="showAccountBatchDialog = true"
            >
              <Users class="w-3.5 h-3.5" />
            </Button>
            <Button
              v-if="selectedProviderId"
              variant="ghost"
              size="icon"
              class="h-8 w-8"
              :class="getProviderToggleButtonClass()"
              :disabled="togglingProviderStatus"
              :title="getProviderToggleButtonTitle()"
              @click="toggleSelectedProviderStatus"
            >
              <Power class="w-3.5 h-3.5" />
            </Button>
            <RefreshButton
              :loading="refreshCurrentPageLoading"
              :title="refreshButtonTitle"
              @click="refreshCurrentPage"
            />
          </div>
        </div>
      </div>

      <!-- Loading (initial) -->
      <div
        v-if="overviewLoading"
        class="flex items-center justify-center py-16"
      >
        <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-primary" />
      </div>

      <!-- No providers -->
      <div
        v-else-if="poolProviders.length === 0"
        class="flex flex-col items-center justify-center py-16 text-center"
      >
        <div class="mx-auto flex h-16 w-16 items-center justify-center rounded-full bg-muted">
          <Database class="h-8 w-8 text-muted-foreground" />
        </div>
        <p class="text-sm text-muted-foreground mt-4">
          {{ t('poolManagement.noProvider') }}
        </p>
        <p class="text-xs text-muted-foreground mt-1">
          {{ t('poolManagement.addProviderHint') }}
        </p>
      </div>

      <!-- No provider selected -->
      <div
        v-else-if="!selectedProviderId"
        class="flex flex-col items-center justify-center py-16 text-center"
      >
        <div class="mx-auto flex h-16 w-16 items-center justify-center rounded-full bg-muted">
          <Database class="h-8 w-8 text-muted-foreground" />
        </div>
        <p class="text-sm text-muted-foreground mt-4">
          {{ t('poolManagement.selectProviderHint') }}
        </p>
      </div>

      <!-- Loading keys -->
      <div
        v-else-if="keysLoading && keyPage.keys.length === 0"
        class="flex items-center justify-center py-16"
      >
        <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-primary" />
      </div>

      <template v-else>
        <div
          v-if="poolSummaryTotal > 0"
          class="border-b border-border/50 bg-muted/[0.12] px-4 py-3 sm:px-6"
        >
          <div class="space-y-3">
            <div class="flex flex-wrap items-center gap-2">
              <span class="mr-1 text-xs font-medium text-muted-foreground">{{ t('poolManagement.subscription') }}</span>
              <Button
                variant="outline"
                size="sm"
                class="h-7 rounded-full px-3 text-xs"
                :class="planTypeFilter === 'all' ? 'border-primary/60 bg-primary/10 text-primary' : ''"
                @click="planTypeFilter = 'all'"
              >
                {{ t('poolManagement.allCount', { count: poolSummaryTotal }) }}
              </Button>
              <Button
                v-for="item in poolPlanSummaryItems"
                :key="item.code"
                variant="outline"
                size="sm"
                class="h-7 rounded-full px-3 text-xs"
                :class="planTypeFilter === item.code ? 'border-primary/60 bg-primary/10 text-primary' : ''"
                @click="selectPoolPlanFilter(item.code)"
              >
                {{ item.label }} {{ item.count }}
              </Button>
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <span class="mr-1 text-xs font-medium text-muted-foreground">{{ t('poolManagement.accountStatus') }}</span>
              <Button
                variant="outline"
                size="sm"
                class="h-7 rounded-full px-3 text-xs"
                :class="statusFilter === 'all' ? 'border-primary/60 bg-primary/10 text-primary' : ''"
                @click="statusFilter = 'all'"
              >
                {{ t('poolManagement.allCount', { count: poolSummaryTotal }) }}
              </Button>
              <Button
                v-for="item in poolStatusSummaryItems"
                :key="item.code"
                variant="outline"
                size="sm"
                class="h-7 rounded-full px-3 text-xs"
                :class="statusFilter === item.code ? 'border-primary/60 bg-primary/10 text-primary' : ''"
                @click="selectPoolStatusFilter(item.code)"
              >
                {{ item.label }} {{ item.count }}
              </Button>
            </div>
          </div>
        </div>

        <div
          v-if="keyPage.keys.length > 0"
          class="flex flex-col gap-2 border-b border-border/50 bg-background/70 px-4 py-2.5 sm:px-6 xl:flex-row xl:items-center xl:justify-between"
        >
          <label class="inline-flex items-center gap-2 text-xs text-muted-foreground">
            <Checkbox
              :checked="isCurrentPageFullySelected"
              :indeterminate="isCurrentPagePartiallySelected"
              :disabled="keysLoading || poolSelectionActionBusy || allFilteredPoolKeysSelected"
              data-testid="pool-select-current-page"
              @update:checked="toggleSelectCurrentPageKeys"
            />
            <span>{{ t('poolManagement.currentPage') }} {{ keyPage.keys.length }}，{{ t('poolManagement.selected') }} {{ selectedPoolKeyCount }}</span>
          </label>
          <div class="flex flex-wrap items-center gap-2">
            <Button
              v-if="keyPage.total > keyPage.keys.length && !allFilteredPoolKeysSelected"
              variant="outline"
              size="sm"
              class="h-7 px-2 text-xs"
              :disabled="keyPage.total === 0 || poolSelectionActionBusy"
              data-testid="pool-select-filtered-results"
              @click="selectAllFilteredPoolKeys"
            >
              {{ t('poolManagement.selectAll') }} {{ keyPage.total }}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="h-7 px-2 text-xs"
              :disabled="selectedPoolKeyCount === 0 || poolSelectionActionBusy"
              @click="clearPoolKeySelection"
            >
              {{ t('poolManagement.clear') }}
            </Button>
            <Button
              v-if="showBulkClearCooldownAction"
              variant="outline"
              size="sm"
              class="h-7 px-2.5 text-xs"
              :disabled="selectedPoolKeyCount === 0 || poolSelectionActionBusy"
              data-testid="pool-bulk-clear-cooldown-selected"
              @click="clearCooldownSelectedPoolKeys"
            >
              {{ clearingSelectedCooldown ? t('poolManagement.clearing') : `${t('poolManagement.clearCooldown')} ${selectedPoolKeyCount}` }}
            </Button>
            <Button
              v-if="showBulkEnableAction"
              variant="outline"
              size="sm"
              class="h-7 px-2.5 text-xs"
              :disabled="selectedPoolKeyCount === 0 || poolSelectionActionBusy"
              data-testid="pool-bulk-enable-selected"
              @click="enableSelectedPoolKeys"
            >
              {{ updatingSelectedKeyStatus === 'enable' ? t('poolManagement.enabling') : `${t('poolManagement.enableSelected')} ${selectedPoolKeyCount}` }}
            </Button>
            <Button
              variant="outline"
              size="sm"
              class="h-7 px-2.5 text-xs"
              :disabled="selectedPoolKeyCount === 0 || poolSelectionActionBusy"
              data-testid="pool-bulk-disable-selected"
              @click="disableSelectedPoolKeys"
            >
              {{ updatingSelectedKeyStatus === 'disable' ? t('poolManagement.disabling') : `${t('poolManagement.disableSelected')} ${selectedPoolKeyCount}` }}
            </Button>
            <Button
              variant="destructive"
              size="sm"
              class="h-7 px-2.5 text-xs"
              :disabled="selectedPoolKeyCount === 0 || poolSelectionActionBusy"
              data-testid="pool-bulk-delete-selected"
              @click="deleteSelectedPoolKeys"
            >
              {{ deletingSelectedKeys ? t('poolManagement.deleting') : `${t('poolManagement.deleteSelected')} ${selectedPoolKeyCount}` }}
            </Button>
          </div>
        </div>

        <!-- Desktop table -->
        <div
          v-if="keyPage.keys.length > 0 || hasPoolKeyFilters"
          class="hidden xl:block overflow-x-auto"
        >
          <Table
            data-testid="pool-accounts-desktop-table"
            class="w-full table-auto"
          >
            <TableHeader>
              <TableRow class="border-b border-border/60 hover:bg-transparent">
                <TableHead
                  class="w-px px-3 text-center whitespace-nowrap"
                >
                  <Checkbox
                    :checked="isCurrentPageFullySelected"
                    :indeterminate="isCurrentPagePartiallySelected"
                    :disabled="keyPage.keys.length === 0 || keysLoading || poolSelectionActionBusy || allFilteredPoolKeysSelected"
                    :title="t('poolManagement.selectPageAccounts')"
                    @update:checked="toggleSelectCurrentPageKeys"
                  />
                </TableHead>
                <SortableTableHead
                  class="min-w-56 font-semibold whitespace-nowrap"
                  :sortable="false"
                >
                  {{ t('poolManagement.account') }}
                </SortableTableHead>
                <SortableTableHead
                  v-if="showAccountQuotaColumn"
                  data-testid="pool-account-quota-heading"
                  class="w-px min-w-44 font-semibold whitespace-nowrap"
                  :sortable="false"
                >
                  {{ t('poolManagement.quota') }}
                </SortableTableHead>
                <SortableTableHead
                  class="w-px px-2 font-semibold text-center whitespace-nowrap"
                  :sortable="false"
                >
                  <div class="flex items-center justify-center gap-1.5">
                    <button
                      v-if="showCodexStatsModeToggle"
                      type="button"
                      class="inline-flex h-6 w-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground"
                      :title="poolStatsMode === 'current_cycle' ? t('poolManagement.switchAccountTotal') : t('poolManagement.switchLocalCycle')"
                      :aria-label="poolStatsMode === 'current_cycle' ? t('poolManagement.switchAccountTotal') : t('poolManagement.switchLocalCycle')"
                      :aria-pressed="poolStatsMode === 'current_cycle'"
                      data-testid="pool-stats-mode-control"
                      @click.stop="togglePoolStatsMode"
                    >
                      <Repeat2 class="h-3.5 w-3.5" />
                    </button>
                    <span>{{ t('poolManagement.stats') }}</span>
                  </div>
                </SortableTableHead>
                <SortableTableHead
                  class="w-px font-semibold text-center whitespace-nowrap"
                  column-key="imported_at"
                  :active-key="sortBy"
                  :direction="sortOrder"
                  default-direction="desc"
                  align="center"
                  :title="t('poolManagement.sortImported')"
                  @sort="handleTableSort"
                >
                  {{ t('poolManagement.importedAt') }}
                </SortableTableHead>
                <SortableTableHead
                  class="w-px font-semibold text-center whitespace-nowrap"
                  column-key="last_used_at"
                  :active-key="sortBy"
                  :direction="sortOrder"
                  default-direction="desc"
                  align="center"
                  :title="t('poolManagement.sortLastUsed')"
                  @sort="handleTableSort"
                >
                  {{ t('poolManagement.lastUsed') }}
                </SortableTableHead>
                <SortableTableHead
                  class="w-px font-semibold text-center whitespace-nowrap"
                  column-key="score"
                  :active-key="sortBy"
                  :direction="sortOrder"
                  default-direction="desc"
                  align="center"
                  :title="t('poolManagement.sortScore')"
                  @sort="handleTableSort"
                >
                  {{ t('poolManagement.score') }}
                </SortableTableHead>
                <SortableTableHead
                  class="w-px font-semibold text-center whitespace-nowrap"
                  column-key="status"
                  :sortable="false"
                  align="center"
                  :filter-active="statusFilter !== 'all'"
                  :filter-title="t('poolManagement.filterStatus')"
                  filter-content-class="w-44 p-1 rounded-2xl border-border bg-card text-foreground shadow-2xl backdrop-blur-xl"
                >
                  {{ t('poolManagement.status') }}
                  <template #filter="{ close }">
                    <TableFilterMenu
                      v-model="statusFilter"
                      :options="poolKeyStatusFilterOptions"
                      @select="close"
                    />
                  </template>
                </SortableTableHead>
                <SortableTableHead
                  class="w-px px-2 font-semibold text-center whitespace-nowrap"
                  :sortable="false"
                >
                  {{ t('poolManagement.actions') }}
                </SortableTableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableRow
                v-for="key in keyPage.keys"
                :key="key.key_id"
                class="border-b border-border/40 last:border-b-0 hover:bg-muted/30 transition-colors"
                :class="keyUiStateMap[key.key_id]?.rowClass || ''"
              >
                <TableCell class="w-px px-3 py-3 text-center align-middle whitespace-nowrap">
                  <Checkbox
                    :checked="isPoolKeySelected(key.key_id)"
                    :disabled="poolSelectionActionBusy || allFilteredPoolKeysSelected"
                    :data-testid="`pool-key-select-${key.key_id}`"
                    :aria-label="t('poolManagement.selectAccount', { name: getPoolAccountDisplayName(key) })"
                    @update:checked="(checked: boolean) => togglePoolKeySelection(key.key_id, checked)"
                  />
                </TableCell>
                <TableCell
                  class="min-w-56 py-3"
                >
                  <div class="min-w-0">
                    <div class="flex items-center gap-1.5 min-w-0">
                      <button
                        type="button"
                        class="min-w-0 truncate text-left text-sm transition-colors hover:text-primary"
                    :title="t('poolManagement.copyAccount', { name: getPoolAccountDisplayName(key) })"
                        @click.stop="copyPoolAccountDisplay(key)"
                      >
                        {{ getPoolAccountDisplayName(key) }}
                      </button>
                    </div>
                    <div class="flex items-center flex-wrap gap-1 text-[11px] text-muted-foreground mt-0.5 min-w-0">
                      <input
                        v-if="editingPriorityKeyId === key.key_id"
                        :value="editingPriorityValue"
                        type="number"
                        min="1"
                        max="999999"
                        autofocus
                        class="h-[18px] w-10 rounded border border-primary/50 bg-background px-1 text-[10px] tabular-nums text-foreground outline-none ring-1 ring-primary/30 shrink-0 [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
                        @input="(e) => editingPriorityValue = Number((e.target as HTMLInputElement).value || 0)"
                        @blur="(e) => finishEditInternalPriority(key, e)"
                        @keydown.enter.prevent="(e) => finishEditInternalPriority(key, e)"
                        @keydown.esc.prevent="cancelEditInternalPriority"
                      >
                      <button
                        v-else
                        type="button"
                        class="h-4 px-1 rounded text-[10px] tabular-nums text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors shrink-0"
                        :title="t('poolManagement.editPriority')"
                        @click="startEditInternalPriority(key)"
                      >
                        P{{ key.internal_priority ?? 50 }}
                      </button>
                      <Button
                        v-if="canExportOAuthCredential(key)"
                        variant="ghost"
                        size="icon"
                        class="h-4 w-4 shrink-0"
                        :title="t('poolManagement.downloadOAuth')"
                        @click.stop="downloadRefreshToken(key)"
                      >
                        <Download class="w-2.5 h-2.5" />
                      </Button>
                      <Button
                        v-else
                        variant="ghost"
                        size="icon"
                        class="h-4 w-4 shrink-0"
                        :title="t('poolManagement.copyKey')"
                        @click.stop="copyFullKey(key)"
                      >
                        <Copy class="w-2.5 h-2.5" />
                      </Button>
                      <span class="font-mono">
                        {{ getProviderMaskedSecretLabel(key, selectedProviderType) }}
                      </span>
                      <template v-if="keyUiStateMap[key.key_id]?.showOAuthRefreshControl">
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-4 w-4 shrink-0"
                          :disabled="refreshingOAuthKeyId === key.key_id || !keyUiStateMap[key.key_id]?.canRefreshToken"
                          :title="keyUiStateMap[key.key_id]?.oauthRefreshButtonTitle || ''"
                          @click.stop="handleRefreshOAuth(key)"
                        >
                          <RefreshCw
                            class="w-2.5 h-2.5"
                            :class="{ 'animate-spin': refreshingOAuthKeyId === key.key_id }"
                          />
                        </Button>
                        <span
                          v-if="keyUiStateMap[key.key_id]?.visibleOAuthState"
                          class="text-[10px]"
                          :class="{
                            'text-destructive': keyUiStateMap[key.key_id]?.visibleOAuthState?.isInvalid || keyUiStateMap[key.key_id]?.visibleOAuthState?.isExpired,
                            'text-warning': keyUiStateMap[key.key_id]?.visibleOAuthState?.isExpiringSoon && !keyUiStateMap[key.key_id]?.visibleOAuthState?.isExpired && !keyUiStateMap[key.key_id]?.visibleOAuthState?.isInvalid,
                            'text-muted-foreground': !keyUiStateMap[key.key_id]?.visibleOAuthState?.isExpired && !keyUiStateMap[key.key_id]?.visibleOAuthState?.isExpiringSoon && !keyUiStateMap[key.key_id]?.visibleOAuthState?.isInvalid
                          }"
                          :title="keyUiStateMap[key.key_id]?.oauthStatusTitle || ''"
                        >
                          {{ keyUiStateMap[key.key_id]?.visibleOAuthState?.text }}
                        </span>
                      </template>
                      <Badge
                        v-if="keyUiStateMap[key.key_id]?.planLabel"
                        variant="outline"
                        class="text-[9px] px-1 py-0 h-4 shrink-0"
                        :class="keyUiStateMap[key.key_id]?.planClass || ''"
                      >
                        {{ keyUiStateMap[key.key_id]?.planLabel }}
                      </Badge>
                      <Badge
                        v-if="keyUiStateMap[key.key_id]?.oauthOrgBadge"
                        variant="secondary"
                        class="text-[9px] px-1 py-0 h-4 shrink-0"
                        :title="keyUiStateMap[key.key_id]?.oauthOrgBadge?.title"
                      >
                        {{ keyUiStateMap[key.key_id]?.oauthOrgBadge?.label }}
                      </Badge>
                    </div>
                  </div>
                </TableCell>
                <TableCell
                  v-if="showAccountQuotaColumn"
                  class="w-px min-w-44 py-3 align-middle"
                >
                  <div
                    v-if="quotaProgressMap[key.key_id]?.length"
                    class="max-w-[208px] space-y-2"
                  >
                    <div
                      v-for="(item, idx) in quotaProgressMap[key.key_id]"
                      :key="`${key.key_id}-quota-${idx}`"
                      class="flex flex-col gap-1 min-w-[140px] max-w-[208px]"
                    >
                      <div class="flex items-center justify-between text-[10px] leading-none">
                        <span
                          data-testid="pool-quota-progress-label"
                          class="text-muted-foreground font-medium shrink-0"
                        >{{ getQuotaProgressLabel(item.label) }}</span>
                        <span
                          v-if="getQuotaProgressResetDisplayText(item)"
                          data-testid="pool-quota-reset-text"
                          class="text-muted-foreground/80 tabular-nums truncate"
                          :title="getQuotaProgressResetDisplayText(item)"
                        >{{ getQuotaProgressResetDisplayText(item) }}</span>
                      </div>
                      <div class="flex items-center gap-1.5">
                        <div class="relative flex-1 h-1.5 rounded-full bg-border overflow-hidden">
                          <div
                            class="absolute left-0 top-0 h-full rounded-full transition-all duration-300"
                            :class="getQuotaRemainingBarColorByRemaining(item.remainingPercent)"
                            :style="{ width: `${item.remainingPercent}%` }"
                          />
                        </div>
                        <span
                          data-testid="pool-quota-meter-text"
                          class="shrink-0 text-[10px] font-medium tabular-nums leading-none"
                          :class="getQuotaRemainingClassByRemaining(item.remainingPercent)"
                        >{{ getQuotaProgressMeterDisplayText(item) }}</span>
                      </div>
                    </div>
                  </div>
                  <span
                    v-else-if="keyUiStateMap[key.key_id]?.quotaFallbackText"
                    :class="keyUiStateMap[key.key_id]?.quotaTextClass || ''"
                  >
                    {{ keyUiStateMap[key.key_id]?.quotaFallbackText }}
                  </span>
                  <span
                    v-else
                    class="text-xs text-muted-foreground"
                  >-</span>
                  <div
                    v-if="keyUiStateMap[key.key_id]?.quotaFreshnessText"
                    data-testid="pool-quota-freshness"
                    class="mt-2 text-[10px] leading-none tabular-nums"
                    :class="keyUiStateMap[key.key_id]?.quotaFreshnessStale
                      ? 'text-amber-600 dark:text-amber-400'
                      : 'text-muted-foreground'"
                  >
                    {{ keyUiStateMap[key.key_id]?.quotaFreshnessText }}
                  </div>
                  <div
                    v-if="selectedProviderType === 'codex'"
                    class="mt-2 flex items-center justify-between gap-2 text-[10px]"
                  >
                    <span class="text-muted-foreground tabular-nums">
                      {{ t('poolManagement.resetCreditCount', { count: getCodexResetCreditCount(key) ?? '-' }) }}
                    </span>
                    <Button
                      variant="ghost"
                      size="sm"
                      class="h-6 px-2 text-[10px]"
                      :disabled="!canResetCodexQuota(key) || resettingCodexQuotaKeyId !== null"
                      :title="getCodexQuotaResetButtonTitle(key)"
                      data-testid="pool-reset-codex-quota"
                      @click="handleResetCodexQuota(key)"
                    >
                      <Loader2
                        v-if="resettingCodexQuotaKeyId === key.key_id"
                        class="mr-1 h-3 w-3 animate-spin"
                      />
                      {{ resettingCodexQuotaKeyId === key.key_id ? t('poolManagement.resettingQuota') : t('poolManagement.resetQuota') }}
                    </Button>
                  </div>
                </TableCell>
                <TableCell class="w-px py-3 px-2 align-middle whitespace-nowrap">
                  <div
                    v-if="isPoolKeyCycleStatsDisplay(key)"
                    class="mx-auto w-[188px] overflow-x-auto text-[10px] leading-4"
                    data-testid="pool-stats-cycle-groups"
                  >
                    <div
                      v-if="getPoolKeyCycleStatsGroups(key).length > 0"
                      class="grid min-h-16 min-w-[188px] items-center gap-x-1"
                      :style="getPoolKeyCycleStatsGridStyle(key)"
                      data-testid="pool-stats-cycle-grid"
                    >
                      <span aria-hidden="true" />
                      <span
                        v-for="group in getPoolKeyCycleStatsGroups(key)"
                        :key="`${key.key_id}-${group.code}-desktop-cycle-heading`"
                        class="text-center text-[9px] font-semibold text-muted-foreground/80"
                        :data-testid="`pool-stats-cycle-group-${group.code}`"
                      >{{ group.label }}</span>

                      <template
                        v-for="row in getPoolKeyCycleStatsRows(key)"
                        :key="`${key.key_id}-${row.key}-desktop-cycle-row`"
                      >
                        <span class="text-muted-foreground truncate">{{ row.label }}</span>
                        <span
                          v-for="(metric, metricIndex) in row.values"
                          :key="`${key.key_id}-${row.key}-${metricIndex}-desktop-cycle-value`"
                          class="min-w-0 truncate text-center tabular-nums text-foreground/90"
                          :class="metric.missing ? 'text-muted-foreground/80' : ''"
                          :data-testid="`pool-stats-${getPoolKeyCycleStatsGroups(key)[metricIndex]?.code}-${row.key}`"
                          :title="metric.value"
                        >{{ metric.value }}</span>
                      </template>
                    </div>
                    <div
                      v-else
                      class="flex min-h-16 items-center justify-center text-muted-foreground"
                    >
                      {{ t('quotaUi.noWindows') }}
                    </div>
                  </div>
                  <div
                    v-else
                    class="grid min-h-16 w-[188px] grid-rows-4 gap-0 mx-auto text-[10px] leading-4"
                    data-testid="pool-stats-account-total"
                  >
                    <div
                      class="invisible h-4"
                      aria-hidden="true"
                    >
                      -
                    </div>
                    <div
                      v-for="metric in getPoolKeyAccountStatsMetrics(key)"
                      :key="`${key.key_id}-${metric.key}-account-total`"
                      class="grid grid-cols-[64px_124px] items-center"
                    >
                      <span class="text-muted-foreground truncate">{{ metric.label }}</span>
                      <span
                        class="min-w-0 truncate text-center tabular-nums text-foreground/90"
                        :title="metric.value"
                      >
                        {{ metric.value }}
                      </span>
                    </div>
                  </div>
                </TableCell>
                <TableCell class="w-px py-3 text-center whitespace-nowrap">
                  <span class="text-[10px] text-muted-foreground whitespace-nowrap">
                    {{ keyUiStateMap[key.key_id]?.importedAtRelative || '-' }}
                  </span>
                </TableCell>
                <TableCell class="w-px py-3 text-center whitespace-nowrap">
                  <span class="text-[10px] text-muted-foreground whitespace-nowrap">
                    {{ keyUiStateMap[key.key_id]?.lastUsedRelative || '-' }}
                  </span>
                </TableCell>
                <TableCell class="w-px py-3 text-center align-middle whitespace-nowrap">
                  <div class="inline-flex items-center justify-center gap-1">
                    <span class="font-mono text-xs tabular-nums text-foreground/90">
                      {{ formatPoolScore(key.pool_score?.score) }}
                    </span>
                    <Popover
                      v-if="key.pool_score"
                      :open="scoreDesktopPopoverOpenKeyId === key.key_id"
                      @update:open="(open: boolean) => handleScoreDesktopPopoverToggle(key.key_id, open)"
                    >
                      <PopoverTrigger as-child>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-5 w-5 rounded-full border border-transparent text-muted-foreground/80 hover:border-border/60 hover:bg-muted/60 hover:text-foreground"
                    :title="t('poolManagement.scoreDetails')"
                    :aria-label="t('poolManagement.scoreDetails')"
                          @click.stop
                        >
                          <CircleHelp class="h-3.5 w-3.5" />
                        </Button>
                      </PopoverTrigger>
                      <PopoverContent
                        v-if="scoreDesktopPopoverOpenKeyId === key.key_id"
                        class="w-[22rem] max-w-[calc(100vw-1rem)] overflow-hidden rounded-xl border-border/60 bg-card/95 p-0 text-card-foreground shadow-xl shadow-black/5 backdrop-blur supports-[backdrop-filter]:bg-card/90"
                        side="bottom"
                        align="end"
                        :side-offset="8"
                      >
                        <div class="text-left">
                          <div class="flex items-center justify-between gap-3 border-b border-border/60 bg-muted/30 px-3 py-2.5">
                            <span class="text-xs font-semibold text-foreground">{{ t('poolManagement.scoreDetails') }}</span>
                            <span class="font-mono text-xs tabular-nums text-foreground/90">
                              {{ formatPoolScore(key.pool_score?.score) }}
                            </span>
                          </div>
                          <div class="space-y-2 px-3 py-2.5">
                            <div class="flex flex-wrap items-center gap-1.5">
                              <Badge
                                variant="outline"
                                class="h-5 rounded-md border-border/60 bg-background/60 px-2 text-[10px] font-normal"
                              >
                                {{ getPoolScoreHardStateLabel(key.pool_score?.hard_state) }}
                              </Badge>
                              <Badge
                                variant="secondary"
                                class="h-5 rounded-md px-2 text-[10px] font-normal"
                              >
                                {{ getPoolScoreProbeStatusLabel(key.pool_score?.probe_status) }}
                              </Badge>
                              <span class="text-[10px] text-muted-foreground">
                                {{ t('poolManagement.updatedAt', { time: formatUnixSeconds(key.pool_score?.updated_at) }) }}
                              </span>
                            </div>
                            <pre class="max-h-56 overflow-auto rounded-md border border-border/50 bg-muted/30 px-3 py-2 font-mono text-[11px] leading-5 text-muted-foreground whitespace-pre-wrap break-words">{{ formatPoolScoreReason(key.pool_score?.score_reason) }}</pre>
                          </div>
                        </div>
                      </PopoverContent>
                    </Popover>
                  </div>
                </TableCell>
                <TableCell class="w-px py-3 text-center whitespace-nowrap">
                  <Badge
                    :variant="keyUiStateMap[key.key_id]?.schedulingBadgeVariant || 'default'"
                    class="text-[10px]"
                    :title="keyUiStateMap[key.key_id]?.schedulingTitle || ''"
                  >
                    {{ keyUiStateMap[key.key_id]?.schedulingBadgeLabel }}
                  </Badge>
                </TableCell>
                <TableCell class="w-px py-3 px-2 align-middle whitespace-nowrap">
                  <div class="flex justify-center gap-0.5">
                    <Button
                      variant="ghost"
                      size="icon"
                      class="h-7 w-7 text-primary"
                      :disabled="!key.is_active || preparingAccountTestKeyId !== null || !selectedProviderData"
                      :title="preparingAccountTestKeyId === key.key_id ? t('poolManagement.preparingAccountTest') : t('poolManagement.testAccount')"
                      data-testid="pool-test-account"
                      @click="handleTestAccount(key)"
                    >
                      <Loader2
                        v-if="preparingAccountTestKeyId === key.key_id"
                        class="w-3.5 h-3.5 animate-spin"
                      />
                      <Play
                        v-else
                        class="w-3.5 h-3.5"
                      />
                    </Button>
                    <Button
                      v-if="key.cooldown_reason"
                      variant="ghost"
                      size="icon"
                      class="h-7 w-7 text-muted-foreground hover:text-green-600"
                      :title="t('poolManagement.clearCooldown')"
                      @click="clearCooldown(key.key_id)"
                    >
                      <RefreshCw class="w-3.5 h-3.5" />
                    </Button>
                    <Button
                      v-if="canResetCycleStats(key)"
                      variant="ghost"
                      size="icon"
                      class="h-7 w-7 text-muted-foreground hover:text-foreground"
                      :disabled="resettingCycleKeyId === key.key_id"
                      :title="t('poolManagement.resetCycle')"
                      data-testid="pool-reset-cycle-stats"
                      @click="handleResetCycleStats(key)"
                    >
                      <RotateCcw
                        class="w-3.5 h-3.5"
                        :class="{ 'animate-spin': resettingCycleKeyId === key.key_id }"
                      />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      class="h-7 w-7"
                      :title="t('poolManagement.modelPermissions')"
                      @click="handleKeyPermissions(key)"
                    >
                      <Shield class="w-3.5 h-3.5" />
                    </Button>
                    <Popover
                      :open="proxyDesktopPopoverOpenKeyId === key.key_id"
                      @update:open="(v: boolean) => handleProxyDesktopPopoverToggle(key.key_id, v)"
                    >
                      <PopoverTrigger as-child>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-7 w-7"
                          :class="key.proxy?.node_id ? 'text-blue-500' : ''"
                          :disabled="savingProxyKeyId === key.key_id"
                    :title="key.proxy?.node_id ? t('poolManagement.proxyValue', { name: getKeyProxyNodeName(key) }) : t('poolManagement.setProxy')"
                          @click.stop
                        >
                          <Globe class="w-3.5 h-3.5" />
                        </Button>
                      </PopoverTrigger>
                      <PopoverContent
                        class="w-72 p-3"
                        side="bottom"
                        align="end"
                      >
                        <div class="space-y-2">
                          <div class="flex items-center justify-between">
                            <span class="text-xs font-medium">{{ t('poolManagement.proxyNode') }}</span>
                            <Button
                              v-if="key.proxy?.node_id"
                              variant="ghost"
                              size="sm"
                              class="h-6 px-2 text-[10px] text-muted-foreground"
                              :disabled="savingProxyKeyId === key.key_id"
                              @click="clearKeyProxy(key)"
                            >
                              {{ t('poolManagement.clear') }}
                            </Button>
                          </div>
                          <ProxyNodeSelect
                            :model-value="key.proxy?.node_id || ''"
                            trigger-class="h-8"
                            @update:model-value="(v: string) => setKeyProxy(key, v)"
                          />
                          <p class="text-[10px] text-muted-foreground">
                            {{ key.proxy?.node_id ? t('poolManagement.independentProxy') : t('poolManagement.providerProxyFallback') }}
                          </p>
                        </div>
                      </PopoverContent>
                    </Popover>
                    <Button
                      variant="ghost"
                      size="icon"
                      class="h-7 w-7"
                      :title="t('poolManagement.editAccount')"
                      @click="handleEditKey(key)"
                    >
                      <SquarePen class="w-3.5 h-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      class="h-7 w-7 text-foreground hover:text-foreground"
                      :disabled="togglingKeyId === key.key_id"
                      :title="key.is_active ? t('poolManagement.disable') : t('poolManagement.enable')"
                      @click="toggleKeyActive(key)"
                    >
                      <Power class="w-3.5 h-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      class="h-7 w-7 text-destructive hover:text-destructive"
                      :disabled="deletingKeyId === key.key_id"
                      :title="t('poolManagement.deleteAccount')"
                      @click="handleDeleteKey(key)"
                    >
                      <Trash2 class="w-3.5 h-3.5" />
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </div>

        <!-- Mobile card list -->
        <div
          v-if="keyPage.keys.length > 0"
          class="xl:hidden divide-y divide-border/40"
        >
          <div
            v-for="key in keyPage.keys"
            :key="key.key_id"
            class="p-4 sm:p-5 hover:bg-muted/30 transition-colors"
            :class="keyUiStateMap[key.key_id]?.rowClass || ''"
          >
            <div class="space-y-3">
              <div class="flex items-start gap-2.5">
                <Checkbox
                  :checked="isPoolKeySelected(key.key_id)"
                  :disabled="poolSelectionActionBusy || allFilteredPoolKeysSelected"
                  :data-testid="`pool-key-select-mobile-${key.key_id}`"
                    :aria-label="t('poolManagement.selectAccount', { name: getPoolAccountDisplayName(key) })"
                  class="mt-0.5 shrink-0"
                  @update:checked="(checked: boolean) => togglePoolKeySelection(key.key_id, checked)"
                />
                <button
                  type="button"
                  class="min-w-0 flex-1 truncate text-left text-sm font-medium transition-colors hover:text-primary"
                    :title="t('poolManagement.copyAccount', { name: getPoolAccountDisplayName(key) })"
                  @click.stop="copyPoolAccountDisplay(key)"
                >
                  {{ getPoolAccountDisplayName(key) }}
                </button>
              </div>

              <div class="flex flex-wrap items-center gap-1.5">
                <Badge
                  :variant="keyUiStateMap[key.key_id]?.schedulingBadgeVariant || 'default'"
                  class="text-[10px] shrink-0"
                  :title="keyUiStateMap[key.key_id]?.schedulingTitle || ''"
                >
                  {{ keyUiStateMap[key.key_id]?.schedulingBadgeLabel }}
                </Badge>
                <span
                  v-if="key.cooldown_ttl_seconds"
                  class="inline-flex items-center rounded-full border border-red-500/30 bg-red-500/10 px-2 py-0.5 text-[10px] font-medium leading-4 text-red-700 dark:text-red-300"
                >
                    {{ t('poolManagement.cooldownValue', { value: formatTTL(key.cooldown_ttl_seconds) }) }}
                </span>
                <template
                  v-for="item in keyUiStateMap[key.key_id]?.mobileTagItems || []"
                  :key="`${key.key_id}-${item.key}`"
                >
                  <button
                    v-if="item.key === 'priority'"
                    type="button"
                    class="inline-flex max-w-full items-center rounded-full border px-2 py-0.5 text-[10px] font-medium leading-4"
                    :class="`${getMobileTagClass(item)} hover:border-primary/40 hover:text-foreground`"
                    :title="t('poolManagement.editPriorityValue', { name: item.label })"
                    @click="quickEditInternalPriority(key)"
                  >
                    {{ item.label }}
                  </button>
                  <Badge
                    v-else-if="item.key === 'plan'"
                    variant="outline"
                    class="text-[9px] px-1 py-0 h-4 shrink-0"
                    :class="keyUiStateMap[key.key_id]?.planClass || ''"
                  >
                    {{ item.label }}
                  </Badge>
                  <Badge
                    v-else-if="item.key === 'org'"
                    variant="secondary"
                    class="text-[9px] px-1 py-0 h-4 shrink-0"
                    :title="keyUiStateMap[key.key_id]?.oauthOrgBadge?.title"
                  >
                    {{ item.label }}
                  </Badge>
                  <span
                    v-else
                    class="inline-flex max-w-full items-center rounded-full border px-2 py-0.5 text-[10px] font-medium leading-4"
                    :class="getMobileTagClass(item)"
                    :title="item.label"
                  >
                    {{ item.label }}
                  </span>
                </template>
              </div>

              <div class="overflow-x-auto rounded-xl border border-border/50 bg-muted/30 px-3 py-2 text-[11px] text-muted-foreground">
                <div class="space-y-1 text-center">
                  <template v-if="isPoolKeyCycleStatsDisplay(key)">
                    <div
                      v-if="getPoolKeyCycleStatsGroups(key).length > 0"
                      class="grid min-h-16 min-w-[188px] items-center gap-x-1 text-left"
                      :style="getPoolKeyCycleStatsGridStyle(key)"
                      data-testid="pool-mobile-stats-cycle-grid"
                    >
                      <span aria-hidden="true" />
                      <span
                        v-for="group in getPoolKeyCycleStatsGroups(key)"
                        :key="`${key.key_id}-${group.code}-mobile-cycle-heading`"
                        class="text-center text-[10px] font-semibold text-foreground"
                        :data-testid="`pool-mobile-stats-cycle-group-${group.code}`"
                      >{{ group.label }}</span>

                      <template
                        v-for="row in getPoolKeyCycleStatsRows(key)"
                        :key="`${key.key_id}-${row.key}-mobile-cycle-row`"
                      >
                        <span class="text-muted-foreground truncate">{{ row.label }}</span>
                        <span
                          v-for="(metric, metricIndex) in row.values"
                          :key="`${key.key_id}-${row.key}-${metricIndex}-mobile-cycle-value`"
                          class="min-w-0 truncate text-center font-medium text-foreground/90 tabular-nums"
                          :class="metric.missing ? 'text-muted-foreground/80' : ''"
                          :title="metric.value"
                        >{{ metric.value }}</span>
                      </template>
                    </div>
                    <div
                      v-else
                      class="flex min-h-16 items-center justify-center text-muted-foreground"
                    >
                      {{ t('quotaUi.noWindows') }}
                    </div>
                  </template>
                  <template v-else>
                    <div
                      class="invisible h-4"
                      aria-hidden="true"
                    >
                      -
                    </div>
                    <div
                      v-for="metric in getPoolKeyAccountStatsMetrics(key)"
                      :key="`${key.key_id}-${metric.key}-mobile-account-total`"
                      class="grid h-4 w-[188px] grid-cols-[64px_124px] items-center text-left"
                    >
                      <span class="text-muted-foreground truncate">{{ metric.label }}</span>
                      <span
                        class="min-w-0 truncate text-center font-medium text-foreground/90"
                        :title="metric.value"
                      >{{ metric.value }}</span>
                    </div>
                  </template>
                  <div class="flex items-center justify-between gap-2 border-t border-border/40 pt-1 mt-1">
                    <span class="text-muted-foreground">{{ t('poolManagement.importedAt') }}</span>
                    <span class="font-medium text-foreground/90">{{ keyUiStateMap[key.key_id]?.importedAtRelative || '-' }}</span>
                  </div>
                  <div class="flex items-center justify-between gap-2">
                    <span class="text-muted-foreground">{{ t('poolManagement.lastUsed') }}</span>
                    <span class="font-medium text-foreground/90">{{ keyUiStateMap[key.key_id]?.lastUsedRelative || '-' }}</span>
                  </div>
                  <div class="flex items-center justify-between gap-2">
                    <span class="text-muted-foreground">{{ t('poolManagement.score') }}</span>
                    <div class="flex items-center gap-1">
                      <span class="font-mono font-medium text-foreground/90 tabular-nums">
                        {{ formatPoolScore(key.pool_score?.score) }}
                      </span>
                      <Popover
                        v-if="key.pool_score"
                        :open="scoreMobilePopoverOpenKeyId === key.key_id"
                        @update:open="(open: boolean) => handleScoreMobilePopoverToggle(key.key_id, open)"
                      >
                        <PopoverTrigger as-child>
                          <Button
                            variant="ghost"
                            size="icon"
                            class="h-5 w-5 rounded-full border border-transparent text-muted-foreground/80 hover:border-border/60 hover:bg-muted/60 hover:text-foreground"
                    :title="t('poolManagement.scoreDetails')"
                    :aria-label="t('poolManagement.scoreDetails')"
                            @click.stop
                          >
                            <CircleHelp class="h-3.5 w-3.5" />
                          </Button>
                        </PopoverTrigger>
                        <PopoverContent
                          v-if="scoreMobilePopoverOpenKeyId === key.key_id"
                          class="w-[22rem] max-w-[calc(100vw-1rem)] overflow-hidden rounded-xl border-border/60 bg-card/95 p-0 text-card-foreground shadow-xl shadow-black/5 backdrop-blur supports-[backdrop-filter]:bg-card/90"
                          side="bottom"
                          align="end"
                          :side-offset="8"
                        >
                          <div class="text-left">
                            <div class="flex items-center justify-between gap-3 border-b border-border/60 bg-muted/30 px-3 py-2.5">
                              <span class="text-xs font-semibold text-foreground">{{ t('poolManagement.scoreDetails') }}</span>
                              <span class="font-mono text-xs tabular-nums text-foreground/90">
                                {{ formatPoolScore(key.pool_score?.score) }}
                              </span>
                            </div>
                            <div class="space-y-2 px-3 py-2.5">
                              <div class="flex flex-wrap items-center gap-1.5">
                                <Badge
                                  variant="outline"
                                  class="h-5 rounded-md border-border/60 bg-background/60 px-2 text-[10px] font-normal"
                                >
                                  {{ getPoolScoreHardStateLabel(key.pool_score?.hard_state) }}
                                </Badge>
                                <Badge
                                  variant="secondary"
                                  class="h-5 rounded-md px-2 text-[10px] font-normal"
                                >
                                  {{ getPoolScoreProbeStatusLabel(key.pool_score?.probe_status) }}
                                </Badge>
                                <span class="text-[10px] text-muted-foreground">
                                  {{ t('poolManagement.updatedAt', { time: formatUnixSeconds(key.pool_score?.updated_at) }) }}
                                </span>
                              </div>
                              <pre class="max-h-56 overflow-auto rounded-md border border-border/50 bg-muted/30 px-3 py-2 font-mono text-[11px] leading-5 text-muted-foreground whitespace-pre-wrap break-words">{{ formatPoolScoreReason(key.pool_score?.score_reason) }}</pre>
                            </div>
                          </div>
                        </PopoverContent>
                      </Popover>
                    </div>
                  </div>
                </div>
              </div>

              <div
                v-if="providerSupportsAccountQuota"
                class="rounded-xl border border-border/50 bg-muted/30 px-3 py-2 text-xs"
              >
                <div class="text-muted-foreground mb-1">
                  {{ t('poolManagement.quota') }}
                </div>
                <div
                  v-if="quotaProgressMap[key.key_id]?.length"
                  class="space-y-2"
                >
                  <div
                    v-for="(item, idx) in quotaProgressMap[key.key_id]"
                    :key="`${key.key_id}-quota-mobile-${idx}`"
                    class="flex flex-col gap-1 min-w-0"
                  >
                    <div class="flex items-center justify-between text-[10px] leading-none">
                      <span
                        data-testid="pool-quota-progress-label"
                        class="text-muted-foreground font-medium shrink-0"
                      >{{ getQuotaProgressLabel(item.label) }}</span>
                      <span
                        v-if="getQuotaProgressResetDisplayText(item)"
                        data-testid="pool-quota-reset-text"
                        class="text-muted-foreground/80 tabular-nums truncate"
                        :title="getQuotaProgressResetDisplayText(item)"
                      >{{ getQuotaProgressResetDisplayText(item) }}</span>
                    </div>
                    <div class="flex items-center gap-1.5">
                      <div class="relative flex-1 h-1.5 rounded-full bg-border overflow-hidden">
                        <div
                          class="absolute left-0 top-0 h-full rounded-full transition-all duration-300"
                          :class="getQuotaRemainingBarColorByRemaining(item.remainingPercent)"
                          :style="{ width: `${item.remainingPercent}%` }"
                        />
                      </div>
                      <span
                        data-testid="pool-quota-meter-text"
                        class="shrink-0 text-[10px] font-medium tabular-nums leading-none"
                        :class="getQuotaRemainingClassByRemaining(item.remainingPercent)"
                      >{{ getQuotaProgressMeterDisplayText(item) }}</span>
                    </div>
                  </div>
                </div>
                <div
                  v-else-if="keyUiStateMap[key.key_id]?.quotaFallbackText"
                  :class="keyUiStateMap[key.key_id]?.quotaTextClass || ''"
                >
                  {{ keyUiStateMap[key.key_id]?.quotaFallbackText }}
                </div>
                <div
                  v-else
                  class="text-muted-foreground"
                >
                  -
                </div>
                <div
                  v-if="keyUiStateMap[key.key_id]?.quotaFreshnessText"
                  data-testid="pool-quota-freshness"
                  class="mt-2 text-[10px] leading-none tabular-nums"
                  :class="keyUiStateMap[key.key_id]?.quotaFreshnessStale
                    ? 'text-amber-600 dark:text-amber-400'
                    : 'text-muted-foreground'"
                >
                  {{ keyUiStateMap[key.key_id]?.quotaFreshnessText }}
                </div>
                <div
                  v-if="selectedProviderType === 'codex'"
                  class="mt-2 flex items-center justify-between gap-2 border-t border-border/40 pt-2 text-[10px]"
                >
                  <span class="text-muted-foreground tabular-nums">
                    {{ t('poolManagement.resetCreditCount', { count: getCodexResetCreditCount(key) ?? '-' }) }}
                  </span>
                  <Button
                    variant="ghost"
                    size="sm"
                    class="h-6 px-2 text-[10px]"
                    :disabled="!canResetCodexQuota(key) || resettingCodexQuotaKeyId !== null"
                    :title="getCodexQuotaResetButtonTitle(key)"
                    data-testid="pool-reset-codex-quota-mobile"
                    @click="handleResetCodexQuota(key)"
                  >
                    <Loader2
                      v-if="resettingCodexQuotaKeyId === key.key_id"
                      class="mr-1 h-3 w-3 animate-spin"
                    />
                    {{ resettingCodexQuotaKeyId === key.key_id ? t('poolManagement.resettingQuota') : t('poolManagement.resetQuota') }}
                  </Button>
                </div>
              </div>

              <div class="flex items-center gap-0.5">
                <div class="min-w-0 flex-1 flex justify-center">
                  <Button
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7 shrink-0 text-primary"
                    :disabled="!key.is_active || preparingAccountTestKeyId !== null || !selectedProviderData"
                    :title="preparingAccountTestKeyId === key.key_id ? t('poolManagement.preparingAccountTest') : t('poolManagement.testAccount')"
                    data-testid="pool-test-account-mobile"
                    @click="handleTestAccount(key)"
                  >
                    <Loader2
                      v-if="preparingAccountTestKeyId === key.key_id"
                      class="w-3.5 h-3.5 animate-spin"
                    />
                    <Play
                      v-else
                      class="w-3.5 h-3.5"
                    />
                  </Button>
                </div>
                <div
                  v-for="actionId in keyUiStateMap[key.key_id]?.mobileActionIds || []"
                  :key="`${key.key_id}-${actionId}`"
                  class="min-w-0 flex-1 flex justify-center"
                >
                  <Button
                    v-if="actionId === 'copy_or_download' && canExportOAuthCredential(key)"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7 shrink-0"
                    :title="t('poolManagement.downloadOAuth')"
                    @click.stop="downloadRefreshToken(key)"
                  >
                    <Download class="w-3.5 h-3.5" />
                  </Button>
                  <Button
                    v-else-if="actionId === 'copy_or_download'"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7 shrink-0"
                    :title="t('poolManagement.copyKey')"
                    @click.stop="copyFullKey(key)"
                  >
                    <Copy class="w-3.5 h-3.5" />
                  </Button>
                  <Button
                    v-else-if="actionId === 'refresh_token'"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7 shrink-0"
                    :disabled="refreshingOAuthKeyId === key.key_id || !keyUiStateMap[key.key_id]?.canRefreshToken"
                    :title="keyUiStateMap[key.key_id]?.oauthRefreshButtonTitle || ''"
                    @click.stop="handleRefreshOAuth(key)"
                  >
                    <RefreshCw
                      class="w-3.5 h-3.5"
                      :class="{ 'animate-spin': refreshingOAuthKeyId === key.key_id }"
                    />
                  </Button>
                  <Button
                    v-else-if="actionId === 'clear_cooldown'"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7 shrink-0 text-muted-foreground hover:text-green-600"
                    :title="t('poolManagement.clearCooldown')"
                    @click="clearCooldown(key.key_id)"
                  >
                    <RefreshCw class="w-3.5 h-3.5" />
                  </Button>
                  <Button
                    v-else-if="actionId === 'permissions'"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7 shrink-0"
                    :title="t('poolManagement.modelPermissions')"
                    @click="handleKeyPermissions(key)"
                  >
                    <Shield class="w-3.5 h-3.5" />
                  </Button>
                  <Popover
                    v-else-if="actionId === 'proxy'"
                    :open="proxyMobilePopoverOpenKeyId === key.key_id"
                    @update:open="(v: boolean) => handleProxyMobilePopoverToggle(key.key_id, v)"
                  >
                    <PopoverTrigger as-child>
                      <Button
                        variant="ghost"
                        size="icon"
                        class="h-7 w-7 shrink-0"
                        :class="key.proxy?.node_id ? 'text-blue-500' : ''"
                        :disabled="savingProxyKeyId === key.key_id"
                        :title="key.proxy?.node_id ? t('poolManagement.proxyValue', { name: getKeyProxyNodeName(key) }) : t('poolManagement.setProxy')"
                        @click.stop
                      >
                        <Globe class="w-3.5 h-3.5" />
                      </Button>
                    </PopoverTrigger>
                    <PopoverContent
                      class="w-72 p-3"
                      side="bottom"
                      align="end"
                    >
                      <div class="space-y-2">
                        <div class="flex items-center justify-between">
                          <span class="text-xs font-medium">{{ t('poolManagement.proxyNode') }}</span>
                          <Button
                            v-if="key.proxy?.node_id"
                            variant="ghost"
                            size="sm"
                            class="h-6 px-2 text-[10px] text-muted-foreground"
                            :disabled="savingProxyKeyId === key.key_id"
                            @click="clearKeyProxy(key)"
                          >
                            {{ t('poolManagement.clear') }}
                          </Button>
                        </div>
                        <ProxyNodeSelect
                          :model-value="key.proxy?.node_id || ''"
                          trigger-class="h-8"
                          @update:model-value="(v: string) => setKeyProxy(key, v)"
                        />
                        <p class="text-[10px] text-muted-foreground">
                          {{ key.proxy?.node_id ? t('poolManagement.independentProxy') : t('poolManagement.providerProxyFallback') }}
                        </p>
                      </div>
                    </PopoverContent>
                  </Popover>
                  <Button
                    v-else-if="actionId === 'reset_cycle_stats'"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7 shrink-0 text-muted-foreground hover:text-foreground"
                    :disabled="resettingCycleKeyId === key.key_id"
                    :title="t('poolManagement.resetCycle')"
                    @click="handleResetCycleStats(key)"
                  >
                    <RotateCcw
                      class="w-3.5 h-3.5"
                      :class="{ 'animate-spin': resettingCycleKeyId === key.key_id }"
                    />
                  </Button>
                  <Button
                    v-else-if="actionId === 'edit'"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7 shrink-0"
                    :title="t('poolManagement.editAccount')"
                    @click="handleEditKey(key)"
                  >
                    <SquarePen class="w-3.5 h-3.5" />
                  </Button>
                  <Button
                    v-else-if="actionId === 'toggle'"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7 shrink-0 text-foreground hover:text-foreground"
                    :disabled="togglingKeyId === key.key_id"
                    :title="key.is_active ? t('poolManagement.disable') : t('poolManagement.enable')"
                    @click="toggleKeyActive(key)"
                  >
                    <Power class="w-3.5 h-3.5" />
                  </Button>
                  <Button
                    v-else-if="actionId === 'delete'"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7 shrink-0 text-destructive hover:text-destructive"
                    :disabled="deletingKeyId === key.key_id"
                    :title="t('poolManagement.deleteAccount')"
                    @click="handleDeleteKey(key)"
                  >
                    <Trash2 class="w-3.5 h-3.5" />
                  </Button>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Empty keys -->
        <div
          v-if="keyPage.keys.length === 0 && !keysLoading && keysLoadedOnce"
          class="flex flex-col items-center justify-center py-16 text-center"
        >
          <div class="mx-auto flex h-16 w-16 items-center justify-center rounded-full bg-muted">
            <KeyRound class="h-8 w-8 text-muted-foreground" />
          </div>
          <p class="text-sm text-muted-foreground mt-4">
            {{ hasPoolKeyFilters ? t('poolManagement.noMatches') : t('poolManagement.noAccounts') }}
          </p>
          <Button
            v-if="hasPoolKeyFilters"
            variant="outline"
            size="sm"
            class="mt-3"
            @click="clearPoolKeyFilters"
          >
            {{ t('poolManagement.clearFilters') }}
          </Button>
          <Button
            v-else
            variant="outline"
            size="sm"
            class="mt-3"
            @click="showImportDialog = true"
          >
            <Upload class="w-3.5 h-3.5 mr-1.5" />
            {{ t('poolManagement.addAccount') }}
          </Button>
        </div>

        <!-- Pagination -->
        <Pagination
          v-if="keyPage.keys.length > 0"
          :current="currentPage"
          :total="keyPage.total"
          :page-size="pageSize"
          cache-key="pool-keys-page-size"
          @update:current="currentPage = $event"
          @update:page-size="pageSize = $event"
        />
      </template>
    </Card>

    <!-- Dialogs -->
    <OAuthAccountDialog
      v-if="selectedProviderId"
      :open="showImportDialog"
      :provider-id="selectedProviderId"
      :provider-type="selectedProviderType || null"
      @close="showImportDialog = false"
      @saved="handleAccountDialogSaved"
    />
    <PoolSchedulingDialog
      v-if="selectedProviderId"
      v-model="showSchedulingDialog"
      :provider-id="selectedProviderId"
      :provider-type="selectedProviderType"
      :current-config="selectedProviderConfig"
      @saved="handleSchedulingSaved"
    />
    <PoolAdvancedDialog
      v-if="selectedProviderId"
      v-model="showAdvancedDialog"
      :provider-id="selectedProviderId"
      :provider-type="selectedProviderType"
      :current-config="selectedProviderConfig"
      :current-claude-config="selectedProviderClaudeConfig"
      @saved="handleSchedulingSaved"
    />
    <PoolDemandMetricsDialog
      v-model="showDemandMetricsDialog"
      :provider-name="selectedProviderOverview?.provider_name"
      :samples="providerDemandMetricSamples"
    />
    <ProviderFormDialog
      v-model="providerEditDialogOpen"
      :provider="providerToEdit"
      @provider-updated="handleProviderEditSaved"
    />
    <EndpointFormDialog
      v-if="selectedProviderData"
      v-model="endpointEditDialogOpen"
      :provider="selectedProviderData"
      :endpoints="providerEndpointsForEdit"
      :provider-format-conversion-enabled="selectedProviderData.enable_format_conversion"
      @endpoint-created="handleEndpointEditSaved"
      @endpoint-updated="handleEndpointEditSaved"
    />
    <PoolAccountBatchDialog
      v-if="selectedProviderId"
      v-model="showAccountBatchDialog"
      :provider-id="selectedProviderId"
      :provider-name="selectedProviderData?.name || ''"
      :provider-type="selectedProviderData?.provider_type || selectedProviderType"
      :batch-concurrency="selectedProviderConfig?.batch_concurrency"
      @changed="handleAccountBatchChanged"
    />
    <KeyFormDialog
      v-if="selectedProviderId"
      :open="keyFormDialogOpen"
      :endpoint="null"
      :provider-type="selectedProviderData?.provider_type || selectedProviderType || null"
      :editing-key="editingKey"
      :provider-id="selectedProviderId"
      :available-api-formats="selectedProviderData?.api_formats || []"
      @close="closeKeyFormDialog"
      @saved="handleDialogSaved"
    />
    <OAuthKeyEditDialog
      :open="oauthKeyEditDialogOpen"
      :editing-key="editingKey"
      @close="closeOAuthEditDialog"
      @saved="handleDialogSaved"
    />
    <PoolAccountTestDialog
      ref="poolAccountTestDialogRef"
      :provider="selectedProviderData"
    />
    <KeyAllowedModelsEditDialog
      v-if="selectedProviderId"
      :open="keyPermissionsDialogOpen"
      :api-key="editingKey"
      :provider-id="selectedProviderId || ''"
      @close="closeKeyPermissionsDialog"
      @saved="handleDialogSaved"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  Search,
  Upload,
  ChevronDown,
  RefreshCw,
  Activity,
  Power,
  Database,
  KeyRound,
  Download,
  Copy,
  Shield,
  Globe,
  Repeat2,
  RotateCcw,
  SquarePen,
  Trash2,
  Users,
  Settings2,
  SlidersHorizontal,
  CircleHelp,
  Edit,
  Plug,
  Loader2,
  Play,
} from 'lucide-vue-next'

import {
  Card,
  Badge,
  Button,
  Input,
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableHead,
  SortableTableHead,
  TableFilterMenu,
  TableCell,
  Checkbox,
  Pagination,
  Popover,
  PopoverTrigger,
  PopoverContent,
} from '@/components/ui'
import RefreshButton from '@/components/ui/refresh-button.vue'
import { useToast } from '@/composables/useToast'
import { useClipboard } from '@/composables/useClipboard'
import { useCountdownTimer, getCodexResetCountdown } from '@/composables/useCountdownTimer'
import { useConfirm } from '@/composables/useConfirm'
import { useRouteQuery } from '@/composables/useRouteQuery'
import { parseApiError } from '@/utils/errorParser'
import { getAccountCopyText, getAccountDisplayName } from '@/features/providers/utils/accountDisplay'
import {
  getPoolOverview,
  getPoolSchedulingPresets,
  listPoolKeys,
  clearPoolCooldown,
  batchActionPoolKeys,
  getPoolBatchDeleteTask,
  resolvePoolKeySelection,
} from '@/api/endpoints/pool'
import {
  revealEndpointKey,
  exportKey,
  deleteEndpointKey,
  updateProviderKey,
  refreshProviderQuota,
  resetCodexQuota,
  resetProviderKeyCycleStats,
} from '@/api/endpoints/keys'
import { refreshProviderOAuth } from '@/api/endpoints/provider_oauth'
import type {
  PoolOverviewItem,
  PoolKeyDetail,
  PoolKeysPageResponse,
  PoolPresetMeta,
  PoolSchedulingState,
} from '@/api/endpoints/pool'
import type {
  ClaudeCodeAdvancedConfig,
  EndpointAPIKey,
  ProviderEndpoint,
  PoolAdvancedConfig,
  ProviderType,
  ProviderWithEndpointsSummary,
} from '@/api/endpoints/types/provider'
import type { QuotaStatusSnapshot, QuotaWindowSnapshot } from '@/api/endpoints/types'
import { getProvider, getProviderEndpoints, updateProvider } from '@/api/endpoints'
import { useProxyNodesStore } from '@/stores/proxy-nodes'
import PoolSchedulingDialog from '@/features/pool/components/PoolSchedulingDialog.vue'
import PoolAdvancedDialog from '@/features/pool/components/PoolAdvancedDialog.vue'
import PoolDemandMetricsDialog from '@/features/pool/components/PoolDemandMetricsDialog.vue'
import PoolAccountBatchDialog from '@/features/pool/components/PoolAccountBatchDialog.vue'
import PoolAccountTestDialog from '@/features/pool/components/PoolAccountTestDialog.vue'
import ProviderProxyPopover from '@/features/pool/components/ProviderProxyPopover.vue'
import KeyAllowedModelsEditDialog from '@/features/providers/components/KeyAllowedModelsEditDialog.vue'
import KeyFormDialog from '@/features/providers/components/KeyFormDialog.vue'
import OAuthKeyEditDialog from '@/features/providers/components/OAuthKeyEditDialog.vue'
import OAuthAccountDialog from '@/features/providers/components/OAuthAccountDialog.vue'
import EndpointFormDialog from '@/features/providers/components/EndpointFormDialog.vue'
import ProviderFormDialog from '@/features/providers/components/ProviderFormDialog.vue'
import ProxyNodeSelect from '@/features/providers/components/ProxyNodeSelect.vue'
import {
  buildPoolMobileTagItems,
  splitPoolMobileActions,
  type PoolMobileActionId,
  type PoolMobileTagItem,
  type PoolMobileTagTone,
} from '@/features/pool/utils/poolMobilePresentation'
import {
  buildPoolManagementQueryPatch,
  readPoolManagementViewState,
  resolvePoolManagementPageAfterLoad,
  type PoolManagementSortBy,
  type PoolManagementSortOrder,
  type PoolManagementStatsMode,
  type PoolManagementViewState,
  writePoolManagementViewState,
} from '@/features/pool/utils/poolManagementState'
import {
  buildPoolStatsDisplay,
  hasPendingCodexCycleStats,
  type PoolCodexCycleStatsGroup,
  type PoolStatsDisplay,
  type PoolStatsMetric,
} from '@/features/pool/utils/poolStatsDisplay'
import { getOAuthOrgBadge } from '@/utils/oauthIdentity'
import { getOAuthRefreshFeedback } from '@/utils/oauthRefreshFeedback'
import {
  canEditOAuthCredential,
  canExportOAuthCredential,
  canRefreshOAuthCredential,
  getProviderAuthLabel,
  getProviderMaskedSecretLabel,
  isOAuthManagedCredential,
  isServiceAccountCredential,
  shouldShowOAuthRefreshControl,
} from '@/utils/providerKeyAuth'
import {
  getAccountStatusDisplay,
  getAccountStatusTitle,
  getOAuthRefreshButtonTitle as resolveOAuthRefreshButtonTitle,
  getOAuthStatusDisplayWithFallback,
  getOAuthStatusTitle as resolveOAuthStatusTitle,
} from '@/utils/providerKeyStatus'
import {
  getGrokOAuthQuotaFreshness,
  getLegacyAccountQuotaText,
  getQuotaDisplayText,
} from '@/utils/providerKeyQuota'

type PoolKeyScore = NonNullable<PoolKeyDetail['pool_score']>

const { success, error: showError, warning: showWarning } = useToast()
const { t } = useI18n()
const { confirm } = useConfirm()
const { copyToClipboard } = useClipboard()
const { tick: countdownTick, start: startCountdownTimer } = useCountdownTimer()
const proxyNodesStore = useProxyNodesStore()
const { getQueryValue, patchQuery } = useRouteQuery()

const PROVIDER_TYPES = new Set<ProviderType>([
  'custom',
  'claude_code',
  'claude_code_api',
  'codex',
  'chatgpt_web',
  'gemini_cli',
  'antigravity',
  'kiro',
  'grok',
  'grok_oauth',
  'vertex_ai',
])

function normalizeProviderType(value: unknown): ProviderType | '' {
  const normalized = String(value || '').trim().toLowerCase()
  return PROVIDER_TYPES.has(normalized as ProviderType) ? normalized as ProviderType : ''
}

const poolManagementViewStorage = typeof window === 'undefined' ? undefined : window.sessionStorage
const restoredViewState = readPoolManagementViewState(
  {
    providerId: getQueryValue('providerId'),
    search: getQueryValue('search'),
    status: getQueryValue('status'),
    page: getQueryValue('page'),
    pageSize: getQueryValue('pageSize'),
    sortBy: getQueryValue('sortBy'),
    sortOrder: getQueryValue('sortOrder'),
    statsMode: getQueryValue('statsMode'),
  },
  poolManagementViewStorage,
)

// --- Overview ---
const poolProviders = ref<PoolOverviewItem[]>([])
const overviewLoading = ref(true)
let overviewRequestId = 0
let selectProviderRequestId = 0
let providerDataRequestId = 0
let keysRequestId = 0
let keysSearchDebounceTimer: number | null = null
let codexCycleStatsReloadTimer: number | null = null
let codexCycleStatsReloadAttempts = 0
let codexCycleStatsReloadContext = ''
let demandMetricsPollingTimer: number | null = null
let demandMetricsRequestId = 0
let suppressFiltersWatch = false
let hasHydratedInitialProviderSelection = false
const POOL_OVERVIEW_CACHE_TTL_MS = 10 * 1000
const POOL_KEYS_CACHE_TTL_MS = 10 * 1000
const CODEX_CYCLE_STATS_RELOAD_INTERVAL_MS = 1000
const CODEX_CYCLE_STATS_RELOAD_MAX_ATTEMPTS = 5
const POOL_SCHEDULING_PRESETS_CACHE_TTL_MS = 5 * 60 * 1000
const POOL_DEMAND_METRICS_SAMPLES_LIMIT = 120
const POOL_DEMAND_METRICS_POLL_INTERVAL_MS = 10 * 1000

interface PoolDemandMetricSample {
  providerId: string
  sampledAt: number
  hotCount: number
  desiredHot: number
  inFlight: number
  emaInFlight: number
  burstPending: boolean
}

const showDemandMetricsDialog = ref(false)
const providerDemandMetricSamples = ref<PoolDemandMetricSample[]>([])
const poolKeyStatusFilterOptions = computed<Array<{ value: PoolManagementViewState['status'], label: string }>>(() => [
  { value: 'all', label: t('poolManagement.statusAll') },
  { value: 'available', label: t('poolManagement.available') },
  { value: 'invalid', label: t('poolManagement.invalid') },
  { value: 'disabled', label: t('poolManagement.disable') },
  { value: 'quota_exhausted', label: t('poolManagement.quotaExhausted') },
  { value: 'temporary_unavailable', label: t('poolManagement.temporaryUnavailable') },
  { value: 'blocked', label: t('poolManagement.abnormal') },
])
const poolScoreHardStateOptions = computed(() => [
  { value: 'all', label: t('poolManagement.statusAll') },
  { value: 'available', label: t('poolManagement.available') },
  { value: 'unknown', label: t('poolManagement.unknown') },
  { value: 'cooldown', label: t('poolManagement.cooldown') },
  { value: 'quota_exhausted', label: t('poolManagement.quotaExhausted') },
  { value: 'auth_invalid', label: t('poolManagement.authInvalid') },
  { value: 'banned', label: t('poolManagement.banned') },
  { value: 'inactive', label: t('poolManagement.disable') },
])
const poolScoreProbeStatusOptions = computed(() => [
  { value: 'all', label: t('poolManagement.probeAll') },
  { value: 'never', label: t('poolManagement.neverProbed') },
  { value: 'ok', label: t('poolManagement.normal') },
  { value: 'failed', label: t('poolManagement.failed') },
  { value: 'stale', label: t('poolManagement.stale') },
  { value: 'in_progress', label: t('poolManagement.probing') },
])

async function loadOverview(options: { cacheTtlMs?: number, silent?: boolean } = {}) {
  const requestId = ++overviewRequestId
  if (!options.silent) {
    overviewLoading.value = true
  }
  try {
    const res = await getPoolOverview({ cacheTtlMs: options.cacheTtlMs ?? 0 })
    if (requestId !== overviewRequestId) return
    const allProviders = Array.isArray(res.items) ? res.items : []
    const enabledProviders = allProviders.filter(item => item.pool_enabled)
    poolProviders.value = enabledProviders

    const queryProviderId = getQueryValue('providerId')
    const queryProviderExists = Boolean(
      queryProviderId && enabledProviders.some(item => item.provider_id === queryProviderId),
    )
    const currentSelectedId = selectedProviderId.value
    const currentSelectedExists = Boolean(
      currentSelectedId && enabledProviders.some(item => item.provider_id === currentSelectedId),
    )
    const selectedId = currentSelectedExists
      ? currentSelectedId
      : (queryProviderExists ? queryProviderId : currentSelectedId)
    const selectedStillExists = Boolean(
      selectedId && enabledProviders.some(item => item.provider_id === selectedId),
    )

    if (selectedStillExists && selectedId) {
      // 页面刷新时可能先恢复了选中的 Provider，但列表请求尚未触发；
      // overview 回来后补一次初始化拉取，确保空态不会卡住。
      if (!hasHydratedInitialProviderSelection || selectedId !== selectedProviderId.value) {
        await selectProvider(selectedId, {
          preserveSearch: true,
          preserveStatus: true,
          preservePagination: true,
          cacheTtlMs: options.cacheTtlMs ? POOL_KEYS_CACHE_TTL_MS : 0,
        })
      }
      return
    }

    if (enabledProviders.length > 0) {
      const fallbackProviderId = enabledProviders[0].provider_id
      const shouldPreserveViewState = Boolean(selectedId)
      await selectProvider(fallbackProviderId, {
        preserveSearch: shouldPreserveViewState,
        preserveStatus: shouldPreserveViewState,
        preservePagination: shouldPreserveViewState,
        cacheTtlMs: options.cacheTtlMs ? POOL_KEYS_CACHE_TTL_MS : 0,
      })
    } else {
      selectedProviderId.value = null
      selectedProviderData.value = null
      keysLoadedOnce.value = false
      clearPoolKeySelection()
      endpointEditDialogOpen.value = false
      providerEndpointsForEdit.value = []
      showAccountBatchDialog.value = false
      closeProviderProxyPopovers()
      resetKeyPage()
    }
  } catch (err) {
    if (requestId !== overviewRequestId) return
    if (!options.silent) {
      showError(parseApiError(err))
    }
  } finally {
    if (requestId === overviewRequestId && !options.silent) {
      overviewLoading.value = false
    }
  }
}

async function handleSchedulingSaved(updatedProvider: ProviderWithEndpointsSummary) {
  // 优先回写保存接口返回值，避免弹窗立即重开时读到旧配置。
  if (selectedProviderId.value && updatedProvider.id === selectedProviderId.value) {
    selectedProviderData.value = updatedProvider
  }
  showSchedulingDialog.value = false
  showAdvancedDialog.value = false
  await loadOverview()
}

// --- Provider Selection ---
const selectedProviderId = ref<string | null>(restoredViewState.providerId)
const selectedProviderData = ref<ProviderWithEndpointsSummary | null>(null)

// Proxy for Select v-model (string, not string|null)
const selectedProviderIdProxy = computed({
  get: () => selectedProviderId.value ?? '',
  set: (val: string) => {
    if (val && val !== selectedProviderId.value) {
      void selectProvider(val, { cacheTtlMs: POOL_KEYS_CACHE_TTL_MS })
    }
  },
})

const providerSelectDisabled = computed(() => poolProviders.value.length === 0)

const selectedProviderConfig = computed<PoolAdvancedConfig | null>(() => {
  return (selectedProviderData.value as Record<string, unknown> | null)?.pool_advanced as PoolAdvancedConfig | null ?? null
})

const selectedProviderClaudeConfig = computed(() => {
  return (selectedProviderData.value as Record<string, unknown> | null)?.claude_code_advanced as ClaudeCodeAdvancedConfig | null ?? null
})

const DEFAULT_ENABLED_PRESETS = new Set(['cache_affinity', 'recent_refresh'])

const DEFAULT_PRESET_LABELS: Record<string, string> = {
  lru: 'LRU',
  free_first: 'Free',
  team_first: 'Team',
  plus_first: 'Plus',
  pro_first: 'Pro',
  recent_refresh: t('poolManagement.recentRefresh'),
  priority_first: t('poolManagement.priorityFirst'),
  quota_balanced: t('poolManagement.quotaBalanced'),
  single_account: t('poolManagement.singleAccount'),
}
const presetLabelsByName = ref<Record<string, string>>({ ...DEFAULT_PRESET_LABELS })

function normalizePresetName(value: unknown): string {
  return String(value ?? '').trim().toLowerCase()
}

async function loadSchedulingPresetMetas(options: { cacheTtlMs?: number } = {}): Promise<void> {
  try {
    const metas = await getPoolSchedulingPresets({ cacheTtlMs: options.cacheTtlMs ?? 0 })
    const next: Record<string, string> = {}
    for (const meta of metas as PoolPresetMeta[]) {
      const name = normalizePresetName(meta.name)
      if (!name) continue
      const label = String(meta.label ?? '').trim()
      next[name] = label || name
    }
    if (Object.keys(next).length > 0) {
      presetLabelsByName.value = next
    }
  } catch {
    presetLabelsByName.value = { ...DEFAULT_PRESET_LABELS }
  }
}

const selectedProviderOverview = computed<PoolOverviewItem | null>(() => {
  const selectedId = selectedProviderId.value
  if (!selectedId) return null
  return poolProviders.value.find(item => item.provider_id === selectedId) || null
})

const showAdaptiveHotPoolMetricsButton = computed(() => {
  if (!selectedProviderId.value) return false
  return selectedProviderConfig.value?.probing_enabled === true
})

function normalizeDemandMetricNumber(value: unknown): number {
  const normalized = Number(value ?? 0)
  if (!Number.isFinite(normalized) || normalized <= 0) return 0
  return normalized
}

function buildDemandMetricSample(overview: PoolOverviewItem): PoolDemandMetricSample {
  return {
    providerId: overview.provider_id,
    sampledAt: Date.now(),
    hotCount: Math.floor(normalizeDemandMetricNumber(overview.provider_hot_count)),
    desiredHot: Math.floor(normalizeDemandMetricNumber(overview.provider_desired_hot)),
    inFlight: Math.floor(normalizeDemandMetricNumber(overview.provider_in_flight)),
    emaInFlight: normalizeDemandMetricNumber(overview.provider_ema_in_flight),
    burstPending: overview.provider_burst_pending === true,
  }
}

function appendDemandMetricSample(overview: PoolOverviewItem | null): void {
  if (!overview || !showDemandMetricsDialog.value || !showAdaptiveHotPoolMetricsButton.value) return
  const nextSample = buildDemandMetricSample(overview)
  const existing = providerDemandMetricSamples.value.filter(
    sample => sample.providerId === overview.provider_id,
  )
  const lastSample = existing.at(-1)
  if (
    lastSample
    && nextSample.sampledAt - lastSample.sampledAt < 1000
    && lastSample.hotCount === nextSample.hotCount
    && lastSample.desiredHot === nextSample.desiredHot
    && lastSample.inFlight === nextSample.inFlight
    && lastSample.emaInFlight === nextSample.emaInFlight
    && lastSample.burstPending === nextSample.burstPending
  ) {
    providerDemandMetricSamples.value = existing
    return
  }
  providerDemandMetricSamples.value = [...existing, nextSample]
    .slice(-POOL_DEMAND_METRICS_SAMPLES_LIMIT)
}

function stopDemandMetricsPolling(): void {
  if (demandMetricsPollingTimer !== null) {
    window.clearInterval(demandMetricsPollingTimer)
    demandMetricsPollingTimer = null
  }
}

async function refreshDemandMetricsOverview(): Promise<void> {
  const providerId = selectedProviderId.value
  if (!showDemandMetricsDialog.value || !showAdaptiveHotPoolMetricsButton.value || !providerId) {
    return
  }

  const requestId = ++demandMetricsRequestId
  try {
    const res = await getPoolOverview({ cacheTtlMs: 0 })
    if (
      requestId !== demandMetricsRequestId
      || !showDemandMetricsDialog.value
      || selectedProviderId.value !== providerId
    ) {
      return
    }
    const allProviders = Array.isArray(res.items) ? res.items : []
    const enabledProviders = allProviders.filter(item => item.pool_enabled)
    poolProviders.value = enabledProviders
    appendDemandMetricSample(
      enabledProviders.find(item => item.provider_id === providerId) || null,
    )
  } catch {
    // 指标弹窗只做尽力刷新，失败不打断主流程。
  }
}

function startDemandMetricsPolling(): void {
  stopDemandMetricsPolling()
  appendDemandMetricSample(selectedProviderOverview.value)
  void refreshDemandMetricsOverview()
  demandMetricsPollingTimer = window.setInterval(() => {
    if (!showDemandMetricsDialog.value || !showAdaptiveHotPoolMetricsButton.value) return
    if (document.visibilityState === 'hidden') return
    void refreshDemandMetricsOverview()
  }, POOL_DEMAND_METRICS_POLL_INTERVAL_MS)
}

const poolSchedulingLabel = computed(() => {
  if (!selectedProviderConfig.value && selectedProviderOverview.value?.pool_enabled === false) {
    return t('poolManagement.disabled')
  }

  const cfg = selectedProviderConfig.value

  // No pool_advanced config at all: use default enabled presets count
  if (!cfg) return t('poolManagement.dimensionCount', { count: DEFAULT_ENABLED_PRESETS.size })

  const presets = Array.isArray(cfg.scheduling_presets) ? cfg.scheduling_presets : []
  const presetLabels = presetLabelsByName.value

  if (presets.length > 0) {
    // New format: object list with { preset, enabled }
    const first = presets[0]
    if (typeof first === 'object' && first !== null && 'preset' in first) {
      const enabledCount = (presets as Array<{ preset: string; enabled?: boolean }>)
        .filter(p => p.enabled !== false)
        .length
      return enabledCount > 0 ? t('poolManagement.dimensionCount', { count: enabledCount }) : t('poolManagement.noEnabledDimensions')
    }

    // Legacy string list format
    if (typeof first === 'string') {
      const labels = (presets as string[])
        .map(p => presetLabels[normalizePresetName(p)])
        .filter(Boolean)
      if (labels.length > 0) return t('poolManagement.dimensionCount', { count: labels.length })
    }
  }

  // Fallback: legacy scheduling_mode field
  if (cfg.scheduling_mode === 'multi_score') {
    return t('poolManagement.multiScore')
  }

  const lruEnabled = cfg.scheduling_mode === 'lru' || cfg.lru_enabled === true
  const stickyTtl = Number(cfg.sticky_session_ttl_seconds ?? 3600)
  const stickyEnabled = Number.isFinite(stickyTtl) && stickyTtl > 0

  if (lruEnabled && stickyEnabled) return t('poolManagement.lruSticky')
  if (lruEnabled) return 'LRU'
  if (!cfg.scheduling_mode && (cfg.lru_enabled === null || cfg.lru_enabled === undefined)) {
    return t('poolManagement.dimensionCount', { count: DEFAULT_ENABLED_PRESETS.size })
  }
  if (stickyEnabled) return t('poolManagement.sticky')
  return t('poolManagement.random')
})

const selectedProviderType = computed<ProviderType | ''>(() => {
  const fromDetail = normalizeProviderType(selectedProviderData.value?.provider_type)
  if (fromDetail) return fromDetail
  return normalizeProviderType(selectedProviderOverview.value?.provider_type)
})

const showCodexStatsModeToggle = computed(() => selectedProviderType.value === 'codex')

const selectedProviderStatusText = computed(() => {
  if (!selectedProviderId.value) return ''
  const providerActive = selectedProviderData.value?.is_active
  if (providerActive === false) return t('poolManagement.disabled')
  if (providerActive === true) return t('poolManagement.enabled')
  if (selectedProviderOverview.value?.pool_enabled === false) return t('poolManagement.disabled')
  if (selectedProviderOverview.value?.pool_enabled === true) return t('poolManagement.enabled')
  return ''
})

function formatDemandEma(value: number | undefined): string {
  const normalized = Number(value ?? 0)
  if (!Number.isFinite(normalized) || normalized <= 0) return '0.0'
  return normalized.toFixed(1)
}

const selectedProviderDemandMetaText = computed(() => {
  const overview = selectedProviderOverview.value
  if (!overview) return ''
  const segments: string[] = []
  const desiredHot = Number(overview.provider_desired_hot ?? 0)
  const hotCount = Number(overview.provider_hot_count ?? 0)
  const inFlight = Number(overview.provider_in_flight ?? 0)
  if (Number.isFinite(desiredHot) && desiredHot > 0) {
    segments.push(t('poolManagement.hotPoolValue', { current: hotCount, desired: desiredHot }))
    segments.push(t('poolManagement.emaValue', { value: formatDemandEma(overview.provider_ema_in_flight) }))
  }
  if (Number.isFinite(inFlight) && inFlight > 0) {
    segments.push(t('poolManagement.processingValue', { count: inFlight }))
  }
  if (overview.provider_burst_pending) {
    segments.push(t('poolManagement.replenishing'))
  }
  return segments.join(' | ')
})

const poolHeaderMetaText = computed(() => {
  return [
    selectedProviderType.value,
    selectedProviderStatusText.value,
    selectedProviderDemandMetaText.value,
  ].filter(Boolean).join(' | ')
})

watch(showDemandMetricsDialog, (open) => {
  if (open) {
    startDemandMetricsPolling()
  } else {
    stopDemandMetricsPolling()
  }
})

watch(selectedProviderId, () => {
  providerDemandMetricSamples.value = []
  if (showDemandMetricsDialog.value) {
    appendDemandMetricSample(selectedProviderOverview.value)
  }
})

watch(selectedProviderOverview, (overview) => {
  appendDemandMetricSample(overview)
})

watch(showAdaptiveHotPoolMetricsButton, (enabled) => {
  if (!enabled && showDemandMetricsDialog.value) {
    showDemandMetricsDialog.value = false
  }
})

const ACCOUNT_QUOTA_PROVIDER_TYPES = new Set([
  'codex',
  'gemini_cli',
  'kiro',
  'antigravity',
  'grok',
  'grok_oauth',
  'chatgpt_web',
])

function hasMeaningfulPoolKeyQuota(key: PoolKeyDetail): boolean {
  if (getLegacyAccountQuotaText(key)) return true

  const quota = key.status_snapshot?.quota
  if (!quota) return false
  if (parseQuotaProgressItems(key).length > 0) return true
  if (getGrokOAuthQuotaFreshness(key, selectedProviderType.value)) return true

  const code = String(quota.code || '').trim().toLowerCase()
  if (code && code !== 'ok' && code !== 'unknown') return true
  if (String(quota.label || '').trim() || String(quota.reason || '').trim()) return true

  const credits = quota.credits
  if (credits && (credits.has_credits != null || credits.balance != null || credits.unlimited != null)) {
    return true
  }
  return quota.reset_credits?.available_count != null
}

const providerSupportsAccountQuota = computed(() => {
  return ACCOUNT_QUOTA_PROVIDER_TYPES.has(selectedProviderType.value)
})

const showAccountQuotaColumn = computed(() => {
  return providerSupportsAccountQuota.value && keyPage.value.keys.some(hasMeaningfulPoolKeyQuota)
})

async function selectProvider(
  id: string,
  options: {
    preserveSearch?: boolean
    preserveStatus?: boolean
    preservePagination?: boolean
    cacheTtlMs?: number
  } = {},
) {
  const requestId = ++selectProviderRequestId
  hasHydratedInitialProviderSelection = true
  selectedProviderId.value = id
  selectedProviderData.value = null
  endpointEditDialogOpen.value = false
  providerEndpointsForEdit.value = []
  editingKeyDetail.value = null
  clearPoolKeySelection()
  showAccountBatchDialog.value = false
  keyPermissionsDialogOpen.value = false
  keyFormDialogOpen.value = false
  oauthKeyEditDialogOpen.value = false
  closeProviderProxyPopovers()
  proxyDesktopPopoverOpenKeyId.value = null
  proxyMobilePopoverOpenKeyId.value = null
  scoreDesktopPopoverOpenKeyId.value = null
  scoreMobilePopoverOpenKeyId.value = null
  suppressFiltersWatch = true
  if (!options.preservePagination) {
    currentPage.value = 1
  }
  if (!options.preserveSearch) {
    searchQuery.value = ''
  }
  if (!options.preserveStatus) {
    statusFilter.value = 'all'
    planTypeFilter.value = 'all'
  }
  suppressFiltersWatch = false
  if (keysSearchDebounceTimer !== null) {
    clearTimeout(keysSearchDebounceTimer)
    keysSearchDebounceTimer = null
  }
  keysLoadedOnce.value = false
  resetKeyPage(currentPage.value, pageSize.value)
  const keysTask = loadKeys({ cacheTtlMs: options.cacheTtlMs ?? 0 })
  // Provider summary is non-blocking for key list rendering.
  void loadProviderData(id)
  await keysTask
  if (requestId !== selectProviderRequestId) return
}

async function loadProviderData(id: string) {
  const requestId = ++providerDataRequestId
  try {
    const providerData = await getProvider(id)
    if (requestId !== providerDataRequestId || selectedProviderId.value !== id) return
    selectedProviderData.value = providerData
  } catch {
    if (requestId !== providerDataRequestId || selectedProviderId.value !== id) return
    selectedProviderData.value = null
  }
}

async function refresh() {
  await loadKeys()
}

// --- Keys ---
function createEmptyKeyPage(page = 1, pageSizeValue = 50): PoolKeysPageResponse {
  return {
    total: 0,
    page,
    page_size: pageSizeValue,
    keys: [],
    summary: { total: 0, plans: [], statuses: [] },
  }
}

const keyPage = ref<PoolKeysPageResponse>(createEmptyKeyPage())
const keysLoading = ref(false)
const keysLoadedOnce = ref(false)
const refreshingCurrentPageQuota = ref(false)
const searchQuery = ref(restoredViewState.search)
const statusFilter = ref(restoredViewState.status)
const planTypeFilter = ref(restoredViewState.planType)
const currentPage = ref(restoredViewState.page)
const pageSize = ref(restoredViewState.pageSize)
const sortBy = ref<PoolManagementSortBy | null>(restoredViewState.sortBy)
const sortOrder = ref<PoolManagementSortOrder>(restoredViewState.sortOrder)
const poolStatsMode = ref<PoolManagementStatsMode>(restoredViewState.statsMode)
const hasPoolKeyFilters = computed(() =>
  searchQuery.value.trim().length > 0 || statusFilter.value !== 'all' || planTypeFilter.value !== 'all'
)
const poolPlanSummaryItems = computed(() => keyPage.value.summary?.plans || [])
const poolStatusSummaryItems = computed(() => keyPage.value.summary?.statuses || [])
const poolSummaryTotal = computed(() => Number(keyPage.value.summary?.total ?? keyPage.value.total ?? 0))
const MANUAL_QUOTA_REFRESH_COOLDOWN_SECONDS = 5 * 60
const refreshingOAuthKeyId = ref<string | null>(null)
const resettingCycleKeyId = ref<string | null>(null)
const resettingCodexQuotaKeyId = ref<string | null>(null)
const preparingAccountTestKeyId = ref<string | null>(null)
const poolAccountTestDialogRef = ref<InstanceType<typeof PoolAccountTestDialog> | null>(null)
const codexQuotaResetIdempotencyKeys = new Map<string, string>()
const savingProxyKeyId = ref<string | null>(null)
const proxyDesktopPopoverOpenKeyId = ref<string | null>(null)
const proxyMobilePopoverOpenKeyId = ref<string | null>(null)
const scoreDesktopPopoverOpenKeyId = ref<string | null>(null)
const scoreMobilePopoverOpenKeyId = ref<string | null>(null)
const deletingKeyId = ref<string | null>(null)
const deletingSelectedKeys = ref(false)
const clearingSelectedCooldown = ref(false)
const updatingSelectedKeyStatus = ref<'enable' | 'disable' | null>(null)
const selectedPoolKeyIds = ref<string[]>([])
const allFilteredPoolKeysSelected = ref(false)
const togglingKeyId = ref<string | null>(null)
const editingPriorityKeyId = ref<string | null>(null)
const editingPriorityValue = ref<number>(0)
const prioritySavingKeyId = ref<string | null>(null)

const keyPermissionsDialogOpen = ref(false)
const keyFormDialogOpen = ref(false)
const oauthKeyEditDialogOpen = ref(false)
const editingKeyDetail = ref<PoolKeyDetail | null>(null)
const showBulkClearCooldownAction = computed(() => statusFilter.value === 'temporary_unavailable')
const showBulkEnableAction = computed(() => statusFilter.value === 'disabled')

function togglePoolStatsMode() {
  poolStatsMode.value = poolStatsMode.value === 'current_cycle'
    ? 'account_total'
    : 'current_cycle'
}

function clearPoolKeyFilters() {
  if (!hasPoolKeyFilters.value) return
  clearPoolKeySelection()
  suppressFiltersWatch = true
  searchQuery.value = ''
  statusFilter.value = 'all'
  planTypeFilter.value = 'all'
  suppressFiltersWatch = false
  if (currentPage.value !== 1) {
    currentPage.value = 1
    return
  }
  void loadKeys({ cacheTtlMs: POOL_KEYS_CACHE_TTL_MS })
}

function selectPoolPlanFilter(planCode: string): void {
  planTypeFilter.value = planTypeFilter.value === planCode ? 'all' : planCode
}

function selectPoolStatusFilter(statusCode: string): void {
  const normalized = statusCode as PoolManagementViewState['status']
  statusFilter.value = statusFilter.value === normalized ? 'all' : normalized
}

watch(
  () => getQueryValue('search') ?? '',
  (value) => {
    if (searchQuery.value === value) return
    searchQuery.value = value
  },
  { immediate: true },
)

watch(
  () => readPoolManagementViewState({ status: getQueryValue('status') }).status,
  (value) => {
    if (statusFilter.value === value) return
    suppressFiltersWatch = true
    statusFilter.value = value
    suppressFiltersWatch = false
  },
  { immediate: true },
)

watch(
  () => readPoolManagementViewState({ planType: getQueryValue('planType') }).planType,
  (value) => {
    if (planTypeFilter.value === value) return
    suppressFiltersWatch = true
    planTypeFilter.value = value
    suppressFiltersWatch = false
  },
  { immediate: true },
)

watch(
  () => readPoolManagementViewState({ page: getQueryValue('page') }).page,
  (value) => {
    if (currentPage.value === value) return
    currentPage.value = value
  },
  { immediate: true },
)

watch(
  () => readPoolManagementViewState({ pageSize: getQueryValue('pageSize') }).pageSize,
  (value) => {
    if (pageSize.value === value) return
    pageSize.value = value
  },
  { immediate: true },
)

watch(
  () => readPoolManagementViewState({
    sortBy: getQueryValue('sortBy'),
    sortOrder: getQueryValue('sortOrder'),
  }),
  (value) => {
    if (sortBy.value === value.sortBy && sortOrder.value === value.sortOrder) return
    sortBy.value = value.sortBy
    sortOrder.value = value.sortOrder
  },
  { immediate: true },
)

watch(
  () => readPoolManagementViewState(
    { statsMode: getQueryValue('statsMode') },
    poolManagementViewStorage,
  ).statsMode,
  (value) => {
    if (poolStatsMode.value === value) return
    poolStatsMode.value = value
  },
  { immediate: true },
)

watch(
  () => getQueryValue('providerId'),
  (value) => {
    if (overviewLoading.value) return
    if (!value || value === selectedProviderId.value) return
    if (!poolProviders.value.some(item => item.provider_id === value)) return
    void selectProvider(value, {
      preserveSearch: true,
      preserveStatus: true,
      preservePagination: true,
      cacheTtlMs: POOL_KEYS_CACHE_TTL_MS,
    })
  },
)

watch(
  [selectedProviderId, searchQuery, statusFilter, planTypeFilter, currentPage, pageSize, sortBy, sortOrder, poolStatsMode],
  ([providerId, search, status, planType, page, pageSizeValue, sortByValue, sortOrderValue, statsMode]) => {
    const nextState: PoolManagementViewState = {
      providerId,
      search,
      status: status as PoolManagementViewState['status'],
      planType: String(planType || 'all'),
      page,
      pageSize: pageSizeValue,
      sortBy: sortByValue,
      sortOrder: sortOrderValue,
      statsMode: statsMode as PoolManagementStatsMode,
    }
    patchQuery(buildPoolManagementQueryPatch(nextState))
    writePoolManagementViewState(nextState, poolManagementViewStorage)
  },
  { immediate: true },
)
interface QuotaProgressItem {
  label: string
  remainingPercent: number
  detail?: string
  resetAtSeconds?: number | null
  resetSeconds?: number | null
  updatedAtSeconds?: number | null
}

interface PoolCodexCycleStatsRow {
  key: PoolStatsMetric['key']
  label: string
  values: PoolStatsMetric[]
}

const CODEX_CYCLE_STAT_KEYS: Array<PoolStatsMetric['key']> = ['request_count', 'total_tokens', 'total_cost_usd']
const CODEX_CYCLE_STAT_LABELS: Record<PoolStatsMetric['key'], string> = {
  request_count: t('poolManagement.requestCount'),
  total_tokens: 'Token',
  total_cost_usd: t('poolManagement.baseCost'),
}

type PoolKeyUiState = {
  rowClass: string
  schedulingBadgeLabel: string
  schedulingBadgeVariant: PoolStatusVariant
  schedulingTitle: string
  oauthOrgBadge: ReturnType<typeof getOAuthOrgBadge>
  visibleOAuthState: ReturnType<typeof getOAuthStatusDisplayWithFallback>
  oauthStatusTitle: string
  oauthRefreshButtonTitle: string
  showOAuthRefreshControl: boolean
  canRefreshToken: boolean
  planLabel: string
  planClass: string
  quotaFallbackText: string | null
  quotaTextClass: string
  quotaFreshnessText: string | null
  quotaFreshnessStale: boolean
  importedAtRelative: string
  lastUsedRelative: string
  statsDisplay: PoolStatsDisplay
  mobileTagItems: PoolMobileTagItem[]
  mobileActionIds: PoolMobileActionId[]
}

const quotaProgressMap = computed<Record<string, QuotaProgressItem[]>>(() => {
  const map: Record<string, QuotaProgressItem[]> = {}
  for (const key of keyPage.value.keys) {
    map[key.key_id] = parseQuotaProgressItems(key)
  }
  return map
})

const keyUiStateMap = computed<Record<string, PoolKeyUiState>>(() => {
  const map: Record<string, PoolKeyUiState> = {}

  for (const key of keyPage.value.keys) {
    const visibleOAuthState = getVisibleOAuthState(key)
    const oauthOrgBadge = getOAuthOrgBadge(key)
    const quotaFallbackText = getQuotaFallbackText(key)
    const quotaFreshness = getGrokOAuthQuotaFreshness(key, selectedProviderType.value)
    const planType = resolvePoolKeyPlanType(key)
    const canRefreshToken = canRefreshOAuthCredential(key)
    const showOAuthRefreshControl = shouldShowOAuthRefreshControl(key, selectedProviderType.value)

    map[key.key_id] = {
      rowClass: getRowClass(key),
      schedulingBadgeLabel: getSchedulingBadgeLabel(key),
      schedulingBadgeVariant: getSchedulingBadgeVariant(key),
      schedulingTitle: getSchedulingTitle(key),
      oauthOrgBadge,
      visibleOAuthState,
      oauthStatusTitle: visibleOAuthState ? getOAuthStatusTitle(key) : '',
      oauthRefreshButtonTitle: showOAuthRefreshControl ? getOAuthRefreshButtonTitle(key) : '',
      showOAuthRefreshControl,
      canRefreshToken,
      planLabel: planType ? formatOAuthPlanType(planType) : '',
      planClass: planType ? getOAuthPlanTypeClass(planType) : '',
      quotaFallbackText,
      quotaTextClass: quotaFallbackText ? getQuotaTextClass(quotaFallbackText) : '',
      quotaFreshnessText: quotaFreshness
        ? `${quotaFreshness.isStale ? '额度数据已过期 · 更新' : '额度更新'} ${formatUnixSeconds(quotaFreshness.updatedAtSeconds)}`
        : null,
      quotaFreshnessStale: quotaFreshness?.isStale ?? false,
      importedAtRelative: formatPoolKeyImportedAt(key),
      lastUsedRelative: key.last_used_at ? formatRelativeTime(key.last_used_at) : '-',
      statsDisplay: buildPoolStatsDisplay(key, selectedProviderType.value, poolStatsMode.value),
      mobileTagItems: getMobileTagItems(key),
      mobileActionIds: splitPoolMobileActions({
        canDownloadOrCopy: true,
        showRefreshToken: showOAuthRefreshControl,
        canResetCycleStats: canResetCycleStats(key),
        canClearCooldown: Boolean(key.cooldown_reason),
        hasProxy: true,
      }).primary,
    }
  }

  return map
})

function getPoolKeyStatsDisplay(key: PoolKeyDetail): PoolStatsDisplay {
  return keyUiStateMap.value[key.key_id]?.statsDisplay
    ?? buildPoolStatsDisplay(key, selectedProviderType.value, poolStatsMode.value)
}

function isPoolKeyCycleStatsDisplay(key: PoolKeyDetail): boolean {
  return getPoolKeyStatsDisplay(key).kind === 'codex_cycle'
}

function getPoolKeyCycleStatsGroups(key: PoolKeyDetail): PoolCodexCycleStatsGroup[] {
  const display = getPoolKeyStatsDisplay(key)
  return display.kind === 'codex_cycle' ? display.groups : []
}

function createMissingCycleMetric(key: PoolStatsMetric['key']): PoolStatsMetric {
  return {
    key,
    label: CODEX_CYCLE_STAT_LABELS[key],
    value: t('commonUi.statisticsPending'),
    missing: true,
  }
}

function findCycleMetric(
  group: PoolCodexCycleStatsGroup | undefined,
  key: PoolStatsMetric['key'],
): PoolStatsMetric {
  return group?.metrics.find(metric => metric.key === key) ?? createMissingCycleMetric(key)
}

function getPoolKeyCycleStatsRows(key: PoolKeyDetail): PoolCodexCycleStatsRow[] {
  const groups = getPoolKeyCycleStatsGroups(key)

  return CODEX_CYCLE_STAT_KEYS.map((metricKey) => {
    return {
      key: metricKey,
      label: CODEX_CYCLE_STAT_LABELS[metricKey],
      values: groups.map(group => findCycleMetric(group, metricKey)),
    }
  })
}

function getPoolKeyCycleStatsGridStyle(key: PoolKeyDetail): Record<string, string> {
  const groupCount = Math.max(getPoolKeyCycleStatsGroups(key).length, 1)
  return {
    gridTemplateColumns: `38px repeat(${groupCount}, minmax(0, 1fr))`,
  }
}

function getPoolKeyAccountStatsMetrics(key: PoolKeyDetail): PoolStatsMetric[] {
  const display = getPoolKeyStatsDisplay(key)
  if (display.kind === 'account_total') return display.metrics
  const accountDisplay = buildPoolStatsDisplay(key, selectedProviderType.value, 'account_total')
  return accountDisplay.kind === 'account_total' ? accountDisplay.metrics : []
}

const quotaRefreshSupported = computed(() => {
  return selectedProviderType.value === 'codex'
    || selectedProviderType.value === 'kiro'
    || selectedProviderType.value === 'antigravity'
    || selectedProviderType.value === 'grok'
    || selectedProviderType.value === 'grok_oauth'
    || selectedProviderType.value === 'chatgpt_web'
})

function canResetCycleStats(_key: PoolKeyDetail): boolean {
  return selectedProviderType.value === 'codex' && Boolean(_key.key_id)
}

function getCodexResetCreditCount(key: PoolKeyDetail): number | null {
  if (selectedProviderType.value !== 'codex') return null
  const raw = key.status_snapshot?.quota?.reset_credits?.available_count
  if (typeof raw !== 'number' || !Number.isFinite(raw) || raw < 0) return null
  return Math.floor(raw)
}

function canResetCodexQuota(key: PoolKeyDetail): boolean {
  return (getCodexResetCreditCount(key) ?? 0) > 0
}

function getCodexQuotaResetButtonTitle(key: PoolKeyDetail): string {
  const count = getCodexResetCreditCount(key)
  if (count === null) return t('poolManagement.resetQuotaUnknown')
  if (count <= 0) return t('poolManagement.resetQuotaUnavailable')
  return t('poolManagement.resetQuota')
}

const refreshCurrentPageLoading = computed(() => {
  return keysLoading.value || refreshingCurrentPageQuota.value
})

function resetKeyPage(page = currentPage.value, pageSizeValue = pageSize.value): void {
  keyPage.value = createEmptyKeyPage(page, pageSizeValue)
}

function clearCodexCycleStatsReload(): void {
  if (codexCycleStatsReloadTimer !== null) {
    clearTimeout(codexCycleStatsReloadTimer)
    codexCycleStatsReloadTimer = null
  }
  codexCycleStatsReloadAttempts = 0
  codexCycleStatsReloadContext = ''
}

function scheduleCodexCycleStatsReload(
  keys: PoolKeyDetail[],
  providerId: string,
  context: string,
): void {
  const hasPendingStats = keys.some(key =>
    hasPendingCodexCycleStats(key, key.provider_type ?? selectedProviderType.value),
  )
  if (!hasPendingStats) {
    clearCodexCycleStatsReload()
    return
  }

  if (codexCycleStatsReloadContext !== context) {
    clearCodexCycleStatsReload()
    codexCycleStatsReloadContext = context
  }
  if (
    codexCycleStatsReloadTimer !== null
    || codexCycleStatsReloadAttempts >= CODEX_CYCLE_STATS_RELOAD_MAX_ATTEMPTS
  ) {
    return
  }

  codexCycleStatsReloadTimer = window.setTimeout(() => {
    codexCycleStatsReloadTimer = null
    if (
      selectedProviderId.value !== providerId
      || codexCycleStatsReloadContext !== context
    ) {
      return
    }
    codexCycleStatsReloadAttempts += 1
    void loadKeys()
  }, CODEX_CYCLE_STATS_RELOAD_INTERVAL_MS)
}

function refreshOverviewInBackground(): void {
  void loadOverview()
}

function applyQuotaRefreshResultToCurrentPage(result: Awaited<ReturnType<typeof refreshProviderQuota>>): void {
  const successfulResults = Array.isArray(result.results)
    ? result.results.filter((item) => item.status === 'success' && item.quota_snapshot)
    : []
  if (successfulResults.length === 0) return

  const quotaByKeyId = new Map(successfulResults.map((item) => [item.key_id, item.quota_snapshot!]))
  keyPage.value.keys = keyPage.value.keys.map((key) => {
    const quotaSnapshot = quotaByKeyId.get(key.key_id)
    if (!quotaSnapshot) return key
    return {
      ...key,
      quota_updated_at: quotaSnapshot.updated_at ?? quotaSnapshot.observed_at ?? key.quota_updated_at ?? null,
      status_snapshot: {
        oauth: key.status_snapshot?.oauth ?? {
          code: 'none',
          label: null,
          reason: null,
          expires_at: null,
          invalid_at: null,
          source: null,
          requires_reauth: false,
          expiring_soon: false,
        },
        account: key.status_snapshot?.account ?? {
          code: 'ok',
          label: null,
          reason: null,
          blocked: false,
          source: null,
          recoverable: false,
        },
        quota: {
          ...(key.status_snapshot?.quota ?? { code: 'unknown', exhausted: false }),
          ...quotaSnapshot,
        },
      },
    }
  })
}

function normalizeQuotaUpdatedAt(raw: number | null | undefined): number | null {
  const value = Number(raw ?? 0)
  if (!Number.isFinite(value) || value <= 0) return null
  if (value > 1_000_000_000_000) {
    return Math.floor(value / 1000)
  }
  return Math.floor(value)
}

const currentPageQuotaRefreshStats = computed(() => {
  void countdownTick.value
  const seen = new Set<string>()
  const eligibleIds: string[] = []
  let cooledDownCount = 0
  let minRemainingSeconds = 0
  const nowSeconds = Math.floor(Date.now() / 1000)
  for (const key of keyPage.value.keys) {
    const id = String(key.key_id || '').trim()
    if (!id || seen.has(id)) continue
    seen.add(id)
    const updatedAt = normalizeQuotaUpdatedAt(key.quota_updated_at ?? null)
    if (updatedAt == null) {
      eligibleIds.push(id)
      continue
    }
    const remaining = MANUAL_QUOTA_REFRESH_COOLDOWN_SECONDS - (nowSeconds - updatedAt)
    if (remaining > 0) {
      cooledDownCount += 1
      if (minRemainingSeconds <= 0 || remaining < minRemainingSeconds) {
        minRemainingSeconds = remaining
      }
      continue
    }
    eligibleIds.push(id)
  }
  return {
    total: seen.size,
    eligibleIds,
    cooledDownCount,
    minRemainingSeconds,
  }
})

async function refreshCurrentPageQuotaInBackground(
  options: { silent?: boolean; reloadAfter?: boolean } = {},
): Promise<boolean> {
  if (!selectedProviderId.value || !quotaRefreshSupported.value) return false

  const providerId = selectedProviderId.value
  const quotaStats = currentPageQuotaRefreshStats.value
  if (quotaStats.eligibleIds.length === 0) {
    if (!options.silent && quotaStats.total > 0 && quotaStats.cooledDownCount > 0) {
      const waitText = quotaStats.minRemainingSeconds > 0
        ? formatTTL(quotaStats.minRemainingSeconds)
        : t('poolManagement.later')
      showWarning(t('poolManagement.allCooldown', { wait: waitText }))
    }
    return false
  }

  if (refreshingCurrentPageQuota.value) {
    return false
  }

  refreshingCurrentPageQuota.value = true
  try {
    const result = await refreshProviderQuota(providerId, quotaStats.eligibleIds)
    applyQuotaRefreshResultToCurrentPage(result)
    const successCount = Number(result.success || 0)
    const failedCount = Number(result.failed || 0)
    const skippedCount = Math.max(quotaStats.total - quotaStats.eligibleIds.length, 0)

    // 刷新当前页数据，展示最新额度与状态
    if (selectedProviderId.value === providerId && options.reloadAfter !== false) {
      await loadKeys()
    }

    if (!options.silent) {
      const skippedText = skippedCount > 0 ? t('poolManagement.cooldownSkipped', { count: skippedCount }) : ''
      const firstFailureMessage = result.results.find(item => item.status !== 'success')?.message?.trim()
      if (successCount === 0 && failedCount > 0 && firstFailureMessage) {
        showError(t('poolManagement.refreshFailedDetail', { error: firstFailureMessage, skipped: skippedText }))
      } else {
        success(t('poolManagement.refreshComplete', { success: successCount, failed: failedCount, skipped: skippedText }))
      }
    }
    return true
  } catch (err) {
    showError(parseApiError(err, t('poolManagement.refreshPageFailed')))
    return false
  } finally {
    refreshingCurrentPageQuota.value = false
  }
}

const refreshButtonTitle = computed(() => {
  if (refreshCurrentPageLoading.value) return t('poolManagement.refreshing', '刷新中...')
  if (!selectedProviderId.value) return t('poolManagement.refresh', '刷新')
  if (!quotaRefreshSupported.value) return t('poolManagement.refreshData', '刷新数据')

  const quotaStats = currentPageQuotaRefreshStats.value
  if (quotaStats.total === 0) return t('poolManagement.refreshDataQuota', '刷新数据和额度')
  if (quotaStats.eligibleIds.length === 0 && quotaStats.cooledDownCount > 0) {
    const waitText = quotaStats.minRemainingSeconds > 0
      ? formatTTL(quotaStats.minRemainingSeconds)
      : t('poolManagement.later')
    return t('poolManagement.refreshDataCooldown', { wait: waitText })
  }
  if (quotaStats.cooledDownCount > 0) {
    return t('poolManagement.refreshEligible', { eligible: quotaStats.eligibleIds.length, total: quotaStats.total })
  }
  return t('poolManagement.refreshDataQuota', '刷新数据和额度')
})

async function refreshCurrentPage() {
  const quotaDidReload = await refreshCurrentPageQuotaInBackground({ reloadAfter: true })
  if (!quotaDidReload) {
    await refresh()
  }
}

async function loadKeys(options: { cacheTtlMs?: number } = {}) {
  if (!selectedProviderId.value) return
  const requestId = ++keysRequestId
  const providerId = selectedProviderId.value
  const page = currentPage.value
  const pageSizeValue = pageSize.value
  const search = searchQuery.value || undefined
  const status = statusFilter.value
  const planType = planTypeFilter.value === 'all' ? undefined : planTypeFilter.value
  const sortByValue = sortBy.value || undefined
  keysLoading.value = true
  try {
    const nextPage = await listPoolKeys(providerId, {
      page,
      page_size: pageSizeValue,
      search,
      status,
      plan_type: planType,
      sort_by: sortByValue || undefined,
      sort_order: sortByValue ? sortOrder.value : undefined,
    }, {
      cacheTtlMs: options.cacheTtlMs ?? 0,
    })
    if (requestId !== keysRequestId || selectedProviderId.value !== providerId) return
    const resolvedPage = resolvePoolManagementPageAfterLoad({
      requestedPage: page,
      pageSize: pageSizeValue,
      total: nextPage.total,
    })
    if (resolvedPage !== page) {
      currentPage.value = resolvedPage
      return
    }
    keyPage.value = nextPage
    keysLoadedOnce.value = true
    scheduleCodexCycleStatsReload(
      nextPage.keys,
      providerId,
      JSON.stringify([
        providerId,
        page,
        pageSizeValue,
        search ?? '',
        status,
        planType ?? '',
        sortByValue ?? '',
        sortByValue ? sortOrder.value : '',
      ]),
    )
  } catch (err) {
    if (requestId !== keysRequestId || selectedProviderId.value !== providerId) return
    resetKeyPage(page, pageSizeValue)
    keysLoadedOnce.value = true
    showError(parseApiError(err))
  } finally {
    if (requestId === keysRequestId) {
      keysLoading.value = false
    }
  }
}

watch([currentPage, pageSize], () => {
  void loadKeys({ cacheTtlMs: POOL_KEYS_CACHE_TTL_MS })
})

watch(statusFilter, () => {
  if (suppressFiltersWatch) return
  clearPoolKeySelection()
  currentPage.value = 1
  void loadKeys({ cacheTtlMs: POOL_KEYS_CACHE_TTL_MS })
})

watch(planTypeFilter, () => {
  if (suppressFiltersWatch) return
  clearPoolKeySelection()
  currentPage.value = 1
  void loadKeys({ cacheTtlMs: POOL_KEYS_CACHE_TTL_MS })
})

watch([sortBy, sortOrder], () => {
  if (currentPage.value !== 1) {
    currentPage.value = 1
    return
  }
  void loadKeys({ cacheTtlMs: POOL_KEYS_CACHE_TTL_MS })
})

watch(searchQuery, () => {
  if (suppressFiltersWatch) return
  clearPoolKeySelection()
  currentPage.value = 1
  if (keysSearchDebounceTimer !== null) {
    clearTimeout(keysSearchDebounceTimer)
  }
  keysSearchDebounceTimer = window.setTimeout(() => {
    keysSearchDebounceTimer = null
    void loadKeys({ cacheTtlMs: POOL_KEYS_CACHE_TTL_MS })
  }, 300)
})

function normalizeAuthTypeForEdit(key: PoolKeyDetail): EndpointAPIKey['auth_type'] {
  if (isOAuthManagedCredential(key)) return 'oauth'
  if (isServiceAccountCredential(key)) return 'service_account'
  if ((key.auth_type || '').trim().toLowerCase() === 'bearer') return 'bearer'
  return 'api_key'
}

function toEndpointApiKey(key: PoolKeyDetail): EndpointAPIKey {
  const nowIso = new Date().toISOString()
  return {
    id: key.key_id,
    provider_id: selectedProviderId.value || '',
    api_formats: key.api_formats || [],
    api_key_masked: getProviderMaskedSecretLabel(key, selectedProviderType.value),
    auth_type: normalizeAuthTypeForEdit(key),
    auth_type_by_format: key.auth_type_by_format ?? null,
    credential_kind: key.credential_kind ?? null,
    runtime_auth_kind: key.runtime_auth_kind ?? null,
    oauth_managed: key.oauth_managed ?? undefined,
    can_refresh_oauth: key.can_refresh_oauth ?? undefined,
    can_export_oauth: key.can_export_oauth ?? undefined,
    can_edit_oauth: key.can_edit_oauth ?? undefined,
    name: key.key_name || t('poolManagement.unnamedAccount'),
    rate_multipliers: key.rate_multipliers ?? null,
    internal_priority: key.internal_priority ?? 50,
    rpm_limit: key.rpm_limit ?? null,
    allowed_models: key.allowed_models ?? null,
    capabilities: key.capabilities ?? null,
    cache_ttl_minutes: key.cache_ttl_minutes ?? 60,
    max_probe_interval_minutes: key.max_probe_interval_minutes ?? 32,
    health_score: key.health_score ?? 1,
    circuit_breaker_open: key.circuit_breaker_open ?? false,
    consecutive_failures: 0,
    request_count: 0,
    success_count: 0,
    error_count: 0,
    success_rate: 0,
    avg_response_time_ms: 0,
    is_active: key.is_active,
    note: key.note || '',
    last_used_at: key.last_used_at || undefined,
    created_at: key.created_at || nowIso,
    updated_at: nowIso,
    auto_fetch_models: key.auto_fetch_models ?? false,
    locked_models: key.locked_models || [],
    model_include_patterns: key.model_include_patterns || [],
    model_exclude_patterns: key.model_exclude_patterns || [],
    oauth_expires_at: key.oauth_expires_at ?? null,
    oauth_email: key.oauth_email ?? null,
    oauth_phone: key.oauth_phone ?? null,
    oauth_plan_type: key.oauth_plan_type ?? null,
    oauth_account_id: key.oauth_account_id ?? null,
    oauth_account_user_id: key.oauth_account_user_id ?? null,
    oauth_account_name: key.oauth_account_name ?? null,
    oauth_organizations: key.oauth_organizations ?? [],
    oauth_temporary: key.oauth_temporary ?? false,
    oauth_invalid_at: key.oauth_invalid_at ?? null,
    oauth_invalid_reason: key.oauth_invalid_reason ?? null,
    status_snapshot: key.status_snapshot ?? null,
    proxy: key.proxy ?? null,
  }
}

const editingKey = computed<EndpointAPIKey | null>(() => {
  if (!editingKeyDetail.value) return null
  return toEndpointApiKey(editingKeyDetail.value)
})

const selectedPoolKeyIdSet = computed(() => new Set(selectedPoolKeyIds.value))
const poolSelectionActionBusy = computed(() =>
  deletingSelectedKeys.value || clearingSelectedCooldown.value || updatingSelectedKeyStatus.value !== null
)
const selectedPoolKeyCount = computed(() => allFilteredPoolKeysSelected.value ? keyPage.value.total : selectedPoolKeyIds.value.length)
const currentPagePoolKeyIds = computed(() => keyPage.value.keys.map(key => key.key_id).filter(Boolean))
const selectedCurrentPagePoolKeyCount = computed(() => {
  if (allFilteredPoolKeysSelected.value) return currentPagePoolKeyIds.value.length
  const selected = selectedPoolKeyIdSet.value
  return currentPagePoolKeyIds.value.filter(id => selected.has(id)).length
})
const isCurrentPageFullySelected = computed(() => {
  const ids = currentPagePoolKeyIds.value
  return ids.length > 0 && selectedCurrentPagePoolKeyCount.value === ids.length
})
const isCurrentPagePartiallySelected = computed(() => {
  const selectedCount = selectedCurrentPagePoolKeyCount.value
  return selectedCount > 0 && selectedCount < currentPagePoolKeyIds.value.length
})

function isPoolKeySelected(keyId: string): boolean {
  if (allFilteredPoolKeysSelected.value) return true
  return selectedPoolKeyIdSet.value.has(keyId)
}

function togglePoolKeySelection(keyId: string, checked: boolean): void {
  if (allFilteredPoolKeysSelected.value) return
  const normalized = String(keyId || '').trim()
  if (!normalized) return
  const next = new Set(selectedPoolKeyIds.value)
  if (checked) next.add(normalized)
  else next.delete(normalized)
  selectedPoolKeyIds.value = [...next]
}

function toggleSelectCurrentPageKeys(checked: boolean): void {
  if (allFilteredPoolKeysSelected.value) return
  const next = new Set(selectedPoolKeyIds.value)
  for (const keyId of currentPagePoolKeyIds.value) {
    if (checked) next.add(keyId)
    else next.delete(keyId)
  }
  selectedPoolKeyIds.value = [...next]
}

function clearPoolKeySelection(): void {
  allFilteredPoolKeysSelected.value = false
  selectedPoolKeyIds.value = []
}

function selectAllFilteredPoolKeys(): void {
  allFilteredPoolKeysSelected.value = true
  selectedPoolKeyIds.value = []
}

function buildPoolSelectionFilters(): { search?: string; status?: PoolManagementViewState['status']; plan_type?: string } {
  const search = searchQuery.value.trim()
  const status = statusFilter.value === 'all' ? undefined : statusFilter.value
  const planType = planTypeFilter.value === 'all' ? undefined : String(planTypeFilter.value || '').trim()
  return {
    ...(search ? { search } : {}),
    ...(status ? { status } : {}),
    ...(planType ? { plan_type: planType } : {}),
  }
}

function sortCurrentPageKeysByPriority() {
  keyPage.value.keys = [...keyPage.value.keys].sort((a, b) => {
    const pa = Number(a.internal_priority ?? 50)
    const pb = Number(b.internal_priority ?? 50)
    if (pa !== pb) return pa - pb
    return (a.created_at || '').localeCompare(b.created_at || '')
  })
}

function handleTableSort(payload: { key: string, direction: PoolManagementSortOrder }) {
  if (payload.key !== 'imported_at' && payload.key !== 'last_used_at' && payload.key !== 'score') return
  sortBy.value = payload.key
  sortOrder.value = payload.direction
}

function startEditInternalPriority(key: PoolKeyDetail) {
  editingPriorityKeyId.value = key.key_id
  editingPriorityValue.value = Number(key.internal_priority ?? 50)
}

function cancelEditInternalPriority() {
  editingPriorityKeyId.value = null
  editingPriorityValue.value = 0
}

async function applyInternalPriority(key: PoolKeyDetail, nextPriority: number) {
  const normalized = Math.max(1, Math.min(999999, Math.floor(nextPriority)))
  if (Number(key.internal_priority ?? 50) === normalized) return

  prioritySavingKeyId.value = key.key_id
  try {
    await updateProviderKey(key.key_id, { internal_priority: normalized })
    key.internal_priority = normalized
    sortCurrentPageKeysByPriority()
    success(t('poolManagement.priorityUpdated'))
  } catch (err) {
    showError(parseApiError(err, t('poolManagement.priorityUpdateFailed')))
  } finally {
    prioritySavingKeyId.value = null
  }
}

async function quickEditInternalPriority(key: PoolKeyDetail) {
  const raw = window.prompt(t('poolManagement.priorityPrompt'), String(key.internal_priority ?? 50))
  if (raw === null) return
  const parsed = Number(raw)
  if (!Number.isFinite(parsed)) {
    showWarning(t('poolManagement.invalidPriority'))
    return
  }
  await applyInternalPriority(key, parsed)
}

async function finishEditInternalPriority(
  key: PoolKeyDetail,
  event: FocusEvent | KeyboardEvent,
) {
  if (prioritySavingKeyId.value) return
  const target = event.target as HTMLInputElement | null
  const raw = target?.value ?? String(editingPriorityValue.value)
  const parsed = Number(raw)
  const nextPriority = Number.isFinite(parsed) ? parsed : Number(key.internal_priority ?? 50)
  cancelEditInternalPriority()
  await applyInternalPriority(key, nextPriority)
}

function handleEditKey(key: PoolKeyDetail) {
  editingKeyDetail.value = key
  if (canEditOAuthCredential(key)) {
    oauthKeyEditDialogOpen.value = true
  } else {
    keyFormDialogOpen.value = true
  }
}

function handleKeyPermissions(key: PoolKeyDetail) {
  editingKeyDetail.value = key
  keyPermissionsDialogOpen.value = true
}

async function handleDialogSaved() {
  editingKeyDetail.value = null
  await loadKeys()
}

function closeKeyFormDialog() {
  keyFormDialogOpen.value = false
  editingKeyDetail.value = null
}

function closeOAuthEditDialog() {
  oauthKeyEditDialogOpen.value = false
  editingKeyDetail.value = null
}

function closeKeyPermissionsDialog() {
  keyPermissionsDialogOpen.value = false
  editingKeyDetail.value = null
}

function getKeyProxyNodeName(key: PoolKeyDetail): string | null {
  if (!key.proxy?.node_id) return null
  const node = proxyNodesStore.nodes.find(n => n.id === key.proxy?.node_id)
  return node ? node.name : `${key.proxy.node_id.slice(0, 8)}...`
}

function handleScoreDesktopPopoverToggle(keyId: string, open: boolean) {
  scoreDesktopPopoverOpenKeyId.value = open ? keyId : null
  if (open) {
    scoreMobilePopoverOpenKeyId.value = null
  }
}

function handleScoreMobilePopoverToggle(keyId: string, open: boolean) {
  scoreMobilePopoverOpenKeyId.value = open ? keyId : null
  if (open) {
    scoreDesktopPopoverOpenKeyId.value = null
  }
}

function handleProxyDesktopPopoverToggle(keyId: string, open: boolean) {
  proxyDesktopPopoverOpenKeyId.value = open ? keyId : null
  if (open) {
    proxyMobilePopoverOpenKeyId.value = null
  }
  if (open) {
    proxyNodesStore.ensureLoaded()
  }
}

function handleProxyMobilePopoverToggle(keyId: string, open: boolean) {
  proxyMobilePopoverOpenKeyId.value = open ? keyId : null
  if (open) {
    proxyDesktopPopoverOpenKeyId.value = null
  }
  if (open) {
    proxyNodesStore.ensureLoaded()
  }
}

async function setKeyProxy(key: PoolKeyDetail, nodeId: string) {
  savingProxyKeyId.value = key.key_id
  try {
    await updateProviderKey(key.key_id, {
      proxy: { node_id: nodeId, enabled: true },
    })
    key.proxy = { node_id: nodeId, enabled: true }
    proxyDesktopPopoverOpenKeyId.value = null
    proxyMobilePopoverOpenKeyId.value = null
    success(t('poolManagement.proxySet'))
  } catch (err) {
    showError(parseApiError(err, t('poolManagement.proxySetFailed')))
  } finally {
    savingProxyKeyId.value = null
  }
}

async function clearKeyProxy(key: PoolKeyDetail) {
  savingProxyKeyId.value = key.key_id
  try {
    await updateProviderKey(key.key_id, { proxy: null })
    key.proxy = null
    proxyDesktopPopoverOpenKeyId.value = null
    proxyMobilePopoverOpenKeyId.value = null
    success(t('poolManagement.proxyCleared'))
  } catch (err) {
    showError(parseApiError(err, t('poolManagement.proxyClearFailed')))
  } finally {
    savingProxyKeyId.value = null
  }
}

async function handleDeleteKey(key: PoolKeyDetail) {
  const confirmed = await confirm({
    title: t('poolManagement.deleteAccount'),
    message: t('poolManagement.deleteAccountConfirm', { name: getPoolAccountDisplayName(key) }),
    confirmText: t('poolManagement.delete'),
    variant: 'destructive',
  })
  if (!confirmed) return

  deletingKeyId.value = key.key_id
  try {
    await deleteEndpointKey(key.key_id)
    success(t('poolManagement.accountDeleted'))
    selectedPoolKeyIds.value = selectedPoolKeyIds.value.filter(id => id !== key.key_id)
    // 乐观更新：直接从本地列表移除，避免等待网络重载
    keyPage.value.keys = keyPage.value.keys.filter(k => k.key_id !== key.key_id)
    keyPage.value.total = Math.max(0, keyPage.value.total - 1)
    // 当前页已空且不是第一页时，自动跳转到前一页
    if (keyPage.value.keys.length === 0 && currentPage.value > 1) {
      currentPage.value--
    }
    refreshOverviewInBackground()
  } catch (err) {
    showError(parseApiError(err, t('poolManagement.deleteAccountFailed')))
  } finally {
    deletingKeyId.value = null
  }
}

const POOL_BULK_DELETE_POLL_INTERVAL_MS = 2000
const POOL_BULK_DELETE_POLL_MAX_MS = 10 * 60 * 1000
const POOL_BULK_DELETE_POLL_MAX_FAILURES = 3

function getPoolDeleteTaskDeletedCount(task: Awaited<ReturnType<typeof getPoolBatchDeleteTask>>): number {
  const raw = task.deleted ?? task.deleted_keys ?? 0
  const value = Number(raw)
  return Number.isFinite(value) && value > 0 ? value : 0
}

async function pollPoolBatchDeleteTask(
  providerId: string,
  taskId: string,
): Promise<{ status: string; deleted: number }> {
  const deadline = Date.now() + POOL_BULK_DELETE_POLL_MAX_MS
  let consecutiveFailures = 0

  while (Date.now() < deadline) {
    try {
      const task = await getPoolBatchDeleteTask(providerId, taskId)
      consecutiveFailures = 0
      const deleted = getPoolDeleteTaskDeletedCount(task)
      if (task.status === 'completed' || task.status === 'failed') {
        return { status: task.status, deleted }
      }
    } catch {
      consecutiveFailures += 1
      if (consecutiveFailures >= POOL_BULK_DELETE_POLL_MAX_FAILURES) {
        return { status: 'failed', deleted: 0 }
      }
    }

    await new Promise((resolve) => window.setTimeout(resolve, POOL_BULK_DELETE_POLL_INTERVAL_MS))
  }

  return { status: 'failed', deleted: 0 }
}

async function resolveSelectedPoolKeyIdsForBatch(providerId: string): Promise<string[]> {
  if (allFilteredPoolKeysSelected.value) {
    const resolved = await resolvePoolKeySelection(providerId, buildPoolSelectionFilters())
    return Array.isArray(resolved.items)
      ? resolved.items.map(item => item.key_id).filter(Boolean)
      : []
  }
  return [...selectedPoolKeyIds.value]
}

async function updateSelectedPoolKeyActiveStatus(action: 'enable' | 'disable'): Promise<void> {
  const providerId = selectedProviderId.value
  const selectedCount = selectedPoolKeyCount.value
  if (!providerId || selectedCount === 0 || poolSelectionActionBusy.value) return

  const enable = action === 'enable'
  const confirmed = await confirm({
    title: enable ? t('poolManagement.enableSelected') : t('poolManagement.disableSelected'),
    message: enable
      ? t('poolManagement.enableSelectedConfirm', { count: selectedCount })
      : t('poolManagement.disableSelectedConfirm', { count: selectedCount }),
    confirmText: enable ? t('poolManagement.confirmEnable') : t('poolManagement.confirmDisable'),
    variant: enable ? 'question' : 'warning',
  })
  if (!confirmed) return

  updatingSelectedKeyStatus.value = action
  try {
    const keyIds = await resolveSelectedPoolKeyIdsForBatch(providerId)
    if (keyIds.length === 0) {
      showWarning(t('poolManagement.noActionableAccounts'))
      return
    }
    const result = await batchActionPoolKeys(providerId, {
      key_ids: keyIds,
      action,
    })
    const affected = Number(result.affected ?? 0)
    if (selectedProviderId.value !== providerId) return
    clearPoolKeySelection()
    await loadKeys()
    refreshOverviewInBackground()
    success(affected > 0
      ? t(enable ? 'poolManagement.enabledCount' : 'poolManagement.disabledCount', { count: affected })
      : t('poolManagement.noAccountsUpdated'))
  } catch (err) {
    showError(parseApiError(err, enable ? t('poolManagement.enableBatchFailed') : t('poolManagement.disableBatchFailed')))
  } finally {
    updatingSelectedKeyStatus.value = null
  }
}

async function enableSelectedPoolKeys(): Promise<void> {
  await updateSelectedPoolKeyActiveStatus('enable')
}

async function disableSelectedPoolKeys(): Promise<void> {
  await updateSelectedPoolKeyActiveStatus('disable')
}

async function clearCooldownSelectedPoolKeys(): Promise<void> {
  const providerId = selectedProviderId.value
  const selectedCount = selectedPoolKeyCount.value
  if (!providerId || selectedCount === 0 || poolSelectionActionBusy.value) return

  const confirmed = await confirm({
    title: t('poolManagement.clearCooldown'),
    message: t('poolManagement.clearCooldownConfirm', { count: selectedCount }),
    confirmText: t('poolManagement.confirmClear'),
    variant: 'question',
  })
  if (!confirmed) return

  clearingSelectedCooldown.value = true
  try {
    const keyIds = await resolveSelectedPoolKeyIdsForBatch(providerId)
    if (keyIds.length === 0) {
      showWarning(t('poolManagement.noCooldownAccounts'))
      return
    }
    const result = await batchActionPoolKeys(providerId, {
      key_ids: keyIds,
      action: 'clear_cooldown',
    })
    const affected = Number(result.affected ?? 0)
    if (selectedProviderId.value !== providerId) return
    clearPoolKeySelection()
    await loadKeys()
    refreshOverviewInBackground()
    success(affected > 0 ? t('poolManagement.cooldownClearedCount', { count: affected }) : t('poolManagement.noAccountsUpdated'))
  } catch (err) {
    showError(parseApiError(err, t('poolManagement.clearCooldownFailed')))
  } finally {
    clearingSelectedCooldown.value = false
  }
}

async function deleteSelectedPoolKeys(): Promise<void> {
  const providerId = selectedProviderId.value
  const selectedCount = selectedPoolKeyCount.value
  if (!providerId || selectedCount === 0 || poolSelectionActionBusy.value) return

  const confirmed = await confirm({
    title: t('poolManagement.deleteSelected'),
    message: t('poolManagement.deleteSelectedConfirm', { count: selectedCount }),
    confirmText: t('poolManagement.confirmDelete'),
    variant: 'destructive',
  })
  if (!confirmed) return

  deletingSelectedKeys.value = true
  try {
    const keyIds = await resolveSelectedPoolKeyIdsForBatch(providerId)
    if (keyIds.length === 0) {
      showWarning(t('poolManagement.noDeletableAccounts'))
      return
    }

    const result = await batchActionPoolKeys(providerId, {
      key_ids: keyIds,
      action: 'delete',
    })
    let deletedCount = Number(result.affected ?? 0)
    let taskFailed = false
    if (!Number.isFinite(deletedCount) || deletedCount < 0) deletedCount = 0

    if (result.task_id) {
      const taskResult = await pollPoolBatchDeleteTask(providerId, result.task_id)
      deletedCount = taskResult.deleted
      if (taskResult.status === 'failed') {
        taskFailed = true
        showWarning(t('poolManagement.batchDeleteIncomplete', { count: deletedCount }))
      }
    }

    if (selectedProviderId.value !== providerId) return
    clearPoolKeySelection()
    await loadKeys()
    refreshOverviewInBackground()
    if (!taskFailed) {
      success(deletedCount > 0 ? t('poolManagement.deletedCount', { count: deletedCount }) : t('poolManagement.noAccountsDeleted'))
    }
  } catch (err) {
    showError(parseApiError(err, t('poolManagement.deleteBatchFailed')))
  } finally {
    deletingSelectedKeys.value = false
  }
}

async function copyFullKey(key: PoolKeyDetail) {
  try {
    const result = await revealEndpointKey(key.key_id)
    let textToCopy = ''

    if (result.auth_type === 'service_account' && result.auth_config) {
      textToCopy = typeof result.auth_config === 'string'
        ? result.auth_config
        : JSON.stringify(result.auth_config, null, 2)
    } else if (result.auth_type === 'oauth') {
      textToCopy = result.refresh_token || ''
    } else {
      textToCopy = result.api_key || ''
    }

    if (!textToCopy) {
      showError(t('poolManagement.noCopyContent'))
      return
    }

    await copyToClipboard(textToCopy)
  } catch (err) {
    showError(parseApiError(err, t('poolManagement.getKeyFailed')))
  }
}

async function downloadRefreshToken(key: PoolKeyDetail) {
  try {
    const data = await exportKey(key.key_id)
    const providerType = selectedProviderType.value || 'unknown'
    const email = typeof data.email === 'string' ? data.email : ''
    const safeName = (email || key.key_name || key.key_id.slice(0, 8)).replace(/[^a-zA-Z0-9_\-@.]/g, '_')

    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `aether_${providerType}_${safeName}.json`
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
  } catch (err) {
    showError(parseApiError(err, t('poolManagement.exportFailed')))
  }
}

async function handleRefreshOAuth(key: PoolKeyDetail) {
  if (refreshingOAuthKeyId.value) return

  refreshingOAuthKeyId.value = key.key_id
  try {
    const result = await refreshProviderOAuth(key.key_id)
    const refreshedExpiresAt = typeof result.expires_at === 'number' ? result.expires_at : null
    const target = keyPage.value.keys.find(k => k.key_id === key.key_id)
    if (target) {
      target.oauth_expires_at = refreshedExpiresAt
    }
    await loadKeys()
    if (refreshedExpiresAt != null) {
      const reloadedTarget = keyPage.value.keys.find(k => k.key_id === key.key_id)
      if (
        reloadedTarget
        && (typeof reloadedTarget.oauth_expires_at !== 'number'
          || reloadedTarget.oauth_expires_at < refreshedExpiresAt)
      ) {
        reloadedTarget.oauth_expires_at = refreshedExpiresAt
      }
    }
    const refreshedKey = keyPage.value.keys.find(k => k.key_id === key.key_id) ?? null
    const feedback = getOAuthRefreshFeedback({
      accountStateRecheckAttempted: result.account_state_recheck_attempted,
      accountStateRecheckError: result.account_state_recheck_error,
      snapshot: refreshedKey,
    })
    if (feedback.tone === 'warning') {
      showWarning(feedback.message)
    } else {
      success(feedback.message)
    }
  } catch (err) {
    showError(parseApiError(err, t('poolManagement.tokenRefreshFailed')))
    await loadKeys()
  } finally {
    refreshingOAuthKeyId.value = null
  }
}

// --- Actions ---
async function handleTestAccount(key: PoolKeyDetail) {
  if (!key.is_active || preparingAccountTestKeyId.value || !poolAccountTestDialogRef.value) return
  preparingAccountTestKeyId.value = key.key_id
  try {
    await poolAccountTestDialogRef.value.openAccountTest(toEndpointApiKey(key))
  } finally {
    preparingAccountTestKeyId.value = null
  }
}

function createCodexQuotaResetIdempotencyKey(): string {
  const randomUUID = globalThis.crypto?.randomUUID?.bind(globalThis.crypto)
  if (randomUUID) return randomUUID()

  const bytes = new Uint8Array(16)
  if (globalThis.crypto?.getRandomValues) {
    globalThis.crypto.getRandomValues(bytes)
  } else {
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Math.floor(Math.random() * 256)
    }
  }
  bytes[6] = (bytes[6] & 0x0f) | 0x40
  bytes[8] = (bytes[8] & 0x3f) | 0x80
  const hex = Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}

function applyCodexQuotaResetResult(
  key: PoolKeyDetail,
  result: Awaited<ReturnType<typeof resetCodexQuota>>,
) {
  const quotaSnapshot = result.quota_snapshot
  if (key.status_snapshot && quotaSnapshot) {
    key.status_snapshot.quota = quotaSnapshot
    key.quota_updated_at = quotaSnapshot.updated_at ?? quotaSnapshot.observed_at ?? key.quota_updated_at
    return
  }

  const resetCredits = key.status_snapshot?.quota?.reset_credits
  if (!resetCredits) return
  const currentCount = getCodexResetCreditCount(key)
  if (result.outcome === 'no_credit') {
    resetCredits.available_count = 0
  } else if (result.reset_applied && currentCount !== null) {
    resetCredits.available_count = Math.max(0, currentCount - 1)
  }
}

async function handleResetCodexQuota(key: PoolKeyDetail) {
  if (resettingCodexQuotaKeyId.value || !canResetCodexQuota(key)) return
  const confirmed = await confirm({
    title: t('poolManagement.resetQuota'),
    message: t('poolManagement.resetQuotaConfirm', {
      name: getPoolAccountDisplayName(key),
    }),
    confirmText: t('poolManagement.confirmResetQuota'),
    variant: 'warning',
  })
  if (!confirmed) return

  const idempotencyKey = codexQuotaResetIdempotencyKeys.get(key.key_id)
    || createCodexQuotaResetIdempotencyKey()
  codexQuotaResetIdempotencyKeys.set(key.key_id, idempotencyKey)
  resettingCodexQuotaKeyId.value = key.key_id
  try {
    const result = await resetCodexQuota(key.key_id, idempotencyKey)
    codexQuotaResetIdempotencyKeys.delete(key.key_id)
    applyCodexQuotaResetResult(key, result)

    if (result.reset_applied && result.refresh_succeeded) {
      success(t('poolManagement.quotaResetSuccess'))
      await loadKeys()
      refreshOverviewInBackground()
    } else if (result.reset_applied) {
      showWarning(t('poolManagement.quotaResetRefreshFailed'))
    } else if (result.outcome === 'no_credit') {
      showWarning(t('poolManagement.quotaNoCredit'))
    } else {
      showWarning(t('poolManagement.quotaNothingToReset'))
    }
  } catch (err) {
    showError(parseApiError(err, t('poolManagement.quotaResetFailed')))
  } finally {
    resettingCodexQuotaKeyId.value = null
  }
}

async function clearCooldown(keyId: string) {
  if (!selectedProviderId.value) return
  try {
    const res = await clearPoolCooldown(selectedProviderId.value, keyId)
    success(res.message)
    await loadKeys()
    refreshOverviewInBackground()
  } catch (err) {
    showError(parseApiError(err))
  }
}

async function handleResetCycleStats(key: PoolKeyDetail) {
  if (resettingCycleKeyId.value || !canResetCycleStats(key)) return

  const confirmed = await confirm({
    title: t('poolManagement.resetCycle'),
    message: t('poolManagement.resetCycleConfirm', { name: getPoolAccountDisplayName(key) }),
    confirmText: t('poolManagement.reset'),
  })
  if (!confirmed) return

  resettingCycleKeyId.value = key.key_id
  try {
    const result = await resetProviderKeyCycleStats(key.key_id)
    success(result.message || t('poolManagement.cycleReset'))
    await loadKeys()
  } catch (err) {
    showError(parseApiError(err, t('poolManagement.resetCycleFailed')))
  } finally {
    resettingCycleKeyId.value = null
  }
}

async function toggleKeyActive(key: PoolKeyDetail) {
  if (togglingKeyId.value) return
  togglingKeyId.value = key.key_id
  try {
    const nextStatus = !key.is_active
    await updateProviderKey(key.key_id, { is_active: nextStatus })
    key.is_active = nextStatus
    if (nextStatus) {
      delete key.scheduling_label
      delete key.scheduling_status
      delete key.scheduling_state
      if (key.scheduling_reason === 'disabled') {
        delete key.scheduling_reason
      }
    } else {
      key.scheduling_state = 'disabled'
      key.scheduling_label = t('poolManagement.disabled')
      key.scheduling_status = 'blocked'
      key.scheduling_reason = 'disabled'
    }
    success(nextStatus ? t('poolManagement.accountEnabled') : t('poolManagement.accountDisabled'))
    await loadKeys()
    refreshOverviewInBackground()
  } catch (err) {
    showError(parseApiError(err))
  } finally {
    togglingKeyId.value = null
  }
}

// --- Dialogs ---
const showImportDialog = ref(false)
const showSchedulingDialog = ref(false)
const showAdvancedDialog = ref(false)
const providerEditDialogOpen = ref(false)
const providerToEdit = ref<ProviderWithEndpointsSummary | null>(null)
const endpointEditDialogOpen = ref(false)
const providerEndpointsForEdit = ref<ProviderEndpoint[]>([])
const showAccountBatchDialog = ref(false)
const providerProxyMobilePopoverOpen = ref(false)
const providerProxyDesktopPopoverOpen = ref(false)
const savingProviderProxy = ref(false)
const togglingProviderStatus = ref(false)
let endpointEditRequestId = 0

function openSchedulingDialog() {
  showSchedulingDialog.value = true
}

async function openProviderEditDialog(): Promise<void> {
  const providerId = selectedProviderId.value
  if (!providerId) return

  try {
    const latest = await getProvider(providerId)
    if (selectedProviderId.value !== providerId) return
    selectedProviderData.value = latest
    providerToEdit.value = latest
  } catch (err) {
    if (selectedProviderId.value !== providerId) return
    if (!selectedProviderData.value) {
      showError(parseApiError(err, t('poolManagement.refreshProviderFailed')))
      return
    }
    providerToEdit.value = selectedProviderData.value
  }

  providerEditDialogOpen.value = true
}

async function handleProviderEditSaved(updatedProvider: ProviderWithEndpointsSummary): Promise<void> {
  if (selectedProviderId.value === updatedProvider.id) {
    selectedProviderData.value = updatedProvider
    providerToEdit.value = updatedProvider
  }
  providerEditDialogOpen.value = false
  await loadOverview()
}

async function openEndpointEditDialog(): Promise<void> {
  const providerId = selectedProviderId.value
  if (!providerId) return

  const requestId = ++endpointEditRequestId
  try {
    const [provider, endpoints] = await Promise.all([
      getProvider(providerId),
      getProviderEndpoints(providerId),
    ])
    if (requestId !== endpointEditRequestId || selectedProviderId.value !== providerId) return
    selectedProviderData.value = provider
    providerEndpointsForEdit.value = endpoints
    endpointEditDialogOpen.value = true
  } catch (err) {
    if (requestId !== endpointEditRequestId || selectedProviderId.value !== providerId) return
    showError(parseApiError(err, t('poolManagement.loadEndpointsFailed')))
  }
}

async function handleEndpointEditSaved(): Promise<void> {
  const providerId = selectedProviderId.value
  if (!providerId) return

  const requestId = ++endpointEditRequestId
  try {
    const [provider, endpoints] = await Promise.all([
      getProvider(providerId),
      getProviderEndpoints(providerId),
    ])
    if (requestId !== endpointEditRequestId || selectedProviderId.value !== providerId) return
    selectedProviderData.value = provider
    providerEndpointsForEdit.value = endpoints
    await Promise.all([loadOverview(), loadKeys()])
  } catch (err) {
    if (requestId !== endpointEditRequestId || selectedProviderId.value !== providerId) return
    showError(parseApiError(err, t('poolManagement.refreshEndpointsFailed')))
  }
}

function getProviderProxyNodeName(): string | null {
  const nodeId = selectedProviderData.value?.proxy?.node_id
  if (!nodeId) return null
  const node = proxyNodesStore.nodes.find(n => n.id === nodeId)
  return node ? node.name : `${nodeId.slice(0, 8)}...`
}

function getPoolAccountDisplayName(key: PoolKeyDetail): string {
  return getAccountDisplayName(key, t('poolManagement.unnamedAccount'))
}

async function copyPoolAccountDisplay(key: PoolKeyDetail): Promise<void> {
  const text = getAccountCopyText(key)
  if (!text) return
  await copyToClipboard(text)
}

function getProviderProxyButtonTitle(): string {
  const nodeName = getProviderProxyNodeName()
  if (nodeName) return t('poolManagement.providerProxyCurrent', { name: nodeName })
  return t('poolManagement.providerProxyUnset')
}

function closeProviderProxyPopovers(): void {
  providerProxyMobilePopoverOpen.value = false
  providerProxyDesktopPopoverOpen.value = false
}

function handleProviderProxyPopoverToggle(scope: 'mobile' | 'desktop', open: boolean): void {
  if (scope === 'mobile') {
    providerProxyMobilePopoverOpen.value = open
    if (open) {
      providerProxyDesktopPopoverOpen.value = false
    }
  } else {
    providerProxyDesktopPopoverOpen.value = open
    if (open) {
      providerProxyMobilePopoverOpen.value = false
    }
  }
  if (open) {
    proxyNodesStore.ensureLoaded()
    proxyDesktopPopoverOpenKeyId.value = null
    proxyMobilePopoverOpenKeyId.value = null
  }
}

async function setProviderProxy(nodeId: string): Promise<void> {
  const providerId = selectedProviderId.value
  if (!providerId) return
  savingProviderProxy.value = true
  try {
    const updated = await updateProvider(providerId, {
      proxy: { node_id: nodeId, enabled: true },
    })
    if (selectedProviderId.value === providerId) {
      selectedProviderData.value = updated
    }
    closeProviderProxyPopovers()
    success(t('poolManagement.providerProxySet'))
  } catch (err) {
    showError(parseApiError(err, t('poolManagement.providerProxySetFailed')))
  } finally {
    savingProviderProxy.value = false
  }
}

async function clearProviderProxy(): Promise<void> {
  const providerId = selectedProviderId.value
  if (!providerId) return
  savingProviderProxy.value = true
  try {
    const updated = await updateProvider(providerId, { proxy: null })
    if (selectedProviderId.value === providerId) {
      selectedProviderData.value = updated
    }
    closeProviderProxyPopovers()
    success(t('poolManagement.providerProxyCleared'))
  } catch (err) {
    showError(parseApiError(err, t('poolManagement.providerProxyClearFailed')))
  } finally {
    savingProviderProxy.value = false
  }
}

function getProviderToggleButtonTitle(): string {
  const active = selectedProviderData.value?.is_active !== false
  return active ? t('poolManagement.providerEnabledTitle') : t('poolManagement.providerDisabledTitle')
}

function getProviderToggleButtonClass(): string {
  return ''
}

async function toggleSelectedProviderStatus(): Promise<void> {
  if (togglingProviderStatus.value) return
  const providerId = selectedProviderId.value
  const current = selectedProviderData.value
  if (!providerId || !current) return

  const nextStatus = !current.is_active
  if (!nextStatus) {
    const confirmed = await confirm({
      title: t('poolManagement.disableProvider'),
      message: t('poolManagement.disableProviderConfirm', { name: current.name }),
      confirmText: t('poolManagement.confirmDisable'),
      variant: 'destructive',
    })
    if (!confirmed) return
  }

  togglingProviderStatus.value = true
  try {
    const updated = await updateProvider(providerId, { is_active: nextStatus })
    if (selectedProviderId.value === providerId) {
      selectedProviderData.value = updated
    }
    success(nextStatus ? t('poolManagement.providerEnabled') : t('poolManagement.providerDisabled'))
    await loadOverview()
  } catch (err) {
    showError(parseApiError(err, nextStatus ? t('poolManagement.enableProviderFailed') : t('poolManagement.disableProviderFailed')))
  } finally {
    togglingProviderStatus.value = false
  }
}

async function handleAccountBatchChanged(): Promise<void> {
  await Promise.all([loadKeys(), loadOverview()])
}

async function handleAccountDialogSaved() {
  showImportDialog.value = false
  await Promise.all([loadKeys(), loadOverview()])
  // 导入账号后补一次静默额度刷新，避免新账号在列表里暂无额度信息
  await refreshCurrentPageQuotaInBackground({ silent: true })
}

// --- Formatting ---
const COOLDOWN_REASON_MAP: Record<string, string> = {
  rate_limited_429: t('poolManagement.reason429'),
  forbidden_403: t('poolManagement.reason403'),
  overloaded_529: t('poolManagement.reason529'),
  auth_failed_401: t('poolManagement.reason401'),
  payment_required_402: t('poolManagement.reason402'),
  server_error_500: t('poolManagement.reason500'),
  request_timeout_408: t('poolManagement.reason408'),
  conflict_409: t('poolManagement.reason409'),
  locked_423: t('poolManagement.reason423'),
  too_early_425: '425 Too Early',
  bad_gateway_502: t('poolManagement.reason502'),
  service_unavailable_503: t('poolManagement.reason503'),
  gateway_timeout_504: t('poolManagement.reason504'),
}

function formatCooldownReason(reason: string): string {
  return COOLDOWN_REASON_MAP[reason] || reason
}

type PoolStatusVariant = 'default' | 'secondary' | 'destructive' | 'outline' | 'success' | 'warning' | 'dark'

function getVisibleSchedulingReason(key: PoolKeyDetail): string | null {
  const reason = String(key.scheduling_reason || '').trim()
  if (!reason) return null
  return reason
}

function getVisibleSchedulingReasons(key: PoolKeyDetail) {
  return key.scheduling_reasons ?? []
}

function getSchedulingState(key: PoolKeyDetail): PoolSchedulingState {
  if (
    key.scheduling_state === 'available'
    || key.scheduling_state === 'temporary_unavailable'
    || key.scheduling_state === 'quota_exhausted'
    || key.scheduling_state === 'blocked'
    || key.scheduling_state === 'invalid'
    || key.scheduling_state === 'disabled'
  ) {
    return key.scheduling_state
  }
  if (key.scheduling_status === 'degraded') return 'temporary_unavailable'
  if (key.scheduling_status === 'blocked') return 'blocked'
  if (!key.is_active) return 'disabled'
  return 'available'
}

function getSchedulingStatus(key: PoolKeyDetail): 'available' | 'degraded' | 'blocked' {
  const state = getSchedulingState(key)
  if (state === 'available') return 'available'
  if (state === 'temporary_unavailable') return 'degraded'
  return 'blocked'
}

function compactPoolStatusLabel(label: string | null | undefined): string | null {
  const normalized = String(label || '').trim()
  if (!normalized) return null

  const mapped: Record<string, string> = {
    'Token 失效': t('poolManagement.invalid'),
    'Token 过期': t('poolManagement.expired'),
    Token失效: t('poolManagement.invalid'),
    Token过期: t('poolManagement.expired'),
    账号已封禁: t('poolManagement.banned'),
    工作区已停用: t('poolManagement.workspaceDisabled'),
    账号访问受限: t('poolManagement.accessRestricted'),
    健康度较低: t('poolManagement.healthLow'),
  }
  const labelText = mapped[normalized] || normalized
  return Array.from(labelText).slice(0, 5).join('')
}

function getOAuthStatusBadgeLabel(status: ReturnType<typeof getVisibleOAuthState>): string | null {
  if (!status) return null
  if (status.requiresReauth) return t('poolManagement.renewalFailed')
  if (status.isInvalid) return t('poolManagement.invalid')
  if (status.isExpired) return t('poolManagement.expired')
  if (status.text === '未添加') return t('poolManagement.notAdded')
  if (status.text === '有效期未知') return t('poolManagement.unknown')
  if (status.isExpiringSoon) return t('poolManagement.expiringSoon')
  return t('poolManagement.valid')
}

function getSchedulingBadgeLabel(key: PoolKeyDetail): string {
  const rawLabel = String(key.scheduling_label || '').trim()
  if (rawLabel) {
    if (rawLabel === '禁用' || rawLabel === '停用') return t('poolManagement.disabled')
    return compactPoolStatusLabel(rawLabel) || rawLabel
  }

  const accountAlert = getAccountAlertLabel(key)
  if (accountAlert) return compactPoolStatusLabel(accountAlert) || accountAlert

  const state = getSchedulingState(key)
  if (state === 'disabled') return t('poolManagement.disabled')
  if (state === 'invalid') return t('poolManagement.invalid')
  if (state === 'blocked') return t('poolManagement.abnormal')
  if (state === 'quota_exhausted') return t('poolManagement.quotaExhausted')
  if (state === 'temporary_unavailable') return t('poolManagement.temporaryUnavailable')
  return t('poolManagement.available')
}

function getSchedulingBadgeVariant(key: PoolKeyDetail): PoolStatusVariant {
  const state = getSchedulingState(key)
  if (state === 'available') return 'default'
  if (state === 'temporary_unavailable') return 'warning'
  if (state === 'disabled') return 'secondary'
  return 'destructive'
}

function getSchedulingTitle(key: PoolKeyDetail): string {
  if (!key.is_active || getSchedulingState(key) === 'disabled') {
    return t('poolManagement.disabledSchedulingHint')
  }

  const accountAlertTitle = getAccountAlertTitle(key)
  if (accountAlertTitle) return accountAlertTitle

  const reasons = getVisibleSchedulingReasons(key)
  if (reasons.length > 0) {
    return reasons.map((item) => {
      const ttl = item.ttl_seconds && item.ttl_seconds > 0 ? ` (${formatTTL(item.ttl_seconds)})` : ''
      const detail = item.detail ? ` - ${item.detail}` : ''
      return `${item.label}${ttl}${detail}`
    }).join('\n')
  }

  if (key.scheduling_ttl_seconds) {
    return `${getSchedulingBadgeLabel(key)} (${formatTTL(key.scheduling_ttl_seconds)})`
  }
  return getSchedulingBadgeLabel(key)
}

function formatTTL(seconds: number): string {
  if (seconds <= 0) return ''
  const m = Math.floor(seconds / 60)
  const s = seconds % 60
  return m > 0 ? `${m}m ${s}s` : `${s}s`
}

function getRowClass(key: PoolKeyDetail): string {
  const status = getSchedulingStatus(key)
  if (!key.is_active || status === 'blocked') return 'bg-muted/50 opacity-60'
  return ''
}

function getAuthTypeChipLabel(key: PoolKeyDetail): string {
  return getProviderAuthLabel(key)
}

function getMobileOAuthTone(key: PoolKeyDetail): PoolMobileTagTone | null {
  const oauthState = getVisibleOAuthState(key)
  if (!oauthState) return null
  if (oauthState.isInvalid || oauthState.isExpired) return 'danger'
  if (oauthState.isExpiringSoon) return 'warning'
  return 'muted'
}

function getMobileTagItems(key: PoolKeyDetail): PoolMobileTagItem[] {
  const accountAlert = getAccountAlertLabel(key)
  const oauthState = getVisibleOAuthState(key)
  const orgBadge = getOAuthOrgBadge(key)
  const planType = resolvePoolKeyPlanType(key)

  return buildPoolMobileTagItems({
    accountStatusLabel: compactPoolStatusLabel(accountAlert),
    accountStatusTone: accountAlert ? 'danger' : null,
    oauthStatusLabel: getOAuthStatusBadgeLabel(oauthState),
    oauthStatusTone: getMobileOAuthTone(key),
    priorityLabel: `P${key.internal_priority ?? 50}`,
    authLabel: getAuthTypeChipLabel(key),
    planLabel: planType ? formatOAuthPlanType(planType) : null,
    orgLabel: orgBadge?.label ?? null,
      proxyLabel: key.proxy?.node_id ? t('poolManagement.independentProxy') : null,
  })
}

function getMobileTagClass(item: PoolMobileTagItem): string {
  if (item.tone === 'danger') {
    return 'border-red-500/30 bg-red-500/10 text-red-700 dark:text-red-300'
  }
  if (item.tone === 'warning') {
    return 'border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300'
  }
  if (item.tone === 'accent') {
    return 'border-blue-500/30 bg-blue-500/10 text-blue-700 dark:text-blue-300'
  }
  if (item.tone === 'muted') {
    return 'border-border/60 bg-background/70 text-muted-foreground'
  }
  return 'border-border/60 bg-background/80 text-foreground/80'
}

function formatOAuthPlanType(planType: string): string {
  const labelMap: Record<string, string> = {
    plus: 'Plus',
    pro: 'Pro',
    free: 'Free',
    paid: 'Paid',
    team: 'Team',
    enterprise: 'Enterprise',
    ultra: 'Ultra',
    'pro+': 'Pro+',
    power: 'Power',
    basic: 'Basic',
    super: 'Super',
    heavy: 'Heavy',
  }
  return labelMap[planType.toLowerCase()] || planType
}

function getOAuthPlanTypeClass(planType: string): string {
  const classes: Record<string, string> = {
    plus: 'border-green-500/50 text-green-600 dark:text-green-400',
    pro: 'border-blue-500/50 text-blue-600 dark:text-blue-400',
    free: 'border-primary/50 text-primary',
    paid: 'border-blue-500/50 text-blue-600 dark:text-blue-400',
    team: 'border-purple-500/50 text-purple-600 dark:text-purple-400',
    enterprise: 'border-amber-500/50 text-amber-600 dark:text-amber-400',
    ultra: 'border-amber-500/50 text-amber-600 dark:text-amber-400',
    'pro+': 'border-purple-500/50 text-purple-600 dark:text-purple-400',
    power: 'border-amber-500/50 text-amber-600 dark:text-amber-400',
    basic: 'border-primary/50 text-primary',
    super: 'border-green-500/50 text-green-600 dark:text-green-400',
    heavy: 'border-amber-500/50 text-amber-600 dark:text-amber-400',
  }
  return classes[planType.toLowerCase()] || ''
}

function getVisibleOAuthState(key: PoolKeyDetail) {
  return getOAuthStatusDisplayWithFallback(key, countdownTick.value)
}

function getOAuthRefreshButtonTitle(key: PoolKeyDetail): string {
  return resolveOAuthRefreshButtonTitle(key, countdownTick.value)
}

function getOAuthStatusTitle(key: PoolKeyDetail): string {
  return resolveOAuthStatusTitle(key, countdownTick.value)
}

const _accountAlertCache = new WeakMap<PoolKeyDetail, string | null>()

function getQuotaAlertSnapshotState(key: PoolKeyDetail): { label: string, title: string } | null {
  const quota = getQuotaSnapshot(key)
  if (!quota) return null

  const code = String(quota.code || '').trim().toLowerCase()
  if (code !== 'banned' && code !== 'forbidden') return null

  let label = String(quota.label || '').trim()
  if (!label) {
    label = code === 'banned' ? t('poolManagement.banned') : t('poolManagement.accessRestricted')
  } else if (label === '账号已封禁' || label === '封禁') {
    label = t('poolManagement.banned')
  }

  const reason = String(quota.reason || '').trim()
  return {
    label,
    title: reason ? `${label}: ${reason}` : label,
  }
}

function getAccountAlertLabel(key: PoolKeyDetail): string | null {
  const cached = _accountAlertCache.get(key)
  if (cached !== undefined) return cached

  let result: string | null = getAccountStatusDisplay(key).label
  const quotaAlert = getQuotaAlertSnapshotState(key)
  if (!result && quotaAlert) result = quotaAlert.label
  if (!result && !getQuotaSnapshot(key)) {
    const quotaText = getLegacyAccountQuotaText(key)
    if (quotaText === '账号已封禁' || quotaText === '封禁') result = t('poolManagement.banned')
    else if (quotaText === '访问受限') result = t('poolManagement.accessRestricted')
  }

  _accountAlertCache.set(key, result)
  return result
}

function getAccountAlertTitle(key: PoolKeyDetail): string {
  const label = getAccountAlertLabel(key)
  if (!label) return ''

  const accountTitle = getAccountStatusTitle(key)
  if (accountTitle) return accountTitle

  const quotaAlert = getQuotaAlertSnapshotState(key)
  if (quotaAlert?.title) return quotaAlert.title

  const quotaText = getLegacyAccountQuotaText(key)
  if (quotaText) return `${label}: ${quotaText}`
  return label
}

function normalizeQuotaLabel(label: string): string {
  const normalized = label.trim()
  if (!normalized) return t('poolManagement.quota')
  if (/spark\s*5h/i.test(normalized) || normalized.includes('Spark5H')) return 'Spark5H'
  if (/spark/i.test(normalized) && normalized.includes('周')) return t('poolManagement.sparkWeek')
  if (normalized.includes('5H')) return '5H'
  if (normalized.includes('周')) return t('poolManagement.week')
  if (normalized.includes('最低剩余')) return t('poolManagement.minimum')
  if (normalized === '剩余' || normalized.includes('剩余')) return t('poolManagement.remainingLabel')
  return normalized
}

function getQuotaProgressLabel(label: string): string {
  if (label === '5H') return '5H'
  if (label === '周') return t('poolManagement.week')
  if (label === 'Spark5H') return 'Spark5H'
  if (label === 'Spark周') return t('poolManagement.sparkWeek')
  if (label === '最低') return t('poolManagement.minimum')
  if (label === '剩余') return t('poolManagement.remainingLabel')
  return label
}

function getQuotaProgressCountdown(item: QuotaProgressItem) {
  if (item.resetAtSeconds == null && item.resetSeconds == null) return null
  return getCodexResetCountdown(
    item.resetAtSeconds,
    item.resetSeconds,
    item.updatedAtSeconds,
    countdownTick.value,
    item.remainingPercent
  )
}

function getQuotaProgressCountdownText(item: QuotaProgressItem): string {
  const status = getQuotaProgressCountdown(item)
  if (!status) return ''
  return status.isExpired ? '' : t('poolManagement.resetAfter', { value: status.text })
}

function formatCompactQuotaCountdownText(text: string): string {
  const normalized = text.trim()
  const dayMatch = normalized.match(/^(\d+)天\s+(.+?)(?:\s+后重置)?$/)
  if (dayMatch) {
    return `${dayMatch[1]}天 ${dayMatch[2]}`
  }
  return normalized.replace(/\s+后重置$/, '')
}

function shouldHideQuotaProgressDetailText(text: string | null | undefined): boolean {
  return (text ?? '').trim().includes('已重置')
}

function getQuotaProgressResetDisplayText(item: QuotaProgressItem): string {
  const countdownText = getQuotaProgressCountdownText(item)
  if (countdownText) return formatCompactQuotaCountdownText(countdownText)
  return ''
}

function getQuotaProgressMeterDisplayText(item: QuotaProgressItem): string {
  const detail = item.detail?.trim() || ''
  if (!shouldHideQuotaProgressDetailText(detail) && detail) return detail
  return `${item.remainingPercent.toFixed(1)}%`
}

function getQuotaFallbackText(key: PoolKeyDetail): string | null {
  return getQuotaDisplayText(key, selectedProviderType.value)
}



function getQuotaLabelOrder(label: string): number {
  if (label === 'Auto') return 0
  if (label === 'Fast') return 1
  if (label === 'Expert') return 2
  if (label === 'Heavy') return 3
  if (label === 'Grok 4.3') return 4
  if (label === '5H') return 0
  if (label === '周') return 1
  if (label === 'Spark5H') return 2
  if (label === 'Spark周') return 3
  if (label === '剩余') return 4
  if (label === '最低') return 5
  if (label === '生图') return 6
  return 10
}

function clampPercent(value: number): number {
  if (!Number.isFinite(value)) return 0
  if (value < 0) return 0
  if (value > 100) return 100
  return value
}

function normalizeUnixSeconds(raw: number | null | undefined): number | null {
  const value = Number(raw ?? 0)
  if (!Number.isFinite(value) || value <= 0) return null
  if (value > 1_000_000_000_000) return Math.floor(value / 1000)
  return Math.floor(value)
}

function normalizeRemainingSeconds(raw: number | null | undefined): number | null {
  const value = Number(raw ?? NaN)
  if (!Number.isFinite(value) || value < 0) return null
  return Math.floor(value)
}

function getQuotaSnapshot(key: PoolKeyDetail): QuotaStatusSnapshot | null {
  const quota = key.status_snapshot?.quota
  if (!quota) return null
  return quota
}

function getQuotaSnapshotProviderType(key: PoolKeyDetail): string {
  const snapshotProviderType = String(getQuotaSnapshot(key)?.provider_type || '').trim().toLowerCase()
  if (snapshotProviderType) return snapshotProviderType
  return selectedProviderType.value
}

function getCodexQuotaSnapshot(key: PoolKeyDetail): QuotaStatusSnapshot | null {
  const quota = getQuotaSnapshot(key)
  if (!quota) return null
  return getQuotaSnapshotProviderType(key) === 'codex' ? quota : null
}

function getQuotaSnapshotUpdatedAtSeconds(quota: QuotaStatusSnapshot | null | undefined): number | null {
  return normalizeUnixSeconds(quota?.updated_at ?? quota?.observed_at ?? null)
}

function getQuotaSnapshotResetAtSeconds(quota: QuotaStatusSnapshot | null | undefined): number | null {
  return normalizeUnixSeconds(quota?.reset_at ?? null)
}

function getQuotaSnapshotResetSeconds(quota: QuotaStatusSnapshot | null | undefined): number | null {
  return normalizeRemainingSeconds(quota?.reset_seconds ?? null)
}

function getQuotaSnapshotWindow(
  quota: QuotaStatusSnapshot | null | undefined,
  code: string,
): QuotaWindowSnapshot | null {
  const windows = quota?.windows
  if (!Array.isArray(windows)) return null

  const normalizedCode = code.trim().toLowerCase()
  return windows.find(window => String(window?.code || '').trim().toLowerCase() === normalizedCode) ?? null
}

function getQuotaSnapshotWindowsByScope(
  quota: QuotaStatusSnapshot | null | undefined,
  scope: string,
): QuotaWindowSnapshot[] {
  const windows = quota?.windows
  if (!Array.isArray(windows)) return []

  const normalizedScope = scope.trim().toLowerCase()
  return windows.filter(window => String(window?.scope || '').trim().toLowerCase() === normalizedScope)
}

function getQuotaWindowUsedPercent(window: QuotaWindowSnapshot | null | undefined): number | null {
  if (!window) return null
  if (typeof window.used_ratio === 'number') {
    return clampPercent(window.used_ratio * 100)
  }
  if (typeof window.remaining_ratio === 'number') {
    return clampPercent((1 - window.remaining_ratio) * 100)
  }
  if (typeof window.limit_value === 'number' && window.limit_value > 0) {
    if (typeof window.remaining_value === 'number') {
      return clampPercent((1 - (window.remaining_value / window.limit_value)) * 100)
    }
    if (typeof window.used_value === 'number') {
      return clampPercent((window.used_value / window.limit_value) * 100)
    }
  }
  return null
}

function getQuotaWindowRemainingPercent(window: QuotaWindowSnapshot | null | undefined): number | null {
  if (!window) return null
  if (typeof window.remaining_ratio === 'number') {
    return clampPercent(window.remaining_ratio * 100)
  }
  const usedPercent = getQuotaWindowUsedPercent(window)
  return usedPercent == null ? null : clampPercent(100 - usedPercent)
}

function formatQuotaValue(value: number | null | undefined): string {
  const normalized = Number(value)
  if (!Number.isFinite(normalized)) return '0'
  const rounded = Math.round(normalized)
  if (Math.abs(normalized - rounded) < 1e-6) {
    return String(rounded)
  }
  return normalized.toFixed(1)
}

function getQuotaWindowValueText(window: QuotaWindowSnapshot | null | undefined): string | undefined {
  if (!window || typeof window.limit_value !== 'number' || window.limit_value <= 0) return undefined
  if (typeof window.remaining_value === 'number') {
    return `${formatQuotaValue(window.remaining_value)}/${formatQuotaValue(window.limit_value)}`
  }
  if (typeof window.used_value === 'number') {
    return `${formatQuotaValue(Math.max(window.limit_value - window.used_value, 0))}/${formatQuotaValue(window.limit_value)}`
  }
  return undefined
}

function resolvePoolKeyPlanType(key: PoolKeyDetail): string | null {
  const direct = key.oauth_plan_type?.trim()
  if (direct) return direct
  const quota = getQuotaSnapshot(key)
  const quotaPlan = quota?.plan_type?.trim()
  if (quotaPlan) return quotaPlan
  const quotaPoolTier = quota?.pool_tier?.trim()
  return quotaPoolTier || null
}

const GROK_QUOTA_MODE_LABELS: Record<string, string> = {
  quota_auto: 'Auto',
  auto: 'Auto',
  quota_fast: 'Fast',
  fast: 'Fast',
  quota_expert: 'Expert',
  expert: 'Expert',
  quota_heavy: 'Heavy',
  heavy: 'Heavy',
  quota_grok_4_3: 'Grok 4.3',
  'grok-420-computer-use-sa': 'Grok 4.3',
}

function getGrokQuotaWindowLabel(window: QuotaWindowSnapshot): string {
  const code = String(window.code || '').trim().replace(/^model:/i, '')
  const label = String(window.label || window.model || code).trim()
  const normalized = (label || code).toLowerCase()
  return GROK_QUOTA_MODE_LABELS[normalized] || GROK_QUOTA_MODE_LABELS[code.toLowerCase()] || label || code || '模式'
}

function getCodexQuotaWindowLabel(window: QuotaWindowSnapshot): string {
  const code = String(window.code || '').trim().toLowerCase()
  if (code === '5h') return '5H'
  if (code === 'weekly' || code === '7d') return '周'
  if (code === '1m' || code === 'monthly') return '月'
  if (code === 'spark_5h' || code === 'spark:5h') return 'Spark5H'
  if (code === 'spark_weekly' || code === 'spark_7d' || code === 'spark:weekly' || code === 'spark:7d') return 'Spark周'

  const label = String(window.label || '').trim()
  if (label) return normalizeQuotaLabel(label)

  const windowMinutes = Number(window.window_minutes ?? 0)
  const windowSeconds = Number(window.window_seconds ?? 0)
  const totalMinutes = windowMinutes > 0
    ? windowMinutes
    : windowSeconds > 0
      ? Math.ceil(windowSeconds / 60)
      : 0
  if (totalMinutes === 300) return '5H'
  if (totalMinutes === 10_080) return '周'
  if (totalMinutes === 43_200) return '月'
  return String(window.code || '额度').trim() || '额度'
}

function buildQuotaProgressItemsFromSnapshot(key: PoolKeyDetail): QuotaProgressItem[] {
  const quota = getQuotaSnapshot(key)
  if (!quota) return []

  const providerType = getQuotaSnapshotProviderType(key)

  if (providerType === 'codex') {
    const quotaResetAtSeconds = getQuotaSnapshotResetAtSeconds(quota)
    const quotaResetSeconds = getQuotaSnapshotResetSeconds(quota)
    const windows = Array.isArray(quota.windows) ? quota.windows : []
    return windows
      .map((window): QuotaProgressItem | null => {
        const remainingPercent = getQuotaWindowRemainingPercent(window)
        if (remainingPercent == null) return null
        return {
          label: getCodexQuotaWindowLabel(window),
          remainingPercent,
          detail: getQuotaWindowValueText(window),
          resetAtSeconds: normalizeUnixSeconds(window.reset_at ?? quotaResetAtSeconds ?? null),
          resetSeconds: normalizeRemainingSeconds(window.reset_seconds ?? quotaResetSeconds ?? null),
          updatedAtSeconds: getQuotaSnapshotUpdatedAtSeconds(quota),
        }
      })
      .filter((item): item is QuotaProgressItem => item != null)
  }

  if (providerType === 'kiro') {
    const quotaResetAtSeconds = getQuotaSnapshotResetAtSeconds(quota)
    const quotaResetSeconds = getQuotaSnapshotResetSeconds(quota)
    const window = getQuotaSnapshotWindow(quota, 'usage')
      ?? getQuotaSnapshotWindowsByScope(quota, 'account')[0]
      ?? null
    const remainingPercent = getQuotaWindowRemainingPercent(window)
    if (remainingPercent == null) return []

    const detail = typeof window?.used_value === 'number' && typeof window?.limit_value === 'number'
      ? `${formatQuotaValue(window.used_value)}/${formatQuotaValue(window.limit_value)}`
      : undefined

    return [{
      label: '剩余',
      remainingPercent,
      detail,
      resetAtSeconds: normalizeUnixSeconds(window?.reset_at ?? quotaResetAtSeconds ?? null),
      resetSeconds: normalizeRemainingSeconds(window?.reset_seconds ?? quotaResetSeconds ?? null),
      updatedAtSeconds: getQuotaSnapshotUpdatedAtSeconds(quota),
    }]
  }

  if (providerType === 'grok_oauth') {
    const quotaResetAtSeconds = getQuotaSnapshotResetAtSeconds(quota)
    const quotaResetSeconds = getQuotaSnapshotResetSeconds(quota)
    return ([
      ['周', 'weekly'],
      ['月', 'monthly'],
    ] as const)
      .map(([label, code]): QuotaProgressItem | null => {
        const window = getQuotaSnapshotWindow(quota, code)
        const remainingPercent = getQuotaWindowRemainingPercent(window)
        if (remainingPercent == null) return null
        const rawValueText = code === 'monthly' ? getQuotaWindowValueText(window) : undefined
        const detail = rawValueText
          ? rawValueText.split('/').map(value => `$${value}`).join('/')
          : undefined
        return {
          label,
          remainingPercent,
          detail,
          resetAtSeconds: normalizeUnixSeconds(window?.reset_at ?? quotaResetAtSeconds ?? null),
          resetSeconds: normalizeRemainingSeconds(window?.reset_seconds ?? quotaResetSeconds ?? null),
          updatedAtSeconds: getQuotaSnapshotUpdatedAtSeconds(quota),
        }
      })
      .filter((item): item is QuotaProgressItem => item != null)
  }

  if (providerType === 'grok') {
    const quotaResetAtSeconds = getQuotaSnapshotResetAtSeconds(quota)
    const quotaResetSeconds = getQuotaSnapshotResetSeconds(quota)
    const modelWindows = getQuotaSnapshotWindowsByScope(quota, 'model')
    if (modelWindows.length > 0) {
      return modelWindows
        .map((window): QuotaProgressItem | null => {
          const remainingPercent = getQuotaWindowRemainingPercent(window)
          if (remainingPercent == null) return null
          return {
            label: getGrokQuotaWindowLabel(window),
            remainingPercent,
            detail: getQuotaWindowValueText(window),
            resetAtSeconds: normalizeUnixSeconds(window?.reset_at ?? quotaResetAtSeconds ?? null),
            resetSeconds: normalizeRemainingSeconds(window?.reset_seconds ?? quotaResetSeconds ?? null),
            updatedAtSeconds: getQuotaSnapshotUpdatedAtSeconds(quota),
          }
        })
        .filter((item): item is QuotaProgressItem => item != null)
    }

    const window = getQuotaSnapshotWindow(quota, 'usage')
      ?? getQuotaSnapshotWindowsByScope(quota, 'account')[0]
      ?? null
    const remainingPercent = getQuotaWindowRemainingPercent(window)
    if (remainingPercent == null) return []

    return [{
      label: '剩余',
      remainingPercent,
      detail: getQuotaWindowValueText(window),
      resetAtSeconds: normalizeUnixSeconds(window?.reset_at ?? quotaResetAtSeconds ?? null),
      resetSeconds: normalizeRemainingSeconds(window?.reset_seconds ?? quotaResetSeconds ?? null),
      updatedAtSeconds: getQuotaSnapshotUpdatedAtSeconds(quota),
    }]
  }

  if (providerType === 'antigravity') {
    const windows = getQuotaSnapshotWindowsByScope(quota, 'model')
    if (windows.length === 0) return []

    const remainingPercents = windows
      .map(getQuotaWindowRemainingPercent)
      .filter((value): value is number => value != null)
    if (remainingPercents.length === 0) return []

    return [{
      label: '最低',
      remainingPercent: Math.min(...remainingPercents),
      detail: t('poolQuotaUi.modelCount', { count: windows.length }),
      resetAtSeconds: null,
      resetSeconds: null,
      updatedAtSeconds: getQuotaSnapshotUpdatedAtSeconds(quota),
    }]
  }

  if (providerType === 'gemini_cli') {
    const windows = getQuotaSnapshotWindowsByScope(quota, 'model')
    if (windows.length === 0) return []

    const remainingPercents = windows
      .map(getQuotaWindowRemainingPercent)
      .filter((value): value is number => value != null)
    if (remainingPercents.length === 0) return []

    return [{
      label: '最低',
      remainingPercent: Math.min(...remainingPercents),
      detail: t('poolQuotaUi.modelCount', { count: windows.length }),
      resetAtSeconds: null,
      resetSeconds: null,
      updatedAtSeconds: getQuotaSnapshotUpdatedAtSeconds(quota),
    }]
  }

  if (providerType === 'chatgpt_web') {
    const window = getQuotaSnapshotWindow(quota, 'image_gen')
      ?? getQuotaSnapshotWindowsByScope(quota, 'account')[0]
      ?? null
    const remainingPercent = getQuotaWindowRemainingPercent(window)
    if (remainingPercent == null) return []

    const remainingValue = typeof window?.remaining_value === 'number' ? window.remaining_value : null
    const limitValue = typeof window?.limit_value === 'number' ? window.limit_value : null
    const usedValue = typeof window?.used_value === 'number' ? window.used_value : null
    const detail = usedValue != null && limitValue != null
      ? `${formatQuotaValue(usedValue)}/${formatQuotaValue(limitValue)}`
      : remainingValue != null && limitValue != null
        ? `${formatQuotaValue(Math.max(limitValue - remainingValue, 0))}/${formatQuotaValue(limitValue)}`
        : remainingValue != null
        ? t('poolQuotaUi.remainingValue', { value: formatQuotaValue(remainingValue) })
          : undefined

    return [{
      label: t('poolQuotaUi.imageGeneration'),
      remainingPercent,
      detail,
      resetAtSeconds: normalizeUnixSeconds(window?.reset_at ?? null),
      resetSeconds: normalizeRemainingSeconds(window?.reset_seconds ?? null),
      updatedAtSeconds: getQuotaSnapshotUpdatedAtSeconds(quota),
    }]
  }

  return []
}

function resolveCodexQuotaCountdown(
  key: PoolKeyDetail,
  label: string
): Pick<QuotaProgressItem, 'resetAtSeconds' | 'resetSeconds' | 'updatedAtSeconds'> | null {
  const codexWindowCodeByLabel: Record<string, string> = {
    '5H': '5h',
    '周': 'weekly',
    '7D': 'weekly',
    '月': '1m',
    '1M': '1m',
    Spark5H: 'spark_5h',
    Spark周: 'spark_weekly',
  }
  const windowCode = codexWindowCodeByLabel[label]
  if (!windowCode) return null

  const codexSnapshot = getCodexQuotaSnapshot(key)
  const snapshotWindow = getQuotaSnapshotWindow(codexSnapshot, windowCode)
  if (!snapshotWindow) return null

  const resetAtSeconds = normalizeUnixSeconds(snapshotWindow.reset_at ?? null)
  const resetSeconds = normalizeRemainingSeconds(snapshotWindow.reset_seconds ?? null)
  const updatedAtSeconds = getQuotaSnapshotUpdatedAtSeconds(codexSnapshot)

  if (resetAtSeconds == null && resetSeconds == null) return null
  return { resetAtSeconds, resetSeconds, updatedAtSeconds }
}

function parseQuotaResetRemainingSeconds(detail: string | undefined): number | null {
  if (!detail) return null
  const text = detail.replace(/\s+/g, '')

  if (text.includes('已重置')) return 0
  if (text.includes('即将重置')) return 1
  if (!text.includes('后重置')) return null

  const dayMatch = text.match(/(\d+)天/)
  const hourMatch = text.match(/(\d+)小时/)
  const minuteMatch = text.match(/(\d+)分钟/)
  const secondMatch = text.match(/(\d+)秒/)

  const days = dayMatch ? Number(dayMatch[1]) : 0
  const hours = hourMatch ? Number(hourMatch[1]) : 0
  const minutes = minuteMatch ? Number(minuteMatch[1]) : 0
  const seconds = secondMatch ? Number(secondMatch[1]) : 0
  const total = days * 86400 + hours * 3600 + minutes * 60 + seconds

  if (total <= 0) return 1
  return total
}

function parseQuotaProgressItems(key: PoolKeyDetail): QuotaProgressItem[] {
  const snapshotItems = buildQuotaProgressItemsFromSnapshot(key)
  if (snapshotItems.length > 0) {
    if (getQuotaSnapshotProviderType(key) === 'codex') {
      return snapshotItems
    }
    return snapshotItems.sort((a, b) => {
      const orderDiff = getQuotaLabelOrder(a.label) - getQuotaLabelOrder(b.label)
      if (orderDiff !== 0) return orderDiff
      return a.label.localeCompare(b.label, 'zh-Hans-CN')
    })
  }

  if (getQuotaSnapshot(key)) return []

  const quotaText = getLegacyAccountQuotaText(key)
  if (!quotaText) return []

  const segments = quotaText
    .split('|')
    .map(s => s.trim())
    .filter(Boolean)

  const items: QuotaProgressItem[] = []
  for (const segment of segments) {
    const match = segment.match(/^(.*?)(-?\d+(?:\.\d+)?)%\s*(.*)$/)
    if (!match) continue

    const [, rawLabel, rawPercent, rawTail] = match
    const remainingPercent = clampPercent(Number(rawPercent))
    const label = normalizeQuotaLabel(rawLabel)
    const detail = rawTail.trim().replace(/^[()]+|[()]+$/g, '').trim()
    const codexCountdown = resolveCodexQuotaCountdown(key, label)
    let resetAtSeconds = codexCountdown?.resetAtSeconds ?? null
    let resetSeconds = codexCountdown?.resetSeconds ?? null
    let updatedAtSeconds = codexCountdown?.updatedAtSeconds ?? null

    if (resetAtSeconds == null && resetSeconds == null) {
      const resetRemainingSeconds = parseQuotaResetRemainingSeconds(detail || undefined)
      resetAtSeconds = resetRemainingSeconds == null
        ? null
        : Math.floor(Date.now() / 1000) + resetRemainingSeconds
      resetSeconds = null
      updatedAtSeconds = null
    }

    items.push({
      label,
      remainingPercent,
      detail: detail || undefined,
      resetAtSeconds,
      resetSeconds,
      updatedAtSeconds,
    })
  }

  return items.sort((a, b) => {
    const orderDiff = getQuotaLabelOrder(a.label) - getQuotaLabelOrder(b.label)
    if (orderDiff !== 0) return orderDiff
    return a.label.localeCompare(b.label, 'zh-Hans-CN')
  })
}

function getQuotaRemainingClassByRemaining(remaining: number): string {
  if (remaining <= 10) return 'text-red-600 dark:text-red-400'
  if (remaining <= 30) return 'text-yellow-600 dark:text-yellow-400'
  return 'text-green-600 dark:text-green-400'
}

function getQuotaRemainingBarColorByRemaining(remaining: number): string {
  if (remaining <= 10) return 'bg-red-500 dark:bg-red-400'
  if (remaining <= 30) return 'bg-yellow-500 dark:bg-yellow-400'
  return 'bg-green-500 dark:bg-green-400'
}

function getQuotaTextClass(quotaText: string): string {
  if (quotaText.includes('封禁') || quotaText.includes('受限')) {
    return 'text-[11px] text-destructive leading-4'
  }
  return 'text-[11px] text-foreground/90 leading-4'
}

function formatPoolScore(value: number | null | undefined): string {
  const n = Number(value)
  if (!Number.isFinite(n)) return '-'
  return n.toFixed(3)
}

function formatPoolScoreReason(value: PoolKeyScore['score_reason'] | null | undefined): string {
  if (!value) return t('poolManagementNoCalculation')
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

function getPoolScoreHardStateLabel(value: PoolKeyScore['hard_state'] | null | undefined): string {
  if (!value) return '-'
  return poolScoreHardStateOptions.value.find(item => item.value === value)?.label || value
}

function getPoolScoreProbeStatusLabel(value: PoolKeyScore['probe_status'] | null | undefined): string {
  if (!value) return '-'
  return poolScoreProbeStatusOptions.value.find(item => item.value === value)?.label || value
}

function formatUnixSeconds(seconds: number | null | undefined): string {
  const raw = Number(seconds ?? 0)
  if (!Number.isFinite(raw) || raw <= 0) return '-'
  return formatRelativeTime(new Date(raw * 1000).toISOString())
}

function formatRelativeTime(isoStr: string): string {
  const date = new Date(isoStr)
  const pad = (n: number) => String(n).padStart(2, '0')
  const M = pad(date.getMonth() + 1)
  const D = pad(date.getDate())
  const h = pad(date.getHours())
  const m = pad(date.getMinutes())
  return `${M}-${D} ${h}:${m}`
}

function formatPoolKeyImportedAt(key: PoolKeyDetail): string {
  const value = key.imported_at || key.created_at
  return value ? formatRelativeTime(value) : '-'
}

// --- Init ---
onMounted(() => {
  startCountdownTimer()
  void loadSchedulingPresetMetas({ cacheTtlMs: POOL_SCHEDULING_PRESETS_CACHE_TTL_MS })
  void loadOverview({ cacheTtlMs: POOL_OVERVIEW_CACHE_TTL_MS })
})

onBeforeUnmount(() => {
  stopDemandMetricsPolling()
  clearCodexCycleStatsReload()
  if (keysSearchDebounceTimer !== null) {
    clearTimeout(keysSearchDebounceTimer)
    keysSearchDebounceTimer = null
  }
  overviewRequestId += 1
  selectProviderRequestId += 1
  providerDataRequestId += 1
  keysRequestId += 1
})
</script>
