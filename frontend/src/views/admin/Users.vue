<template>
  <div class="space-y-6 pb-8">
    <!-- 用户表格 -->
    <Card
      variant="default"
      class="overflow-hidden"
    >
      <!-- 标题和筛选器 -->
      <div class="px-4 sm:px-6 py-3.5 border-b border-border/60">
        <!-- 移动端：标题行 + 筛选器行 -->
        <div class="flex flex-col gap-3 sm:hidden">
          <div class="flex items-center justify-between">
            <h3 class="text-base font-semibold">
              {{ t('userManagement.title') }}
            </h3>
            <div class="flex items-center gap-2">
              <!-- 新增用户按钮 -->
              <Button
                variant="ghost"
                size="icon"
                class="h-8 w-8"
                :title="t('userManagement.groups')"
                @click="showUserGroupsDialog = true"
              >
                <FolderKanban class="w-3.5 h-3.5" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                class="h-8 w-8"
                :title="t('userManagement.add')"
                @click="openCreateDialog"
              >
                <Plus class="w-3.5 h-3.5" />
              </Button>
              <!-- 刷新按钮 -->
              <RefreshButton
                :loading="usersStore.loading"
                @click="refreshUsers"
              />
            </div>
          </div>
          <!-- 筛选器 -->
          <div class="flex flex-wrap items-center gap-2">
            <div class="relative min-w-[12rem] flex-1">
              <Search class="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground z-10 pointer-events-none" />
              <Input
                id="users-search-mobile"
                v-model="searchQuery"
                type="text"
                :placeholder="t('userManagement.search')"
                class="w-full pl-8 pr-3 h-8 text-sm bg-background/50 border-border/60"
              />
            </div>
            <Select
              v-model="filterRole"
            >
              <SelectTrigger class="w-[calc(50vw-1.75rem)] min-w-28 h-8 text-xs border-border/60">
                <SelectValue :placeholder="t('userManagement.role')" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">
                  {{ t('userManagement.all') }}
                </SelectItem>
                <SelectItem value="admin">
                  {{ t('userManagement.admin') }}
                </SelectItem>
                <SelectItem value="audit_admin">
                  {{ t('userManagement.auditAdmin') }}
                </SelectItem>
                <SelectItem value="user">
                  {{ t('userManagement.user') }}
                </SelectItem>
              </SelectContent>
            </Select>
            <Select
              v-model="filterGroup"
            >
              <SelectTrigger class="w-[calc(50vw-1.75rem)] min-w-28 h-8 text-xs border-border/60">
                <SelectValue :placeholder="t('userManagement.group')" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">
                  {{ t('userManagement.allGroups') }}
                </SelectItem>
                <SelectItem
                  v-for="group in userGroups"
                  :key="group.id"
                  :value="group.id"
                >
                  {{ group.name }}
                </SelectItem>
              </SelectContent>
            </Select>
            <Select
              v-model="filterApiKeyGroup"
            >
              <SelectTrigger class="w-[calc(50vw-1.75rem)] min-w-28 h-8 text-xs border-border/60">
                <SelectValue :placeholder="t('userManagement.keyGroup')" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">
                  {{ t('userManagement.allKeys') }}
                </SelectItem>
                <SelectItem
                  v-for="group in userGroups"
                  :key="group.id"
                  :value="group.id"
                >
                  {{ group.name }}
                </SelectItem>
              </SelectContent>
            </Select>
            <Select
              v-model="filterStatus"
            >
              <SelectTrigger class="w-[calc(50vw-1.75rem)] min-w-28 h-8 text-xs border-border/60">
                <SelectValue :placeholder="t('userManagement.status')" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">
                  {{ t('userManagement.allGroups') }}
                </SelectItem>
                <SelectItem value="active">
                  {{ t('userManagement.active') }}
                </SelectItem>
                <SelectItem value="inactive">
                  {{ t('userManagement.disabled') }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>

        <!-- 桌面端：单行布局 -->
        <div class="hidden sm:flex items-center justify-between gap-4">
          <h3 class="text-base font-semibold">
            {{ t('userManagement.title') }}
          </h3>

          <!-- 筛选器和操作按钮 -->
          <div class="flex items-center gap-2">
            <!-- 搜索框 -->
            <div class="relative">
              <Search class="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground z-10 pointer-events-none" />
              <Input
                id="users-search"
                v-model="searchQuery"
                type="text"
                :placeholder="t('userManagement.searchDetail')"
                class="w-48 pl-8 pr-3 h-8 text-sm bg-background/50 border-border/60 focus:border-primary/40 transition-colors"
              />
            </div>

            <!-- 分隔线 -->
            <div class="h-4 w-px bg-border" />

            <!-- 角色筛选 -->
            <div class="xl:hidden">
              <Select
                v-model="filterRole"
              >
                <SelectTrigger class="w-32 h-8 text-xs border-border/60">
                  <SelectValue :placeholder="t('userManagement.allRoles')" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">
                    {{ t('userManagement.allRoles') }}
                  </SelectItem>
                  <SelectItem value="admin">
                    {{ t('userManagement.admin') }}
                  </SelectItem>
                  <SelectItem value="audit_admin">
                    {{ t('userManagement.auditAdmin') }}
                  </SelectItem>
                  <SelectItem value="user">
                    {{ t('userManagement.normalUser') }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>

            <!-- 状态筛选 -->
            <div class="xl:hidden">
              <Select
                v-model="filterStatus"
              >
                <SelectTrigger class="w-28 h-8 text-xs border-border/60">
                  <SelectValue :placeholder="t('userManagement.allStatus')" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">
                    {{ t('userManagement.allStatus') }}
                  </SelectItem>
                  <SelectItem value="active">
                    {{ t('userManagement.active') }}
                  </SelectItem>
                  <SelectItem value="inactive">
                    {{ t('userManagement.disabled') }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>

            <Select v-model="filterGroup">
              <SelectTrigger class="w-32 h-8 text-xs border-border/60">
                <SelectValue :placeholder="t('userManagement.allGroups')" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">
                  {{ t('userManagement.allGroups') }}
                </SelectItem>
                <SelectItem
                  v-for="group in userGroups"
                  :key="group.id"
                  :value="group.id"
                >
                  {{ group.name }}
                </SelectItem>
              </SelectContent>
            </Select>
            <Select v-model="filterApiKeyGroup">
              <SelectTrigger class="w-36 h-8 text-xs border-border/60">
                <SelectValue :placeholder="t('userManagement.allKeyGroups')" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">
                  {{ t('userManagement.allKeyGroups') }}
                </SelectItem>
                <SelectItem
                  v-for="group in userGroups"
                  :key="group.id"
                  :value="group.id"
                >
                  {{ group.name }}
                </SelectItem>
              </SelectContent>
            </Select>

            <!-- 分隔线 -->
            <div class="h-4 w-px bg-border" />

            <!-- 新增用户按钮 -->
            <Button
              v-if="authStore.canOperateAdmin"
              variant="ghost"
              size="icon"
              class="h-8 w-8"
              :title="t('userManagement.groups')"
              @click="showUserGroupsDialog = true"
            >
              <FolderKanban class="w-3.5 h-3.5" />
            </Button>

            <!-- 新增用户按钮 -->
            <Button
              v-if="authStore.canOperateAdmin"
              variant="ghost"
              size="icon"
              class="h-8 w-8"
              :title="t('userManagement.add')"
              @click="openCreateDialog"
            >
              <Plus class="w-3.5 h-3.5" />
            </Button>

            <!-- 刷新按钮 -->
            <RefreshButton
              :loading="usersStore.loading"
              @click="refreshUsers"
            />
          </div>
        </div>
      </div>

      <div class="flex flex-col gap-2 border-b border-border/60 bg-muted/20 px-4 py-2.5 text-xs sm:flex-row sm:items-center sm:justify-between sm:px-6 xl:px-4">
        <div class="flex flex-wrap items-center gap-2 text-muted-foreground">
          <label class="flex items-center gap-2">
            <Checkbox
              :checked="isAllFilteredSelected"
              :indeterminate="isPartiallyFilteredSelected"
              :disabled="filteredUsers.length === 0 || usersStore.loading"
              @update:checked="toggleSelectFiltered"
            />
            <span>{{ t('userManagement.selectAll') }}</span>
          </label>
          <span>{{ t('userManagement.matchSummary', { filtered: filteredUsers.length, page: paginatedUsers.length, selected: selectedCount }) }}</span>
        </div>
        <div class="flex flex-wrap items-center gap-1.5">
          <Button
            variant="ghost"
            size="sm"
            class="h-7 px-2 text-[11px]"
            :disabled="paginatedUsers.length === 0 || selectAllFiltered || usersStore.loading"
            @click="toggleSelectCurrentPage"
          >
            {{ isCurrentPageFullySelected ? t('userManagement.deselectPage') : t('userManagement.selectPage') }}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="h-7 px-2 text-[11px]"
            :disabled="!canClearSelection || usersStore.loading"
            @click="clearSelection"
          >
            {{ t('userManagement.clearSelection') }}
          </Button>
          <Button
            v-if="authStore.canOperateAdmin"
            size="sm"
            class="h-7 px-3 text-[11px]"
            :disabled="(selectedCount === 0 && userGroups.length === 0) || usersStore.loading"
            @click="openUserBatchDialog"
          >
            {{ t('userManagement.bulkActions') }}
          </Button>
        </div>
      </div>

      <!-- 桌面端表格 -->
      <div class="hidden xl:block overflow-x-auto">
        <Table
          class="w-full min-w-[var(--admin-users-table-width)] table-fixed"
          :style="{ '--admin-users-table-width': userTableWidth }"
        >
          <colgroup>
            <col :style="{ width: userTableColumnWidths.select }">
            <col :style="{ width: userTableColumnWidths.user }">
            <col :style="{ width: userTableColumnWidths.wallet }">
            <col :style="{ width: userTableColumnWidths.plan }">
            <col :style="{ width: userTableColumnWidths.stats }">
            <col :style="{ width: userTableColumnWidths.created }">
            <col :style="{ width: userTableColumnWidths.status }">
            <col :style="{ width: userTableColumnWidths.actions }">
          </colgroup>
          <TableHeader>
            <TableRow class="border-b border-border/60 hover:bg-transparent">
              <TableHead class="h-11 px-3">
                <Checkbox
                  :checked="isCurrentPageFullySelected || isAllFilteredSelected"
                  :indeterminate="isPartiallyFilteredSelected && !isCurrentPageFullySelected"
                  :disabled="paginatedUsers.length === 0 || selectAllFiltered || usersStore.loading"
                  @update:checked="toggleSelectCurrentPage"
                />
              </TableHead>
              <SortableTableHead
                class="h-11 px-3 font-semibold"
                column-key="role"
                :sortable="false"
                resize-column-key="user"
                :resizable="true"
                :filter-active="filterRole !== 'all'"
                :filter-title="t('userManagement.filterRole')"
                filter-content-class="w-40 p-1 rounded-2xl border-border bg-card text-foreground shadow-2xl backdrop-blur-xl"
                @resize-start="handleUserTableColumnResizeStart"
              >
                {{ t('userManagement.userInfo') }}
                <template #filter="{ close }">
                  <TableFilterMenu
                    v-model="filterRole"
                    :options="userRoleFilterOptions"
                    @select="close"
                  />
                </template>
              </SortableTableHead>
              <SortableTableHead
                class="h-11 px-3 font-semibold"
                :sortable="false"
                resize-column-key="wallet"
                :resizable="true"
                @resize-start="handleUserTableColumnResizeStart"
              >
                {{ t('userManagement.wallet') }}
              </SortableTableHead>
              <SortableTableHead
                class="h-11 px-3 font-semibold"
                :sortable="false"
                resize-column-key="plan"
                :resizable="true"
                @resize-start="handleUserTableColumnResizeStart"
              >
                {{ t('planQuota.plan') }}
              </SortableTableHead>
              <SortableTableHead
                class="h-11 px-3 font-semibold"
                :sortable="false"
                resize-column-key="stats"
                :resizable="true"
                @resize-start="handleUserTableColumnResizeStart"
              >
                {{ t('userManagement.statsRate') }}
              </SortableTableHead>
              <SortableTableHead
                class="h-11 px-3 font-semibold"
                :sortable="false"
                resize-column-key="created"
                :resizable="true"
                @resize-start="handleUserTableColumnResizeStart"
              >
                {{ t('userManagement.createdAt') }}
              </SortableTableHead>
              <SortableTableHead
                class="h-11 px-3 font-semibold"
                column-key="status"
                :sortable="false"
                resize-column-key="status"
                :resizable="true"
                :filter-active="filterStatus !== 'all'"
                :filter-title="t('userManagement.filterStatus')"
                filter-content-class="w-40 p-1 rounded-2xl border-border bg-card text-foreground shadow-2xl backdrop-blur-xl"
                @resize-start="handleUserTableColumnResizeStart"
              >
                {{ t('userManagement.status') }}
                <template #filter="{ close }">
                  <TableFilterMenu
                    v-model="filterStatus"
                    :options="userStatusFilterOptions"
                    @select="close"
                  />
                </template>
              </SortableTableHead>
              <SortableTableHead
                class="h-11 px-2 text-center font-semibold"
                :sortable="false"
                align="center"
                resize-column-key="actions"
                :resizable="true"
                @resize-start="handleUserTableColumnResizeStart"
              >
                {{ t('userManagement.actions') }}
              </SortableTableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow
              v-for="user in paginatedUsers"
              :key="user.id"
              class="border-b border-border/40 hover:bg-muted/30 transition-colors"
            >
              <TableCell class="px-3 py-3">
                <Checkbox
                  :checked="selectAllFiltered || selectedIdSet.has(user.id)"
                  :disabled="selectAllFiltered || usersStore.loading"
                  @update:checked="(checked) => toggleOne(user.id, checked === true)"
                />
              </TableCell>
              <TableCell class="px-3 py-3">
                <div class="flex items-center gap-3">
                  <Avatar class="h-10 w-10 ring-2 ring-background shadow-md">
                    <AvatarFallback class="bg-primary text-sm font-bold text-white">
                      {{ user.username.charAt(0).toUpperCase() }}
                    </AvatarFallback>
                  </Avatar>
                  <div class="flex-1 min-w-0">
                    <div class="mb-1 flex items-center gap-1.5">
                      <div
                        class="break-all text-sm font-semibold leading-4"
                        :title="user.username"
                      >
                        {{ user.username }}
                      </div>
                      <Badge
                        :variant="userRoleBadgeVariant(user.role)"
                        class="h-5 px-1.5 py-0 text-[10px] font-medium flex-shrink-0"
                      >
                        {{ formatUserRole(user.role) }}
                      </Badge>
                    </div>
                    <div
                      class="break-all text-xs text-muted-foreground leading-4"
                      :title="user.email || '-'"
                    >
                      {{ user.email || '-' }}
                    </div>
                    <div
                      v-if="user.groups?.length"
                      class="mt-1 flex flex-wrap gap-1"
                    >
                      <Badge
                        v-for="group in user.groups"
                        :key="group.id"
                        variant="outline"
                        class="min-h-5 px-1.5 py-0 text-[10px] leading-4"
                        :title="group.name"
                      >
                        {{ group.name }}
                      </Badge>
                    </div>
                  </div>
                </div>
              </TableCell>
              <TableCell class="px-3 py-3">
                <div class="space-y-1.5">
                  <div class="flex items-center gap-1 text-[11px] text-muted-foreground">
                    <span>{{ t('userManagement.walletBalance') }}</span>
                    <Badge
                      v-if="isUserUnlimited(user)"
                      variant="secondary"
                      class="h-5 px-1.5 py-0 text-[10px] font-medium"
                    >
                      {{ t('userManagement.unlimited') }}
                    </Badge>
                    <span
                      v-else
                      class="text-sm font-semibold tabular-nums"
                      :class="isNegativeWalletValue(getUserWalletBalance(user)) ? 'text-rose-600' : 'text-foreground'"
                    >
                      {{ formatCurrencyValue(getUserWalletBalance(user), '-') }}
                    </span>
                  </div>
                  <div
                    v-if="!isUserUnlimited(user) && getUserWallet(user.id) && isNegativeWalletValue(getUserWalletBalance(user))"
                    class="text-[11px] text-muted-foreground"
                  >
                    <span
                      class="font-medium text-rose-600"
                    >
                      {{ t('userManagement.inDebt') }}
                    </span>
                  </div>
                  <div class="flex items-center gap-2 text-[11px] text-muted-foreground flex-wrap">
                    <span>
                      {{ t('userManagement.consumed') }}
                      <span class="font-medium tabular-nums text-foreground">${{ getUserWalletConsumed(user).toFixed(2) }}</span>
                    </span>
                  </div>
                </div>
              </TableCell>
              <TableCell class="px-3 py-3">
                <div
                  v-if="user.plan_summary_status === 'unavailable'"
                  class="text-xs font-medium text-amber-700 dark:text-amber-300"
                >
                  {{ t('planQuota.planUnavailable') }}
                </div>
                <div
                  v-else-if="user.plan"
                  class="space-y-1 text-[11px]"
                >
                  <div
                    class="truncate text-sm font-semibold text-foreground"
                    :title="user.plan.plan_title"
                  >
                    {{ user.plan.plan_title }}
                  </div>
                  <div
                    v-if="user.plan.daily_total_usd != null"
                    class="flex items-center gap-1 text-muted-foreground"
                  >
                    <span>{{ t('planQuota.todayRemaining') }}</span>
                    <span
                      class="font-medium tabular-nums"
                      :class="getUserPlanRemaining(user) <= 0 ? 'text-rose-600' : 'text-foreground'"
                    >
                      {{ formatCurrencyValue(getUserPlanRemaining(user)) }} / {{ formatCurrencyValue(getUserPlanTotal(user)) }}
                    </span>
                  </div>
                  <div
                    v-else-if="user.plan.quota_total_usd > 0"
                    class="flex items-center gap-1 text-muted-foreground"
                  >
                    <span>{{ t('planQuota.currentRemaining') }}</span>
                    <span
                      class="font-medium tabular-nums"
                      :class="getUserPlanRemaining(user) <= 0 ? 'text-rose-600' : 'text-foreground'"
                    >
                      {{ formatCurrencyValue(getUserPlanRemaining(user)) }} / {{ formatCurrencyValue(getUserPlanTotal(user)) }}
                    </span>
                  </div>
                  <div
                    v-else
                    class="text-muted-foreground"
                  >
                    {{ t('planQuota.noDailyQuota') }}
                  </div>
                  <div
                    v-if="hasTighterOverallQuota(user)"
                    class="flex items-center gap-1 text-muted-foreground"
                  >
                    <span>{{ t('planQuota.currentRemaining') }}</span>
                    <span
                      class="font-medium tabular-nums"
                      :class="getUserPlanCurrentRemaining(user) <= 0 ? 'text-rose-600' : 'text-foreground'"
                    >
                      {{ formatCurrencyValue(getUserPlanCurrentRemaining(user)) }}
                    </span>
                  </div>
                  <div
                    v-if="user.plan.daily_window_ends_at"
                    class="text-muted-foreground"
                  >
                    {{ t('planQuota.refreshAt', { time: formatDateTime(user.plan.daily_window_ends_at) }) }}
                  </div>
                  <div class="text-muted-foreground">
                    {{ t('planQuota.expiresAt', { time: formatDateTime(user.plan.expires_at) }) }}
                  </div>
                </div>
                <div
                  v-else
                  class="text-xs text-muted-foreground"
                >
                  {{ t('planQuota.noPlan') }}
                </div>
              </TableCell>
              <TableCell class="px-3 py-3">
                <div class="space-y-1 text-xs">
                  <div class="flex items-center text-muted-foreground">
                    <span class="w-14">{{ t('userManagement.requests') }}</span>
                    <span class="font-medium text-foreground">{{ formatNumber(user.request_count) }}</span>
                  </div>
                  <div class="flex items-center text-muted-foreground">
                    <span class="w-14">Tokens:</span>
                    <span class="font-medium text-foreground">{{ formatTokens(user.total_tokens ?? 0) }}</span>
                  </div>
                  <div class="flex items-center text-muted-foreground">
                    <span class="w-14">{{ t('userManagement.rateLimit') }}</span>
                    <Badge
                      v-if="isRateLimitInherited(user.rate_limit) || isRateLimitUnlimited(user.rate_limit)"
                      variant="secondary"
                      class="h-5 px-1.5 py-0 text-[10px] font-medium"
                    >
                      {{ formatRateLimitInheritable(user.rate_limit) }}
                    </Badge>
                    <span
                      v-else
                      class="font-medium text-foreground"
                    >
                      {{ formatRateLimitInheritable(user.rate_limit) }}
                    </span>
                  </div>
                </div>
              </TableCell>
              <TableCell class="px-3 py-3 text-xs text-muted-foreground whitespace-nowrap">
                {{ formatDateTime(user.created_at) }}
              </TableCell>
              <TableCell class="px-3 py-3">
                <div class="flex flex-col items-start gap-1.5">
                  <Badge
                    :variant="user.is_active ? 'success' : 'destructive'"
                    class="h-5 px-1.5 py-0 text-[10px] font-medium"
                  >
                    {{ user.is_active ? t('userManagement.active') : t('userManagement.disabled') }}
                  </Badge>
                  <Badge
                    v-if="getUserWallet(user.id)"
                    :variant="walletStatusBadge(getUserWalletStatus(user.id))"
                    class="h-5 px-1.5 py-0 text-[10px] font-medium"
                  >
                    {{ walletStatusLabel(getUserWalletStatus(user.id)) }}
                  </Badge>
                </div>
              </TableCell>
              <TableCell class="px-2 py-3">
                <div class="flex justify-end gap-1">
                  <Button
                    v-if="authStore.canOperateAdmin"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7"
                    :title="t('userManagement.edit')"
                    @click="editUser(user)"
                  >
                    <SquarePen class="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    v-if="authStore.canOperateAdmin"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7"
                    :title="t('userManagement.funds')"
                    @click="openWalletActionDialog(user)"
                  >
                    <DollarSign class="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7"
                    title="API Keys"
                    @click="manageApiKeys(user)"
                  >
                    <Key class="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    v-if="authStore.canOperateAdmin"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7"
                    :title="t('userManagement.plans')"
                    @click="manageUserPlans(user)"
                  >
                    <PackageCheck class="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    v-if="authStore.canOperateAdmin"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7"
                    :title="t('userManagement.sessions')"
                    @click="manageUserSessions(user)"
                  >
                    <MonitorSmartphone class="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    v-if="authStore.canOperateAdmin"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7"
                    :title="user.is_active ? t('userManagement.disable') : t('userManagement.enable')"
                    @click="toggleUserStatus(user)"
                  >
                    <PauseCircle
                      v-if="user.is_active"
                      class="h-3.5 w-3.5"
                    />
                    <PlayCircle
                      v-else
                      class="h-3.5 w-3.5"
                    />
                  </Button>
                  <Button
                    v-if="authStore.canOperateAdmin"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7 text-destructive hover:text-destructive"
                    :title="t('userManagement.delete')"
                    @click="deleteUser(user)"
                  >
                    <Trash2 class="h-3.5 w-3.5" />
                  </Button>
                </div>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>

      <!-- 移动端卡片列表 -->
      <div class="xl:hidden bg-muted/[0.14] p-3 sm:p-4">
        <div
          v-if="paginatedUsers.length === 0"
          class="rounded-2xl border border-dashed border-border/60 bg-card/70 px-6 py-10 text-center"
        >
          <Avatar class="mx-auto mb-3 h-12 w-12">
            <AvatarFallback class="bg-muted text-base font-semibold text-muted-foreground">
              U
            </AvatarFallback>
          </Avatar>
          <p class="text-sm font-medium text-foreground">
            {{ hasActiveUserFilter ? t('userManagement.noMatch') : t('userManagement.empty') }}
          </p>
          <p
            v-if="hasActiveUserFilter"
            class="mt-1 text-xs text-muted-foreground"
          >
            {{ t('userManagement.adjustFilters') }}
          </p>
        </div>

        <div
          v-else
          class="space-y-3.5"
        >
          <div
            v-for="user in paginatedUsers"
            :key="user.id"
            class="rounded-2xl border border-border/60 bg-card/95 p-4 shadow-[0_10px_26px_-22px_hsl(var(--foreground))]"
          >
            <div class="space-y-4">
              <div class="flex items-start gap-3">
                <Checkbox
                  class="mt-2 shrink-0"
                  :checked="selectAllFiltered || selectedIdSet.has(user.id)"
                  :disabled="selectAllFiltered || usersStore.loading"
                  @update:checked="(checked) => toggleOne(user.id, checked === true)"
                />
                <Avatar class="h-10 w-10 ring-2 ring-background shadow-md flex-shrink-0">
                  <AvatarFallback class="bg-primary text-sm font-bold text-white">
                    {{ user.username.charAt(0).toUpperCase() }}
                  </AvatarFallback>
                </Avatar>
                <div class="min-w-0 flex-1 space-y-1.5">
                  <div class="flex items-center gap-1.5">
                    <div
                      class="truncate text-sm font-semibold text-foreground"
                      :title="user.username"
                    >
                      {{ user.username }}
                    </div>
                    <Badge
                      :variant="userRoleBadgeVariant(user.role)"
                      class="h-5 px-1.5 py-0 text-[10px] font-medium flex-shrink-0"
                    >
                      {{ formatUserRole(user.role) }}
                    </Badge>
                  </div>
                  <div
                    class="truncate text-[11px] text-muted-foreground"
                    :title="user.email || '-'"
                  >
                    {{ user.email || '-' }}
                  </div>
                </div>
              </div>

              <div class="flex flex-wrap items-center gap-1.5">
                <Badge
                  :variant="user.is_active ? 'success' : 'destructive'"
                  class="h-5 px-1.5 py-0 text-[10px] font-medium"
                >
                  {{ user.is_active ? t('userManagement.active') : t('userManagement.disabled') }}
                </Badge>
                <Badge
                  v-if="getUserWallet(user.id)"
                  :variant="walletStatusBadge(getUserWalletStatus(user.id))"
                  class="h-5 px-1.5 py-0 text-[10px] font-medium"
                >
                  {{ walletStatusLabel(getUserWalletStatus(user.id)) }}
                </Badge>
                <Badge
                  variant="secondary"
                  class="h-5 px-1.5 py-0 text-[10px] font-medium"
                  :title="formatUserEffectiveRateLimitSource(user)"
                >
                  {{ formatRateLimitInheritable(user.rate_limit) }}
                </Badge>
                <Badge
                  v-for="group in user.groups || []"
                  :key="group.id"
                  variant="outline"
                  class="h-5 px-1.5 py-0 text-[10px] font-medium"
                >
                  {{ group.name }}
                </Badge>
              </div>

              <div class="rounded-xl border border-border/60 bg-muted/40 p-3.5">
                <div class="flex items-start justify-between gap-3">
                  <div class="space-y-1">
                    <p class="text-[11px] text-muted-foreground">
                      {{ t('userManagement.walletBalance') }}
                    </p>
                    <Badge
                      v-if="isUserUnlimited(user)"
                      variant="secondary"
                      class="h-5 px-1.5 py-0 text-[10px] font-medium"
                    >
                      {{ t('userManagement.unlimited') }}
                    </Badge>
                    <p
                      v-else
                      class="text-base font-semibold tabular-nums leading-none"
                      :class="isNegativeWalletValue(getUserWalletBalance(user)) ? 'text-rose-600' : 'text-foreground'"
                    >
                      {{ formatCurrencyValue(getUserWalletBalance(user), '-') }}
                    </p>
                    <p
                      v-if="!isUserUnlimited(user) && getUserWallet(user.id) && isNegativeWalletValue(getUserWalletBalance(user))"
                      class="text-[11px] text-muted-foreground"
                    >
                      <span
                        class="font-medium text-rose-600"
                      >
                        {{ t('userManagement.inDebt') }}
                      </span>
                    </p>
                  </div>
                  <div class="text-right">
                    <p class="text-[11px] text-muted-foreground">
                      {{ t('userManagement.consumed') }}
                    </p>
                    <p class="text-sm font-medium tabular-nums text-foreground">
                      ${{ getUserWalletConsumed(user).toFixed(2) }}
                    </p>
                  </div>
                </div>
              </div>

              <div class="rounded-xl border border-border/60 bg-muted/40 p-3.5 text-xs">
                <div
                  v-if="user.plan_summary_status === 'unavailable'"
                  class="font-medium text-amber-700 dark:text-amber-300"
                >
                  {{ t('planQuota.planUnavailable') }}
                </div>
                <div
                  v-else-if="user.plan"
                  class="space-y-2"
                >
                  <div class="flex items-start justify-between gap-3">
                    <div>
                      <p class="text-[11px] text-muted-foreground">
                        {{ t('planQuota.plan') }}
                      </p>
                      <p class="mt-0.5 font-semibold text-foreground">
                        {{ user.plan.plan_title }}
                      </p>
                    </div>
                    <div
                      v-if="user.plan.daily_total_usd != null || user.plan.quota_total_usd > 0"
                      class="text-right"
                    >
                      <p class="text-[11px] text-muted-foreground">
                        {{ user.plan.daily_total_usd != null ? t('planQuota.todayRemaining') : t('planQuota.currentRemaining') }}
                      </p>
                      <p
                        class="mt-0.5 font-semibold tabular-nums"
                        :class="getUserPlanRemaining(user) <= 0 ? 'text-rose-600' : 'text-foreground'"
                      >
                        {{ formatCurrencyValue(getUserPlanRemaining(user)) }} / {{ formatCurrencyValue(getUserPlanTotal(user)) }}
                      </p>
                    </div>
                    <p
                      v-else
                      class="text-right text-[11px] text-muted-foreground"
                    >
                      {{ t('planQuota.noDailyQuota') }}
                    </p>
                  </div>
                  <div class="flex flex-wrap justify-between gap-x-3 gap-y-1 border-t border-border/50 pt-2 text-[11px] text-muted-foreground">
                    <span v-if="hasTighterOverallQuota(user)">
                      {{ t('planQuota.currentRemaining') }}：{{ formatCurrencyValue(getUserPlanCurrentRemaining(user)) }}
                    </span>
                    <span v-if="user.plan.daily_window_ends_at">
                      {{ t('planQuota.refreshAt', { time: formatDateTime(user.plan.daily_window_ends_at) }) }}
                    </span>
                    <span>{{ t('planQuota.expiresAt', { time: formatDateTime(user.plan.expires_at) }) }}</span>
                  </div>
                </div>
                <div
                  v-else
                  class="text-muted-foreground"
                >
                  {{ t('planQuota.noPlan') }}
                </div>
              </div>

              <div class="grid grid-cols-2 gap-2.5 text-xs">
                <div class="rounded-lg border border-border/50 bg-background/70 p-2.5">
                  <div class="mb-1 text-muted-foreground">
                    {{ t('userManagement.requestsCount') }}
                  </div>
                  <div class="font-semibold text-foreground">
                    {{ formatNumber(user.request_count) }}
                  </div>
                </div>
                <div class="rounded-lg border border-border/50 bg-background/70 p-2.5">
                  <div class="mb-1 text-muted-foreground">
                    Tokens
                  </div>
                  <div class="font-semibold text-foreground">
                    {{ formatTokens(user.total_tokens ?? 0) }}
                  </div>
                </div>
              </div>

              <div class="rounded-lg bg-muted/35 p-2.5 text-[11px] text-muted-foreground">
                <div class="flex items-center justify-between gap-2">
                  <span>{{ t('userManagement.createdAt') }}</span>
                  <span class="font-medium text-foreground">{{ formatDateTime(user.created_at) }}</span>
                </div>
              </div>

              <div class="grid grid-cols-2 gap-2 pt-0.5">
                <Button
                  v-if="authStore.canOperateAdmin"
                  variant="outline"
                  size="sm"
                  class="h-8 text-xs"
                  @click="editUser(user)"
                >
                  <SquarePen class="mr-1.5 h-3.5 w-3.5" />
                  {{ t('userManagement.edit') }}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  class="h-8 text-xs"
                  @click="openWalletActionDialog(user)"
                >
                  <DollarSign class="mr-1.5 h-3.5 w-3.5" />
                  {{ t('userManagement.funds') }}
                </Button>
                <Button
                  v-if="authStore.canOperateAdmin"
                  variant="outline"
                  size="sm"
                  class="h-8 text-xs"
                  @click="manageUserPlans(user)"
                >
                  <PackageCheck class="mr-1.5 h-3.5 w-3.5" />
                  {{ t('userManagement.plans') }}
                </Button>
                <Button
                  v-if="authStore.canOperateAdmin"
                  variant="outline"
                  size="sm"
                  class="h-8 text-xs"
                  @click="manageApiKeys(user)"
                >
                  <Key class="mr-1.5 h-3.5 w-3.5" />
                  API Keys
                </Button>
                <Button
                  v-if="authStore.canOperateAdmin"
                  variant="outline"
                  size="sm"
                  class="h-8 text-xs"
                  @click="manageUserSessions(user)"
                >
                  <MonitorSmartphone class="mr-1.5 h-3.5 w-3.5" />
                  {{ t('userManagement.sessions') }}
                </Button>
                <Button
                  v-if="authStore.canOperateAdmin"
                  variant="outline"
                  size="sm"
                  class="h-8 text-xs"
                  @click="toggleUserStatus(user)"
                >
                  <PauseCircle
                    v-if="user.is_active"
                    class="mr-1.5 h-3.5 w-3.5"
                  />
                  <PlayCircle
                    v-else
                    class="mr-1.5 h-3.5 w-3.5"
                  />
                  {{ user.is_active ? t('userManagement.disabled') : t('userManagement.active') }}
                </Button>
                <Button
                  v-if="authStore.canOperateAdmin"
                  variant="outline"
                  size="sm"
                  class="col-span-2 h-8 border-rose-200 text-xs text-rose-600 hover:bg-rose-50 dark:border-rose-900/60 dark:hover:bg-rose-950/40"
                  @click="deleteUser(user)"
                >
                  <Trash2 class="mr-1.5 h-3.5 w-3.5" />
                  {{ t('userManagement.delete') }}
                </Button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- 分页控件 -->
      <Pagination
        :current="currentPage"
        :total="filteredUsers.length"
        :page-size="pageSize"
        cache-key="users-page-size"
        @update:current="currentPage = $event"
        @update:page-size="pageSize = $event"
      />
    </Card>

    <!-- 用户表单对话框（创建/编辑共用） -->
    <UserFormDialog
      ref="userFormDialogRef"
      :open="showUserFormDialog"
      :user="editingUser"
      :groups="userGroups"
      @close="closeUserFormDialog"
      @submit="handleUserFormSubmit"
    />

    <UserBatchActionDialog
      :open="showUserBatchDialog"
      :selected-ids="selectedIds"
      :select-all-filtered="selectAllFiltered"
      :selected-count="selectedCount"
      :filters="batchSelectionFilters"
      :groups="userGroups"
      @close="showUserBatchDialog = false"
      @completed="handleUserBatchCompleted"
    />

    <UserGroupsDialog
      :open="showUserGroupsDialog"
      :users="usersStore.users"
      @close="showUserGroupsDialog = false"
      @changed="handleUserGroupsChanged"
      @inspect-api-key-group="handleInspectApiKeyGroup"
    />

    <Dialog
      v-model="showUserPlansDialog"
      size="xl"
    >
      <template #header>
        <div class="border-b border-border px-6 py-4">
          <div class="flex items-center gap-3">
            <div class="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg bg-kraft/10">
              <PackageCheck class="h-5 w-5 text-kraft" />
            </div>
            <div class="min-w-0 flex-1">
              <h3 class="text-lg font-semibold leading-tight text-foreground">
                {{ t('userPlans.title') }}
              </h3>
              <p class="text-xs text-muted-foreground">
                {{ selectedUser?.username || '-' }} · {{ t('userPlans.description') }}
              </p>
            </div>
          </div>
        </div>
      </template>

      <div class="max-h-[64vh] space-y-4 overflow-y-auto">
        <div class="rounded-lg border border-amber-500/20 bg-amber-500/10 px-3 py-2.5 text-xs text-amber-100/90">
          {{ t('userPlans.notice') }}
        </div>

        <section class="space-y-2.5">
          <div class="flex items-center justify-between gap-3">
            <h4 class="text-sm font-semibold text-foreground">
              {{ t('userPlans.activePlans') }}
            </h4>
            <Button
              variant="ghost"
              size="sm"
              class="h-7 px-2 text-[11px]"
              :disabled="loadingUserPlans || !selectedUser"
              @click="selectedUser && loadUserPlanEntitlements(selectedUser.id)"
            >
              {{ loadingUserPlans ? t('userPlans.loading') : t('userPlans.refresh') }}
            </Button>
          </div>

          <div
            v-if="loadingUserPlans"
            class="rounded-lg border border-dashed border-border/60 bg-muted/20 px-4 py-8 text-center text-sm text-muted-foreground"
          >
            {{ t('userPlans.loadingPlans') }}
          </div>
          <div
            v-else-if="activeUserPlanEntitlements.length === 0"
            class="rounded-lg border border-dashed border-border/60 bg-muted/20 px-4 py-8 text-center text-sm text-muted-foreground"
          >
            {{ t('userPlans.noActivePlans') }}
          </div>
          <div
            v-else
            class="space-y-2.5"
          >
            <div
              v-for="item in activeUserPlanEntitlements"
              :key="item.id"
              class="rounded-lg border border-border bg-card/80 p-3"
            >
              <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                <div class="min-w-0 flex-1">
                  <div class="flex flex-wrap items-center gap-2">
                    <span class="font-medium text-foreground">
                      {{ item.plan_title || item.plan?.title || item.plan_id }}
                    </span>
                    <Badge
                      :variant="item.active ? 'success' : 'secondary'"
                      class="h-5 px-1.5 py-0 text-[10px]"
                    >
                      {{ item.active ? t('userPlans.active') : item.status }}
                    </Badge>
                  </div>
                  <div class="mt-2 flex flex-wrap gap-1.5">
                    <Badge
                      v-for="label in entitlementLabels(item.entitlements, item.allowed_provider_ids)"
                      :key="label"
                      variant="outline"
                      class="h-5 px-1.5 py-0 text-[10px]"
                    >
                      {{ label }}
                    </Badge>
                  </div>
                </div>
                <div class="space-y-2 text-left text-[11px] text-muted-foreground sm:text-right">
                  <div>{{ t('userPlans.obtained') }}：{{ formatDateTime(item.created_at) }}</div>
                  <div>{{ t('userPlans.starts') }}：{{ formatDateTime(item.starts_at) }}</div>
                  <div>{{ t('userPlans.expires') }}：{{ formatDateTime(item.expires_at) }}</div>
                  <div class="flex flex-wrap justify-start gap-2 sm:justify-end">
                    <Button
                      v-if="item.active"
                      variant="outline"
                      size="sm"
                      class="h-7 px-2 text-[11px]"
                      :disabled="updatingUserPlanEntitlement"
                      @click="openEditUserPlanEntitlement(item)"
                    >
                      {{ t('userPlans.edit') }}
                    </Button>
                    <Button
                      v-if="item.active"
                      variant="outline"
                      size="sm"
                      class="h-7 px-2 text-[11px] text-destructive hover:text-destructive"
                      :disabled="cancellingUserPlanEntitlementId === item.id"
                      @click="cancelUserPlanEntitlement(item)"
                    >
                      {{ cancellingUserPlanEntitlementId === item.id ? t('userPlans.cancelling') : t('userPlans.cancelPlan') }}
                    </Button>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </section>

        <section class="space-y-3 rounded-lg border border-border bg-card/70 p-4">
          <div class="space-y-1">
            <h4 class="text-sm font-semibold text-foreground">
              {{ t('userPlans.grantPlan') }}
            </h4>
            <p class="text-xs text-muted-foreground">
              {{ t('userPlans.grantHint') }}
            </p>
          </div>

          <Select v-model="selectedGrantPlanId">
            <SelectTrigger
              class="h-9 rounded-md bg-muted/50 px-3"
              :disabled="loadingBillingPlans || grantableBillingPlans.length === 0"
            >
              <SelectValue :placeholder="loadingBillingPlans ? t('userPlans.loadingOptions') : t('userPlans.choosePlan')" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="plan in grantableBillingPlans"
                :key="plan.id"
                :value="plan.id"
              >
                <div class="flex min-w-0 items-center gap-2">
                  <span class="truncate">{{ plan.title }}</span>
                  <span class="shrink-0 text-xs text-muted-foreground">
                    {{ formatPlanPrice(plan) }} · {{ formatPlanDuration(plan) }} · {{ planModelScopeLabel(plan) }}
                  </span>
                  <span
                    v-if="!plan.enabled"
                    class="shrink-0 text-[10px] text-amber-400"
                  >
                    {{ t('userPlans.unlisted') }}
                  </span>
                </div>
              </SelectItem>
            </SelectContent>
          </Select>

          <Textarea
            v-model="grantReason"
            class="min-h-[60px] resize-y rounded-md bg-muted/50 text-sm"
            maxlength="512"
            :placeholder="t('userPlans.notePlaceholder')"
          />

          <div class="grid gap-3 sm:grid-cols-2">
            <div class="space-y-1.5">
              <Label class="text-xs font-medium text-muted-foreground">{{ t('userPlans.optionalStart') }}</Label>
              <input
                v-model="grantStartsAt"
                type="datetime-local"
                class="flex h-9 w-full rounded-md border border-border/60 bg-muted/50 px-3 py-2 text-sm text-foreground ring-offset-background transition-all focus-visible:border-primary/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
                autocomplete="off"
                @input="handleGrantStartsAtChanged"
                @change="handleGrantStartsAtChanged"
              />
              <p class="text-[11px] text-muted-foreground">
                {{ t('userPlans.startHint') }}
              </p>
            </div>
            <div class="space-y-1.5">
              <Label class="text-xs font-medium text-muted-foreground">{{ t('userPlans.optionalExpiry') }}</Label>
              <input
                v-model="grantExpiresAt"
                type="datetime-local"
                class="flex h-9 w-full rounded-md border border-border/60 bg-muted/50 px-3 py-2 text-sm text-foreground ring-offset-background transition-all focus-visible:border-primary/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
                autocomplete="off"
                @input="handleGrantExpiresAtChanged"
                @change="handleGrantExpiresAtChanged"
              />
              <p class="text-[11px] text-muted-foreground">
                {{ t('userPlans.expiryHint') }}
              </p>
            </div>
          </div>

          <div class="space-y-1.5">
            <Label class="text-xs font-medium text-muted-foreground">{{ t('userPlans.migrationQuota') }}</Label>
            <Input
              v-model="grantInitialRemainingQuotaUsd"
              type="number"
              min="0"
              step="0.01"
              class="h-9 rounded-md bg-muted/50 text-sm"
              :placeholder="t('userPlans.quotaPlaceholder')"
            />
            <p class="text-[11px] text-muted-foreground">
              {{ t('userPlans.migrationQuotaHint') }}
            </p>
          </div>

          <div class="flex justify-end">
            <Button
              size="sm"
              :disabled="grantingUserPlan || !selectedUser || !selectedGrantPlanId"
              @click="grantPlanToSelectedUser"
            >
              {{ grantingUserPlan ? t('userPlans.granting') : t('userPlans.grantPlan') }}
            </Button>
          </div>
        </section>
      </div>

      <template #footer>
        <Button
          variant="outline"
          class="h-10 px-5"
          @click="showUserPlansDialog = false"
        >
          {{ t('userPlans.close') }}
        </Button>
      </template>
    </Dialog>

    <Dialog
      v-model="showEditUserPlanDialog"
      size="lg"
    >
      <template #header>
        <div class="border-b border-border px-6 py-4">
          <div class="flex items-center gap-3">
            <div class="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg bg-kraft/10">
              <PackageCheck class="h-5 w-5 text-kraft" />
            </div>
            <div class="min-w-0 flex-1">
              <h3 class="text-lg font-semibold leading-tight text-foreground">
                {{ t('userPlans.editPlan') }}
              </h3>
              <p class="text-xs text-muted-foreground">
                {{ selectedUser?.username || '-' }} · {{ editingUserPlanEntitlement?.plan_title || editingUserPlanEntitlement?.plan?.title || editingUserPlanEntitlement?.plan_id || '-' }}
              </p>
            </div>
          </div>
        </div>
      </template>

      <div class="space-y-4">
        <div class="rounded-lg border border-amber-500/20 bg-amber-500/10 px-3 py-2.5 text-xs text-amber-100/90">
          {{ t('userPlans.editNotice') }}
        </div>

        <div class="grid gap-3 sm:grid-cols-2">
          <div class="space-y-1.5">
            <Label class="text-xs font-medium text-muted-foreground">{{ t('userPlans.startTime') }}</Label>
            <input
              v-model="editUserPlanStartsAt"
              type="datetime-local"
              class="flex h-9 w-full rounded-md border border-border/60 bg-muted/50 px-3 py-2 text-sm text-foreground ring-offset-background transition-all focus-visible:border-primary/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
              autocomplete="off"
            >
          </div>
          <div class="space-y-1.5">
            <Label class="text-xs font-medium text-muted-foreground">{{ t('userPlans.expiryTime') }}</Label>
            <input
              v-model="editUserPlanExpiresAt"
              type="datetime-local"
              class="flex h-9 w-full rounded-md border border-border/60 bg-muted/50 px-3 py-2 text-sm text-foreground ring-offset-background transition-all focus-visible:border-primary/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
              autocomplete="off"
            >
          </div>
        </div>

        <div class="space-y-1.5">
          <Label class="text-xs font-medium text-muted-foreground">{{ t('userPlans.quotaLimit') }}</Label>
          <Input
            v-model="editUserPlanQuotaUsd"
            type="number"
            min="0"
            step="0.01"
            class="h-9 rounded-md bg-muted/50 text-sm"
            :placeholder="t('userPlans.quotaLimitPlaceholder')"
          />
          <p class="text-[11px] text-muted-foreground">
            {{ t('userPlans.quotaLimitHint') }}
          </p>
        </div>
      </div>

      <template #footer>
        <Button
          variant="outline"
          class="h-10 px-5"
          :disabled="updatingUserPlanEntitlement"
          @click="showEditUserPlanDialog = false"
        >
          {{ t('userPlans.cancel') }}
        </Button>
        <Button
          class="h-10 px-5"
          :disabled="updatingUserPlanEntitlement"
          @click="updateUserPlanEntitlement"
        >
          {{ updatingUserPlanEntitlement ? t('userPlans.saving') : t('userPlans.save') }}
        </Button>
      </template>
    </Dialog>

    <!-- API Keys 管理对话框 -->
    <Dialog
      v-model="showApiKeysDialog"
      size="xl"
    >
      <template #header>
        <div class="border-b border-border px-6 py-4">
          <div class="flex items-center gap-3">
            <div class="flex h-9 w-9 items-center justify-center rounded-lg bg-kraft/10 flex-shrink-0">
              <Key class="h-5 w-5 text-kraft" />
            </div>
            <div class="flex-1 min-w-0">
              <h3 class="text-lg font-semibold text-foreground leading-tight">
                {{ t('userApiKeys.title') }}
              </h3>
              <p class="text-xs text-muted-foreground">
                {{ t('userApiKeys.description') }}
              </p>
            </div>
          </div>
        </div>
      </template>

      <div class="max-h-[60vh] overflow-y-auto space-y-3">
        <template v-if="visibleUserApiKeys.length > 0">
          <div
            v-for="apiKey in visibleUserApiKeys"
            :key="apiKey.id"
            class="rounded-lg border border-border bg-card p-4 hover:border-primary/30 transition-colors"
          >
            <div class="flex items-center justify-between gap-3">
              <!-- 左侧信息 -->
              <div class="flex items-center gap-3 min-w-0 flex-1">
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2 flex-wrap">
                    <span class="font-semibold text-foreground">
                      {{ apiKey.name || t('userApiKeys.unnamed') }}
                    </span>
                    <Badge
                      :variant="apiKey.is_active ? 'success' : 'secondary'"
                      class="text-xs"
                    >
                      {{ apiKey.is_active ? t('userApiKeys.active') : t('userApiKeys.disabled') }}
                    </Badge>
                    <Badge
                      v-if="apiKey.is_locked"
                      variant="secondary"
                      class="text-xs"
                    >
                      {{ t('userApiKeys.locked') }}
                    </Badge>
                    <Badge
                      v-if="apiKey.is_standalone"
                      variant="default"
                      class="text-xs bg-purple-500"
                    >
                      {{ t('userApiKeys.standaloneBalance') }}
                    </Badge>
                    <Badge
                      variant="secondary"
                      class="text-xs"
                    >
                      {{ t('userApiKeys.group') }}：{{ apiKey.group_name || apiKeyGroupName(apiKey.group_id) }}
                    </Badge>
                    <Badge
                      v-if="apiKey.legacy_group_binding_read_only"
                      variant="outline"
                      class="text-xs"
                      :title="apiKey.legacy_group_binding_read_only_reason"
                    >
                      {{ t('userApiKeys.productPolicyReadOnly') }}
                    </Badge>
                    <Badge
                      variant="secondary"
                      class="text-xs"
                    >
                      {{ formatRateLimitSimple(apiKey.rate_limit) }}
                    </Badge>
                    <Badge
                      variant="secondary"
                      class="text-xs"
                    >
                      {{ formatConcurrentLimitSimple(apiKey.concurrent_limit) }}
                    </Badge>
                  </div>
                  <div class="flex items-center gap-1 mt-0.5">
                    <code class="text-xs font-mono text-muted-foreground">
                      {{ apiKey.key_display || '****' }}
                    </code>
                    <button
                      class="p-0.5 hover:bg-muted rounded transition-colors"
                      :title="t('userApiKeys.copyFullKey')"
                      @click="copyFullKey(apiKey)"
                    >
                      <Copy class="w-3 h-3 text-muted-foreground" />
                    </button>
                  </div>
                </div>
              </div>
              <!-- 右侧统计和操作 -->
              <div class="flex items-center gap-4 flex-shrink-0">
                <div class="text-right text-sm">
                  <div class="text-muted-foreground">
                    {{ t('userApiKeys.requestCount', { count: (apiKey.total_requests || 0).toLocaleString() }) }}
                  </div>
                  <div class="font-semibold text-rose-600">
                    ${{ (apiKey.total_cost_usd || 0).toFixed(4) }}
                  </div>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-8 w-8"
                  :title="t('userApiKeys.edit')"
                  @click="openEditUserApiKeyDialog(apiKey)"
                >
                  <SquarePen class="h-4 w-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-8 w-8"
                  :title="apiKey.is_locked ? t('userApiKeys.unlock') : t('userApiKeys.lock')"
                  @click="toggleLockApiKey(apiKey)"
                >
                  <Lock
                    v-if="apiKey.is_locked"
                    class="h-4 w-4"
                  />
                  <LockOpen
                    v-else
                    class="h-4 w-4"
                  />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-8 w-8"
                  :title="t('userApiKeys.delete')"
                  @click="deleteApiKey(apiKey)"
                >
                  <Trash2 class="h-4 w-4" />
                </Button>
              </div>
            </div>
          </div>
        </template>
        <div
          v-else
          class="rounded-lg border-2 border-dashed border-muted-foreground/20 bg-muted/20 px-4 py-12 text-center"
        >
          <div class="flex flex-col items-center gap-3">
            <div class="flex h-14 w-14 items-center justify-center rounded-full bg-muted">
              <Key class="h-6 w-6 text-muted-foreground/50" />
            </div>
            <div>
              <p class="mb-1 text-base font-semibold text-foreground">
                {{ filterApiKeyGroup === 'all' ? t('userApiKeys.empty') : t('userApiKeys.emptyGroup') }}
              </p>
              <p class="text-sm text-muted-foreground">
                {{ filterApiKeyGroup === 'all' ? t('userApiKeys.createHint') : t('userApiKeys.groupHint') }}
              </p>
            </div>
          </div>
        </div>
      </div>

      <template #footer>
        <Button
          variant="outline"
          class="h-10 px-5"
          @click="showApiKeysDialog = false"
        >
          {{ t('userApiKeys.cancel') }}
        </Button>
        <Button
          class="h-10 px-5"
          :disabled="creatingApiKey"
          @click="openCreateUserApiKeyDialog"
        >
          {{ creatingApiKey ? t('userApiKeys.creating') : t('userApiKeys.create') }}
        </Button>
      </template>
    </Dialog>

    <Dialog
      v-model="showUserApiKeyFormDialog"
      size="lg"
    >
      <template #header>
        <div class="border-b border-border px-6 py-4">
          <div class="flex items-center gap-3">
            <div class="flex h-9 w-9 items-center justify-center rounded-lg bg-kraft/10 flex-shrink-0">
              <Key class="h-5 w-5 text-kraft" />
            </div>
            <div class="flex-1 min-w-0">
              <h3 class="text-lg font-semibold text-foreground leading-tight">
                {{ editingUserApiKey ? t('userApiKeys.editTitle') : t('userApiKeys.createTitle') }}
              </h3>
              <p class="text-xs text-muted-foreground">
                {{ editingUserApiKey ? t('userApiKeys.editDescription') : t('userApiKeys.createDescription') }}
              </p>
            </div>
          </div>
        </div>
      </template>

      <div class="space-y-4">
        <div class="space-y-2">
          <Label
            for="admin-user-key-name"
            class="text-sm font-medium"
          >{{ t('userApiKeys.name') }}</Label>
          <Input
            id="admin-user-key-name"
            v-model="userApiKeyForm.name"
            class="h-10"
            :placeholder="t('userApiKeys.namePlaceholder')"
          />
        </div>
        <div class="space-y-2">
          <Label
            for="admin-user-key-group"
            class="text-sm font-medium"
          >{{ t('userApiKeys.useGroup') }}</Label>
          <select
            id="admin-user-key-group"
            v-model="userApiKeyForm.group_id"
            class="h-10 w-full rounded-md border border-border bg-background px-3 text-sm"
            :disabled="apiKeyGroupOptions.length === 0 || editingUserApiKeyGroupBindingReadOnly"
          >
            <option
              v-if="apiKeyGroupOptions.length === 0"
              value=""
            >
              {{ t('userApiKeys.noGroups') }}
            </option>
            <option
              v-for="group in apiKeyGroupOptions"
              :key="group.id"
              :value="group.id"
            >
              {{ group.name }}{{ group.visibility === 'internal' ? t('userApiKeys.internalSuffix') : '' }}
            </option>
          </select>
          <p
            v-if="editingUserApiKeyGroupBindingReadOnly"
            class="text-xs text-amber-700 dark:text-amber-300"
          >
            {{ editingUserApiKey?.legacy_group_binding_read_only_reason || t('userApiKeys.readOnlyReason') }}
          </p>
          <p class="text-xs text-muted-foreground">
            {{ t('userApiKeys.groupHintDetail') }}
          </p>
        </div>
        <div class="space-y-2">
          <Label
            for="admin-user-key-rate-limit"
            class="text-sm font-medium"
          >{{ t('userApiKeys.rateLimit') }}</Label>
          <Input
            id="admin-user-key-rate-limit"
            :model-value="userApiKeyForm.rate_limit ?? ''"
            type="number"
            min="0"
            max="10000"
            class="h-10"
            :placeholder="t('userApiKeys.unlimitedPlaceholder')"
            @update:model-value="(v) => userApiKeyForm.rate_limit = parseNumberInput(v, { min: 0, max: 10000 })"
          />
          <p class="text-xs text-muted-foreground">
            {{ t('userApiKeys.unlimitedHint') }}
          </p>
        </div>
        <div class="space-y-2">
          <Label
            for="admin-user-key-concurrent-limit"
            class="text-sm font-medium"
          >{{ t('userApiKeys.concurrency') }}</Label>
          <Input
            id="admin-user-key-concurrent-limit"
            :model-value="userApiKeyForm.concurrent_limit ?? ''"
            type="number"
            min="0"
            max="10000"
            class="h-10"
            :placeholder="t('userApiKeys.concurrencyPlaceholder')"
            @update:model-value="(v) => userApiKeyForm.concurrent_limit = parseNumberInput(v, { min: 0, max: 10000 })"
          />
          <p class="text-xs text-muted-foreground">
            {{ editingUserApiKey ? t('userApiKeys.editConcurrencyHint') : t('userApiKeys.createConcurrencyHint') }}
          </p>
        </div>

        <div class="rounded-lg border border-border bg-muted/30 p-3 space-y-3">
          <div class="flex items-center justify-between gap-3">
            <Label class="text-sm font-medium">{{ t('userApiKeys.piiProtection') }}</Label>
            <Switch v-model="userApiKeyForm.chat_pii_redaction_enabled" />
          </div>
          <div class="flex items-center justify-between gap-3">
            <Label class="text-sm font-medium">{{ t('userApiKeys.placeholderNotice') }}</Label>
            <Switch
              v-model="userApiKeyForm.chat_pii_redaction_placeholder_notice"
              :disabled="!userApiKeyForm.chat_pii_redaction_enabled"
            />
          </div>
        </div>
      </div>

      <template #footer>
        <Button
          variant="outline"
          class="h-10 px-5"
          @click="closeUserApiKeyFormDialog"
        >
          {{ t('userApiKeys.cancel') }}
        </Button>
        <Button
          class="h-10 px-5"
          :disabled="creatingApiKey"
          @click="submitUserApiKeyForm"
        >
          {{ creatingApiKey ? (editingUserApiKey ? t('userApiKeys.saving') : t('userApiKeys.creating')) : (editingUserApiKey ? t('userApiKeys.save') : t('userApiKeys.create')) }}
        </Button>
      </template>
    </Dialog>

    <Dialog
      v-model="showUserSessionsDialog"
      size="xl"
    >
      <template #header>
        <div class="border-b border-border px-6 py-4">
          <div class="flex items-center gap-3">
            <div class="flex h-9 w-9 items-center justify-center rounded-lg bg-primary/10 flex-shrink-0">
              <MonitorSmartphone class="h-5 w-5 text-primary" />
            </div>
            <div class="flex-1 min-w-0">
              <h3 class="text-lg font-semibold text-foreground leading-tight">
                {{ t('userSessions.title') }}
              </h3>
              <p class="text-xs text-muted-foreground">
                {{ t('userSessions.description') }}
              </p>
            </div>
          </div>
        </div>
      </template>

      <div class="max-h-[60vh] overflow-y-auto space-y-3">
        <div
          v-if="loadingUserSessions"
          class="rounded-lg border border-dashed border-border/60 bg-muted/20 px-4 py-10 text-center text-sm text-muted-foreground"
        >
          {{ t('userSessions.loading') }}
        </div>
        <div
          v-else-if="userSessions.length === 0"
          class="rounded-lg border border-dashed border-border/60 bg-muted/20 px-4 py-10 text-center text-sm text-muted-foreground"
        >
          {{ t('userSessions.empty') }}
        </div>
        <div
          v-else
          class="space-y-3"
        >
          <div
            v-for="session in userSessions"
            :key="session.id"
            class="rounded-lg border border-border bg-card p-4 hover:border-primary/30 transition-colors"
          >
            <div class="flex items-center justify-between gap-3">
              <div class="min-w-0 flex-1">
                <div class="font-semibold text-foreground">
                  {{ session.device_label }}
                </div>
                <div class="mt-1 text-xs text-muted-foreground">
                  {{ formatSessionMeta(session) }}
                </div>
                <div class="mt-1 text-xs text-muted-foreground">
                  {{ t('userSessions.lastActive') }} {{ formatDate(session.last_seen_at || session.created_at) }}
                  <span v-if="session.ip_address"> · IP {{ session.ip_address }}</span>
                </div>
              </div>
              <Button
                variant="outline"
                size="sm"
                :disabled="sessionDialogActionLoading === session.id"
                @click="revokeSelectedUserSession(session.id)"
              >
                {{ sessionDialogActionLoading === session.id ? t('userSessions.processing') : t('userSessions.forceLogout') }}
              </Button>
            </div>
          </div>
        </div>
      </div>

      <template #footer>
        <Button
          variant="outline"
          class="h-10 px-5"
          @click="showUserSessionsDialog = false"
        >
          {{ t('userSessions.close') }}
        </Button>
        <Button
          class="h-10 px-5"
          :disabled="loadingUserSessions || userSessions.length === 0 || sessionDialogActionLoading === 'all'"
          @click="revokeAllSelectedUserSessions"
        >
          {{ sessionDialogActionLoading === 'all' ? t('userSessions.processing') : t('userSessions.logoutAll') }}
        </Button>
      </template>
    </Dialog>

    <WalletOpsDrawer
      :open="showWalletActionDialogState"
      :wallet="walletActionTarget?.wallet || null"
      :owner-name="walletActionTarget?.user.username || ''"
      :owner-subtitle="walletActionTarget?.user.email || t('userSessions.noEmail')"
      :user-id="walletActionTarget?.user.id || null"
      :context-label="t('userManagement.funds')"
      accent="emerald"
      @close="closeWalletActionDrawer"
      @changed="handleWalletDrawerChanged"
    />

    <!-- 新 API Key 显示对话框 -->
    <Dialog
      v-model="showNewApiKeyDialog"
      size="lg"
    >
      <template #header>
        <div class="border-b border-border px-6 py-4">
          <div class="flex items-center gap-3">
            <div class="flex h-9 w-9 items-center justify-center rounded-lg bg-emerald-100 dark:bg-emerald-900/30 flex-shrink-0">
              <CheckCircle class="h-5 w-5 text-emerald-600 dark:text-emerald-400" />
            </div>
            <div class="flex-1 min-w-0">
              <h3 class="text-lg font-semibold text-foreground leading-tight">
                {{ t('userApiKeys.created') }}
              </h3>
              <p class="text-xs text-muted-foreground">
                {{ t('userApiKeys.secretNotice') }}
              </p>
            </div>
          </div>
        </div>
      </template>

      <div class="space-y-4">
        <div class="space-y-2">
          <Label class="text-sm font-medium">API Key</Label>
          <div class="flex items-center gap-2">
            <Input
              ref="apiKeyInput"
              type="text"
              :value="newApiKey"
              readonly
              class="flex-1 font-mono text-sm bg-muted/50 h-11"
              @click="selectApiKey"
            />
            <Button
              class="h-11"
              @click="copyApiKey"
            >
              {{ t('userApiKeys.copy') }}
            </Button>
          </div>
        </div>
      </div>

      <template #footer>
        <Button
          class="h-10 px-5"
          @click="closeNewApiKeyDialog"
        >
          {{ t('userApiKeys.confirm') }}
        </Button>
      </template>
    </Dialog>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useUsersStore } from '@/stores/users'
import { useAuthStore } from '@/stores/auth'
import { usersApi, type User, type ApiKey, type UserSession, type UserBatchActionResponse, type UserBatchSelectionFilters, type UserGroup, type AdminUserPlanEntitlement, type UpsertUserApiKeyRequest } from '@/api/users'
import { formatSessionMeta } from '@/types/session'
import { adminWalletApi, type AdminWallet } from '@/api/admin-wallets'
import { adminBillingPlansApi, type BillingPlan, type DailyQuotaEntitlement } from '@/api/billing'
import { useToast } from '@/composables/useToast'
import { useConfirm } from '@/composables/useConfirm'
import { useClipboard } from '@/composables/useClipboard'
import { useResizableTableColumns, type ResizableTableColumn } from '@/composables/useResizableTableColumns'
import { adminApi } from '@/api/admin'
import { walletStatusBadge, walletStatusLabel } from '@/utils/walletDisplay'
import {
  hasPackageBillingEntitlement,
  normalizeBillingEntitlements,
  quotaConsumptionMultiplierLabel,
  type BillingEntitlementsInput,
} from '@/utils/billingEntitlements'

// UI 组件
import {
  Dialog,
  Card,
  Button,
  Badge,
  Input,
  Label,
  Textarea,
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
  Avatar,
  AvatarFallback,
  Pagination,
  RefreshButton,
  Checkbox,
  Switch
} from '@/components/ui'

import {
  Plus,
  SquarePen,
  Key,
  PauseCircle,
  PlayCircle,
  DollarSign,
  Trash2,
  Copy,
  Search,
  CheckCircle,
  Lock,
  LockOpen,
  MonitorSmartphone,
  FolderKanban,
  PackageCheck,
} from 'lucide-vue-next'

// 功能组件
import UserFormDialog, { type UserFormData } from '@/features/users/components/UserFormDialog.vue'
import UserBatchActionDialog from '@/features/users/components/UserBatchActionDialog.vue'
import UserGroupsDialog from '@/features/users/components/UserGroupsDialog.vue'
import WalletOpsDrawer from '@/features/wallet/components/WalletOpsDrawer.vue'
import { parseApiError } from '@/utils/errorParser'
import { formatTokens, formatRateLimitInheritable, formatRateLimitSimple, isRateLimitInherited, isRateLimitUnlimited } from '@/utils/format'
import { parseNumberInput } from '@/utils/form'
import {
  mergeChatPiiRedactionFeatureSettings,
  readChatPiiRedactionFeatureSettings,
} from '@/utils/featureSettings'
import { log } from '@/utils/logger'
import { useBatchSelection } from '@/composables/useBatchSelection'
import {
  addPlanDuration,
  datetimeLocalToIso,
  defaultGrantPlanTimeWindow,
  formatDatetimeLocal,
  parseDatetimeLocal,
} from '@/features/users/utils/grantPlanTime'

const { success, error, warning } = useToast()
const { t } = useI18n()
const { confirmDanger } = useConfirm()
const { copyToClipboard } = useClipboard()
const usersStore = useUsersStore()
const authStore = useAuthStore()

// 用户表单对话框状态
const showUserFormDialog = ref(false)
const editingUser = ref<UserFormData | null>(null)
const userFormDialogRef = ref<InstanceType<typeof UserFormDialog>>()

// API Keys 对话框状态
const showApiKeysDialog = ref(false)
const showUserSessionsDialog = ref(false)
const showUserPlansDialog = ref(false)
const showNewApiKeyDialog = ref(false)
const showUserApiKeyFormDialog = ref(false)
const selectedUser = ref<User | null>(null)
const userApiKeys = ref<ApiKey[]>([])
const userSessions = ref<UserSession[]>([])
const userPlanEntitlements = ref<AdminUserPlanEntitlement[]>([])
const availableBillingPlans = ref<BillingPlan[]>([])
const selectedGrantPlanId = ref('')
const grantReason = ref('')
const grantStartsAt = ref('')
const grantExpiresAt = ref('')
const grantExpiresAtEdited = ref(false)
const grantInitialRemainingQuotaUsd = ref('')
const newApiKey = ref('')
const creatingApiKey = ref(false)
const loadingUserSessions = ref(false)
const loadingUserPlans = ref(false)
const loadingBillingPlans = ref(false)
const grantingUserPlan = ref(false)
const cancellingUserPlanEntitlementId = ref<string | null>(null)
const showEditUserPlanDialog = ref(false)
const editingUserPlanEntitlement = ref<AdminUserPlanEntitlement | null>(null)
const editUserPlanStartsAt = ref('')
const editUserPlanExpiresAt = ref('')
const editUserPlanQuotaUsd = ref('')
const updatingUserPlanEntitlement = ref(false)
const sessionDialogActionLoading = ref<string | null>(null)
const apiKeyInput = ref<HTMLInputElement>()
const editingUserApiKey = ref<ApiKey | null>(null)
const editingUserApiKeyGroupBindingReadOnly = computed(
  () => editingUserApiKey.value?.legacy_group_binding_read_only === true,
)
const userApiKeyForm = ref({
  name: '',
  group_id: '',
  rate_limit: undefined as number | undefined,
  concurrent_limit: undefined as number | undefined,
  chat_pii_redaction_enabled: false,
  chat_pii_redaction_placeholder_notice: true,
})

// 用户统计
const userWalletMap = ref<Record<string, AdminWallet>>({})

const showWalletActionDialogState = ref(false)
const walletActionTarget = ref<{ user: User; wallet: AdminWallet } | null>(null)
const showUserBatchDialog = ref(false)
const showUserGroupsDialog = ref(false)

const searchQuery = ref('')
const filterRole = ref('all')
const filterStatus = ref('all')
const filterGroup = ref('all')
const filterApiKeyGroup = ref('all')
const userGroups = ref<UserGroup[]>([])
const defaultUserGroupId = ref<string | null>(null)
const userRoleFilterOptions = computed(() => [
  { value: 'all', label: t('userManagement.allRoles') },
  { value: 'admin', label: t('userManagement.admin') },
  { value: 'audit_admin', label: t('userManagement.auditAdmin') },
  { value: 'user', label: t('userManagement.normalUser') },
])
const userStatusFilterOptions = computed(() => [
  { value: 'all', label: t('userManagement.allStatus') },
  { value: 'active', label: t('userManagement.active') },
  { value: 'inactive', label: t('userManagement.disabled') },
])
type UserTableColumnKey = 'select' | 'user' | 'wallet' | 'plan' | 'stats' | 'created' | 'status' | 'actions'
const userTableColumns: ResizableTableColumn<UserTableColumnKey>[] = [
  { key: 'select', width: '40px', minWidth: 40 },
  { key: 'user', width: '320px', minWidth: 260 },
  { key: 'wallet', width: '210px', minWidth: 180 },
  { key: 'plan', width: '260px', minWidth: 220 },
  { key: 'stats', width: '150px', minWidth: 140 },
  { key: 'created', width: '140px', minWidth: 128 },
  { key: 'status', width: '100px', minWidth: 92 },
  { key: 'actions', width: '260px', minWidth: 224 },
]
const {
  columnWidths: userTableColumnWidths,
  startResize: handleUserTableColumnResizeStart,
} = useResizableTableColumns<UserTableColumnKey>({
  storageKey: 'admin-users-table-column-widths-v3',
  columns: userTableColumns,
  defaultMinWidth: 96,
})

function parsePixelWidth(value: string): number {
  const parsed = Number.parseFloat(value)
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0
}

const userTableWidth = computed(() => {
  const total = userTableColumns.reduce((sum, column) => {
    const width = userTableColumnWidths.value[column.key] ?? column.width
    return sum + parsePixelWidth(width)
  }, 0)

  return `${Math.max(total, 960)}px`
})
const hasActiveUserFilter = computed(() =>
  Boolean(searchQuery.value.trim())
  || filterRole.value !== 'all'
  || filterStatus.value !== 'all'
  || filterGroup.value !== 'all'
  || filterApiKeyGroup.value !== 'all'
)

const currentPage = ref(1)
const pageSize = ref(20)
const USERS_PAGE_CACHE_TTL_MS = 10 * 1000

const filteredUsers = computed(() => {
  let filtered = [...usersStore.users]

  // 先排序：管理员优先，然后按创建时间倒序
  filtered.sort((a, b) => {
    const roleRank = (role: string) => role === 'admin' ? 0 : role === 'audit_admin' ? 1 : 2
    const roleDiff = roleRank(a.role) - roleRank(b.role)
    if (roleDiff !== 0) return roleDiff
    // 同角色按创建时间倒序（新用户在前）
    return new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
  })

  // 搜索（支持空格分隔的多关键词 AND 搜索）
  if (searchQuery.value) {
    const keywords = searchQuery.value.toLowerCase().split(/\s+/).filter(k => k.length > 0)
    filtered = filtered.filter(u => {
      const searchableText = `${u.username} ${u.email || ''}`.toLowerCase()
      return keywords.every(keyword => searchableText.includes(keyword))
    })
  }

  if (filterRole.value !== 'all') {
    filtered = filtered.filter(u => u.role === filterRole.value)
  }

  if (filterStatus.value !== 'all') {
    filtered = filtered.filter(u =>
      filterStatus.value === 'active' ? u.is_active : !u.is_active
    )
  }

  if (filterGroup.value !== 'all') {
    filtered = filtered.filter(u => (u.groups || []).some(group => group.id === filterGroup.value))
  }

  return filtered
})
const visibleUserApiKeys = computed(() => {
  if (filterApiKeyGroup.value === 'all') return userApiKeys.value
  return userApiKeys.value.filter(apiKey => apiKey.group_id === filterApiKeyGroup.value)
})

const paginatedUsers = computed(() => {
  const start = (currentPage.value - 1) * pageSize.value
  return filteredUsers.value.slice(start, start + pageSize.value)
})

const filteredUserCount = computed(() => filteredUsers.value.length)
const {
  selectedIds,
  selectAllFiltered,
  selectedIdSet,
  selectedCount,
  isAllFilteredSelected,
  isPartiallyFilteredSelected,
  isCurrentPageFullySelected,
  canClearSelection,
  rememberItems: rememberBatchPageUsers,
  resetSelection: resetBatchSelection,
  toggleOne,
  toggleSelectFiltered,
  toggleSelectCurrentPage,
  clearSelection,
} = useBatchSelection<User>({
  pageItems: paginatedUsers,
  filteredTotal: filteredUserCount,
  getItemId: (user) => user.id,
})

const batchSelectionFilters = computed<UserBatchSelectionFilters>(() => {
  const filters: UserBatchSelectionFilters = {}
  const search = searchQuery.value.trim()
  if (search) filters.search = search
  if (filterRole.value === 'admin' || filterRole.value === 'audit_admin' || filterRole.value === 'user') filters.role = filterRole.value
  if (filterStatus.value === 'active') filters.is_active = true
  if (filterStatus.value === 'inactive') filters.is_active = false
  if (filterGroup.value !== 'all') filters.group_id = filterGroup.value
  if (filterApiKeyGroup.value !== 'all') filters.api_key_group_id = filterApiKeyGroup.value
  return filters
})

const grantableBillingPlans = computed(() =>
  availableBillingPlans.value.filter((plan) => hasPackageEntitlement(plan.entitlements))
)
const selectedGrantPlan = computed(() =>
  grantableBillingPlans.value.find((plan) => plan.id === selectedGrantPlanId.value) || null
)
const activeUserPlanEntitlements = computed(() =>
  userPlanEntitlements.value.filter((item) => item.active)
)
const selectedUserGroupIds = computed(() =>
  new Set((selectedUser.value?.groups || []).map((group) => group.id))
)
const apiKeyGroupOptions = computed(() => {
  const currentGroupId = editingUserApiKey.value?.group_id || ''
  return userGroups.value.filter((group) => {
    if (group.visibility !== 'internal') return true
    return selectedUserGroupIds.value.has(group.id) || group.id === currentGroupId
  })
})

// Watch filter changes and reset to first page
watch([searchQuery, filterRole, filterStatus, filterGroup, filterApiKeyGroup], () => {
  currentPage.value = 1
  resetBatchSelection()
})
watch(filterApiKeyGroup, () => {
  void refreshUsers()
})

watch(paginatedUsers, (users) => rememberBatchPageUsers(users), { immediate: true })
watch(selectedGrantPlanId, () => {
  if (!showUserPlansDialog.value) return
  applyDefaultGrantPlanTimeWindow()
})

function formatUserRole(role: string) {
  if (role === 'admin') return t('userManagement.admin')
  if (role === 'audit_admin') return t('userManagement.auditAdmin')
  return t('userManagement.normalUser')
}

function userRoleBadgeVariant(role: string) {
  return role === 'admin' ? 'default' : 'secondary'
}

onMounted(() => {
  void refreshUsers({ preferCache: true })
})

async function refreshUsers(options: { preferCache?: boolean } = {}) {
  const cacheTtlMs = options.preferCache ? USERS_PAGE_CACHE_TTL_MS : 0
  const apiKeyGroupId = filterApiKeyGroup.value !== 'all' ? filterApiKeyGroup.value : undefined
  await Promise.all([
    usersStore.fetchUsers({
      cacheTtlMs,
      api_key_group_id: apiKeyGroupId,
      limit: 1000,
    }),
    loadUserGroups(),
  ])
  syncUserWalletsFromUsers()
}

async function loadUserGroups(): Promise<void> {
  try {
    const response = await usersStore.listUserGroups()
    userGroups.value = response.items
    defaultUserGroupId.value = response.default_group_id ?? null
    if (filterGroup.value !== 'all' && !userGroups.value.some((group) => group.id === filterGroup.value)) {
      filterGroup.value = 'all'
    }
    if (filterApiKeyGroup.value !== 'all' && !userGroups.value.some((group) => group.id === filterApiKeyGroup.value)) {
      filterApiKeyGroup.value = 'all'
    }
  } catch (err) {
    log.error(t('userManagement.loadGroupsFailed'), err)
  }
}

async function handleUserGroupsChanged(): Promise<void> {
  await refreshUsers()
}

async function handleInspectApiKeyGroup(groupId: string): Promise<void> {
  showUserGroupsDialog.value = false
  if (filterApiKeyGroup.value === groupId) {
    await refreshUsers()
    return
  }
  filterApiKeyGroup.value = groupId
}

function openUserBatchDialog(): void {
  if (selectedCount.value === 0 && userGroups.value.length === 0) return
  showUserBatchDialog.value = true
}

async function handleUserBatchCompleted(_result: UserBatchActionResponse): Promise<void> {
  await refreshUsers()
  resetBatchSelection(true)
}

function formatDate(dateString: string) {
  return new Date(dateString).toLocaleDateString('zh-CN')
}

function formatDateTime(value?: string | null): string {
  if (!value) return '-'
  return new Date(value).toLocaleString('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function isoToDatetimeLocal(value?: string | null): string {
  if (!value) return ''
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return ''
  return formatDatetimeLocal(date)
}

function formatPlanPrice(plan: BillingPlan): string {
  return `${Number(plan.price_amount || 0).toFixed(2)} ${plan.price_currency || 'CNY'}`
}

function formatPlanDuration(plan: BillingPlan): string {
  const labels: Record<string, string> = {
    day: t('userManagement.day'),
    month: t('userManagement.month'),
    year: t('userManagement.year'),
    custom: t('userManagement.day'),
  }
  const unit = labels[plan.duration_unit] || t('userManagement.day')
  return `${Number(plan.duration_value || 1)}${unit}`
}

function entitlementLabels(items: BillingEntitlementsInput, providerIds: string[] = []): string[] {
  return normalizeBillingEntitlements(items).map((item) => {
    if (item.type === 'wallet_credit') {
      return t('userManagement.bonusBalance', { amount: Number(item.amount_usd || 0).toFixed(2) })
    }
    if (item.type === 'daily_quota') {
      return quotaEntitlementLabel(item, providerIds)
    }
    if (item.type === 'membership_group') {
      return t('userManagement.membershipBenefit')
    }
    return t('userManagement.unknownBenefit')
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
  if (daily > 0) parts.push(t('userManagement.dailyQuota', { amount: daily.toFixed(2) }))
  if (fiveHour > 0) parts.push(`5H $${fiveHour.toFixed(2)}`)
  if (weekly > 0) parts.push(t('userManagement.weeklyQuota', { amount: weekly.toFixed(2) }))
  if (monthly > 0) parts.push(t('userManagement.monthlyQuota', { amount: monthly.toFixed(2) }))
  const quotaText = parts.join(' / ') || t('userManagement.usageQuota')
  const labels = [providerIds.length > 0
    ? t('userManagement.modelsByProviders')
    : t('userManagement.noPlanProviders')]
  const multiplierLabel = quotaConsumptionMultiplierLabel(item)
  if (multiplierLabel) labels.push(multiplierLabel)
  return `${quotaText} · ${labels.join(' · ')}`
}

function planModelScopeLabel(plan: BillingPlan): string {
  const count = Array.isArray(plan.allowed_provider_ids) ? plan.allowed_provider_ids.length : 0
  return count > 0
    ? t('userManagement.providerCount', { count })
    : t('userManagement.noPlanProviders')
}

function syncUserWalletsFromUsers() {
  userWalletMap.value = usersStore.users.reduce<Record<string, AdminWallet>>((acc, user) => {
    if (user.wallet?.user_id) {
      acc[user.wallet.user_id] = user.wallet
    }
    return acc
  }, {})
}

function formatNumber(value?: number | null): string {
  const numericValue = typeof value === 'number' && Number.isFinite(value) ? value : 0
  return numericValue.toLocaleString()
}

function getUserWallet(userId: string): AdminWallet | null {
  return userWalletMap.value[userId] || null
}

function isUserUnlimited(user: User): boolean {
  const wallet = getUserWallet(user.id)
  if (wallet?.limit_mode === 'unlimited' || wallet?.unlimited === true) {
    return true
  }
  return Boolean(user.unlimited)
}

function getUserWalletBalance(user: User): number {
  const wallet = getUserWallet(user.id)
  const value = wallet?.actual_wallet_balance ?? wallet?.wallet_balance ?? wallet?.balance ?? 0
  return Number.isFinite(value) ? value : 0
}

function getUserWalletConsumed(user: User): number {
  return getUserWallet(user.id)?.total_consumed ?? 0
}

function getUserPlanTotal(user: User): number {
  const value = user.plan?.daily_total_usd ?? user.plan?.quota_total_usd ?? 0
  return Number.isFinite(value) ? Math.max(0, value) : 0
}

function getUserPlanRemaining(user: User): number {
  if (!user.plan) return 0
  const value = user.plan.daily_remaining_usd == null
    ? Number(user.plan.quota_remaining_usd ?? 0)
    : Number(user.plan.daily_remaining_usd)
  return Number.isFinite(value) ? Math.max(0, value) : 0
}

function getUserPlanCurrentRemaining(user: User): number {
  const value = Number(user.plan?.quota_remaining_usd ?? 0)
  return Number.isFinite(value) ? Math.max(0, value) : 0
}

function hasTighterOverallQuota(user: User): boolean {
  return user.plan?.daily_remaining_usd != null
    && getUserPlanCurrentRemaining(user) + Number.EPSILON < getUserPlanRemaining(user)
}

function getUserWalletStatus(userId: string): string | null {
  return getUserWallet(userId)?.status ?? null
}

function formatCurrencyValue(value: number | null, nullLabel = '-'): string {
  if (value == null) {
    return nullLabel
  }
  return `$${value.toFixed(2)}`
}

function formatConcurrentLimitSimple(concurrentLimit?: number | null): string {
  if (concurrentLimit == null || concurrentLimit === 0) {
    return t('userManagement.unlimitedConcurrency')
  }
  return t('userManagement.concurrency', { count: concurrentLimit })
}

function apiKeyGroupName(groupId?: string | null): string {
  if (!groupId) return t('userManagement.defaultGroup')
  return userGroups.value.find((group) => group.id === groupId)?.name || t('userManagement.unknownGroup')
}

function defaultApiKeyGroupId(): string {
  const defaultGroupId = defaultUserGroupId.value
  if (defaultGroupId && apiKeyGroupOptions.value.some((group) => group.id === defaultGroupId)) {
    return defaultGroupId
  }
  return apiKeyGroupOptions.value[0]?.id || ''
}

function formatUserEffectiveRateLimitSource(user: User): string {
  const source = user.effective_policy?.rate_limit
  if (!source) return ''
  if (source.source === 'group' && source.group_name) {
    return t('userManagement.inheritedFromGroup', { group: source.group_name })
  }
  if (source.source === 'combined') {
    const groupNames = Array.isArray(source.group_names) ? source.group_names.join('、') : ''
    return groupNames ? t('userManagement.combinedWithGroups', { groups: groupNames }) : t('userManagement.combinedLimits')
  }
  if (source.source === 'user') {
    return t('userManagement.userConfigured')
  }
  return t('userManagement.systemDefault')
}

function isNegativeWalletValue(value: number | null): boolean {
  return typeof value === 'number' && value < 0
}

async function toggleUserStatus(user: User) {
  const action = user.is_active ? t('userManagement.disable') : t('userManagement.enable')
  const confirmed = await confirmDanger(
    t('userManagement.statusConfirm', { action, user: user.username }),
    t('userManagement.statusActionTitle', { action }),
    action
  )

  if (!confirmed) return

  try {
    await usersStore.updateUser(user.id, { is_active: !user.is_active })
    success(t('userManagement.statusUpdated', { action }))
  } catch (err: unknown) {
    error(parseApiError(err, t('userManagement.unknownError')), t('userManagement.statusUpdateFailed', { action }))
  }
}

// ========== 用户表单对话框方法 ==========

function openCreateDialog() {
  editingUser.value = null
  showUserFormDialog.value = true
}

function editUser(user: User) {
  // 创建数组副本，避免与 store 数据共享引用
  editingUser.value = {
    id: user.id,
    username: user.username,
    email: user.email,
    unlimited: user.unlimited,
    role: user.role,
    is_active: user.is_active,
    group_ids: (user.groups || []).map(group => group.id),
    feature_settings: user.feature_settings ?? null,
  }
  showUserFormDialog.value = true
}

function closeUserFormDialog() {
  showUserFormDialog.value = false
  editingUser.value = null
}

async function handleUserFormSubmit(data: UserFormData & { password?: string; unlimited?: boolean }) {
  userFormDialogRef.value?.setSaving(true)
  try {
    if (data.id) {
      // 更新用户
      const updateData: Record<string, unknown> = {
        username: data.username,
        email: data.email || undefined,
        unlimited: data.unlimited,
        role: data.role,
        group_ids: data.group_ids ?? [],
        feature_settings: data.feature_settings ?? null,
      }
      if (data.password) {
        updateData.password = data.password
      }
      await usersStore.updateUser(data.id, updateData)
      await refreshUsers()
       success(t('userManagement.userUpdated'))
    } else {
      // 创建用户
      const newUser = await usersStore.createUser({
        username: data.username,
        password: data.password ?? '',
        email: data.email || '',
        initial_gift_usd: data.initial_gift_usd,
        unlimited: data.unlimited,
        role: data.role,
        group_ids: data.group_ids ?? [],
        feature_settings: data.feature_settings ?? null,
      })
      // 如果创建时指定为禁用，则更新状态
      if (data.is_active === false && newUser) {
        await usersStore.updateUser(newUser.id, { is_active: false })
      }
      await refreshUsers()
       success(t('userManagement.userCreated'))
    }
    closeUserFormDialog()
  } catch (err: unknown) {
    const title = data.id ? t('userManagement.updateUserFailed') : t('userManagement.createUserFailed')
    error(parseApiError(err, t('userManagement.unknownError')), title)
  } finally {
    userFormDialogRef.value?.setSaving(false)
  }
}

async function manageApiKeys(user: User) {
  selectedUser.value = user
  showApiKeysDialog.value = true
  await Promise.all([
    loadUserGroups(),
    loadUserApiKeys(user.id),
  ])
}

async function manageUserSessions(user: User) {
  selectedUser.value = user
  showUserSessionsDialog.value = true
  loadingUserSessions.value = true
  try {
    userSessions.value = await usersStore.getUserSessions(user.id)
  } catch (err) {
    error(parseApiError(err, t('userManagement.loadSessionsFailed')))
  } finally {
    loadingUserSessions.value = false
  }
}

async function manageUserPlans(user: User) {
  selectedUser.value = user
  showUserPlansDialog.value = true
  selectedGrantPlanId.value = ''
  grantReason.value = ''
  grantStartsAt.value = ''
  grantExpiresAt.value = ''
  grantExpiresAtEdited.value = false
  grantInitialRemainingQuotaUsd.value = ''
  await Promise.all([
    loadUserPlanEntitlements(user.id),
    loadAvailableBillingPlans(),
  ])
  if (!selectedGrantPlanId.value && grantableBillingPlans.value.length > 0) {
    selectedGrantPlanId.value = grantableBillingPlans.value[0].id
  }
  applyDefaultGrantPlanTimeWindow()
}

async function loadUserPlanEntitlements(userId: string) {
  loadingUserPlans.value = true
  try {
    const response = await usersApi.listUserPlanEntitlements(userId)
    userPlanEntitlements.value = response.items
  } catch (err) {
    error(parseApiError(err, t('userManagement.loadPlansFailed')))
    userPlanEntitlements.value = []
  } finally {
    loadingUserPlans.value = false
  }
}

async function loadAvailableBillingPlans() {
  loadingBillingPlans.value = true
  try {
    const response = await adminBillingPlansApi.list()
    availableBillingPlans.value = response.items
    if (
      selectedGrantPlanId.value
      && !response.items.some((plan) => plan.id === selectedGrantPlanId.value)
    ) {
      selectedGrantPlanId.value = ''
    }
  } catch (err) {
    error(parseApiError(err, t('userManagement.loadPlanListFailed')))
    availableBillingPlans.value = []
  } finally {
    loadingBillingPlans.value = false
  }
}

function applyDefaultGrantPlanTimeWindow(plan = selectedGrantPlan.value): void {
  if (!plan) {
    grantStartsAt.value = ''
    grantExpiresAt.value = ''
    grantExpiresAtEdited.value = false
    return
  }
  const window = defaultGrantPlanTimeWindow(plan)
  grantStartsAt.value = window.startsAt
  grantExpiresAt.value = window.expiresAt
  grantExpiresAtEdited.value = false
}

function refreshGrantExpiresAtFromStart(): void {
  const plan = selectedGrantPlan.value
  if (!plan) return
  const startsAt = parseDatetimeLocal(grantStartsAt.value)
  if (!startsAt) {
    grantExpiresAt.value = ''
    return
  }
  grantExpiresAt.value = formatDatetimeLocal(addPlanDuration(startsAt, plan))
}

function handleGrantStartsAtChanged(event?: Event): void {
  const value = (event?.target as HTMLInputElement | null)?.value
  if (typeof value === 'string') {
    grantStartsAt.value = value
  }
  if (!grantExpiresAtEdited.value) {
    refreshGrantExpiresAtFromStart()
  }
}

function handleGrantExpiresAtChanged(event?: Event): void {
  const value = (event?.target as HTMLInputElement | null)?.value
  if (typeof value === 'string') {
    grantExpiresAt.value = value
  }
  grantExpiresAtEdited.value = true
}

function optionalUsdAmount(value: string): number | null | undefined {
  const trimmed = value.trim()
  if (!trimmed) return null
  const amount = Number(trimmed)
  if (!Number.isFinite(amount) || amount < 0) return undefined
  return amount
}

async function grantPlanToSelectedUser() {
  if (!selectedUser.value || !selectedGrantPlanId.value) return
  const startsAt = datetimeLocalToIso(grantStartsAt.value)
  const expiresAt = datetimeLocalToIso(grantExpiresAt.value)
  const initialRemainingQuotaUsd = optionalUsdAmount(grantInitialRemainingQuotaUsd.value)
  if (startsAt === undefined || expiresAt === undefined) {
    error(t('userManagement.invalidTime'))
    return
  }
  if (initialRemainingQuotaUsd === undefined) {
    error(t('userManagement.quotaLimitInvalid'))
    return
  }
  const now = Date.now()
  if (startsAt && new Date(startsAt).getTime() > now) {
    error(t('userManagement.startAfterNow'))
    return
  }
  if (expiresAt) {
    const startsAtMs = startsAt ? new Date(startsAt).getTime() : now
    if (new Date(expiresAt).getTime() <= startsAtMs) {
      error(t('userManagement.endBeforeStart'))
      return
    }
  }
  grantingUserPlan.value = true
  try {
    const response = await usersApi.grantUserPlan(selectedUser.value.id, {
      plan_id: selectedGrantPlanId.value,
      reason: grantReason.value.trim() || null,
      starts_at: startsAt,
      expires_at: expiresAt,
      initial_remaining_quota_usd: initialRemainingQuotaUsd,
    })
    userPlanEntitlements.value = response.items
    await refreshUsers()
    grantReason.value = ''
    grantInitialRemainingQuotaUsd.value = ''
    applyDefaultGrantPlanTimeWindow()
    success(t('userManagement.planGranted'))
  } catch (err) {
    error(parseApiError(err, t('userManagement.grantPlanFailed')))
  } finally {
    grantingUserPlan.value = false
  }
}

function openEditUserPlanEntitlement(item: AdminUserPlanEntitlement): void {
  editingUserPlanEntitlement.value = item
  editUserPlanStartsAt.value = isoToDatetimeLocal(item.starts_at)
  editUserPlanExpiresAt.value = isoToDatetimeLocal(item.expires_at)
  editUserPlanQuotaUsd.value = ''
  showEditUserPlanDialog.value = true
}

async function updateUserPlanEntitlement(): Promise<void> {
  if (!selectedUser.value || !editingUserPlanEntitlement.value) return
  const startsAt = datetimeLocalToIso(editUserPlanStartsAt.value)
  const expiresAt = datetimeLocalToIso(editUserPlanExpiresAt.value)
  const quotaUsd = optionalUsdAmount(editUserPlanQuotaUsd.value)
  if (startsAt === undefined || expiresAt === undefined) {
    error(t('userManagement.invalidTime'))
    return
  }
  if (!startsAt || !expiresAt) {
    error(t('userManagement.timeRangeRequired'))
    return
  }
  if (quotaUsd === undefined) {
    error(t('userManagement.quotaLimitInvalid'))
    return
  }
  const now = Date.now()
  if (new Date(startsAt).getTime() > now) {
    error(t('userManagement.startAfterNow'))
    return
  }
  if (new Date(expiresAt).getTime() <= new Date(startsAt).getTime()) {
      error(t('userManagement.endBeforeStart'))
    return
  }

  updatingUserPlanEntitlement.value = true
  try {
    const response = await usersApi.updateUserPlanEntitlement(
      selectedUser.value.id,
      editingUserPlanEntitlement.value.id,
      {
        starts_at: startsAt,
        expires_at: expiresAt,
        ...(quotaUsd === null ? {} : { initial_remaining_quota_usd: quotaUsd }),
      }
    )
    userPlanEntitlements.value = response.items
    await refreshUsers()
    showEditUserPlanDialog.value = false
    editingUserPlanEntitlement.value = null
    success(t('userManagement.planSaved'))
  } catch (err) {
    error(parseApiError(err, t('userManagement.savePlanFailed')))
  } finally {
    updatingUserPlanEntitlement.value = false
  }
}

async function cancelUserPlanEntitlement(item: AdminUserPlanEntitlement): Promise<void> {
  if (!selectedUser.value || !item.active) return
  const planName = item.plan_title || item.plan?.title || item.plan_id
  const confirmed = await confirmDanger(
    t('userManagement.cancelPlanConfirm', { user: selectedUser.value.username, plan: planName }),
    t('userManagement.cancelPlanTitle'),
    t('userManagement.confirmCancel')
  )
  if (!confirmed) return

  cancellingUserPlanEntitlementId.value = item.id
  try {
    const response = await usersApi.cancelUserPlanEntitlement(selectedUser.value.id, item.id)
    userPlanEntitlements.value = response.items
    await refreshUsers()
    success(t('userManagement.planCancelled'))
  } catch (err) {
    error(parseApiError(err, t('userManagement.cancelPlanFailed')))
  } finally {
    cancellingUserPlanEntitlementId.value = null
  }
}

async function loadUserApiKeys(userId: string) {
  try {
    userApiKeys.value = await usersStore.getUserApiKeys(userId)
  } catch (err) {
    log.error('加载API Keys失败:', err)
    userApiKeys.value = []
  }
}

function openCreateUserApiKeyDialog() {
  const redactionFeature = readChatPiiRedactionFeatureSettings(null)
  editingUserApiKey.value = null
  userApiKeyForm.value = {
    name: `Key-${new Date().toISOString().split('T')[0]}`,
    group_id: defaultApiKeyGroupId(),
    rate_limit: undefined,
    concurrent_limit: undefined,
    chat_pii_redaction_enabled: redactionFeature.enabled,
    chat_pii_redaction_placeholder_notice: redactionFeature.inject_model_instruction,
  }
  showUserApiKeyFormDialog.value = true
}

function openEditUserApiKeyDialog(apiKey: ApiKey) {
  const redactionFeature = readChatPiiRedactionFeatureSettings(apiKey.feature_settings)
  editingUserApiKey.value = apiKey
  userApiKeyForm.value = {
    name: apiKey.name || '',
    group_id: apiKey.group_id || defaultApiKeyGroupId(),
    rate_limit: apiKey.rate_limit ?? undefined,
    concurrent_limit: apiKey.concurrent_limit ?? undefined,
    chat_pii_redaction_enabled: redactionFeature.enabled,
    chat_pii_redaction_placeholder_notice: redactionFeature.inject_model_instruction,
  }
  showUserApiKeyFormDialog.value = true
}

function closeUserApiKeyFormDialog() {
  showUserApiKeyFormDialog.value = false
  editingUserApiKey.value = null
  userApiKeyForm.value = {
    name: '',
    group_id: '',
    rate_limit: undefined,
    concurrent_limit: undefined,
    chat_pii_redaction_enabled: false,
    chat_pii_redaction_placeholder_notice: true,
  }
}

async function submitUserApiKeyForm() {
  if (!selectedUser.value) return
  if (!userApiKeyForm.value.name.trim()) {
    error(t('userManagement.keyNameRequired'), editingUserApiKey.value ? t('userManagement.updateKeyFailed') : t('userManagement.createKeyFailed'))
    return
  }
  if (!userApiKeyForm.value.group_id) {
    error(t('userManagement.keyGroupRequired'), editingUserApiKey.value ? t('userManagement.updateKeyFailed') : t('userManagement.createKeyFailed'))
    return
  }

  creatingApiKey.value = true
  try {
    if (editingUserApiKey.value) {
      const updatePayload: UpsertUserApiKeyRequest = {
        name: userApiKeyForm.value.name,
        rate_limit: userApiKeyForm.value.rate_limit ?? 0,
        concurrent_limit: userApiKeyForm.value.concurrent_limit,
        feature_settings: mergeChatPiiRedactionFeatureSettings(editingUserApiKey.value.feature_settings, {
          enabled: userApiKeyForm.value.chat_pii_redaction_enabled,
          inject_model_instruction: userApiKeyForm.value.chat_pii_redaction_placeholder_notice,
        }),
      }
      if (!editingUserApiKeyGroupBindingReadOnly.value) {
        updatePayload.group_id = userApiKeyForm.value.group_id
      } else {
        warning(editingUserApiKey.value.legacy_group_binding_read_only_reason || t('userManagement.keyReadOnly'))
      }
      await usersStore.updateApiKey(selectedUser.value.id, editingUserApiKey.value.id, updatePayload)
      success(t('userManagement.keyUpdated'))
    } else {
      const response = await usersStore.createApiKey(selectedUser.value.id, {
        name: userApiKeyForm.value.name,
        group_id: userApiKeyForm.value.group_id,
        rate_limit: userApiKeyForm.value.rate_limit ?? 0,
        concurrent_limit: userApiKeyForm.value.concurrent_limit,
        feature_settings: mergeChatPiiRedactionFeatureSettings(null, {
          enabled: userApiKeyForm.value.chat_pii_redaction_enabled,
          inject_model_instruction: userApiKeyForm.value.chat_pii_redaction_placeholder_notice,
        }),
      })
      newApiKey.value = response.key || ''
      showNewApiKeyDialog.value = true
      success(t('userManagement.keyCreated'))
    }
    await loadUserApiKeys(selectedUser.value.id)
    closeUserApiKeyFormDialog()
  } catch (err: unknown) {
    error(parseApiError(err, t('userManagement.unknownError')), editingUserApiKey.value ? t('userManagement.updateKeyFailed') : t('userManagement.createKeyFailed'))
  } finally {
    creatingApiKey.value = false
  }
}

async function revokeSelectedUserSession(sessionId: string) {
  if (!selectedUser.value) return
  sessionDialogActionLoading.value = sessionId
  try {
    await usersStore.revokeUserSession(selectedUser.value.id, sessionId)
    userSessions.value = userSessions.value.filter((session) => session.id !== sessionId)
    success(t('userManagement.deviceRevoked'))
  } catch (err) {
    error(parseApiError(err, t('userManagement.revokeDeviceFailed')))
  } finally {
    sessionDialogActionLoading.value = null
  }
}

async function revokeAllSelectedUserSessions() {
  if (!selectedUser.value) return
  sessionDialogActionLoading.value = 'all'
  try {
    const result = await usersStore.revokeAllUserSessions(selectedUser.value.id)
    userSessions.value = []
    success(result.revoked_count > 0 ? t('userManagement.devicesRevoked', { count: result.revoked_count }) : t('userManagement.noDevicesToRevoke'))
  } catch (err) {
    error(parseApiError(err, t('userManagement.revokeAllDevicesFailed')))
  } finally {
    sessionDialogActionLoading.value = null
  }
}

function selectApiKey() {
  apiKeyInput.value?.select()
}

async function copyApiKey() {
  await copyToClipboard(newApiKey.value)
}

async function closeNewApiKeyDialog() {
  showNewApiKeyDialog.value = false
  newApiKey.value = ''
}

async function deleteApiKey(apiKey: ApiKey) {
  const user = selectedUser.value
  if (!user) return

  const confirmed = await confirmDanger(
    t('userManagement.deleteKeyConfirm', { key: apiKey.key_display || '****' }),
    t('userManagement.deleteKeyTitle')
  )

  if (!confirmed) return

  try {
    await usersStore.deleteApiKey(user.id, apiKey.id)
    await loadUserApiKeys(user.id)
    success(t('userManagement.keyDeleted'))
  } catch (err: unknown) {
    error(parseApiError(err, t('userManagement.unknownError')), t('userManagement.deleteKeyFailed'))
  }
}

async function toggleLockApiKey(apiKey: ApiKey) {
  if (!selectedUser.value) return
  try {
    const response = await adminApi.toggleUserApiKeyLock(selectedUser.value.id, apiKey.id)
    // 更新本地状态
    const index = userApiKeys.value.findIndex(k => k.id === apiKey.id)
    if (index !== -1) {
      userApiKeys.value[index].is_locked = response.is_locked
    }
    success(response.message)
  } catch (err: unknown) {
    log.error('切换密钥锁定状态失败:', err)
    error(parseApiError(err, t('userManagement.operationFailed')), t('userManagement.lockToggleFailed'))
  }
}

async function copyFullKey(apiKey: ApiKey) {
  if (!selectedUser.value) return
  try {
    const response = await usersStore.getFullApiKey(selectedUser.value.id, apiKey.id)
    await copyToClipboard(response.key)
  } catch (err: unknown) {
    log.error('复制密钥失败:', err)
    error(parseApiError(err, t('userManagement.unknownError')), t('userManagement.copyKeyFailed'))
  }
}

function openWalletActionDialog(user: User) {
  const wallet = getUserWallet(user.id)
  if (!wallet) {
    error(t('userManagement.walletNotInitialized'))
    return
  }

  walletActionTarget.value = {
    user,
    wallet,
  }
  showWalletActionDialogState.value = true
}

function closeWalletActionDrawer() {
  showWalletActionDialogState.value = false
}

async function handleWalletDrawerChanged() {
  if (!walletActionTarget.value) return
  try {
    const latestWallet = await adminWalletApi.getWalletDetail(walletActionTarget.value.wallet.id)
    const userId = walletActionTarget.value.user.id
    userWalletMap.value[userId] = latestWallet
    walletActionTarget.value.wallet = latestWallet
  } catch (err) {
    log.error(t('userManagement.loadWalletsFailed'), err)
  }
}

async function deleteUser(user: User) {
  const confirmed = await confirmDanger(
    t('userManagement.deleteUserConfirm', { user: user.username }),
    t('userManagement.deleteUserTitle')
  )

  if (!confirmed) return

  try {
    await usersStore.deleteUser(user.id)
    success(t('userManagement.userDeleted'))
  } catch (err: unknown) {
    error(parseApiError(err, t('userManagement.unknownError')), t('userManagement.deleteUserFailed'))
  }
}
</script>
