<template>
  <TableCard :title="t('usageRecords.title')">
    <template #actions>
      <!-- 时间范围筛选 -->
      <TimeRangePicker
        v-model="timeRangeModel"
        :show-granularity="false"
        show-time
      />

      <!-- 分隔线 -->
      <div class="hidden sm:block h-4 w-px bg-border" />

      <!-- 通用搜索 -->
      <div class="relative">
        <Search class="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground z-10 pointer-events-none" />
        <Input
          id="usage-records-search"
          v-model="localSearch"
          :placeholder="isAdmin ? t('usageRecords.searchAdmin') : t('usageRecords.searchUser')"
          class="w-[7.5rem] sm:w-48 h-8 text-xs border-border/60 pl-8"
        />
      </div>

      <div class="contents md:hidden">
        <!-- 用户筛选（仅管理员可见） -->
        <ServerUserSelector
          v-if="isAdmin"
          class="flex-1 min-w-0 sm:flex-none sm:w-40"
          :model-value="filterUser"
          :initial-users="availableUsers"
          dropdown
          @update:model-value="$emit('update:filterUser', $event)"
        />

        <!-- Key 分组筛选（仅管理员可见） -->
        <Select
          v-if="isAdmin"
          :model-value="filterApiKeyGroup"
          @update:model-value="$emit('update:filterApiKeyGroup', $event)"
        >
          <SelectTrigger class="flex-1 min-w-0 sm:flex-none sm:w-36 h-8 text-xs border-border/60">
            <SelectValue :placeholder="t('usageRecords.keyGroup')" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__all__">
              {{ t('usageRecords.allKeyGroups') }}
            </SelectItem>
            <SelectItem
              v-for="group in availableApiKeyGroups"
              :key="group.id"
              :value="group.id"
            >
              {{ group.name }}
            </SelectItem>
          </SelectContent>
        </Select>

        <!-- 模型筛选 -->
        <Select
          :model-value="filterModel"
          @update:model-value="$emit('update:filterModel', $event)"
        >
          <SelectTrigger class="flex-1 min-w-0 sm:flex-none sm:w-40 h-8 text-xs border-border/60">
            <SelectValue :placeholder="t('usageRecords.model')" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__all__">
              {{ t('usageRecords.allModels') }}
            </SelectItem>
            <SelectItem
              v-for="model in availableModels"
              :key="model"
              :value="model"
            >
              {{ model.replace('claude-', '') }}
            </SelectItem>
          </SelectContent>
        </Select>

        <!-- 提供商筛选（仅管理员可见） -->
        <Select
          v-if="isAdmin"
          :model-value="filterProvider"
          @update:model-value="$emit('update:filterProvider', $event)"
        >
          <SelectTrigger class="flex-1 min-w-0 sm:flex-none sm:w-32 h-8 text-xs border-border/60">
            <SelectValue :placeholder="t('usageRecords.provider')" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__all__">
              {{ t('usageRecords.allProviders') }}
            </SelectItem>
            <SelectItem
              v-for="provider in availableProviders"
              :key="provider"
              :value="provider"
            >
              {{ provider }}
            </SelectItem>
          </SelectContent>
        </Select>

        <!-- API格式筛选 -->
        <Select
          :model-value="filterApiFormat"
          @update:model-value="$emit('update:filterApiFormat', $event)"
        >
          <SelectTrigger class="flex-1 min-w-0 sm:flex-none sm:w-32 h-8 text-xs border-border/60">
            <SelectValue :placeholder="t('usageRecords.format')" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__all__">
              {{ t('usageRecords.allFormats') }}
            </SelectItem>
            <SelectItem
              v-for="format in availableApiFormats"
              :key="format.value"
              :value="format.value"
            >
              {{ format.label }}
            </SelectItem>
          </SelectContent>
        </Select>

        <!-- 状态筛选 -->
        <Select
          :model-value="filterStatus"
          @update:model-value="$emit('update:filterStatus', $event)"
        >
          <SelectTrigger class="flex-1 min-w-0 sm:flex-none sm:w-28 h-8 text-xs border-border/60">
            <SelectValue :placeholder="t('usageRecords.status')" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__all__">
              {{ t('usageRecords.allStatuses') }}
            </SelectItem>
            <SelectItem value="stream">
              {{ t('usageRecords.streaming') }}
            </SelectItem>
            <SelectItem value="standard">
              {{ t('usageRecords.standard') }}
            </SelectItem>
            <SelectItem value="active">
              {{ t('usageRecords.active') }}
            </SelectItem>
            <SelectItem value="failed">
              {{ t('usageRecords.failed') }}
            </SelectItem>
            <SelectItem value="cancelled">
              {{ t('usageRecords.cancelled') }}
            </SelectItem>
            <SelectItem value="has_retry">
              {{ t('usageRecords.hasRetry') }}
            </SelectItem>
            <SelectItem value="has_fallback">
              {{ t('usageRecords.hasFallback') }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <!-- 分隔线 -->
      <div class="hidden sm:block h-4 w-px bg-border" />

      <!-- Key 分组筛选（桌面端） -->
      <Select
        v-if="isAdmin"
        :model-value="filterApiKeyGroup"
        @update:model-value="$emit('update:filterApiKeyGroup', $event)"
      >
        <SelectTrigger class="hidden md:flex w-36 h-8 text-xs border-border/60">
          <SelectValue :placeholder="t('usageRecords.keyGroup')" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="__all__">
            {{ t('usageRecords.allKeyGroups') }}
          </SelectItem>
          <SelectItem
            v-for="group in availableApiKeyGroups"
            :key="group.id"
            :value="group.id"
          >
            {{ group.name }}
          </SelectItem>
        </SelectContent>
      </Select>

      <!-- 分隔线 -->
      <div
        v-if="isAdmin"
        class="hidden sm:block h-4 w-px bg-border"
      />

      <!-- 列显示配置（桌面端） -->
      <MultiSelect
        v-model="visibleColumnIds"
        :options="columnSelectOptions"
        :placeholder="t('usageRecords.visibleColumns')"
        trigger-class="hidden md:flex w-40 h-8 text-xs border-border/60"
        dropdown-min-width="14rem"
        :searchable="false"
      />

      <!-- 分隔线 -->
      <div class="hidden sm:block h-4 w-px bg-border" />

      <!-- 自动刷新按钮 -->
      <Button
        variant="ghost"
        size="icon"
        class="h-8 w-8"
        :class="autoRefresh ? 'text-primary' : ''"
        :title="autoRefresh ? t('usageRecords.disableAutoRefresh') : t('usageRecords.enableAutoRefresh')"
        @click="$emit('update:autoRefresh', !autoRefresh)"
      >
        <RefreshCcw
          class="w-3.5 h-3.5"
          :class="autoRefresh ? 'animate-spin' : ''"
        />
      </Button>
    </template>

    <!-- 移动端卡片视图 -->
    <div class="md:hidden">
      <div
        v-if="records.length === 0"
        class="text-center py-12 text-muted-foreground"
      >
        {{ t('usageRecords.empty') }}
      </div>
      <div
        v-for="record in records"
        v-else
        :key="record.id"
        class="border-b border-border/40 py-2.5 px-2"
        :class="isAdmin ? 'cursor-pointer active:bg-muted/30 transition-colors' : ''"
        @click="isAdmin && emit('showDetail', record.id)"
      >
        <!-- 第一行：模型 + 费用 -->
        <div class="flex items-center justify-between gap-2">
          <div class="min-w-0 flex-1">
            <span class="text-sm font-medium truncate block">{{ record.model }}</span>
            <span
              v-if="getActualModel(record)"
              class="text-[11px] text-muted-foreground truncate block"
            >-> {{ getActualModel(record) }}</span>
            <span
              v-if="getReasoningEffort(record)"
              data-usage-model-badge="reasoning"
              class="mt-0.5 inline-flex rounded border border-border/60 px-1 py-0.5 text-[10px] leading-none text-muted-foreground"
            >{{ getReasoningEffort(record) }}</span>
          </div>
          <div
            class="flex flex-col items-end flex-shrink-0"
            :title="getRecordCostTitle(record)"
          >
            <span class="text-xs text-primary font-medium">
              {{ t('usageRecords.officialCost', { amount: formatCurrency(getOfficialCost(record)) }) }}
            </span>
            <span
              v-for="line in getChargeLines(record)"
              :key="line.label"
              class="text-[10px] text-muted-foreground"
            >
              {{ t('usageRecords.deductedCost', { label: formatChargeLabel(line.label), amount: formatCurrency(line.amount), multiplier: formatChargeFactor(record, line) }) }}
            </span>
            <span
              v-if="hasModerationCost(record)"
              class="text-[10px] text-muted-foreground"
            >{{ t('usageRecords.modelCharge', { amount: formatCurrency(getModelCost(record)) }) }}</span>
            <span
              v-if="hasModerationCost(record)"
              class="text-[10px] text-muted-foreground"
            >{{ t('usageRecords.moderationCost', { amount: formatCurrency(getModerationCost(record)) }) }}</span>
            <span
              v-if="showActualCost && hasPlatformCost(record)"
              class="text-[10px] text-muted-foreground"
            >{{ t('usageRecords.platformCostShort', { amount: formatCurrency(getPlatformCost(record)) }) }}</span>
            <span
              v-if="showActualCost && hasPlatformCost(record)"
              class="text-[10px] text-muted-foreground"
            >{{ formatCostMultiplier(record) }}</span>
          </div>
        </div>

        <!-- 第二行：状态 | 时间 | API格式 | 耗时 | Tokens -->
        <div class="flex items-center justify-between text-[11px] text-muted-foreground mt-1 leading-4">
          <div class="flex items-center gap-1.5">
            <!-- 状态 Badge -->
            <Badge
              v-if="isUsageRecordFailed(record)"
              variant="destructive"
              class="whitespace-nowrap text-[10px] px-1.5 h-4 leading-4 inline-flex items-center"
            >
              {{ t('usageRecords.failed') }}
            </Badge>
            <Badge
              v-else-if="getDisplayStatus(record) === 'pending'"
              variant="outline"
              class="whitespace-nowrap animate-pulse border-muted-foreground/30 text-muted-foreground text-[10px] px-1.5 h-4 leading-4 inline-flex items-center"
            >
              {{ t('usageRecords.pending') }}
            </Badge>
            <Badge
              v-else-if="getDisplayStatus(record) === 'streaming'"
              variant="outline"
              class="whitespace-nowrap animate-pulse border-primary/50 text-primary text-[10px] px-1.5 h-4 leading-4 inline-flex items-center"
            >
              {{ t('usageRecords.streamingShort') }}
            </Badge>
            <Badge
              v-else-if="record.status === 'cancelled'"
              variant="outline"
              class="whitespace-nowrap border-amber-500/50 text-amber-600 dark:text-amber-400 text-[10px] px-1.5 h-4 leading-4 inline-flex items-center"
            >
              {{ t('usageRecords.cancelled') }}
            </Badge>
            <Badge
              v-else-if="getStreamModeSegments(record).hasConversion"
              :variant="getStreamModeSegments(record).client === '流式' ? 'secondary' : 'outline'"
              :class="getStreamModeSegments(record).client === '流式'
                ? 'whitespace-nowrap text-[10px] px-1.5 h-4 leading-4 inline-flex items-center gap-0.5'
                : 'whitespace-nowrap border-border/60 text-muted-foreground text-[10px] px-1.5 h-4 leading-4 inline-flex items-center gap-0.5'"
            >
              <span>{{ translateStreamMode(getStreamModeSegments(record).client) }}</span>
              <span class="opacity-60">→</span>
              <span>{{ translateStreamMode(getStreamModeSegments(record).upstream) }}</span>
            </Badge>
            <Badge
              v-else
              :variant="getUpstreamStream(record) ? 'secondary' : 'outline'"
              :class="getUpstreamStream(record)
                ? 'whitespace-nowrap text-[10px] px-1.5 h-4 leading-4 inline-flex items-center'
                : 'whitespace-nowrap border-border/60 text-muted-foreground text-[10px] px-1.5 h-4 leading-4 inline-flex items-center'"
            >
              {{ translateStreamMode(getStreamModeLabel(record)) }}
            </Badge>
            <span class="text-muted-foreground/50">|</span>
            <div class="flex flex-col leading-tight tabular-nums">
              <span class="text-[11px] text-foreground whitespace-nowrap">
                {{ formatRecordTime(record.created_at) }}
              </span>
              <span class="text-[10px] text-muted-foreground whitespace-nowrap">
                {{ formatRecordDate(record.created_at) }}
              </span>
            </div>
            <template v-if="record.api_format">
              <span class="text-muted-foreground/50">|</span>
              <span>{{ formatApiFormat(record.api_format) }}</span>
            </template>
          </div>
          <div class="flex items-center gap-1.5">
            <!-- 耗时 -->
            <span
              v-if="getDisplayStatus(record) === 'pending' || getDisplayStatus(record) === 'streaming'"
              class="tabular-nums whitespace-nowrap"
            >
              <span>{{ formatRecordDurationSeconds(record.first_byte_time_ms) }}</span>
              <span class="text-muted-foreground"> / </span>
              <ElapsedTimeText
                class="text-primary"
                :created-at="record.created_at"
                :status="getDisplayStatus(record)"
                :response-time-ms="record.response_time_ms ?? null"
              />
            </span>
            <span
              v-else-if="record.response_time_ms != null || record.first_byte_time_ms != null"
              class="flex flex-col items-end tabular-nums leading-3 shrink-0"
              :title="getRecordPerformanceTitle(record)"
            >
              <span class="whitespace-nowrap">{{ formatRecordLatencyPair(record) }}</span>
              <span class="text-muted-foreground tabular-nums whitespace-nowrap">
                {{ formatOutputRate(getRecordDisplayOutputRate(record)) }}
              </span>
            </span>
            <span
              v-else
              class="tabular-nums"
            >-</span>
            <span class="text-muted-foreground/50">|</span>
            <!-- Tokens -->
            <span>{{ formatTokens(getRecordEffectiveInputTokens(record)) }}/{{ formatTokens(record.output_tokens || 0) }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- 桌面端表格视图 -->
    <Table
      class="hidden md:table table-fixed w-full"
      :class="[desktopTableMinWidthClass]"
    >
      <colgroup v-if="isAdmin">
        <col v-if="isColumnVisible('time')" :style="{ width: desktopColumnWidths.time }">
        <col v-if="isColumnVisible('user')" :style="{ width: desktopColumnWidths.user }">
        <col v-if="isColumnVisible('model')" :style="{ width: desktopColumnWidths.model }">
        <col v-if="isColumnVisible('provider')" :style="{ width: desktopColumnWidths.provider }">
        <col v-if="isColumnVisible('api_format')" :style="{ width: desktopColumnWidths.api_format }">
        <col v-if="isColumnVisible('status')" :style="{ width: desktopColumnWidths.status }">
        <col v-if="isColumnVisible('tokens')" :style="{ width: desktopColumnWidths.tokens }">
        <col v-if="isColumnVisible('cost')" :style="{ width: desktopColumnWidths.cost }">
        <col v-if="isColumnVisible('performance')" :style="{ width: desktopColumnWidths.performance }">
        <col v-if="isColumnVisible('client_family')" :style="{ width: desktopColumnWidths.client_family }">
        <col v-if="isColumnVisible('client_ip')" :style="{ width: desktopColumnWidths.client_ip }">
        <col v-if="isColumnVisible('user_agent')" :style="{ width: desktopColumnWidths.user_agent }">
      </colgroup>
      <colgroup v-else>
        <col v-if="isColumnVisible('time')" :style="{ width: desktopColumnWidths.time }">
        <col v-if="isColumnVisible('key')" :style="{ width: desktopColumnWidths.key }">
        <col v-if="isColumnVisible('model')" :style="{ width: desktopColumnWidths.model }">
        <col v-if="isColumnVisible('api_format')" :style="{ width: desktopColumnWidths.api_format }">
        <col v-if="isColumnVisible('status')" :style="{ width: desktopColumnWidths.status }">
        <col v-if="isColumnVisible('tokens')" :style="{ width: desktopColumnWidths.tokens }">
        <col v-if="isColumnVisible('cost')" :style="{ width: desktopColumnWidths.cost }">
        <col v-if="isColumnVisible('performance')" :style="{ width: desktopColumnWidths.performance }">
        <col v-if="isColumnVisible('client_family')" :style="{ width: desktopColumnWidths.client_family }">
        <col v-if="isColumnVisible('client_ip')" :style="{ width: desktopColumnWidths.client_ip }">
        <col v-if="isColumnVisible('user_agent')" :style="{ width: desktopColumnWidths.user_agent }">
      </colgroup>
      <TableHeader>
        <TableRow class="border-b border-border/60 hover:bg-transparent">
          <SortableTableHead
            v-if="isColumnVisible('time')"
            class="h-12 font-semibold"
            :sortable="false"
            resize-column-key="time"
            :resizable="true"
            @resize-start="handleUsageColumnResizeStart"
          >
            {{ t('usageRecords.time') }}
          </SortableTableHead>
          <SortableTableHead
            v-if="isAdmin && isColumnVisible('user')"
            class="h-12 font-semibold"
            column-key="user"
            :sortable="false"
            resize-column-key="user"
            :resizable="true"
            :filter-active="filterUser !== '__all__'"
            :filter-title="t('usageRecords.filterUser')"
            filter-content-class="w-64 p-1 rounded-2xl border-border bg-card text-foreground shadow-2xl backdrop-blur-xl"
            @resize-start="handleUsageColumnResizeStart"
          >
            {{ t('usageRecords.user') }}
            <template #filter="{ close }">
              <ServerUserSelector
                :model-value="filterUser"
                :initial-users="availableUsers"
                @update:model-value="$emit('update:filterUser', $event)"
                @select="close"
              />
            </template>
          </SortableTableHead>
          <SortableTableHead
            v-if="!isAdmin && isColumnVisible('key')"
            class="h-12 font-semibold"
            :sortable="false"
            resize-column-key="key"
            :resizable="true"
            @resize-start="handleUsageColumnResizeStart"
          >
            {{ t('usageRecords.key') }}
          </SortableTableHead>
          <SortableTableHead
            v-if="isColumnVisible('model')"
            class="h-12 font-semibold"
            column-key="model"
            :sortable="false"
            resize-column-key="model"
            :resizable="true"
            :filter-active="filterModel !== '__all__'"
            :filter-title="t('usageRecords.filterModel')"
            filter-content-class="w-64 p-1 rounded-2xl border-border bg-card text-foreground shadow-2xl backdrop-blur-xl"
            @resize-start="handleUsageColumnResizeStart"
          >
            {{ t('usageRecords.model') }}
            <template #filter="{ close }">
              <TableFilterMenu
                :model-value="filterModel"
                :options="modelFilterOptions"
                @update:model-value="$emit('update:filterModel', $event)"
                @select="close"
              />
            </template>
          </SortableTableHead>
          <SortableTableHead
            v-if="isAdmin && isColumnVisible('provider')"
            class="h-12 font-semibold"
            column-key="provider"
            :sortable="false"
            resize-column-key="provider"
            :resizable="true"
            :filter-active="filterProvider !== '__all__'"
            :filter-title="t('usageRecords.filterProvider')"
            filter-content-class="w-48 p-1 rounded-2xl border-border bg-card text-foreground shadow-2xl backdrop-blur-xl"
            @resize-start="handleUsageColumnResizeStart"
          >
            {{ t('usageRecords.provider') }}
            <template #filter="{ close }">
              <TableFilterMenu
                :model-value="filterProvider"
                :options="providerFilterOptions"
                @update:model-value="$emit('update:filterProvider', $event)"
                @select="close"
              />
            </template>
          </SortableTableHead>
          <SortableTableHead
            v-if="isColumnVisible('api_format')"
            class="h-12 font-semibold"
            column-key="api_format"
            :sortable="false"
            resize-column-key="api_format"
            :resizable="true"
            :filter-active="filterApiFormat !== '__all__'"
            :filter-title="t('usageRecords.filterApiFormat')"
            filter-content-class="w-72 p-1 rounded-2xl border-border bg-card text-foreground shadow-2xl backdrop-blur-xl"
            @resize-start="handleUsageColumnResizeStart"
          >
            {{ t('usageRecords.apiFormat') }}
            <template #filter="{ close }">
              <TableFilterMenu
                :model-value="filterApiFormat"
                :options="apiFormatFilterOptions"
                @update:model-value="$emit('update:filterApiFormat', $event)"
                @select="close"
              />
            </template>
          </SortableTableHead>
          <SortableTableHead
            v-if="isColumnVisible('status')"
            class="h-12 font-semibold text-center"
            column-key="status"
            :sortable="false"
            align="center"
            resize-column-key="status"
            :resizable="true"
            :filter-active="filterStatus !== '__all__'"
            :filter-title="t('usageRecords.filterType')"
            filter-content-class="w-44 p-1 rounded-2xl border-border bg-card text-foreground shadow-2xl backdrop-blur-xl"
            @resize-start="handleUsageColumnResizeStart"
          >
            {{ t('usageRecords.type') }}
            <template #filter="{ close }">
              <TableFilterMenu
                :model-value="filterStatus"
                :options="statusFilterOptions"
                @update:model-value="$emit('update:filterStatus', $event)"
                @select="close"
              />
            </template>
          </SortableTableHead>
          <SortableTableHead
            v-if="isColumnVisible('tokens')"
            class="h-12 font-semibold text-center"
            :sortable="false"
            align="center"
            resize-column-key="tokens"
            :resizable="true"
            @resize-start="handleUsageColumnResizeStart"
          >
            Tokens
          </SortableTableHead>
          <SortableTableHead
            v-if="isColumnVisible('cost')"
            class="h-12 font-semibold text-right"
            :sortable="false"
            align="right"
            resize-column-key="cost"
            :resizable="true"
            @resize-start="handleUsageColumnResizeStart"
          >
            {{ t('usageRecords.costMultiplier') }}
          </SortableTableHead>
          <SortableTableHead
            v-if="isColumnVisible('performance')"
            class="h-12 font-semibold text-right"
            :sortable="false"
            align="right"
            resize-column-key="performance"
            :resizable="true"
            @resize-start="handleUsageColumnResizeStart"
          >
            <div class="flex flex-col items-end text-xs gap-0.5">
              <span class="whitespace-nowrap">{{ t('usageRecords.firstByteTotal') }}</span>
              <span class="text-muted-foreground font-normal">{{ t('usageRecords.outputSpeed') }}</span>
            </div>
          </SortableTableHead>
          <SortableTableHead
            v-if="isColumnVisible('client_family')"
            class="h-12 font-semibold"
            column-key="client_family"
            :sortable="false"
            resize-column-key="client_family"
            :resizable="true"
            :filter-active="filterClientFamily !== '__all__'"
            :filter-title="t('usageRecords.filterClient')"
            filter-content-class="w-44 p-1 rounded-2xl border-border bg-card text-foreground shadow-2xl backdrop-blur-xl"
            @resize-start="handleUsageColumnResizeStart"
          >
            {{ t('usageRecords.client') }}
            <template #filter="{ close }">
              <TableFilterMenu
                :model-value="filterClientFamily"
                :options="clientFamilyFilterOptions"
                @update:model-value="$emit('update:filterClientFamily', $event)"
                @select="close"
              />
            </template>
          </SortableTableHead>
          <SortableTableHead
            v-if="isColumnVisible('client_ip')"
            class="h-12 font-semibold"
            :sortable="false"
            resize-column-key="client_ip"
            :resizable="true"
            @resize-start="handleUsageColumnResizeStart"
          >
            {{ t('usageRecords.ipAddress') }}
          </SortableTableHead>
          <SortableTableHead
            v-if="isColumnVisible('user_agent')"
            class="h-12 font-semibold"
            :sortable="false"
            resize-column-key="user_agent"
            :resizable="true"
            @resize-start="handleUsageColumnResizeStart"
          >
            User-Agent
          </SortableTableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableRow v-if="records.length === 0">
          <TableCell
            :colspan="visibleColumnCount"
            class="text-center py-12 text-muted-foreground"
          >
            {{ t('usageRecords.empty') }}
          </TableCell>
        </TableRow>
        <TableRow
          v-for="record in records"
          v-else
          :key="record.id"
          :class="isAdmin ? 'cursor-pointer border-b border-border/40 hover:bg-muted/30 transition-colors h-[72px]' : 'border-b border-border/40 hover:bg-muted/30 transition-colors h-[72px]'"
          @mousedown="handleRowMouseDown($event, record.id)"
          @click="handleRowClick($event, record.id)"
        >
          <TableCell v-if="isColumnVisible('time')" class="py-4 align-top">
            <div class="flex flex-col gap-0.5 leading-tight">
              <span class="text-xs text-foreground tabular-nums whitespace-nowrap">
                {{ formatRecordTime(record.created_at) }}
              </span>
              <span class="text-[11px] text-muted-foreground tabular-nums whitespace-nowrap">
                {{ formatRecordDate(record.created_at) }}
              </span>
            </div>
          </TableCell>
          <TableCell
            v-if="isAdmin && isColumnVisible('user')"
            class="py-4 align-top"
            :title="getUsageRecordUserDisplay(record)"
          >
            <div class="flex flex-col text-xs gap-0.5">
              <span class="break-words leading-4">
                {{ getUsageRecordUserDisplay(record) }}
              </span>
              <span
                v-if="record.api_key?.name"
                class="break-words text-muted-foreground leading-4"
                :title="record.api_key.name"
              >
                {{ record.api_key.name }}
              </span>
            </div>
          </TableCell>
          <!-- 用户页面的密钥列 -->
          <TableCell
            v-if="!isAdmin && isColumnVisible('key')"
            class="py-4 align-top"
            :title="record.api_key?.name || '-'"
          >
            <div class="flex flex-col text-xs gap-0.5">
              <span class="break-words leading-4">{{ record.api_key?.name || '-' }}</span>
              <span
                v-if="record.api_key?.display"
                class="break-all text-muted-foreground leading-4"
              >
                {{ record.api_key.display }}
              </span>
            </div>
          </TableCell>
          <TableCell
            v-if="isColumnVisible('model')"
            class="font-medium py-4 align-top"
            :title="getModelTooltip(record)"
          >
            <div
              v-if="getActualModel(record)"
              class="flex flex-col text-xs gap-0.5"
            >
              <div class="flex min-w-0 items-center gap-1">
                <span class="min-w-0 break-all leading-4">{{ record.model }}</span>
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  viewBox="0 0 20 20"
                  fill="currentColor"
                  class="w-3 h-3 text-muted-foreground flex-shrink-0"
                >
                  <path
                    fill-rule="evenodd"
                    d="M3 10a.75.75 0 01.75-.75h10.638L10.23 5.29a.75.75 0 111.04-1.08l5.5 5.25a.75.75 0 010 1.08l-5.5 5.25a.75.75 0 11-1.04-1.08l4.158-3.96H3.75A.75.75 0 013 10z"
                    clip-rule="evenodd"
                  />
                </svg>
              </div>
              <span class="break-all text-muted-foreground leading-4">{{ getActualModel(record) }}</span>
            </div>
            <span
              v-else
              class="block break-all leading-4"
            >{{ record.model }}</span>
            <span
              v-if="getReasoningEffort(record)"
              data-usage-model-badge="reasoning"
              class="mt-1 inline-flex rounded border border-border/60 px-1 py-0.5 text-[10px] font-normal leading-none text-muted-foreground"
            >{{ getReasoningEffort(record) }}</span>
          </TableCell>
          <TableCell
            v-if="isAdmin && isColumnVisible('provider')"
            class="py-4 align-top"
          >
            <div class="flex min-w-0 items-center gap-1">
              <div class="flex min-w-0 flex-col text-xs gap-0.5">
                <div
                  v-if="getProviderRouteDisplay(record).length > 1"
                  class="flex min-w-0 flex-wrap items-center gap-x-1 gap-y-0.5"
                  :title="getProviderRouteDisplay(record).join(' → ')"
                >
                  <template
                    v-for="(provider, routeIndex) in getProviderRouteDisplay(record)"
                    :key="`${record.id}-provider-route-${routeIndex}`"
                  >
                    <span class="break-words leading-4">{{ provider }}</span>
                    <span
                      v-if="routeIndex < getProviderRouteDisplay(record).length - 1"
                      class="text-amber-600 dark:text-amber-400"
                    >→</span>
                  </template>
                </div>
                <span
                  v-else
                  class="break-words leading-4"
                >{{ getProviderRouteDisplay(record)[0] || record.provider }}</span>
                <button
                  v-if="getProviderAccountDisplay(record)"
                  type="button"
                  class="break-words text-left text-muted-foreground transition-colors hover:text-foreground"
                  :title="t('usageRecords.clickToCopy', { value: getProviderAccountDisplay(record) })"
                  @click.stop="copyProviderAccountDisplay(record)"
                >
                  {{ getProviderAccountDisplay(record) }}
                  <span
                    v-if="record.rate_multiplier && record.rate_multiplier !== 1.0"
                    class="text-foreground/60"
                  >({{ record.rate_multiplier }}x)</span>
                </button>
              </div>
              <!-- 故障转移图标（优先显示） -->
              <svg
                v-if="hasProviderTransfer(record)"
                xmlns="http://www.w3.org/2000/svg"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                class="w-3.5 h-3.5 text-amber-600 dark:text-amber-400 flex-shrink-0"
                :title="getProviderTransferTitle(record)"
              >
                <path d="m16 3 4 4-4 4" />
                <path d="M20 7H4" />
                <path d="m8 21-4-4 4-4" />
                <path d="M4 17h16" />
              </svg>
              <!-- 重试图标（仅在无故障转移时显示） -->
              <svg
                v-else-if="record.has_retry"
                xmlns="http://www.w3.org/2000/svg"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                class="w-3.5 h-3.5 text-primary flex-shrink-0"
                :title="t('usageRecords.cacheAffinityRetry')"
              >
                <path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16" />
                <path d="M21 21v-5h-5" />
                <path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
                <path d="M3 3v5h5" />
              </svg>
            </div>
          </TableCell>
          <TableCell
            v-if="isColumnVisible('api_format')"
            class="py-4"
            :title="getApiFormatTooltip(record)"
          >
            <!-- 有格式转换或同族格式差异：两行显示 -->
            <div
              v-if="shouldShowFormatConversion(record)"
              class="flex flex-col text-xs gap-0.5"
            >
              <div class="flex items-center gap-1 whitespace-nowrap">
                <span>{{ formatApiFormat(record.api_format!) }}</span>
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  viewBox="0 0 20 20"
                  fill="currentColor"
                  class="w-3 h-3 text-muted-foreground flex-shrink-0"
                >
                  <path
                    fill-rule="evenodd"
                    d="M3 10a.75.75 0 01.75-.75h10.638L10.23 5.29a.75.75 0 111.04-1.08l5.5 5.25a.75.75 0 010 1.08l-5.5 5.25a.75.75 0 11-1.04-1.08l4.158-3.96H3.75A.75.75 0 013 10z"
                    clip-rule="evenodd"
                  />
                </svg>
              </div>
              <span class="text-muted-foreground whitespace-nowrap">{{ formatApiFormat(record.endpoint_api_format!) }}</span>
            </div>
            <!-- 无格式转换：单行显示 -->
            <span
              v-else-if="record.api_format"
              class="text-xs whitespace-nowrap"
            >{{ formatApiFormat(record.api_format) }}</span>
            <span
              v-else
              class="text-muted-foreground text-xs"
            >-</span>
          </TableCell>
          <TableCell v-if="isColumnVisible('status')" class="text-center py-4 align-top">
            <!-- 优先显示请求状态 -->
            <Badge
              v-if="getDisplayStatus(record) === 'pending'"
              variant="outline"
              class="whitespace-nowrap animate-pulse border-muted-foreground/30 text-muted-foreground"
            >
              {{ t('usageRecords.pendingLong') }}
            </Badge>
            <Badge
              v-else-if="getDisplayStatus(record) === 'streaming'"
              variant="outline"
              class="whitespace-nowrap animate-pulse border-primary/50 text-primary"
            >
              {{ t('usageRecords.streamingLong') }}
            </Badge>
            <Badge
              v-else-if="isUsageRecordFailed(record)"
              variant="destructive"
              class="whitespace-nowrap"
            >
              {{ t('usageRecords.failed') }}
            </Badge>
            <Badge
              v-else-if="record.status === 'cancelled'"
              variant="outline"
              class="whitespace-nowrap border-amber-500/50 text-amber-600 dark:text-amber-400"
            >
              {{ t('usageRecords.cancelledLong') }}
            </Badge>
            <Badge
              v-else-if="getStreamModeSegments(record).hasConversion"
              :variant="getStreamModeSegments(record).client === '流式' ? 'secondary' : 'outline'"
              :class="getStreamModeSegments(record).client === '流式'
                ? 'whitespace-nowrap inline-flex items-center gap-1'
                : 'whitespace-nowrap border-border/60 text-muted-foreground inline-flex items-center gap-1'"
            >
              <span>{{ translateStreamMode(getStreamModeSegments(record).client) }}</span>
              <span class="opacity-60">→</span>
              <span>{{ translateStreamMode(getStreamModeSegments(record).upstream) }}</span>
            </Badge>
            <Badge
              v-else
              :variant="getUpstreamStream(record) ? 'secondary' : 'outline'"
              :class="getUpstreamStream(record)
                ? 'whitespace-nowrap'
                : 'whitespace-nowrap border-border/60 text-muted-foreground'"
            >
              {{ translateStreamMode(getStreamModeLabel(record)) }}
            </Badge>
          </TableCell>
          <TableCell v-if="isColumnVisible('tokens')" class="py-4 align-top">
            <div class="grid w-full min-w-0 grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] gap-x-1 text-xs leading-tight tabular-nums">
              <span class="justify-self-end whitespace-nowrap text-right">
                {{ formatTokens(getRecordEffectiveInputTokens(record)) }}
              </span>
              <span class="justify-self-center text-muted-foreground">
                /
              </span>
              <span class="justify-self-start whitespace-nowrap text-left">
                {{ formatTokens(record.output_tokens || 0) }}
              </span>
            </div>
            <div class="mt-0.5 grid w-full min-w-0 grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] gap-x-1 text-xs leading-tight tabular-nums text-muted-foreground">
              <span
                class="justify-self-end whitespace-nowrap text-right"
                :class="[
                  hasPositiveTokens(getRecordCacheReadTokens(record)) ? 'text-foreground/70' : ''
                ]"
              >
                {{ formatOptionalTokens(getRecordCacheReadTokens(record)) }}
              </span>
              <span class="justify-self-center">
                /
              </span>
              <span
                class="justify-self-start whitespace-nowrap text-left"
                :class="[
                  hasPositiveTokens(getRecordCacheCreationTokens(record)) ? 'text-foreground/70' : ''
                ]"
              >
                {{ formatOptionalTokens(getRecordCacheCreationTokens(record)) }}
              </span>
            </div>
          </TableCell>
          <TableCell
            v-if="isColumnVisible('cost')"
            class="text-right py-4 align-top"
            :title="getRecordCostTitle(record)"
          >
            <div class="flex flex-col items-end text-xs gap-0.5">
              <span class="text-primary font-medium">
                {{ t('usageRecords.officialCost', { amount: formatCurrency(getOfficialCost(record)) }) }}
              </span>
              <span
                v-for="line in getChargeLines(record)"
                :key="line.label"
                class="text-muted-foreground"
              >
                {{ t('usageRecords.deductedCost', { label: formatChargeLabel(line.label), amount: formatCurrency(line.amount), multiplier: formatChargeFactor(record, line) }) }}
              </span>
              <span
                v-if="hasModerationCost(record)"
                class="text-muted-foreground"
              >
                {{ t('usageRecords.modelCharge', { amount: formatCurrency(getModelCost(record)) }) }}
              </span>
              <span
                v-if="hasModerationCost(record)"
                class="text-muted-foreground"
              >
                {{ t('usageRecords.moderationCost', { amount: formatCurrency(getModerationCost(record)) }) }}
              </span>
              <span
                v-if="showActualCost && hasPlatformCost(record)"
                class="text-muted-foreground"
              >
                {{ t('usageRecords.platformCostShort', { amount: formatCurrency(getPlatformCost(record)) }) }}
              </span>
              <span
                v-if="showActualCost && hasPlatformCost(record)"
                class="text-[11px] text-muted-foreground"
              >
                {{ formatCostMultiplier(record) }}
              </span>
            </div>
          </TableCell>
          <TableCell v-if="isColumnVisible('performance')" class="text-right py-4 align-top">
            <!-- pending/streaming 状态：首字与动态总耗时保留在同一行 -->
            <div
              v-if="getDisplayStatus(record) === 'pending' || getDisplayStatus(record) === 'streaming'"
              class="flex flex-col items-end text-xs gap-0.5"
            >
              <span class="tabular-nums whitespace-nowrap">
                <span>{{ formatRecordDurationSeconds(record.first_byte_time_ms) }}</span>
                <span class="text-muted-foreground"> / </span>
                <ElapsedTimeText
                  class="text-primary"
                  :created-at="record.created_at"
                  :status="getDisplayStatus(record)"
                  :response-time-ms="record.response_time_ms ?? null"
                />
              </span>
            </div>
            <!-- 已完成状态：首字 + 总耗时 -->
            <div
              v-else-if="record.response_time_ms != null || record.first_byte_time_ms != null"
              class="flex flex-col items-end text-xs gap-0.5"
              :title="getRecordPerformanceTitle(record)"
            >
              <span class="tabular-nums whitespace-nowrap">{{ formatRecordLatencyPair(record) }}</span>
              <span class="text-muted-foreground tabular-nums whitespace-nowrap">
                {{ formatOutputRate(getRecordDisplayOutputRate(record)) }}
              </span>
            </div>
            <span
              v-else
              class="text-muted-foreground"
            >-</span>
          </TableCell>
          <TableCell
            v-if="isColumnVisible('client_family')"
            class="py-4 text-xs align-top"
            :title="formatClientFamily(record.client_family)"
          >
            <Badge
              variant="outline"
              class="w-fit max-w-full border-border/60 text-muted-foreground"
            >
              <span class="truncate">{{ formatClientFamily(record.client_family) }}</span>
            </Badge>
          </TableCell>
          <TableCell
            v-if="isColumnVisible('client_ip')"
            class="py-4 text-xs break-all align-top"
            :title="record.client_ip || '-'"
          >
            {{ record.client_ip || '-' }}
          </TableCell>
          <TableCell
            v-if="isColumnVisible('user_agent')"
            class="py-4 text-xs break-all align-top"
            :title="record.user_agent || '-'"
          >
            {{ formatUserAgent(record.user_agent) }}
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>

    <!-- 分页控件 -->
    <template #pagination>
      <Pagination
        v-if="totalRecords > 0"
        :current="currentPage"
        :total="totalRecords"
        :page-size="pageSize"
        :page-size-options="pageSizeOptions"
        cache-key="usage-records-page-size"
        @update:current="$emit('update:currentPage', $event)"
        @update:page-size="$emit('update:pageSize', $event)"
      />
    </template>
  </TableCard>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDebounceFn, useLocalStorage } from '@vueuse/core'
import {
  TableCard,
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
  TableCell,
  Pagination,
  SortableTableHead,
  TableFilterMenu,
} from '@/components/ui'
import { RefreshCcw, Search } from 'lucide-vue-next'
import { formatTokens, formatCurrency } from '@/utils/format'
import { getCacheCreationTokens, getCacheReadTokens, getEffectiveInputTokens } from '../token-normalization'
import {
  formatOutputRate,
  formatOutputRateValue,
  getDisplayOutputRate,
  getGenerationTimeMs,
} from '../performance'
import {
  formatUsageStreamLabel,
  isUsageRecordFailed,
  isUsageUpstreamStream,
  resolveDisplayRequestStatus,
  resolveUsageStreamLabelSegments
} from '../utils/status'
import { useRowClick } from '@/composables/useRowClick'
import { useClipboard } from '@/composables/useClipboard'
import { useResizableTableColumns, type ResizableTableColumn } from '@/composables/useResizableTableColumns'
import { formatApiFormat } from '@/api/endpoints/types/api-format'
import type { DateRangeParams, UsageRecord } from '../types'
import { MultiSelect, TimeRangePicker } from '@/components/common'
import type { MultiSelectOption } from '@/components/common/MultiSelect.vue'
import ElapsedTimeText from './ElapsedTimeText.vue'
import ServerUserSelector from './ServerUserSelector.vue'

const { t } = useI18n()

export interface UserOption {
  id: string
  username: string
  email: string
}

export interface UserGroupOption {
  id: string
  name: string
}

interface FilterOption {
  value: string
  label: string
  disabled?: boolean
}

type UsageRecordColumnId =
  | 'time'
  | 'user'
  | 'key'
  | 'model'
  | 'provider'
  | 'api_format'
  | 'status'
  | 'tokens'
  | 'cost'
  | 'performance'
  | 'client_family'
  | 'client_ip'
  | 'user_agent'

interface UsageRecordColumnOption {
  id: UsageRecordColumnId
  label: string
  adminOnly?: boolean
  userOnly?: boolean
}

const USAGE_RECORD_COLUMN_OPTIONS = computed<UsageRecordColumnOption[]>(() => [
  { id: 'time', label: t('usageRecords.time') },
  { id: 'user', label: t('usageRecords.user'), adminOnly: true },
  { id: 'key', label: t('usageRecords.key'), userOnly: true },
  { id: 'model', label: t('usageRecords.model') },
  { id: 'provider', label: t('usageRecords.provider'), adminOnly: true },
  { id: 'api_format', label: t('usageRecords.apiFormat') },
  { id: 'status', label: t('usageRecords.typeStatus') },
  { id: 'tokens', label: 'Tokens' },
  { id: 'cost', label: t('usageRecords.costMultiplier') },
  { id: 'performance', label: t('usageRecords.latencySpeed') },
  { id: 'client_family', label: t('usageRecords.clientType') },
  { id: 'client_ip', label: t('usageRecords.ipAddress') },
  { id: 'user_agent', label: 'User-Agent' },
])

const DEFAULT_ADMIN_COLUMNS: UsageRecordColumnId[] = [
  'time',
  'user',
  'model',
  'provider',
  'api_format',
  'status',
  'tokens',
  'cost',
  'performance',
]

const DEFAULT_USER_COLUMNS: UsageRecordColumnId[] = [
  'time',
  'key',
  'model',
  'api_format',
  'status',
  'tokens',
  'cost',
  'performance',
]

const props = defineProps<{
  records: UsageRecord[]
  isAdmin: boolean
  showActualCost: boolean
  loading: boolean
  // 时间范围
  timeRange: DateRangeParams
  // 筛选
  filterSearch: string
  filterUser: string
  filterApiKeyGroup: string
  filterModel: string
  filterProvider: string
  filterApiFormat: string
  filterStatus: string
  filterClientFamily: string
  availableUsers: UserOption[]
  availableApiKeyGroups: UserGroupOption[]
  availableModels: string[]
  availableProviders: string[]
  availableClientFamilies: string[]
  // 分页
  currentPage: number
  pageSize: number
  totalRecords: number
  pageSizeOptions: number[]
  // 自动刷新
  autoRefresh: boolean
}>()

const emit = defineEmits<{
  'update:timeRange': [value: DateRangeParams]
  'update:filterSearch': [value: string]
  'update:filterUser': [value: string]
  'update:filterApiKeyGroup': [value: string]
  'update:filterModel': [value: string]
  'update:filterProvider': [value: string]
  'update:filterApiFormat': [value: string]
  'update:filterStatus': [value: string]
  'update:filterClientFamily': [value: string]
  'update:currentPage': [value: number]
  'update:pageSize': [value: number]
  'update:autoRefresh': [value: boolean]
  'refresh': []
  'showDetail': [id: string]
  'prefetchDetail': [id: string]
}>()

const { copyToClipboard } = useClipboard()

function normalizeProviderDisplayText(value: string | null | undefined): string {
  return (value || '').trim().toLowerCase()
}

function getProviderRouteDisplay(record: UsageRecord): string[] {
  const route = Array.isArray(record.provider_route)
    ? record.provider_route
      .map(item => item.trim())
      .filter(Boolean)
    : []
  if (route.length > 0) {
    return route
  }
  return record.provider ? [record.provider] : []
}

function getProviderTransferTitle(record: UsageRecord): string {
  const route = getProviderRouteDisplay(record)
  if (route.length > 1) {
    return t('usageRecords.serviceTransfer', { route: route.join(' → ') })
  }
  return t('usageRecords.serviceTransferOccurred')
}

function hasProviderTransfer(record: UsageRecord): boolean {
  return Boolean(record.has_fallback) && getProviderRouteDisplay(record).length > 1
}

function getProviderAccountDisplay(record: UsageRecord): string {
  const accountDisplay = record.provider_key_account_label || record.provider_key_name || ''
  if (!accountDisplay) return ''
  const providerNames = new Set(getProviderRouteDisplay(record).map(normalizeProviderDisplayText))
  if (providerNames.has(normalizeProviderDisplayText(accountDisplay))) {
    return ''
  }
  return accountDisplay
}

function getUsageRecordUserDisplay(record: UsageRecord): string {
  if (record.username) return record.username
  if (record.user_email) return record.user_email
  if (record.user_id) return t('usageRecords.deletedUser')
  return t('usageRecords.unauthenticatedRequest')
}

async function copyProviderAccountDisplay(record: UsageRecord): Promise<void> {
  const text = getProviderAccountDisplay(record)
  if (!text) return
  await copyToClipboard(text)
}

// 静态常量（放在 defineProps/defineEmits 之后）
const AVAILABLE_API_FORMATS = [
  { value: 'openai:chat', label: 'OpenAI Chat' },
  { value: 'openai:responses', label: 'OpenAI Responses' },
  { value: 'openai:responses:compact', label: 'OpenAI Responses Compact' },
  { value: 'openai:video', label: 'OpenAI Video' },
  { value: 'claude:messages', label: 'Claude Messages' },
  { value: 'gemini:generate_content', label: 'Gemini Generate Content' },
  { value: 'gemini:video', label: 'Gemini Video' },
  { value: 'gemini:files', label: 'Gemini Files' },
] as const

// 使用模块级常量
const availableApiFormats = AVAILABLE_API_FORMATS
const browserWindow = typeof window !== 'undefined' ? window : undefined

const adminVisibleColumnIds = useLocalStorage<UsageRecordColumnId[]>(
  'usage-records-visible-columns-admin',
  DEFAULT_ADMIN_COLUMNS,
  { window: browserWindow },
)
const userVisibleColumnIds = useLocalStorage<UsageRecordColumnId[]>(
  'usage-records-visible-columns-user',
  DEFAULT_USER_COLUMNS,
  { window: browserWindow },
)

const roleColumnOptions = computed(() => USAGE_RECORD_COLUMN_OPTIONS.value.filter((column) => {
  if (column.adminOnly && !props.isAdmin) return false
  if (column.userOnly && props.isAdmin) return false
  return true
}))

const roleColumnIds = computed(() => new Set(roleColumnOptions.value.map(column => column.id)))

function sanitizeColumnIds(
  ids: readonly string[],
  fallback: readonly UsageRecordColumnId[],
): UsageRecordColumnId[] {
  const seen = new Set<UsageRecordColumnId>()
  const sanitized = ids.filter((id): id is UsageRecordColumnId => {
    if (!roleColumnIds.value.has(id as UsageRecordColumnId)) return false
    if (seen.has(id as UsageRecordColumnId)) return false
    seen.add(id as UsageRecordColumnId)
    return true
  })
  return sanitized.length > 0 ? sanitized : [...fallback]
}

const visibleColumnIds = computed<UsageRecordColumnId[]>({
  get: () => sanitizeColumnIds(
    props.isAdmin ? adminVisibleColumnIds.value : userVisibleColumnIds.value,
    props.isAdmin ? DEFAULT_ADMIN_COLUMNS : DEFAULT_USER_COLUMNS,
  ),
  set: (value) => {
    const sanitized = sanitizeColumnIds(value, props.isAdmin ? DEFAULT_ADMIN_COLUMNS : DEFAULT_USER_COLUMNS)
    if (props.isAdmin) {
      adminVisibleColumnIds.value = sanitized
    } else {
      userVisibleColumnIds.value = sanitized
    }
  },
})

const visibleColumnSet = computed(() => new Set<UsageRecordColumnId>(visibleColumnIds.value))
const visibleColumnCount = computed(() => visibleColumnIds.value.length)
const desktopTableMinWidthClass = computed(() => {
  const metadataColumnCount = visibleColumnIds.value.filter(column => (
    column === 'client_family' ||
    column === 'client_ip' ||
    column === 'user_agent'
  )).length
  if (metadataColumnCount >= 3) return 'min-w-[1520px]'
  if (metadataColumnCount > 0) return 'min-w-[1320px]'
  return props.isAdmin ? 'min-w-[1120px]' : 'min-w-[960px]'
})
const usageDesktopColumns = computed<ResizableTableColumn<UsageRecordColumnId>[]>(() => {
  const widths: Record<UsageRecordColumnId, string> = props.isAdmin
    ? {
        time: '8%',
        user: '12%',
        key: '16%',
        model: '13%',
        provider: '14%',
        api_format: '13%',
        status: '9%',
        tokens: '9%',
        cost: '12%',
        performance: '10%',
        client_family: '12%',
        client_ip: '10%',
        user_agent: '13%',
      }
    : {
        time: '9%',
        user: '12%',
        key: '16%',
        model: '20%',
        provider: '14%',
        api_format: '13%',
        status: '10%',
        tokens: '10%',
        cost: '12%',
        performance: '10%',
        client_family: '12%',
        client_ip: '10%',
        user_agent: '13%',
      }
  const minWidths: Record<UsageRecordColumnId, number> = {
    time: 112,
    user: 140,
    key: 150,
    model: 180,
    provider: 190,
    api_format: 150,
    status: 112,
    tokens: 112,
    cost: 140,
    performance: 128,
    client_family: 130,
    client_ip: 120,
    user_agent: 220,
  }

  return visibleColumnIds.value.map(column => ({
    key: column,
    width: widths[column],
    minWidth: minWidths[column],
  }))
})
const {
  columnWidths: desktopColumnWidths,
  startResize: handleUsageColumnResizeStart,
} = useResizableTableColumns<UsageRecordColumnId>({
  storageKey: props.isAdmin
    ? 'usage-records-admin-table-column-widths'
    : 'usage-records-user-table-column-widths',
  columns: usageDesktopColumns,
  defaultMinWidth: 96,
})

const columnSelectOptions = computed<MultiSelectOption[]>(() => roleColumnOptions.value.map(column => ({
  value: column.id,
  label: column.label,
})))

const COST_EPSILON = 0.0000001

interface ChargeLine {
  label: 'package' | 'wallet'
  amount: number
  multiplier: number | null
}

interface ResolvedChargeBreakdown {
  officialCost: number
  packageDebit: number
  packageMultiplier: number | null
  walletDebit: number
  walletMultiplier: number | null
  userDebit: number
}

function isColumnVisible(column: UsageRecordColumnId): boolean {
  return visibleColumnSet.value.has(column)
}

function toFiniteNumber(value: number | null | undefined): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function getSalesMultiplier(record: UsageRecord): number | null {
  return toFiniteNumber(record.discount) ?? toFiniteNumber(record.sales_multiplier)
}

function usesDiscountTerms(record: UsageRecord): boolean {
  return Object.prototype.hasOwnProperty.call(record, 'discount')
    || Object.prototype.hasOwnProperty.call(record.charge_breakdown || {}, 'wallet_discount')
}

function resolveChargeBreakdown(record: UsageRecord): ResolvedChargeBreakdown {
  const rawBreakdown = record.charge_breakdown
  const salesMultiplier = getSalesMultiplier(record)
  const official = toFiniteNumber(rawBreakdown?.official_cost) ?? toFiniteNumber(record.official_cost)
  const recordCost = toFiniteNumber(record.cost) ?? 0
  let officialCost = official
  if (officialCost === null && salesMultiplier !== null && salesMultiplier > 0) {
    officialCost = recordCost / salesMultiplier
  } else if (officialCost === null) {
    officialCost = recordCost
  }
  const resolvedOfficialCost = officialCost ?? recordCost

  const packageDebit = Math.max(toFiniteNumber(rawBreakdown?.package_debit) ?? 0, 0)
  const hasBreakdown = rawBreakdown !== null && rawBreakdown !== undefined
  const walletFromBreakdown = toFiniteNumber(rawBreakdown?.wallet_debit)
  const walletDebit = Math.max(
    walletFromBreakdown ?? (hasBreakdown ? 0 : recordCost),
    0,
  )
  const userDebit = Math.max(
    toFiniteNumber(rawBreakdown?.user_debit) ?? (packageDebit + walletDebit),
    0,
  )
  const packageMultiplier = toFiniteNumber(rawBreakdown?.package_multiplier)
    ?? (packageDebit > COST_EPSILON ? 1 : null)
  const walletMultiplier = toFiniteNumber(rawBreakdown?.wallet_discount)
    ?? toFiniteNumber(rawBreakdown?.wallet_multiplier)
    ?? salesMultiplier
    ?? (resolvedOfficialCost > COST_EPSILON && walletDebit > COST_EPSILON ? walletDebit / resolvedOfficialCost : null)

  return {
    officialCost: resolvedOfficialCost,
    packageDebit,
    packageMultiplier,
    walletDebit,
    walletMultiplier,
    userDebit,
  }
}

function getUserCharge(record: UsageRecord): number {
  return resolveChargeBreakdown(record).userDebit
}

function getOfficialCost(record: UsageRecord): number {
  return resolveChargeBreakdown(record).officialCost
}

function getModerationCost(record: UsageRecord): number {
  return Math.max(toFiniteNumber(record.moderation_cost) ?? 0, 0)
}

function hasModerationCost(record: UsageRecord): boolean {
  return getModerationCost(record) > COST_EPSILON
}

function getModelCost(record: UsageRecord): number {
  const saved = toFiniteNumber(record.model_cost)
  if (saved !== null) return Math.max(saved, 0)
  return Math.max(getUserCharge(record) - getModerationCost(record), 0)
}

function getActualModelCost(record: UsageRecord): number | null {
  const saved = toFiniteNumber(record.actual_model_cost)
  return saved === null ? null : Math.max(saved, 0)
}

function getActualModerationCost(record: UsageRecord): number | null {
  const saved = toFiniteNumber(record.actual_moderation_cost)
  return saved === null ? null : Math.max(saved, 0)
}

function getChargeLines(record: UsageRecord): ChargeLine[] {
  const breakdown = resolveChargeBreakdown(record)
  const lines: ChargeLine[] = []
  if (breakdown.packageDebit > COST_EPSILON) {
    lines.push({
      label: 'package',
      amount: breakdown.packageDebit,
      multiplier: breakdown.packageMultiplier,
    })
  }
  if (breakdown.walletDebit > COST_EPSILON) {
    lines.push({
      label: 'wallet',
      amount: breakdown.walletDebit,
      multiplier: breakdown.walletMultiplier,
    })
  }
  if (lines.length === 0 && breakdown.userDebit > COST_EPSILON) {
    lines.push({
      label: 'wallet',
      amount: breakdown.userDebit,
      multiplier: breakdown.walletMultiplier,
    })
  }
  return lines
}

function hasPlatformCost(record: UsageRecord): boolean {
  return toFiniteNumber(record.actual_cost) !== null
}

function getPlatformCost(record: UsageRecord): number {
  return toFiniteNumber(record.actual_cost) ?? getOfficialCost(record)
}

function getCostMultiplier(record: UsageRecord): number | null {
  const saved = toFiniteNumber(record.rate_multiplier)
  if (saved !== null) return saved
  const officialCost = getOfficialCost(record)
  if (officialCost <= 0 || !hasPlatformCost(record)) return null
  return getPlatformCost(record) / officialCost
}

function formatMultiplier(value: number | null): string {
  if (value === null) return '-'
  return `${value.toFixed(4).replace(/0+$/, '').replace(/\.$/, '')}x`
}

function formatChargeFactor(record: UsageRecord, line: ChargeLine): string {
  if (usesDiscountTerms(record) && line.label === 'wallet') {
    if (line.multiplier === null) return '-'
    const value = line.multiplier.toFixed(4).replace(/0+$/, '').replace(/\.$/, '')
    return t('models.discountFactor', { value })
  }
  return formatMultiplier(line.multiplier)
}

function formatCostMultiplier(record: UsageRecord): string {
  return t('usageRecords.costMultiplierValue', { value: formatMultiplier(getCostMultiplier(record)) })
}

function formatChargeLabel(label: ChargeLine['label']): string {
  return label === 'package' ? t('usageRecords.package') : t('usageRecords.wallet')
}

function getRecordCostTitle(record: UsageRecord): string {
  const chargeLines = getChargeLines(record)
  const lines = [
    t('usageRecords.officialPriceTooltip', { amount: formatCurrency(getOfficialCost(record)) }),
    ...chargeLines.map(line => t('usageRecords.deductedTooltip', { label: formatChargeLabel(line.label), amount: formatCurrency(line.amount), multiplier: formatChargeFactor(record, line) })),
  ]
  if (chargeLines.length === 0) lines.push(t('usageRecords.noUserCharge'))
  if (hasModerationCost(record)) {
    lines.push(t('usageRecords.modelChargeTooltip', { amount: formatCurrency(getModelCost(record)) }))
    lines.push(t('usageRecords.moderationCostTooltip', { amount: formatCurrency(getModerationCost(record)) }))
  }
  if (props.showActualCost && hasPlatformCost(record)) {
    lines.push(t('usageRecords.platformCostTooltip', { amount: formatCurrency(getPlatformCost(record)) }))
    const actualModelCost = getActualModelCost(record)
    const actualModerationCost = getActualModerationCost(record)
    if (actualModelCost !== null) {
      lines.push(t('usageRecords.platformModelCostTooltip', { amount: formatCurrency(actualModelCost) }))
    }
    if (actualModerationCost !== null) {
      lines.push(t('usageRecords.platformModerationCostTooltip', { amount: formatCurrency(actualModerationCost) }))
    }
    lines.push(t('usageRecords.costMultiplierTooltip', { value: formatMultiplier(getCostMultiplier(record)) }))
  }
  return lines.join('\n')
}

const modelFilterOptions = computed<FilterOption[]>(() => [
  { value: '__all__', label: t('usageRecords.allModels') },
  ...props.availableModels.map((model) => ({
    value: model,
    label: model.replace('claude-', ''),
  })),
])

const providerFilterOptions = computed<FilterOption[]>(() => [
  { value: '__all__', label: t('usageRecords.allProviders') },
  ...props.availableProviders.map((provider) => ({
    value: provider,
    label: provider,
  })),
])

function formatClientFamily(value: string | null | undefined): string {
  const normalized = value?.trim().toLowerCase()
  if (!normalized) return '-'
  if (normalized === 'codex') return 'Codex'
  if (normalized === 'codex_vscode') return 'Codex VS Code'
  if (normalized === 'claude_code') return 'Claude Code'
  if (normalized === 'opencode') return 'OpenCode'
  if (normalized === 'gemini_cli') return 'Gemini CLI'
  if (normalized === 'openai_js_sdk') return 'OpenAI JS SDK'
  if (normalized === 'generic') return t('usageRecords.genericClient')
  return value?.trim() || '-'
}

const clientFamilyFilterOptions = computed<FilterOption[]>(() => {
  const families = new Set<string>(props.availableClientFamilies)
  props.records.forEach((record) => {
    const family = record.client_family?.trim()
    if (family) families.add(family)
  })
  return [
    { value: '__all__', label: t('usageRecords.allClients') },
    ...Array.from(families).sort().map((family) => ({
      value: family,
      label: formatClientFamily(family),
    })),
  ]
})

const apiFormatFilterOptions = computed<FilterOption[]>(() => [
  { value: '__all__', label: t('usageRecords.allFormats') },
  ...availableApiFormats.map((format) => ({
    value: format.value,
    label: format.label,
  })),
])

const statusFilterOptions = computed<FilterOption[]>(() => [
  { value: '__all__', label: t('usageRecords.allStatuses') },
  { value: 'stream', label: t('usageRecords.streaming') },
  { value: 'standard', label: t('usageRecords.standard') },
  { value: 'active', label: t('usageRecords.active') },
  { value: 'failed', label: t('usageRecords.failed') },
  { value: 'cancelled', label: t('usageRecords.cancelledLong') },
  { value: 'has_retry', label: t('usageRecords.hasRetry') },
  { value: 'has_fallback', label: t('usageRecords.hasFallback') },
])

const timeRangeModel = computed({
  get: () => props.timeRange,
  set: (value: DateRangeParams) => emit('update:timeRange', value)
})

// 通用搜索（输入防抖）
const localSearch = ref(props.filterSearch)
const emitSearchDebounced = useDebounceFn((value: string) => {
  emit('update:filterSearch', value)
}, 300)

function getDisplayStatus(record: UsageRecord) {
  return resolveDisplayRequestStatus(record)
}

function getStreamModeLabel(record: UsageRecord): string {
  return formatUsageStreamLabel(record)
}

function translateStreamMode(label: string): string {
  return label
    .replaceAll('流式', t('usageRecords.streaming'))
    .replaceAll('标准', t('usageRecords.standard'))
}

function getStreamModeSegments(record: UsageRecord) {
  return resolveUsageStreamLabelSegments(record)
}

function getUpstreamStream(record: UsageRecord): boolean {
  return isUsageUpstreamStream(record)
}

function parseRecordDateTime(dateStr: string): Date {
  const utcDateStr = dateStr.includes('Z') || dateStr.includes('+') ? dateStr : `${dateStr}Z`
  return new Date(utcDateStr)
}

function formatRecordDate(dateStr: string): string {
  const date = parseRecordDateTime(dateStr)
  const year = String(date.getFullYear())
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

function formatRecordTime(dateStr: string): string {
  const date = parseRecordDateTime(dateStr)
  const hours = String(date.getHours()).padStart(2, '0')
  const minutes = String(date.getMinutes()).padStart(2, '0')
  const seconds = String(date.getSeconds()).padStart(2, '0')
  return `${hours}:${minutes}:${seconds}`
}

watch(() => props.filterSearch, (value) => {
  if (value !== localSearch.value) {
    localSearch.value = value
  }
})

watch(localSearch, (value) => {
  emitSearchDebounced(value)
})

// 使用复用的行点击逻辑
const { handleMouseDown, shouldTriggerRowClick } = useRowClick()

function handleRowMouseDown(event: MouseEvent, id: string) {
  handleMouseDown(event)
  if (!props.isAdmin) return
  if (event.button !== 0) return
  emit('prefetchDetail', id)
}

// 处理行点击，排除文本选择操作
function handleRowClick(event: MouseEvent, id: string) {
  if (!props.isAdmin) return
  if (!shouldTriggerRowClick(event)) return
  emit('showDetail', id)
}

function getRecordEffectiveInputTokens(record: UsageRecord): number {
  return getEffectiveInputTokens(record)
}

function getRecordCacheReadTokens(record: UsageRecord): number {
  return getCacheReadTokens(record)
}

function getRecordCacheCreationTokens(record: UsageRecord): number {
  return getCacheCreationTokens(record)
}

function hasPositiveTokens(value: number | null | undefined): boolean {
  return typeof value === 'number' && Number.isFinite(value) && value > 0
}

function formatOptionalTokens(value: number | null | undefined): string {
  return hasPositiveTokens(value) ? formatTokens(value) : '-'
}

function formatRecordLatencyPair(record: UsageRecord): string {
  const firstByte = formatRecordDurationSeconds(record.first_byte_time_ms)
  const total = formatRecordDurationSeconds(record.response_time_ms)
  return `${firstByte} / ${total}`
}

function formatRecordDurationSeconds(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms)) return '-'
  return `${(ms / 1000).toFixed(2)}s`
}

function getRecordDisplayOutputRate(record: UsageRecord): number | null {
  return getDisplayOutputRate({
    output_tokens: record.output_tokens,
    response_time_ms: record.response_time_ms,
    first_byte_time_ms: record.first_byte_time_ms,
    is_stream: record.is_stream,
    upstream_is_stream: record.upstream_is_stream,
  })
}

function getRecordPerformanceTitle(record: UsageRecord): string {
  const outputRate = getRecordDisplayOutputRate(record)
  return [
    t('usageRecords.firstByteTooltip', { value: formatRecordDurationSeconds(record.first_byte_time_ms) }),
    t('usageRecords.totalLatencyTooltip', { value: formatRecordDurationSeconds(record.response_time_ms) }),
    t('usageRecords.generationLatencyTooltip', { value: formatRecordDurationSeconds(getGenerationTimeMs(record)) }),
    t('usageRecords.outputSpeedTooltip', { value: formatOutputRateTokensPerSecond(outputRate) }),
  ].join('\n')
}

function formatOutputRateTokensPerSecond(outputRate: number | null | undefined): string {
  const value = formatOutputRateValue(outputRate)
  if (value === '-') return value
  return `${value} tokens/s`
}

function formatUserAgent(value: string | null | undefined): string {
  const userAgent = value?.trim()
  if (!userAgent) return '-'
  return userAgent.length > 48 ? `${userAgent.slice(0, 45)}...` : userAgent
}

// useDebounceFn 自动处理清理，无需 onUnmounted

// 判断是否应该显示格式转换信息
// 包括：1. 跨格式转换（has_format_conversion=true）2. 同族格式差异
function shouldShowFormatConversion(record: UsageRecord): boolean {
  if (!record.api_format || !record.endpoint_api_format) {
    return false
  }
  // 跨格式转换
  if (record.has_format_conversion) {
    return true
  }
  // 同族格式差异（精确字符串比较，不区分大小写）
  return record.api_format.trim().toLowerCase() !== record.endpoint_api_format.trim().toLowerCase()
}

// 获取 API 格式的 tooltip（包含转换信息）
function getApiFormatTooltip(record: UsageRecord): string {
  if (!record.api_format) {
    return ''
  }
  const displayFormat = formatApiFormat(record.api_format)

  // 如果发生了格式转换或同族格式差异，显示详细信息
  if (shouldShowFormatConversion(record)) {
    const endpointApiFormat = record.endpoint_api_format ?? record.api_format
    const endpointDisplayFormat = formatApiFormat(endpointApiFormat)
    const conversionType = record.has_format_conversion ? t('usageRecords.formatConversion') : t('usageRecords.formatCompatible')
    return t('usageRecords.formatTooltip', { request: displayFormat, endpoint: endpointDisplayFormat, conversion: conversionType })
  }

  return record.api_format
}

function getActualModel(record: UsageRecord): string | null {
  if (record.target_model && record.target_model !== record.model) {
    return record.target_model
  }
  // 其次显示 Provider 返回的实际版本（如 Gemini 的 modelVersion）
  if (record.model_version && record.model_version !== record.model) {
    return record.model_version
  }
  return null
}

function getReasoningEffort(record: UsageRecord): string | null {
  const requested = record.requested_reasoning_effort?.trim()
  const actual = record.reasoning_effort?.trim()
  if (requested && actual && requested.toLowerCase() !== actual.toLowerCase()) {
    return `${requested} -> ${actual}`
  }
  return actual || requested || null
}

// 获取模型列的 tooltip
function getModelTooltip(record: UsageRecord): string {
  const actualModel = getActualModel(record)
  const reasoningEffort = getReasoningEffort(record)
  const reasoningSuffix = reasoningEffort ? `\nReasoning: ${reasoningEffort}` : ''
  if (actualModel) {
    return `${record.model} -> ${actualModel}${reasoningSuffix}`
  }
  return `${record.model}${reasoningSuffix}`
}
</script>
