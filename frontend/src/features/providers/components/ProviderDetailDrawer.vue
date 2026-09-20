<template>
  <!-- 自定义抽屉 -->
  <Teleport to="body">
    <Transition name="drawer">
      <div
        v-if="open && (loading || provider)"
        class="fixed inset-0 z-50 flex justify-end"
        @click.self="handleBackdropClick"
      >
        <!-- 背景遮罩 -->
        <div
          class="absolute inset-0 bg-black/30 backdrop-blur-sm"
          @click="handleBackdropClick"
        />

        <!-- 抽屉内容 -->
        <Card class="relative h-full w-full sm:w-[700px] sm:max-w-[90vw] rounded-none shadow-2xl overflow-y-auto">
          <!-- 加载状态 -->
          <div
            v-if="loading"
            class="flex items-center justify-center py-12"
          >
            <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-primary" />
          </div>

          <template v-else-if="provider">
            <!-- 头部:名称 + 快捷操作 -->
            <div class="sticky top-0 z-10 bg-background border-b px-4 sm:px-6 pt-4 sm:pt-6 pb-3 sm:pb-3">
              <div class="flex items-center justify-between gap-x-3 sm:gap-x-4 flex-wrap">
                <div class="flex items-center gap-2 min-w-0">
                  <h2 class="text-lg sm:text-xl font-bold truncate">
                    {{ provider.name }}
                  </h2>
                  <Badge
                    :variant="provider.is_active ? 'default' : 'secondary'"
                    class="text-xs shrink-0"
                  >
                    {{ provider.is_active ? t('providerDetail.active') : t('providerDetail.disabled') }}
                  </Badge>
                  <Badge
                    v-if="providerReadOnly"
                    variant="outline"
                    class="text-xs shrink-0"
                    :title="provider.legacy_read_only_reason"
                  >
                    {{ t('providerDetail.coreReadOnly') }}
                  </Badge>
                </div>
                <div class="flex items-center gap-1 shrink-0">
                  <span :title="systemFormatConversionEnabled ? t('providerDetail.systemConversion') : (provider.enable_format_conversion ? t('providerDetail.disableConversion') : t('providerDetail.enableConversion'))">
                    <Button
                      variant="ghost"
                      size="icon"
                      :class="(provider.enable_format_conversion || systemFormatConversionEnabled) ? 'text-primary' : ''"
                      :disabled="systemFormatConversionEnabled || providerReadOnly"
                      @click="toggleFormatConversion"
                    >
                      <Shuffle class="w-4 h-4" />
                    </Button>
                  </span>
                  <span :title="hasFailoverRules ? t('providerDetail.failoverConfigured') : t('providerDetail.configureFailover')">
                    <Button
                      variant="ghost"
                      size="icon"
                      :class="hasFailoverRules ? 'text-orange-500 dark:text-orange-400' : ''"
                      :disabled="providerReadOnly"
                      @click="openFailoverRulesDialog"
                    >
                      <GitBranch class="w-4 h-4" />
                    </Button>
                  </span>
                  <Popover
                    :open="providerProxyPopoverOpen"
                    @update:open="handleProviderProxyPopoverToggle"
                  >
                    <PopoverTrigger as-child>
                      <Button
                        variant="ghost"
                        size="icon"
                        :class="provider.proxy?.node_id ? 'text-blue-500' : ''"
                        :disabled="savingProviderProxy || providerReadOnly"
                        :title="provider.proxy?.node_id ? t('providerDetail.proxySelected', { name: getProviderProxyNodeName() }) : t('providerDetail.setProxy')"
                      >
                        <Globe class="w-4 h-4" />
                      </Button>
                    </PopoverTrigger>
                    <PopoverContent
                      class="w-72 p-3"
                      side="bottom"
                      align="end"
                    >
                      <div class="space-y-2">
                        <div class="flex items-center justify-between">
                          <span class="text-xs font-medium">{{ t('providerDetail.proxyNode') }}</span>
                          <Button
                            v-if="provider.proxy?.node_id"
                            variant="ghost"
                            size="sm"
                            class="h-6 px-2 text-[10px] text-muted-foreground"
                            :disabled="savingProviderProxy || providerReadOnly"
                            @click="clearProviderProxy"
                          >
                            {{ t('providerDetail.clear') }}
                          </Button>
                        </div>
                        <ProxyNodeSelect
                          :model-value="provider.proxy?.node_id || ''"
                          trigger-class="h-8"
                          @update:model-value="setProviderProxy"
                        />
                        <p class="text-[10px] text-muted-foreground">
                          {{ provider.proxy?.node_id ? t('providerDetail.usingIndependentProxy') : t('providerDetail.proxyNotSet') }}
                        </p>
                      </div>
                    </PopoverContent>
                  </Popover>
                  <Button
                    variant="ghost"
                    size="icon"
                    :disabled="providerReadOnly"
                    :title="providerReadOnly ? t('providerDetail.editInCore') : t('providerDetail.editProvider')"
                    @click="emitEditProvider"
                  >
                    <Edit class="w-4 h-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    :disabled="providerReadOnly"
                    :title="providerReadOnly ? t('providerDetail.editInCore') : provider.is_active ? t('providerDetail.clickDisable') : t('providerDetail.clickEnable')"
                    @click="emitToggleProviderStatus"
                  >
                    <Power class="w-4 h-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    :title="t('providerDetail.close')"
                    @click="handleClose"
                  >
                    <X class="w-4 h-4" />
                  </Button>
                </div>
              </div>
              <!-- 网站地址（独占整行，紧贴名称行下方） -->
              <div
                v-if="provider.website"
                class="-mt-0.5"
              >
                <a
                  :href="provider.website"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="text-xs text-muted-foreground hover:text-primary hover:underline transition-colors truncate block"
                  :title="provider.website"
                >{{ provider.website }}</a>
              </div>
              <!-- 端点 API 格式 -->
              <div class="flex items-center gap-1.5 flex-wrap mt-3">
                <template v-if="loadingProviderEndpoints && endpoints.length === 0">
                  <span class="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
                    <Loader2 class="w-3.5 h-3.5 animate-spin" />
                    {{ t('providerDetail.loadingEndpoints') }}
                  </span>
                </template>
                <template v-else>
                  <template
                    v-for="endpoint in endpoints"
                    :key="endpoint.id"
                  >
                    <span
                      class="text-xs px-2 py-0.5 rounded-md border border-border bg-background hover:bg-accent hover:border-accent-foreground/20 cursor-pointer transition-colors font-medium"
                      :class="{ 'opacity-40': !endpoint.is_active }"
                      :title="providerReadOnly ? t('providerDetail.editInCore') : t('providerDetail.editEndpointFormat', { format: formatApiFormat(endpoint.api_format) })"
                      @click="handleEditEndpoint(endpoint)"
                    >{{ formatApiFormat(endpoint.api_format) }}</span>
                  </template>
                  <span
                    v-if="endpoints.length > 0"
                    class="text-xs px-2 py-0.5 rounded-md border border-dashed border-border hover:bg-accent hover:border-accent-foreground/20 cursor-pointer transition-colors text-muted-foreground"
                      :title="providerReadOnly ? t('providerDetail.editInCore') : t('providerDetail.editEndpoint')"
                    @click="showAddEndpointDialog"
                  >{{ t('providerDetail.edit') }}</span>
                  <Button
                    v-else
                    variant="outline"
                    size="sm"
                    class="h-7 text-xs"
                    :disabled="providerReadOnly"
                    @click="showAddEndpointDialog"
                  >
                    <Plus class="w-3 h-3 mr-1" />
                    {{ t('providerDetail.addEndpoint') }}
                  </Button>
                </template>
              </div>
            </div>

            <div class="space-y-6 p-4 sm:p-6">
              <!-- 配额使用情况 -->
              <Card
                v-if="provider.billing_type === 'monthly_quota' && provider.monthly_quota_usd"
                class="p-4"
              >
                <div class="space-y-3">
                  <div class="flex items-center justify-between">
                    <h3 class="text-sm font-semibold">
                      {{ t('providerDetail.subscriptionQuota') }}
                    </h3>
                    <Badge
                      variant="secondary"
                      class="text-xs"
                    >
                      {{ ((provider.monthly_used_usd || 0) / provider.monthly_quota_usd * 100).toFixed(1) }}%
                    </Badge>
                  </div>
                  <div class="relative w-full h-2 bg-border rounded-full overflow-hidden">
                    <div
                      class="absolute left-0 top-0 h-full transition-all duration-300"
                      :class="{
                        'bg-green-500': (provider.monthly_used_usd || 0) / provider.monthly_quota_usd < 0.7,
                        'bg-yellow-500': (provider.monthly_used_usd || 0) / provider.monthly_quota_usd >= 0.7 && (provider.monthly_used_usd || 0) / provider.monthly_quota_usd < 0.9,
                        'bg-red-500': (provider.monthly_used_usd || 0) / provider.monthly_quota_usd >= 0.9
                      }"
                      :style="{ width: `${Math.min((provider.monthly_used_usd || 0) / provider.monthly_quota_usd * 100, 100)}%` }"
                    />
                  </div>
                  <div class="flex items-center justify-between text-xs">
                    <span class="font-semibold">
                      ${{ (provider.monthly_used_usd || 0).toFixed(2) }} / ${{ provider.monthly_quota_usd.toFixed(2) }}
                    </span>
                    <span
                      v-if="provider.quota_reset_day"
                      class="text-muted-foreground"
                    >
                      {{ t('providerDetail.monthlyReset', { day: provider.quota_reset_day }) }}
                    </span>
                  </div>
                </div>
              </Card>

              <!-- 密钥管理 -->
              <Card class="overflow-hidden">
                <div class="p-4 border-b border-border/60">
                  <div class="flex items-center justify-between">
                    <h3 class="text-sm font-semibold">
                      {{ isKeyManagedProviderType(provider.provider_type) ? t('providerDetail.keyManagement') : t('providerDetail.accountManagement') }}
                    </h3>
                    <div class="flex items-center gap-2">
                      <Button
                        v-if="endpoints.length > 0"
                        variant="outline"
                        size="sm"
                        class="h-8"
                        :disabled="providerReadOnly"
                        @click="handleAddKeyToFirstEndpoint"
                      >
                        <Plus class="w-3.5 h-3.5 mr-1.5" />
                        {{ isKeyManagedProviderType(provider.provider_type) ? t('providerDetail.addKey') : t('providerDetail.addAccount') }}
                      </Button>
                    </div>
                  </div>
                </div>

                <!-- 密钥列表 -->
                <div
                  v-if="loadingProviderKeys && allKeys.length === 0"
                  class="flex items-center justify-center gap-2 py-12 text-sm text-muted-foreground"
                >
                  <Loader2 class="w-4 h-4 animate-spin" />
                  {{ t('providerDetail.loadingCredentials', { type: isKeyManagedProviderType(provider.provider_type) ? t('providerDetail.keys') : t('providerDetail.accounts') }) }}
                </div>

                <div
                  v-else-if="allKeys.length > 0"
                  class="divide-y divide-border/40"
                  :class="shouldPaginateKeys && 'flex flex-col'"
                >
                  <div
                    v-for="({ key, endpoint }, localIdx) in paginatedKeys"
                    :key="key.id"
                    class="px-4 py-2.5 hover:bg-muted/30 transition-colors group/item"
                    :class="{
                      'opacity-50': keyDragState.isDragging && keyDragState.draggedIndex === getGlobalKeyIndex(localIdx),
                      'bg-primary/5 border-l-2 border-l-primary': keyDragState.targetIndex === getGlobalKeyIndex(localIdx) && keyDragState.isDragging,
                      'opacity-40 bg-muted/20': !key.is_active
                    }"
                    draggable="true"
                    @dragstart="handleKeyDragStart($event, getGlobalKeyIndex(localIdx))"
                    @dragend="handleKeyDragEnd"
                    @dragover="handleKeyDragOver($event, getGlobalKeyIndex(localIdx))"
                    @dragleave="handleKeyDragLeave"
                    @drop="handleKeyDrop($event, getGlobalKeyIndex(localIdx))"
                  >
                    <!-- 第一行：名称 + 状态 + 操作按钮 -->
                    <div class="flex items-center justify-between gap-2">
                      <div class="flex items-center gap-2 flex-1 min-w-0">
                        <!-- 拖拽手柄 -->
                        <div class="cursor-grab active:cursor-grabbing text-muted-foreground/30 group-hover/item:text-muted-foreground transition-colors shrink-0">
                          <GripVertical class="w-4 h-4" />
                        </div>
                        <div class="flex flex-col min-w-0">
                          <div class="flex items-center gap-1.5">
                            <button
                              type="button"
                              class="min-w-0 truncate text-left text-sm font-medium transition-colors hover:text-primary"
                              :class="!(key.is_active) ? 'text-muted-foreground' : ''"
                              :title="`${getProviderAccountDisplayName(key)}\n${t('providerDetail.clickCopy')}`"
                              @click.stop="copyProviderAccountDisplay(key)"
                            >
                              {{ getProviderAccountDisplayName(key) }}
                            </button>
                            <!-- OAuth 订阅类型标签 (Codex) -->
                            <Badge
                              v-if="key.oauth_plan_type"
                              variant="outline"
                              class="text-[10px] px-1.5 py-0 shrink-0"
                              :class="getOAuthPlanTypeClass(key.oauth_plan_type)"
                            >
                              {{ formatOAuthPlanType(key.oauth_plan_type) }}
                            </Badge>
                            <Badge
                              v-if="getOAuthOrgBadge(key)"
                              variant="secondary"
                              class="text-[9px] px-1 py-0 h-4 shrink-0"
                              :title="getOAuthOrgBadge(key)?.title"
                            >
                              {{ getOAuthOrgBadge(key)?.label }}
                            </Badge>
                            <!-- Kiro 订阅类型标签 -->
                            <Badge
                              v-if="shouldShowKiroSubscriptionBadge(key)"
                              variant="outline"
                              class="text-[10px] px-1.5 py-0 shrink-0"
                              :class="getOAuthPlanTypeClass(getKiroSubscriptionBadgeLabel(key))"
                            >
                              {{ getKiroSubscriptionBadgeLabel(key) }}
                            </Badge>
                          </div>
                          <div class="flex items-center gap-1">
                            <span class="text-[11px] font-mono text-muted-foreground">
                              {{ isOAuthManagedCredential(key) ? '[Refresh Token]' : (isServiceAccountCredential(key) ? '[Service Account]' : key.api_key_masked) }}
                            </span>
                            <Button
                              v-if="canExportOAuthCredential(key)"
                              variant="ghost"
                              size="icon"
                              class="h-4 w-4 shrink-0"
                              :title="t('providerDetail.downloadRefreshToken')"
                              @click.stop="downloadRefreshToken(key)"
                            >
                              <Download class="w-2.5 h-2.5" />
                            </Button>
                            <Button
                              v-else
                              variant="ghost"
                              size="icon"
                              class="h-4 w-4 shrink-0"
                              :title="t('providerDetail.copyKey')"
                              @click.stop="copyFullKey(key)"
                            >
                              <Copy class="w-2.5 h-2.5" />
                            </Button>
                            <!-- OAuth 状态（失效/过期/倒计时）和刷新按钮 -->
                            <template v-if="shouldShowOAuthRefreshControl(key, provider.provider_type)">
                              <!-- 账号级别异常：醒目提示 + 清除按钮 -->
                              <template v-if="isAccountLevelBlock(key)">
                                <Badge
                                  variant="destructive"
                                  class="text-[10px] px-1.5 py-0 shrink-0 gap-0.5"
                                  :title="getOAuthStatusTitle(key)"
                                >
                                  <ShieldX class="w-2.5 h-2.5" />
                                  {{ t('providerDetail.accountAnomaly') }}
                                </Badge>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  class="h-4 w-4 shrink-0 text-destructive hover:text-destructive"
                                  :disabled="clearingOAuthInvalidKeyId === key.id"
                                  :title="t('providerDetail.clearAnomalyHint')"
                                  @click.stop="handleClearOAuthInvalid(key)"
                                >
                                  <RefreshCw
                                    class="w-2.5 h-2.5"
                                    :class="{ 'animate-spin': clearingOAuthInvalidKeyId === key.id }"
                                  />
                                </Button>
                              </template>
                              <!-- 普通 OAuth 状态 -->
                              <template v-else>
                                <span
                                  class="text-[10px]"
                                  :class="{
                                    'text-destructive': getKeyOAuthExpires(key)?.isInvalid || getKeyOAuthExpires(key)?.isExpired,
                                    'text-warning': getKeyOAuthExpires(key)?.isExpiringSoon && !getKeyOAuthExpires(key)?.isExpired && !getKeyOAuthExpires(key)?.isInvalid,
                                    'text-muted-foreground': !getKeyOAuthExpires(key)?.isExpired && !getKeyOAuthExpires(key)?.isExpiringSoon && !getKeyOAuthExpires(key)?.isInvalid
                                  }"
                                  :title="getOAuthStatusTitle(key)"
                                >
                                  {{ getKeyOAuthExpires(key)?.text }}
                                </span>
                                <Badge
                                  v-if="key.oauth_temporary"
                                  variant="outline"
                                  class="text-[10px] px-1.5 py-0 shrink-0"
                                  :title="t('providerDetail.temporaryHint')"
                                >
                                  {{ t('providerDetail.temporary') }}
                                </Badge>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  class="h-4 w-4 shrink-0"
                                  :disabled="refreshingOAuthKeyId === key.id || !canRefreshOAuthCredential(key)"
                                  :title="getOAuthRefreshButtonTitle(key)"
                                  @click.stop="handleRefreshOAuth(key)"
                                >
                                  <RefreshCw
                                    class="w-2.5 h-2.5"
                                    :class="{ 'animate-spin': refreshingOAuthKeyId === key.id }"
                                  />
                                </Button>
                              </template>
                            </template>
                            <!-- Antigravity 账号未激活提示 -->
                            <span
                              v-if="provider.provider_type === 'antigravity' && key.is_active && isOAuthManagedCredential(key) && !hasAntigravityQuotaDisplayData(key)"
                              class="text-[10px] text-orange-500 dark:text-orange-400"
                              :title="t('providerDetailExtra.geminiNotActivated')"
                            >
                              {{ t('providerDetail.accountNotActivated') }}
                            </span>
                          </div>
                        </div>
                      </div>
                      <!-- 并发 + 健康度 + 操作按钮 -->
                      <div class="flex items-center gap-1 shrink-0">
                        <!-- Key 主状态 -->
                        <Badge
                          v-if="getKeySchedulingState(key) !== 'available'"
                          :variant="getKeySchedulingBadgeVariant(key)"
                          class="text-[10px] px-1.5 py-0 shrink-0"
                          :title="getKeySchedulingTitle(key)"
                        >
                          {{ getKeySchedulingLabel(key) }}
                        </Badge>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-7 w-7 text-primary"
                          :disabled="!key.is_active || loadingProviderModels || loadingProviderKeys"
                          :title="'测试账号'"
                          @click.stop="handleTestAccount(key)"
                        >
                          <Play class="w-3.5 h-3.5" />
                        </Button>
                        <Badge
                          variant="outline"
                          class="text-[10px] px-1.5 py-0 shrink-0 tabular-nums"
                          :title="keyConcurrencyTitle(key)"
                        >
                          {{ formatKeyConcurrency(key) }}
                        </Badge>
                        <!-- 健康度 -->
                        <div
                          v-if="key.health_score !== undefined"
                          class="flex items-center gap-1 mr-1"
                        >
                          <div class="w-10 h-1.5 bg-border rounded-full overflow-hidden">
                            <div
                              class="h-full transition-all duration-300"
                              :class="getHealthScoreBarColor(key.health_score || 0)"
                              :style="{ width: `${(key.health_score || 0) * 100}%` }"
                            />
                          </div>
                          <span
                            class="text-[10px] font-medium tabular-nums"
                            :class="getHealthScoreColor(key.health_score || 0)"
                          >
                            {{ ((key.health_score || 0) * 100).toFixed(0) }}%
                          </span>
                        </div>
                        <Button
                          v-if="isKeyRecoverable(key)"
                          variant="ghost"
                          size="icon"
                          class="h-7 w-7 text-green-600"
                          :disabled="isKeyReadOnly(key)"
                          :title="isKeyReadOnly(key) ? t('providerDetail.editInCore') : getRecoverKeyTitle(key)"
                          @click="handleRecoverKey(key)"
                        >
                          <RefreshCw class="w-3.5 h-3.5" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-7 w-7"
                          :disabled="isKeyReadOnly(key)"
                          :title="isKeyReadOnly(key) ? t('providerDetail.editInCore') : t('providerDetail.modelPermissions')"
                          @click="handleKeyPermissions(key)"
                        >
                          <Shield class="w-3.5 h-3.5" />
                        </Button>
                        <!-- 代理节点配置 -->
                        <Popover
                          :open="proxyPopoverOpenKeyId === key.id"
                          @update:open="(v: boolean) => handleProxyPopoverToggle(key.id, v)"
                        >
                          <PopoverTrigger as-child>
                            <Button
                              variant="ghost"
                              size="icon"
                              class="h-7 w-7"
                              :class="key.proxy?.node_id ? 'text-blue-500' : ''"
                              :disabled="savingProxyKeyId === key.id || isKeyReadOnly(key)"
                              :title="isKeyReadOnly(key) ? t('providerDetail.editInCore') : key.proxy?.node_id ? t('providerDetail.proxySelected', { name: getKeyProxyNodeName(key) }) : t('providerDetail.setProxy')"
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
                                <span class="text-xs font-medium">{{ t('providerDetail.proxyNode') }}</span>
                                <Button
                                  v-if="key.proxy?.node_id"
                                  variant="ghost"
                                  size="sm"
                                  class="h-6 px-2 text-[10px] text-muted-foreground"
                                  :disabled="savingProxyKeyId === key.id || isKeyReadOnly(key)"
                                  @click="clearKeyProxy(key)"
                                >
                                  {{ t('providerDetail.clear') }}
                                </Button>
                              </div>
                              <ProxyNodeSelect
                                :model-value="key.proxy?.node_id || ''"
                                trigger-class="h-8"
                                @update:model-value="(v: string) => setKeyProxy(key, v)"
                              />
                              <p class="text-[10px] text-muted-foreground">
                                {{ key.proxy?.node_id ? t('providerDetail.usingIndependentProxy') : t('providerDetail.providerProxyFallback') }}
                              </p>
                            </div>
                          </PopoverContent>
                        </Popover>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-7 w-7"
                          :disabled="isKeyReadOnly(key)"
                          :title="isKeyReadOnly(key) ? t('providerDetail.editInCore') : t('providerDetail.editKey')"
                          @click="handleEditKey(endpoint, key)"
                        >
                          <Edit class="w-3.5 h-3.5" />
                        </Button>
                        <Button
                          v-if="provider.provider_type === 'codex' && key.auth_type === 'oauth'"
                          variant="outline"
                          size="sm"
                          class="h-7 px-2"
                          @click="codexStateDialogKey = key"
                        >
                          {{ t('codexState.button') }}
                        </Button>
                        <Button
                          v-if="provider.provider_type === 'antigravity'"
                          variant="ghost"
                          size="icon"
                          class="h-7 w-7"
                          :title="t('providerDetail.quotaDetails')"
                          @click="openAntigravityQuotaDialog(key)"
                        >
                          <BarChart3 class="w-3.5 h-3.5" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-7 w-7"
                          :disabled="togglingKeyId === key.id || isKeyReadOnly(key)"
                          :title="isKeyReadOnly(key) ? t('providerDetail.editInCore') : key.is_active ? t('providerDetail.clickDisable') : t('providerDetail.clickEnable')"
                          @click="toggleKeyActive(key)"
                        >
                          <Power class="w-3.5 h-3.5" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-7 w-7"
                          :disabled="isKeyReadOnly(key)"
                          :title="isKeyReadOnly(key) ? t('providerDetail.editInCore') : t('providerDetail.deleteKey')"
                          @click="handleDeleteKey(key)"
                        >
                          <Trash2 class="w-3.5 h-3.5" />
                        </Button>
                      </div>
                    </div>
                    <!-- Codex 上游额度信息（仅当有元数据时显示） -->
                    <div
                      v-if="hasCodexQuotaDisplayData(key)"
                      class="mt-2 p-2 bg-muted/30 rounded-md"
                    >
                      <div class="flex items-center justify-between mb-1">
                        <span class="text-[10px] text-muted-foreground">{{ t('providerDetail.accountQuota') }}</span>
                        <div class="flex items-center gap-1">
                          <RefreshCw
                            v-if="refreshingQuota"
                            class="w-3 h-3 text-muted-foreground/70 animate-spin"
                          />
                          <span
                            v-if="getCodexQuotaDisplay(key)?.updated_at"
                            class="text-[9px] text-muted-foreground/70"
                          >
                            {{ formatCodexUpdatedAt(getCodexQuotaDisplay(key)?.updated_at || 0) }}
                          </span>
                        </div>
                      </div>
                      <div class="grid gap-3 grid-cols-1 sm:grid-cols-2">
                        <div
                          v-for="window in getCodexQuotaWindowsForDisplay(key)"
                          :key="window.code"
                        >
                          <div class="flex items-center justify-between text-[10px] mb-0.5">
                            <span class="text-muted-foreground">
                              {{ getCodexQuotaWindowDisplayLabel(window) }}
                            </span>
                            <span :class="getQuotaRemainingClass(getQuotaWindowUsedPercent(window) || 0)">
                              {{ (getQuotaWindowRemainingPercent(window) || 0).toFixed(1) }}%
                            </span>
                          </div>
                          <div class="relative w-full h-1.5 bg-border rounded-full overflow-hidden">
                            <div
                              class="absolute left-0 top-0 h-full transition-all duration-300"
                              :class="getQuotaRemainingBarColor(getQuotaWindowUsedPercent(window) || 0)"
                              :style="{ width: `${getQuotaWindowRemainingPercent(window) || 0}%` }"
                            />
                          </div>
                          <div
                            v-if="getQuotaWindowResetAt(window) || getQuotaWindowResetSeconds(window)"
                            class="text-[9px] mt-0.5 tabular-nums"
                            :class="getResetCountdownClass(
                              getQuotaWindowResetAt(window),
                              getQuotaWindowResetSeconds(window),
                              getQuotaSnapshotUpdatedAt(getQuotaSnapshotForProvider(key, 'codex')),
                              getQuotaWindowUsedPercent(window)
                            )"
                          >
                            {{ getResetCountdownText(
                              getQuotaWindowResetAt(window),
                              getQuotaWindowResetSeconds(window),
                              getQuotaSnapshotUpdatedAt(getQuotaSnapshotForProvider(key, 'codex')),
                              getQuotaWindowUsedPercent(window)
                            ) }}
                          </div>
                        </div>
                      </div>
                    </div>
                    <!-- Antigravity 上游额度摘要（按家族分组展示关键配额） -->
                    <div
                      v-if="provider.provider_type === 'antigravity' && (hasAntigravityQuotaDisplayData(key) || isAntigravityForbiddenKey(key))"
                      class="mt-2 p-2 rounded-md"
                      :class="isAntigravityForbiddenKey(key) ? 'bg-destructive/10 border border-destructive/30' : 'bg-muted/30'"
                    >
                      <!-- 封禁状态显示 -->
                      <div
                        v-if="isAntigravityForbiddenKey(key)"
                        class="flex items-center gap-2 text-destructive"
                      >
                        <ShieldX class="w-4 h-4 shrink-0" />
                        <div class="flex-1 min-w-0">
                          <div class="text-[11px] font-medium">
                            {{ t('providerDetail.accountBanned') }}
                          </div>
                          <div
                            v-if="getAntigravityForbiddenReason(key)"
                            class="text-[10px] text-destructive/80 truncate"
                            :title="getAntigravityForbiddenReason(key)"
                          >
                            {{ getAntigravityForbiddenReason(key) }}
                          </div>
                        </div>
                        <span
                          v-if="getAntigravityForbiddenAt(key)"
                          class="text-[9px] text-destructive/60 shrink-0"
                        >
                          {{ formatBanTimestamp(getAntigravityForbiddenAt(key)) }}
                        </span>
                      </div>
                      <!-- 正常配额显示 -->
                      <template v-else>
                        <div class="flex items-center justify-between mb-1">
                          <span class="text-[10px] text-muted-foreground">{{ t('providerDetail.modelQuota') }}</span>
                          <div class="flex items-center gap-1">
                            <RefreshCw
                              v-if="refreshingQuota"
                              class="w-3 h-3 text-muted-foreground/70 animate-spin"
                            />
                            <span
                              v-if="getAntigravityQuotaUpdatedAt(key)"
                              class="text-[9px] text-muted-foreground/70"
                            >
                              {{ formatAntigravityUpdatedAt(getAntigravityQuotaUpdatedAt(key) || 0) }}
                            </span>
                          </div>
                        </div>
                        <div class="grid grid-cols-2 gap-3">
                          <div
                            v-for="group in getAntigravityQuotaSummaryForKey(key)"
                            :key="group.key"
                          >
                            <div class="flex items-center justify-between text-[10px] mb-0.5">
                              <span class="text-muted-foreground truncate mr-2 min-w-0 flex-1">
                                {{ group.label }}
                              </span>
                              <span :class="getQuotaRemainingClass(group.usedPercent)">
                                {{ group.remainingPercent.toFixed(1) }}%
                              </span>
                            </div>
                            <div class="relative w-full h-1.5 bg-border rounded-full overflow-hidden">
                              <div
                                class="absolute left-0 top-0 h-full transition-all duration-300"
                                :class="getQuotaRemainingBarColor(group.usedPercent)"
                                :style="{ width: `${Math.max(group.remainingPercent, 0)}%` }"
                              />
                            </div>
                            <div
                              v-if="group.resetSeconds !== null || group.usedPercent > 0"
                              class="text-[9px] text-muted-foreground/70 mt-0.5"
                            >
                              <template v-if="group.resetSeconds !== null && group.resetSeconds > 0">
                                {{ t('providerDetail.resetAfter', { time: formatResetTime(group.resetSeconds) }) }}
                              </template>
                              <template v-else-if="group.resetSeconds !== null && group.resetSeconds <= 0">
                                {{ t('providerDetail.reset') }}
                              </template>
                              <template v-else>
                            {{ t('providerDetail.resetUnknown') }}
                              </template>
                            </div>
                          </div>
                        </div>
                      </template>
                    </div>
                    <!-- Kiro 上游额度信息（仅当有元数据时显示） -->
                    <div
                      v-if="provider.provider_type === 'kiro' && (hasKiroQuotaDisplayData(key) || isKiroBannedKey(key))"
                      class="mt-2 p-2 rounded-md"
                      :class="isKiroBannedKey(key) ? 'bg-destructive/10 border border-destructive/30' : 'bg-muted/30'"
                    >
                      <!-- 封禁状态显示 -->
                      <div
                        v-if="isKiroBannedKey(key)"
                        class="flex items-center gap-2 text-destructive"
                      >
                        <ShieldX class="w-4 h-4 shrink-0" />
                        <div class="flex-1 min-w-0">
                          <div class="text-[11px] font-medium">
                            {{ t('providerDetail.accountBanned') }}
                          </div>
                          <div
                            v-if="getKiroQuotaDisplay(key)?.ban_reason"
                            class="text-[10px] text-destructive/80 truncate"
                            :title="getKiroQuotaDisplay(key)?.ban_reason"
                          >
                            {{ getKiroQuotaDisplay(key)?.ban_reason }}
                          </div>
                        </div>
                        <span
                          v-if="getKiroQuotaDisplay(key)?.banned_at"
                          class="text-[9px] text-destructive/60 shrink-0"
                        >
                          {{ formatBanTimestamp(getKiroQuotaDisplay(key)?.banned_at) }}
                        </span>
                      </div>
                      <!-- 正常配额显示 -->
                      <template v-else>
                        <div class="flex items-center justify-between mb-1">
                          <span class="text-[10px] text-muted-foreground">{{ t('providerDetail.accountQuota') }}</span>
                          <div class="flex items-center gap-1">
                            <RefreshCw
                              v-if="refreshingQuota"
                              class="w-3 h-3 text-muted-foreground/70 animate-spin"
                            />
                            <span
                              v-if="getKiroQuotaDisplay(key)?.updated_at"
                              class="text-[9px] text-muted-foreground/70"
                            >
                              {{ formatKiroUpdatedAt(getKiroQuotaDisplay(key)?.updated_at || 0) }}
                            </span>
                          </div>
                        </div>
                        <!-- Kiro 额度显示：使用进度 -->
                        <div>
                          <!-- 使用额度进度条 -->
                          <div>
                            <div class="flex items-center justify-between text-[10px] mb-0.5">
                              <span class="text-muted-foreground">{{ t('providerDetail.usedQuota') }}</span>
                              <span :class="getQuotaRemainingClass(getKiroQuotaDisplay(key)?.usage_percentage || 0)">
                                {{ (100 - (getKiroQuotaDisplay(key)?.usage_percentage || 0)).toFixed(1) }}%
                              </span>
                            </div>
                            <div class="relative w-full h-1.5 bg-border rounded-full overflow-hidden">
                              <div
                                class="absolute left-0 top-0 h-full transition-all duration-300"
                                :class="getQuotaRemainingBarColor(getKiroQuotaDisplay(key)?.usage_percentage || 0)"
                                :style="{ width: `${Math.max(100 - (getKiroQuotaDisplay(key)?.usage_percentage || 0), 0)}%` }"
                              />
                            </div>
                            <div class="flex items-center justify-between text-[9px] text-muted-foreground/70 mt-0.5">
                              <span>
                                {{ formatKiroUsage(getKiroQuotaDisplay(key)?.current_usage) }} /
                                {{ formatKiroUsage(getKiroQuotaDisplay(key)?.usage_limit) }}
                              </span>
                              <span v-if="getKiroQuotaDisplay(key)?.next_reset_at">
                                {{ t('providerDetail.resetAfter', { time: formatKiroResetTime(getKiroQuotaDisplay(key)?.next_reset_at) }) }}
                              </span>
                            </div>
                          </div>
                        </div>
                      </template>
                    </div>
                    <!-- ChatGPT Web 上游额度信息（生图配额） -->
                    <div
                      v-if="provider.provider_type === 'chatgpt_web' && hasChatGPTWebQuotaDisplayData(key)"
                      class="mt-2 p-2 rounded-md bg-muted/30"
                    >
                      <div class="flex items-center justify-between mb-1">
                        <span class="text-[10px] text-muted-foreground">{{ t('providerDetail.accountQuota') }}</span>
                        <div class="flex items-center gap-1">
                          <RefreshCw
                            v-if="refreshingQuota"
                            class="w-3 h-3 text-muted-foreground/70 animate-spin"
                          />
                          <span
                            v-if="getChatGPTWebQuotaDisplay(key)?.updated_at"
                            class="text-[9px] text-muted-foreground/70"
                          >
                            {{ formatKiroUpdatedAt(getChatGPTWebQuotaDisplay(key)?.updated_at || 0) }}
                          </span>
                        </div>
                      </div>
                      <div>
                        <div class="flex items-center justify-between text-[10px] mb-0.5">
                          <span class="text-muted-foreground">{{ t('providerDetail.usedQuota') }}</span>
                          <span :class="getQuotaRemainingClass(getChatGPTWebQuotaUsedPercent(key))">
                            {{ getChatGPTWebQuotaRemainingPercent(key).toFixed(1) }}%
                          </span>
                        </div>
                        <div class="relative w-full h-1.5 bg-border rounded-full overflow-hidden">
                          <div
                            class="absolute left-0 top-0 h-full transition-all duration-300"
                            :class="getQuotaRemainingBarColor(getChatGPTWebQuotaUsedPercent(key))"
                            :style="{ width: `${Math.max(getChatGPTWebQuotaRemainingPercent(key), 0)}%` }"
                          />
                        </div>
                        <div class="flex items-center justify-between text-[9px] text-muted-foreground/70 mt-0.5">
                          <span>
                            {{ formatChatGPTWebUsage(getChatGPTWebQuotaDisplay(key)?.image_quota_used) }} /
                            {{ formatChatGPTWebUsage(getChatGPTWebQuotaDisplay(key)?.image_quota_total) }}
                          </span>
                          <span v-if="getChatGPTWebQuotaDisplay(key)?.image_quota_reset_at">
                            {{ t('providerDetail.resetAfter', { time: formatKiroResetTime(getChatGPTWebQuotaDisplay(key)?.image_quota_reset_at) }) }}
                          </span>
                        </div>
                      </div>
                    </div>
                    <!-- 第二行：优先级 + API 格式（展开显示） + 统计信息 -->
                    <div class="flex items-center gap-1.5 mt-1 text-[11px] text-muted-foreground">
                      <!-- 优先级放最前面，支持点击编辑 -->
                      <span
                        v-if="editingPriorityKey !== key.id"
                        :title="isKeyReadOnly(key) ? t('providerDetail.editInCore') : t('providerDetailExtra.editPriority')"
                        class="font-medium text-foreground/80"
                        :class="isKeyReadOnly(key) ? 'cursor-default' : 'cursor-pointer hover:text-primary hover:underline'"
                        @click="startEditPriority(key)"
                      >P{{ key.internal_priority }}</span>
                      <input
                        v-else
                        ref="priorityInputRef"
                        v-model="editingPriorityValue"
                        type="text"
                        inputmode="numeric"
                        pattern="[0-9]*"
                        class="w-8 h-5 px-1 text-[11px] text-center border rounded bg-background focus:outline-none focus:ring-1 focus:ring-primary font-medium text-foreground/80"
                        @keydown="(e) => handlePriorityKeydown(e, key)"
                        @blur="handlePriorityBlur(key)"
                      >
                      <!-- 自动获取模型状态 -->
                      <template v-if="key.auto_fetch_models">
                        <span class="text-muted-foreground/40">|</span>
                        <span
                          class="cursor-help"
                          :class="key.last_models_fetch_error ? 'text-amber-600 dark:text-amber-400' : ''"
                          :title="getAutoFetchStatusTitle(key)"
                        >
                          {{ key.last_models_fetch_error ? t('providerDetail.syncFailed') : t('providerDetail.autoSync') }}
                        </span>
                      </template>
                      <!-- RPM 限制信息（第二位） -->
                      <template v-if="key.rpm_limit || key.is_adaptive">
                        <span class="text-muted-foreground/40">|</span>
                        <span v-if="key.is_adaptive">
                          {{ key.learned_rpm_limit != null ? `${key.learned_rpm_limit}` : t('providerDetail.probing') }} RPM
                          <span class="text-muted-foreground/60">({{ t('providerDetail.adaptive') }})</span>
                        </span>
                        <span v-else>{{ key.rpm_limit }} RPM</span>
                      </template>
                      <span class="text-muted-foreground/40">|</span>
                      <!-- API 格式：展开显示每个格式、倍率、历史健康记录 -->
                      <template
                        v-for="(format, idx) in getKeyApiFormats(key, endpoint)"
                        :key="format"
                      >
                        <span
                          v-if="idx > 0"
                          class="text-muted-foreground/40"
                        >/</span>
                        <span :class="{ 'text-destructive': isFormatCircuitOpen(key, format) }">
                          {{ formatApiFormatShort(format) }}
                        </span>
                        <span
                          v-if="editingMultiplierKey !== key.id || editingMultiplierFormat !== format"
                          :title="t('providerDetail.editMultiplier')"
                          class="cursor-pointer hover:text-primary hover:underline"
                          :class="{ 'text-destructive': isFormatCircuitOpen(key, format) }"
                          @click="startEditMultiplier(key, format)"
                        >{{ getKeyRateMultiplier(key, format) }}x</span>
                        <input
                          v-else
                          ref="multiplierInputRef"
                          v-model="editingMultiplierValue"
                          type="text"
                          inputmode="decimal"
                          pattern="[0-9]*\.?[0-9]*"
                          class="w-10 h-5 px-1 text-[11px] text-center border rounded bg-background focus:outline-none focus:ring-1 focus:ring-primary font-medium text-foreground/80"
                          @keydown="(e) => handleMultiplierKeydown(e, key, format)"
                          @blur="handleMultiplierBlur(key, format)"
                        >
                      </template>
                    </div>
                  </div>
                  <!-- 分页控制 -->
                  <div
                    v-if="shouldPaginateKeys"
                    class="px-4 py-2 flex items-center justify-between text-xs text-muted-foreground mt-auto"
                  >
                    <span>{{ t('providerDetailExtra.credentialTotal', { count: allKeys.length, type: isKeyManagedProviderType(provider.provider_type) ? t('providerDetail.keys') : t('providerDetail.accounts') }) }}</span>
                    <div class="flex items-center gap-1.5">
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-6 px-2 text-xs"
                        :disabled="loadingProviderKeys || currentKeyPage <= 1"
                        @click="goToKeyPage(currentKeyPage - 1)"
                      >
                        ‹
                      </Button>
                      <span class="tabular-nums">{{ currentKeyPage }} / {{ totalKeyPages }}</span>
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-6 px-2 text-xs"
                        :disabled="loadingProviderKeys || currentKeyPage >= totalKeyPages"
                        @click="goToKeyPage(currentKeyPage + 1)"
                      >
                        ›
                      </Button>
                    </div>
                  </div>
                </div>

                <!-- 空状态 -->
                <div
                  v-else
                  class="p-8 text-center text-muted-foreground"
                >
                  <Key class="w-12 h-12 mx-auto mb-3 opacity-50" />
                  <p class="text-sm">
                    {{ isKeyManagedProviderType(provider.provider_type) ? t('providerDetailExtra.noKeys') : t('providerDetailExtra.noAccounts') }}
                  </p>
                  <p class="text-xs mt-1">
                    {{ endpoints.length > 0
                      ? (isKeyManagedProviderType(provider.provider_type) ? t('providerDetailExtra.addFirstKey') : t('providerDetailExtra.addFirstAccount'))
                      : t('providerDetailExtra.addEndpointFirst') }}
                  </p>
                </div>
              </Card>

              <!-- 模型查看 -->
              <ModelsTab
                v-if="provider"
                ref="modelsTabRef"
                :key="`models-${provider.id}`"
                :provider="provider"
                :models="providerModels"
                :endpoints="endpoints"
                :provider-keys="providerKeys"
                :loading="loadingProviderModels || loadingProviderKeys"
                @edit-model="handleEditModel"
                @batch-assign="handleBatchAssign"
                @import-upstream="handleImportUpstream"
                @refresh="loadEndpoints"
              />

              <!-- 模型映射 -->
              <ModelMappingTab
                v-if="provider"
                ref="modelMappingTabRef"
                :key="`mapping-${provider.id}`"
                :provider="provider"
                :endpoints="endpoints"
                :provider-keys="providerKeys"
                :models="providerModels"
                :mapping-preview="providerMappingPreview"
                :loading="loadingProviderEndpoints || loadingProviderKeys || loadingProviderModels || loadingProviderMappingPreview"
                @refresh="handleModelMappingChanged"
              />
            </div>
          </template>
        </Card>
      </div>
    </Transition>
  </Teleport>

  <!-- 端点表单对话框（管理/编辑） -->
  <EndpointFormDialog
    v-if="provider && open"
    v-model="endpointDialogOpen"
    :provider="provider"
    :endpoints="endpoints"
    :system-format-conversion-enabled="systemFormatConversionEnabled"
    :provider-format-conversion-enabled="provider.enable_format_conversion"
    @endpoint-created="handleEndpointChanged"
    @endpoint-updated="handleEndpointChanged"
  />

  <!-- 密钥编辑对话框 -->
  <KeyFormDialog
    v-if="open"
    :open="keyFormDialogOpen"
    :endpoint="currentEndpoint"
    :editing-key="editingKey"
    :provider-id="provider ? provider.id : null"
    :provider-type="provider?.provider_type || null"
    :available-api-formats="availableKeyApiFormats"
    @close="keyFormDialogOpen = false"
    @saved="handleKeyChanged"
  />

  <!-- OAuth 账号对话框 -->
  <OAuthAccountDialog
    v-if="open && provider"
    :open="oauthAccountDialogOpen"
    :provider-id="provider.id"
    :provider-type="provider.provider_type ?? null"
    @close="oauthAccountDialogOpen = false"
    @saved="handleKeyChanged"
  />

  <!-- OAuth 密钥编辑对话框 -->
  <OAuthKeyEditDialog
    v-if="open"
    :open="oauthKeyEditDialogOpen"
    :editing-key="editingKey"
    @close="oauthKeyEditDialogOpen = false"
    @saved="handleKeyChanged"
  />

  <!-- 模型权限对话框 -->
  <KeyAllowedModelsEditDialog
    v-if="open"
    :open="keyPermissionsDialogOpen"
    :api-key="editingKey"
    :provider-id="providerId || ''"
    @close="keyPermissionsDialogOpen = false"
    @saved="handleKeyChanged"
  />

  <!-- 删除密钥确认对话框 -->
  <AlertDialog
    v-if="open"
    :model-value="deleteKeyConfirmOpen"
    :title="t('providerDetail.deleteKey')"
    :description="t('providerDetailExtra.deleteAccountConfirm', { name: keyToDelete ? getProviderAccountDisplayName(keyToDelete) : '' })"
    :confirm-text="t('providerDetail.deleteKey')"
    :cancel-text="t('endpointForm.cancel')"
    type="danger"
    @update:model-value="deleteKeyConfirmOpen = $event"
    @confirm="confirmDeleteKey"
    @cancel="deleteKeyConfirmOpen = false"
  />

  <!-- 添加/编辑模型对话框 -->
  <ProviderModelFormDialog
    v-if="open && provider"
    :open="modelFormDialogOpen"
    :provider-id="provider.id"
    :provider-name="provider.name"
    :editing-model="editingModel"
    @update:open="modelFormDialogOpen = $event"
    @saved="handleModelSaved"
  />

  <!-- 批量关联模型对话框 -->
  <BatchAssignModelsDialog
    v-if="open && provider"
    :open="batchAssignDialogOpen"
    :provider-id="provider.id"
    :provider-name="provider.name"
    @update:open="batchAssignDialogOpen = $event"
    @changed="handleBatchAssignChanged"
  />

  <!-- 从上游获取模型对话框 -->
  <KeyAllowedModelsDialog
    v-if="open && provider"
    :open="upstreamModelsDialogOpen"
    :api-key="null"
    :provider-id="provider.id"
    @close="upstreamModelsDialogOpen = false"
    @saved="handleUpstreamModelsImported"
  />

  <CodexStateDialog
    v-if="open && providerId && codexStateDialogKey"
    :open="true"
    :provider-id="providerId"
    :key-id="codexStateDialogKey.id"
    :key-name="getProviderAccountDisplayName(codexStateDialogKey)"
    @update:open="codexStateDialogKey = null"
  />

  <!-- Antigravity 配额详情弹窗 -->
  <AntigravityQuotaDialog
    v-if="antigravityQuotaDialogKey"
    :open="antigravityQuotaDialogOpen"
    :metadata="antigravityQuotaDialogKey.upstream_metadata ?? null"
    :quota-snapshot="antigravityQuotaDialogKey.status_snapshot?.quota ?? null"
    :key-name="getProviderAccountDisplayName(antigravityQuotaDialogKey)"
    :provider-id="providerId ?? undefined"
    :key-id="antigravityQuotaDialogKey.id"
    @update:open="antigravityQuotaDialogOpen = $event"
  />

  <!-- 故障转移规则弹窗 -->
  <FailoverRulesDialog
    :open="failoverRulesDialogOpen"
    :provider="provider ?? null"
    @update:open="failoverRulesDialogOpen = $event"
    @saved="loadProvider()"
  />
</template>

<script setup lang="ts">
import { ref, watch, computed, nextTick, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  Plus,
  Key,
  Loader2,
  Edit,
  Trash2,
  RefreshCw,
  X,
  Power,
  GripVertical,
  Copy,
  Download,
  Shield,
  Shuffle,
  BarChart3,
  ShieldX,
  Globe,
  GitBranch,
  Play,
} from 'lucide-vue-next'
import { parseApiError } from '@/utils/errorParser'
import { useEscapeKey } from '@/composables/useEscapeKey'
import Button from '@/components/ui/button.vue'
import Badge from '@/components/ui/badge.vue'
import Card from '@/components/ui/card.vue'
import { Popover, PopoverTrigger, PopoverContent } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { useConfirm } from '@/composables/useConfirm'
import { useClipboard } from '@/composables/useClipboard'
import { useCountdownTimer, getCodexResetCountdown } from '@/composables/useCountdownTimer'
import { getAccountCopyText, getAccountDisplayName } from '@/features/providers/utils/accountDisplay'
import {
  getProvider,
  getProviderEndpoints,
  updateProvider,
  getProviderModels,
  getProviderMappingPreview,
  type ProviderMappingPreviewResponse,
  type ProviderWithEndpointsSummary,
} from '@/api/endpoints'
import { adminApi } from '@/api/admin'
import {
  KeyFormDialog,
  KeyAllowedModelsDialog,
  KeyAllowedModelsEditDialog,
  ModelsTab,
  BatchAssignModelsDialog,
  OAuthAccountDialog,
  OAuthKeyEditDialog
} from '@/features/providers/components'
import ModelMappingTab from '@/features/providers/components/provider-tabs/ModelMappingTab.vue'
import EndpointFormDialog from '@/features/providers/components/EndpointFormDialog.vue'
import ProviderModelFormDialog from '@/features/providers/components/ProviderModelFormDialog.vue'
import AlertDialog from '@/components/common/AlertDialog.vue'
import AntigravityQuotaDialog from '@/features/providers/components/AntigravityQuotaDialog.vue'
import CodexStateDialog from '@/features/providers/components/CodexStateDialog.vue'
import FailoverRulesDialog from '@/features/providers/components/FailoverRulesDialog.vue'
import ProxyNodeSelect from '@/features/providers/components/ProxyNodeSelect.vue'
import { useProxyNodesStore } from '@/stores/proxy-nodes'
import {
  deleteEndpointKey,
  recoverKeyHealth,
  getProviderKeysPage,
  updateProviderKey,
  revealEndpointKey,
  exportKey,
  refreshProviderOAuth,
  refreshProviderQuota,
  clearOAuthInvalid,
  type ProviderEndpoint,
  type EndpointAPIKey,
  type Model,
  API_FORMAT_ORDER,
  sortApiFormats,
} from '@/api/endpoints'
import type {
  UpstreamMetadata,
  AntigravityModelQuota,
  CodexUpstreamMetadata,
  ChatGPTWebUpstreamMetadata,
  GrokUpstreamMetadata,
  KiroUpstreamMetadata,
  QuotaStatusSnapshot,
  QuotaWindowSnapshot,
} from '@/api/endpoints/types'
import { formatApiFormat, formatApiFormatShort } from '@/api/endpoints/types/api-format'
import { isOAuthAccountProviderType, isKeyManagedProviderType } from '../utils/providerTypeUtils'
import {
  isProviderQuotaAutoRefreshCoolingDown,
  markProviderQuotaAutoRefreshAttempt,
} from '../utils/quotaAutoRefreshCooldown'
import { getOAuthOrgBadge } from '@/utils/oauthIdentity'
import { getOAuthRefreshFeedback } from '@/utils/oauthRefreshFeedback'
import {
  canEditOAuthCredential,
  canExportOAuthCredential,
  canRefreshOAuthCredential,
  isOAuthManagedCredential,
  isServiceAccountCredential,
  shouldShowOAuthRefreshControl,
} from '@/utils/providerKeyAuth'
import {
  getAccountStatusDisplay,
  getAccountStatusTitle,
  getOAuthRefreshButtonTitle as resolveOAuthRefreshButtonTitle,
  getOAuthStatusDisplay,
  getOAuthStatusDisplayWithFallback,
  getOAuthStatusTitle as resolveOAuthStatusTitle,
} from '@/utils/providerKeyStatus'

// 扩展端点类型,包含密钥列表
interface ProviderEndpointWithKeys extends ProviderEndpoint {
  keys?: EndpointAPIKey[]
  rpm_limit?: number
}

interface Props {
  providerId: string | null
  open: boolean
  initialProvider?: ProviderWithEndpointsSummary | null
}

const props = defineProps<Props>()
const { t } = useI18n()
const emit = defineEmits<{
  (e: 'update:open', value: boolean): void
  (e: 'edit', provider: ProviderWithEndpointsSummary): void
  (e: 'toggleStatus', provider: ProviderWithEndpointsSummary): void
  (e: 'refresh'): void
}>()

const { error: showError, success: showSuccess, warning: showWarning } = useToast()
const { confirm } = useConfirm()
const { copyToClipboard } = useClipboard()
const { tick: countdownTick, start: startCountdownTimer, stop: stopCountdownTimer } = useCountdownTimer()

const loading = ref(false)
const provider = ref<ProviderWithEndpointsSummary | null>(null)
const endpoints = ref<ProviderEndpointWithKeys[]>([])
const providerKeys = ref<EndpointAPIKey[]>([])  // Provider 级别的 keys
const providerModels = ref<Model[]>([])  // Provider 级别的 models
const providerMappingPreview = ref<ProviderMappingPreviewResponse | null>(null)  // 映射预览
const loadingProviderEndpoints = ref(false)
const loadingProviderKeys = ref(false)
const loadingProviderModels = ref(false)
const loadingProviderMappingPreview = ref(false)
let providerLoadRequestId = 0
let endpointsLoadRequestId = 0
let keysLoadRequestId = 0
let mappingPreviewLoadRequestId = 0
const DEFAULT_PROVIDER_KEYS_PAGE_SIZE = 3
const CUSTOM_PROVIDER_KEYS_PAGE_SIZE = 4

function getProviderKeysPageSize(providerType?: string | null): number {
  return (providerType || '').trim().toLowerCase() === 'custom'
    ? CUSTOM_PROVIDER_KEYS_PAGE_SIZE
    : DEFAULT_PROVIDER_KEYS_PAGE_SIZE
}

// 系统级格式转换配置
const systemFormatConversionEnabled = ref(false)

// 端点相关状态
const endpointDialogOpen = ref(false)

// 密钥相关状态
const keyFormDialogOpen = ref(false)
const keyPermissionsDialogOpen = ref(false)
const oauthAccountDialogOpen = ref(false)
const oauthKeyEditDialogOpen = ref(false)
const currentEndpoint = ref<ProviderEndpoint | null>(null)
const editingKey = ref<EndpointAPIKey | null>(null)
const deleteKeyConfirmOpen = ref(false)
const keyToDelete = ref<EndpointAPIKey | null>(null)
const togglingKeyId = ref<string | null>(null)

// 密钥显示状态：key_id -> 完整密钥
const revealedKeys = ref<Map<string, string>>(new Map())

// 模型相关状态
const modelFormDialogOpen = ref(false)
const editingModel = ref<Model | null>(null)
const batchAssignDialogOpen = ref(false)
const upstreamModelsDialogOpen = ref(false)
const modelMappingTabRef = ref<InstanceType<typeof ModelMappingTab> | null>(null)
const modelsTabRef = ref<InstanceType<typeof ModelsTab> | null>(null)
let concurrencyRefreshTimer: ReturnType<typeof setInterval> | null = null

// 密钥列表拖拽排序状态
const keyDragState = ref({
  isDragging: false,
  draggedIndex: null as number | null,
  targetIndex: null as number | null
})

// 点击编辑优先级相关状态
const editingPriorityKey = ref<string | null>(null)
const editingPriorityValue = ref<number>(0)
const priorityInputRef = ref<HTMLInputElement[] | null>(null)
const prioritySaving = ref(false)

// OAuth 刷新状态
const refreshingOAuthKeyId = ref<string | null>(null)

// OAuth 失效清除状态
const clearingOAuthInvalidKeyId = ref<string | null>(null)

// 限额刷新状态（Codex / Antigravity）
const refreshingQuota = ref(false)

// Antigravity 配额详情弹窗状态
const antigravityQuotaDialogOpen = ref(false)
const antigravityQuotaDialogKey = ref<EndpointAPIKey | null>(null)
const codexStateDialogKey = ref<EndpointAPIKey | null>(null)

// 故障转移规则
const failoverRulesDialogOpen = ref(false)
const hasFailoverRules = computed(() => {
  const rules = provider.value?.failover_rules
  if (!rules) return false
  return (rules.success_failover_patterns?.length || 0) > 0
    || (rules.error_stop_patterns?.length || 0) > 0
})
const providerReadOnly = computed(() => provider.value?.legacy_read_only === true)

function showProviderReadOnly() {
  showWarning(provider.value?.legacy_read_only_reason || t('providerDetail.readOnlyMessage'))
}

function isKeyReadOnly(key: EndpointAPIKey): boolean {
  return providerReadOnly.value || key.legacy_read_only === true
}

function isModelReadOnly(model: Model): boolean {
  return providerReadOnly.value || model.legacy_read_only === true
}

function openFailoverRulesDialog() {
  if (providerReadOnly.value) {
    showProviderReadOnly()
    return
  }
  failoverRulesDialogOpen.value = true
}

function emitEditProvider() {
  if (!provider.value) return
  if (providerReadOnly.value) {
    showProviderReadOnly()
    return
  }
  emit('edit', provider.value)
}

function emitToggleProviderStatus() {
  if (!provider.value) return
  if (providerReadOnly.value) {
    showProviderReadOnly()
    return
  }
  emit('toggleStatus', provider.value)
}

// Provider 级别代理配置状态
const proxyNodesStore = useProxyNodesStore()
const providerProxyPopoverOpen = ref(false)
const savingProviderProxy = ref(false)

// Key 级别代理配置状态
const savingProxyKeyId = ref<string | null>(null)
const proxyPopoverOpenKeyId = ref<string | null>(null)

// 点击编辑倍率相关状态
const editingMultiplierKey = ref<string | null>(null)
const editingMultiplierFormat = ref<string | null>(null)
const editingMultiplierValue = ref<number>(1.0)
const multiplierInputRef = ref<HTMLInputElement[] | null>(null)
const multiplierSaving = ref(false)

// 任意模态窗口打开时,阻止抽屉被误关闭
const hasBlockingDialogOpen = computed(() =>
  endpointDialogOpen.value ||
  keyFormDialogOpen.value ||
  keyPermissionsDialogOpen.value ||
  oauthAccountDialogOpen.value ||
  oauthKeyEditDialogOpen.value ||
  deleteKeyConfirmOpen.value ||
  modelFormDialogOpen.value ||
  batchAssignDialogOpen.value ||
  antigravityQuotaDialogOpen.value ||
  modelMappingTabRef.value?.dialogOpen
)

// 当前后端分页页内的密钥列表。key 通过 api_formats 字段确定支持的格式，endpoint 可能为 undefined。
const allKeys = computed(() => {
  return providerKeys.value.map(key => ({ key, endpoint: undefined as ProviderEndpointWithKeys | undefined }))
})

const availableKeyApiFormats = computed(() => {
  const formatSet = new Set<string>()

  for (const format of provider.value?.api_formats || []) {
    if (format) {
      formatSet.add(format)
    }
  }

  for (const endpoint of endpoints.value) {
    if (endpoint.api_format) {
      formatSet.add(endpoint.api_format)
    }
  }

  return sortApiFormats([...formatSet])
})

function syncCurrentSelections(
  nextEndpoints: ProviderEndpointWithKeys[] = endpoints.value,
  nextProviderKeys: EndpointAPIKey[] = providerKeys.value
) {
  if (currentEndpoint.value) {
    currentEndpoint.value = nextEndpoints.find(endpoint => endpoint.id === currentEndpoint.value?.id) ?? null
  }

  if (!editingKey.value) {
    return
  }

  const latestKeys: EndpointAPIKey[] = []
  const seenKeyIds = new Set<string>()

  for (const key of nextProviderKeys) {
    if (!seenKeyIds.has(key.id)) {
      seenKeyIds.add(key.id)
      latestKeys.push(key)
    }
  }

  for (const endpoint of nextEndpoints) {
    for (const key of endpoint.keys || []) {
      if (!seenKeyIds.has(key.id)) {
        seenKeyIds.add(key.id)
        latestKeys.push(key)
      }
    }
  }

  const latestEditingKey = latestKeys.find(key => key.id === editingKey.value?.id) || null
  editingKey.value = latestEditingKey

  if (!latestEditingKey) {
    keyFormDialogOpen.value = false
    keyPermissionsDialogOpen.value = false
    oauthKeyEditDialogOpen.value = false
  }
}

// ===== 账号列表后端分页 =====
const providerKeysTotal = ref(0)
const currentKeyPage = ref(1)
const keyPageSize = ref(DEFAULT_PROVIDER_KEYS_PAGE_SIZE)
const totalKeyPages = computed(() => Math.max(1, Math.ceil(providerKeysTotal.value / keyPageSize.value)))
const shouldPaginateKeys = computed(() => totalKeyPages.value > 1)
const paginatedKeys = computed(() => allKeys.value)

function getGlobalKeyIndex(localIdx: number): number {
  return localIdx
}

async function goToKeyPage(page: number) {
  const nextPage = Math.min(Math.max(page, 1), totalKeyPages.value)
  if (nextPage === currentKeyPage.value && providerKeys.value.length > 0) return
  await loadProviderKeysPage(nextPage)
}

function resetProviderScopedState() {
  endpoints.value = []
  providerKeys.value = []
  providerKeysTotal.value = 0
  currentKeyPage.value = 1
  keyPageSize.value = DEFAULT_PROVIDER_KEYS_PAGE_SIZE
  providerModels.value = []
  providerMappingPreview.value = null
  currentEndpoint.value = null
  editingKey.value = null
  editingModel.value = null
  keyToDelete.value = null
  revealedKeys.value.clear()

  endpointDialogOpen.value = false
  keyFormDialogOpen.value = false
  keyPermissionsDialogOpen.value = false
  oauthAccountDialogOpen.value = false
  oauthKeyEditDialogOpen.value = false
  deleteKeyConfirmOpen.value = false
  modelFormDialogOpen.value = false
  batchAssignDialogOpen.value = false
  upstreamModelsDialogOpen.value = false
  antigravityQuotaDialogOpen.value = false
  antigravityQuotaDialogKey.value = null
  codexStateDialogKey.value = null
}

// 合并监听 providerId 和 open，避免同一 tick 内两个 watcher 都触发导致重复请求
watch(
  [() => props.providerId, () => props.open],
  async ([newId, newOpen], [oldId, oldOpen]) => {
    if (newOpen && newId) {
      const providerChanged = !oldOpen || oldId !== newId || provider.value?.id !== newId
      if (providerChanged) {
        // 先清空上一家 Provider 的明细，避免加载期间短暂显示串台数据。
        endpointsLoadRequestId += 1
        keysLoadRequestId += 1
        mappingPreviewLoadRequestId += 1
        resetProviderScopedState()
        if (provider.value?.id !== newId) {
          provider.value = null
        }
      }
      const hasInitialProvider = props.initialProvider?.id === newId
      if (hasInitialProvider) {
        provider.value = props.initialProvider
        keyPageSize.value = getProviderKeysPageSize(provider.value?.provider_type)
        loading.value = false
      }
      void loadSystemFormatConversionConfig()
      // mapping-preview 较慢，不阻塞首屏渲染
      void loadMappingPreview()
      if (!hasInitialProvider) {
        await loadProvider()
      }
      const endpointsPromise = loadEndpoints()
      // 仅在抽屉刚打开时启动倒计时
      if (newOpen && !oldOpen) {
        startCountdownTimer()
        startConcurrencyRefresh()
      }
      void endpointsPromise.then(() => autoRefreshQuotaInBackground())
    } else if (!newOpen && oldOpen) {
      // 使在途请求失效，避免关闭后旧响应回写
      providerLoadRequestId += 1
      endpointsLoadRequestId += 1
      keysLoadRequestId += 1
      mappingPreviewLoadRequestId += 1

      // 停止倒计时定时器
      stopCountdownTimer()
      stopConcurrencyRefresh()
      // 重置所有状态
      loading.value = false
      provider.value = null
      resetProviderScopedState()
      loadingProviderEndpoints.value = false
      loadingProviderKeys.value = false
      loadingProviderModels.value = false
      loadingProviderMappingPreview.value = false
    }
  },
  { immediate: true },
)

// 处理背景点击
function handleBackdropClick() {
  if (!hasBlockingDialogOpen.value) {
    handleClose()
  }
}

// 关闭抽屉
function handleClose() {
  if (!hasBlockingDialogOpen.value) {
    emit('update:open', false)
  }
}

// 切换格式转换开关
async function toggleFormatConversion() {
  if (!provider.value) return
  if (providerReadOnly.value) {
    showProviderReadOnly()
    return
  }
  const newValue = !provider.value.enable_format_conversion
  try {
    const updated = await updateProvider(provider.value.id, { enable_format_conversion: newValue })
    provider.value = updated
    showSuccess(newValue ? t('providerDetail.conversionEnabled') : t('providerDetail.conversionDisabled'))
    emit('refresh')
  } catch {
    showError(t('providerDetail.conversionFailed'))
  }
}

// Provider 级别代理配置
function handleProviderProxyPopoverToggle(open: boolean) {
  if (open && providerReadOnly.value) {
    showProviderReadOnly()
    providerProxyPopoverOpen.value = false
    return
  }
  providerProxyPopoverOpen.value = open
  if (open) {
    proxyNodesStore.ensureLoaded()
  }
}

function getProviderProxyNodeName(): string {
  const nodeId = provider.value?.proxy?.node_id
  if (!nodeId) return t('providerDetail.unknownNode')
  const node = proxyNodesStore.nodes.find(n => n.id === nodeId)
  return node ? node.name : `${nodeId.slice(0, 8)}...`
}

async function setProviderProxy(nodeId: string) {
  if (!provider.value) return
  if (providerReadOnly.value) {
    showProviderReadOnly()
    return
  }
  savingProviderProxy.value = true
  try {
    const updated = await updateProvider(provider.value.id, {
      proxy: { node_id: nodeId, enabled: true },
    })
    provider.value = updated
    providerProxyPopoverOpen.value = false
    showSuccess(t('providerDetail.proxySet'))
    emit('refresh')
  } catch (err: unknown) {
    showError(parseApiError(err, t('providerDetail.proxySetFailed')))
  } finally {
    savingProviderProxy.value = false
  }
}

async function clearProviderProxy() {
  if (!provider.value) return
  if (providerReadOnly.value) {
    showProviderReadOnly()
    return
  }
  savingProviderProxy.value = true
  try {
    const updated = await updateProvider(provider.value.id, { proxy: null })
    provider.value = updated
    providerProxyPopoverOpen.value = false
    showSuccess(t('providerDetail.providerProxyCleared'))
    emit('refresh')
  } catch (err: unknown) {
    showError(parseApiError(err, t('providerDetail.proxyClearFailed')))
  } finally {
    savingProviderProxy.value = false
  }
}

// 显示端点管理对话框
function showAddEndpointDialog() {
  if (providerReadOnly.value) {
    showProviderReadOnly()
    return
  }
  endpointDialogOpen.value = true
}

// ===== 端点事件处理 =====
function handleEditEndpoint(_endpoint: ProviderEndpoint) {
  if (providerReadOnly.value) {
    showProviderReadOnly()
    return
  }
  // 点击任何端点都打开管理对话框
  endpointDialogOpen.value = true
}

async function handleEndpointChanged() {
  await Promise.all([loadProvider(), loadEndpoints()])
  emit('refresh')
}

// ===== 密钥事件处理 =====
function handleAddKey(endpoint: ProviderEndpoint) {
  if (providerReadOnly.value) {
    showProviderReadOnly()
    return
  }
  currentEndpoint.value = endpoint
  editingKey.value = null
  keyFormDialogOpen.value = true
}

// 添加密钥/账号（如果有多个端点则添加到第一个）
function handleAddKeyToFirstEndpoint() {
  if (providerReadOnly.value) {
    showProviderReadOnly()
    return
  }
  if (endpoints.value.length === 0) return

  // OAuth 账号型提供商：打开 OAuth 账号对话框
  if (isOAuthAccountProviderType(provider.value?.provider_type)) {
    oauthAccountDialogOpen.value = true
  } else {
    // 密钥型提供商（custom/vertex_ai）：打开密钥表单对话框
    handleAddKey(endpoints.value[0])
  }
}

function handleEditKey(endpoint: ProviderEndpoint | undefined, key: EndpointAPIKey) {
  if (isKeyReadOnly(key)) {
    showProviderReadOnly()
    return
  }
  currentEndpoint.value = endpoint || null
  editingKey.value = key
  // OAuth 密钥使用专门的编辑对话框
  if (canEditOAuthCredential(key)) {
    oauthKeyEditDialogOpen.value = true
  } else {
    keyFormDialogOpen.value = true
  }
}

function handleKeyPermissions(key: EndpointAPIKey) {
  if (isKeyReadOnly(key)) {
    showProviderReadOnly()
    return
  }
  editingKey.value = key
  keyPermissionsDialogOpen.value = true
}

// 复制完整密钥或认证配置
async function copyFullKey(key: EndpointAPIKey) {
  const cached = revealedKeys.value.get(key.id)
  if (cached) {
    copyToClipboard(cached)
    return
  }

  // 否则先获取再复制
  try {
    const result = await revealEndpointKey(key.id)
    let textToCopy: string

    if (result.auth_type === 'service_account' && result.auth_config) {
      // Service Account 类型：复制 auth_config JSON
      textToCopy = typeof result.auth_config === 'string'
        ? result.auth_config
        : JSON.stringify(result.auth_config, null, 2)
    } else {
      // API Key 类型：复制 api_key
      textToCopy = result.api_key || ''
    }

    revealedKeys.value.set(key.id, textToCopy)
    copyToClipboard(textToCopy)
  } catch (err: unknown) {
    showError(parseApiError(err, t('providerDetail.getKeyFailed')), t('providerDetail.error'))
  }
}

function getProviderAccountDisplayName(key: EndpointAPIKey): string {
  return getAccountDisplayName(key, t('providerDetail.unnamedAccount'))
}

function formatKeyConcurrency(key: EndpointAPIKey): string {
  const current = Math.max(0, key.current_concurrency ?? 0)
  const limit = key.concurrent_limit ?? 0
  return limit > 0 ? `${current}/${limit}` : String(current)
}

function keyConcurrencyTitle(key: EndpointAPIKey): string {
  return key.concurrent_limit && key.concurrent_limit > 0
    ? `当前并发请求数：${formatKeyConcurrency(key)}`
    : '当前并发请求数（不限制上限）'
}

function handleTestAccount(key: EndpointAPIKey) {
  modelsTabRef.value?.openAccountTest(key.id)
}

async function copyProviderAccountDisplay(key: EndpointAPIKey): Promise<void> {
  const text = getAccountCopyText(key)
  if (!text) return
  await copyToClipboard(text)
}

// 下载 OAuth 凭据文件（后端统一导出，前端只负责下载）
async function downloadRefreshToken(key: EndpointAPIKey) {
  try {
    const data = await exportKey(key.id)
    const providerType = provider.value?.provider_type || 'unknown'
    const exportedEmail = typeof data.email === 'string' ? data.email : ''
    const safeName = (exportedEmail || key.name || key.id.slice(0, 8)).replace(/[^a-zA-Z0-9_\-@.]/g, '_')

    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `aether_${providerType}_${safeName}.json`
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
  } catch (err: unknown) {
    showError(parseApiError(err, t('providerDetail.exportFailed')), t('providerDetail.error'))
  }
}


function handleDeleteKey(key: EndpointAPIKey) {
  if (isKeyReadOnly(key)) {
    showProviderReadOnly()
    return
  }
  keyToDelete.value = key
  deleteKeyConfirmOpen.value = true
}

async function confirmDeleteKey() {
  if (!keyToDelete.value) return
  if (isKeyReadOnly(keyToDelete.value)) {
    showProviderReadOnly()
    deleteKeyConfirmOpen.value = false
    keyToDelete.value = null
    return
  }

  const keyId = keyToDelete.value.id
  deleteKeyConfirmOpen.value = false
  keyToDelete.value = null

  try {
    await deleteEndpointKey(keyId)
    showSuccess(t('providerDetail.keyDeleted'))
    // 刷新端点列表及模型数据（删除 Key 触发自动解除模型关联）
    await loadEndpoints()
    emit('refresh')
  } catch (err: unknown) {
    showError(parseApiError(err, t('providerDetail.deleteKeyFailed')), t('providerDetail.error'))
  }
}

async function handleRecoverKey(key: EndpointAPIKey) {
  if (isKeyReadOnly(key)) {
    showProviderReadOnly()
    return
  }
  try {
    const result = await recoverKeyHealth(key.id)
    showSuccess(result.message || t('providerDetail.keyRecovered'))
    await loadEndpoints()
    emit('refresh')
  } catch (err: unknown) {
    showError(parseApiError(err, t('providerDetail.keyRecoverFailed')), t('providerDetail.error'))
  }
}

async function handleRefreshOAuth(key: EndpointAPIKey) {
  if (refreshingOAuthKeyId.value) return
  if (isKeyReadOnly(key)) {
    showProviderReadOnly()
    return
  }
  refreshingOAuthKeyId.value = key.id
  try {
    const result = await refreshProviderOAuth(key.id)
    const refreshedExpiresAt = typeof result.expires_at === 'number' ? result.expires_at : null
    let refreshedKey: EndpointAPIKey | null = null
    // 更新本地数据
    const keyInList = providerKeys.value.find(k => k.id === key.id)
    if (keyInList) {
      keyInList.oauth_expires_at = refreshedExpiresAt
    }
    // 只重新加载当前 keys 页，避免整个表格刷新
    if (props.providerId) {
      const freshPage = await getProviderKeysPage(props.providerId, {
        page: currentKeyPage.value,
        page_size: keyPageSize.value,
      }).catch(() => null)
      if (freshPage) {
        const mergedKeys = freshPage.keys.map((item) => {
          if (item.id !== key.id) return item
          if (refreshedExpiresAt == null) return item
          if (typeof item.oauth_expires_at === 'number' && item.oauth_expires_at >= refreshedExpiresAt) {
            return item
          }
          return { ...item, oauth_expires_at: refreshedExpiresAt }
        })
        providerKeys.value = mergedKeys
        providerKeysTotal.value = freshPage.total
        currentKeyPage.value = freshPage.page
        keyPageSize.value = freshPage.page_size
        syncCurrentSelections(endpoints.value, mergedKeys)
        refreshedKey = mergedKeys.find(item => item.id === key.id) ?? null
      }
    }
    const feedback = getOAuthRefreshFeedback({
      accountStateRecheckAttempted: result.account_state_recheck_attempted,
      accountStateRecheckError: result.account_state_recheck_error,
      snapshot: refreshedKey,
    })
    if (feedback.tone === 'warning') {
      showWarning(feedback.message)
    } else {
      showSuccess(feedback.message)
    }
    // Antigravity：token 刷新后可能完成了账号激活，触发配额获取
    // （不 emit('refresh')，避免触发全局 provider 余额刷新）
    void autoRefreshQuotaInBackground({ ignoreCooldown: true })
  } catch (err: unknown) {
    showError(parseApiError(err, t('providerDetail.tokenRefreshFailed')), t('providerDetail.error'))
  } finally {
    refreshingOAuthKeyId.value = null
  }
}

// 判断是否为账号级别的封禁（刷新 token 无法修复）
function isAccountLevelBlock(key: EndpointAPIKey): boolean {
  const account = getAccountStatusDisplay(key)
  const oauth = getOAuthStatusDisplay(key, countdownTick.value)
  return account.blocked && !oauth?.isInvalid
}

// 清除 OAuth 失效标记
async function handleClearOAuthInvalid(key: EndpointAPIKey) {
  if (clearingOAuthInvalidKeyId.value) return
  if (isKeyReadOnly(key)) {
    showProviderReadOnly()
    return
  }

  const confirmed = await confirm({
    title: t('providerDetail.clearAnomalyTitle'),
    message: t('providerDetail.clearAnomalyMessage', { name: getProviderAccountDisplayName(key) }),
    confirmText: t('providerDetail.confirmClear'),
    variant: 'info',
  })
  if (!confirmed) return

  clearingOAuthInvalidKeyId.value = key.id
  try {
    await clearOAuthInvalid(key.id)
    showSuccess(t('providerDetail.anomalyCleared'))
    // 更新本地数据
    const keyInList = providerKeys.value.find(k => k.id === key.id)
    if (keyInList) {
      keyInList.oauth_invalid_at = null
      keyInList.oauth_invalid_reason = null
      if (keyInList.status_snapshot) {
        keyInList.status_snapshot = {
          ...keyInList.status_snapshot,
          oauth: {
            ...keyInList.status_snapshot.oauth,
            code: 'none',
            label: null,
            reason: null,
            invalid_at: null,
            requires_reauth: false,
          },
          account: {
            ...keyInList.status_snapshot.account,
            code: 'ok',
            label: null,
            reason: null,
            blocked: false,
            recoverable: false,
          },
        }
      }
    }
    await loadEndpoints()
  } catch (err: unknown) {
    showError(parseApiError(err, t('providerDetail.clearFailed')), t('providerDetail.error'))
  } finally {
    clearingOAuthInvalidKeyId.value = null
  }
}

// Codex / Antigravity / Kiro / ChatGPT Web：打开抽屉后自动后台刷新（配额缓存缺失/过期，或 Token 即将过期时触发）
const AUTO_QUOTA_REFRESH_STALE_SECONDS = 5 * 60
// 与后端 OAuth 懒刷新阈值对齐：到期前 2 分钟内视为需要刷新
const AUTO_TOKEN_REFRESH_SKEW_SECONDS = 2 * 60

function quotaSnapshotHasDisplayData(quota: QuotaStatusSnapshot | null | undefined): boolean {
  if (!quota) return false
  return Boolean(
    (typeof quota.code === 'string' && quota.code.trim().toLowerCase() !== 'unknown')
    || quota.updated_at != null
    || quota.observed_at != null
    || quota.usage_ratio != null
    || (Array.isArray(quota.windows) && quota.windows.length > 0)
    || quota.credits,
  )
}

function getQuotaSnapshotForProvider(
  key: EndpointAPIKey,
  providerType: 'codex' | 'kiro' | 'antigravity' | 'chatgpt_web' | 'gemini_cli' | 'grok',
): QuotaStatusSnapshot | null {
  const quota = key.status_snapshot?.quota
  if (!quota) return null

  const snapshotProviderType = quota.provider_type?.trim().toLowerCase()
  if (snapshotProviderType) {
    return snapshotProviderType === providerType ? quota : null
  }

  return quotaSnapshotHasDisplayData(quota) ? quota : null
}

function getQuotaSnapshotUpdatedAt(quota: QuotaStatusSnapshot | null | undefined): number | undefined {
  const updatedAt = quota?.updated_at ?? quota?.observed_at
  return typeof updatedAt === 'number' ? updatedAt : undefined
}

function getQuotaWindow(
  quota: QuotaStatusSnapshot | null | undefined,
  code: string,
): QuotaWindowSnapshot | null {
  const windows = quota?.windows
  if (!Array.isArray(windows)) return null
  return windows.find(window => String(window?.code || '').trim().toLowerCase() === code.trim().toLowerCase()) ?? null
}

function getQuotaWindowUsedPercent(window: QuotaWindowSnapshot | null | undefined): number | undefined {
  if (!window) return undefined
  if (typeof window.used_ratio === 'number') {
    return Math.max(Math.min(window.used_ratio * 100, 100), 0)
  }
  if (typeof window.remaining_ratio === 'number') {
    return Math.max(Math.min((1 - window.remaining_ratio) * 100, 100), 0)
  }
  return undefined
}

function getQuotaWindowRemainingPercent(window: QuotaWindowSnapshot | null | undefined): number | undefined {
  if (!window) return undefined
  if (typeof window.remaining_ratio === 'number') {
    return Math.max(Math.min(window.remaining_ratio * 100, 100), 0)
  }
  if (typeof window.used_ratio === 'number') {
    return Math.max(Math.min((1 - window.used_ratio) * 100, 100), 0)
  }
  return undefined
}

function getQuotaWindowResetAt(window: QuotaWindowSnapshot | null | undefined): number | undefined {
  return typeof window?.reset_at === 'number' ? window.reset_at : undefined
}

function getQuotaWindowResetSeconds(window: QuotaWindowSnapshot | null | undefined): number | undefined {
  return typeof window?.reset_seconds === 'number' ? window.reset_seconds : undefined
}

function getQuotaWindowByScope(
  quota: QuotaStatusSnapshot | null | undefined,
  scope: string,
): QuotaWindowSnapshot[] {
  const windows = quota?.windows
  if (!Array.isArray(windows)) return []
  return windows.filter(window => String(window?.scope || '').trim().toLowerCase() === scope.trim().toLowerCase())
}

function getQuotaWindowLiveResetSeconds(
  quota: QuotaStatusSnapshot | null | undefined,
  window: QuotaWindowSnapshot | null | undefined,
): number | null {
  if (!window) return null

  const now = Math.floor(Date.now() / 1000)
  if (typeof window.reset_at === 'number') {
    return Math.max(window.reset_at - now, 0)
  }

  if (typeof window.reset_seconds === 'number') {
    const updatedAt = getQuotaSnapshotUpdatedAt(quota)
    const elapsed = typeof updatedAt === 'number' ? Math.max(now - updatedAt, 0) : 0
    return Math.max(window.reset_seconds - elapsed, 0)
  }

  return null
}

function getCodexQuotaWindowsForDisplay(key: EndpointAPIKey): QuotaWindowSnapshot[] {
  const quota = getQuotaSnapshotForProvider(key, 'codex')
  if (!quota || !Array.isArray(quota.windows)) return []
  return quota.windows.filter((window) => (
    getQuotaWindowUsedPercent(window) !== undefined
    || getQuotaWindowRemainingPercent(window) !== undefined
  ))
}

function getCodexQuotaWindowDisplayLabel(window: QuotaWindowSnapshot): string {
  const label = String(window.label || '').trim()
  if (label) return label
  const seconds = typeof window.window_seconds === 'number' ? window.window_seconds : 0
  if (seconds > 0) return formatCodexQuotaWindowSeconds(seconds)
  const minutes = typeof window.window_minutes === 'number' ? window.window_minutes : 0
  if (minutes === 300) return '5H'
  if (minutes === 10_080) return '7D'
  if (minutes === 43_200) return '1M'
  if (minutes > 0) return formatCodexQuotaWindowMinutes(minutes)
  return String(window.code || t('quotaUi.window'))
}

function formatCodexQuotaWindowSeconds(totalSeconds: number): string {
  return formatCodexQuotaWindowMinutes(Math.max(Math.ceil(totalSeconds / 60), 1))
}

function formatCodexQuotaWindowMinutes(totalMinutes: number): string {
  const normalizedMinutes = Math.max(Math.ceil(totalMinutes), 1)
  const days = Math.floor(normalizedMinutes / (24 * 60))
  const hours = Math.floor((normalizedMinutes % (24 * 60)) / 60)
  const minutes = normalizedMinutes % 60
  const parts: string[] = []
  if (days > 0) parts.push(t('quotaUi.durationDays', { value: days }))
  if (hours > 0) parts.push(t('quotaUi.durationHours', { value: hours }))
  if (minutes > 0 || parts.length === 0) {
    parts.push(t('quotaUi.durationMinutes', { value: minutes }))
  }
  return parts.join('')
}

function getCodexQuotaDisplay(key: EndpointAPIKey): CodexUpstreamMetadata | null {
  const quota = getQuotaSnapshotForProvider(key, 'codex')
  if (!quota) return null

  const display: CodexUpstreamMetadata = {}
  const updatedAt = getQuotaSnapshotUpdatedAt(quota)
  if (updatedAt !== undefined) display.updated_at = updatedAt
  if (quota.plan_type) display.plan_type = quota.plan_type

  const primaryWindow = getQuotaWindow(quota, '7d') ?? getQuotaWindow(quota, 'weekly')
  const primaryUsedPercent = getQuotaWindowUsedPercent(primaryWindow)
  if (primaryUsedPercent !== undefined) display.primary_used_percent = primaryUsedPercent
  const primaryResetAt = getQuotaWindowResetAt(primaryWindow)
  if (primaryResetAt !== undefined) display.primary_reset_at = primaryResetAt
  const primaryResetSeconds = getQuotaWindowResetSeconds(primaryWindow)
  if (primaryResetSeconds !== undefined) display.primary_reset_seconds = primaryResetSeconds
  if (typeof primaryWindow?.window_minutes === 'number') {
    display.primary_window_minutes = primaryWindow.window_minutes
  }

  const secondaryWindow = getQuotaWindow(quota, '5h')
  const secondaryUsedPercent = getQuotaWindowUsedPercent(secondaryWindow)
  if (secondaryUsedPercent !== undefined) display.secondary_used_percent = secondaryUsedPercent
  const secondaryResetAt = getQuotaWindowResetAt(secondaryWindow)
  if (secondaryResetAt !== undefined) display.secondary_reset_at = secondaryResetAt
  const secondaryResetSeconds = getQuotaWindowResetSeconds(secondaryWindow)
  if (secondaryResetSeconds !== undefined) display.secondary_reset_seconds = secondaryResetSeconds
  if (typeof secondaryWindow?.window_minutes === 'number') {
    display.secondary_window_minutes = secondaryWindow.window_minutes
  }

  const sparkPrimaryWindow = getQuotaWindow(quota, 'spark:5h') ?? getQuotaWindow(quota, 'spark_5h')
  const sparkPrimaryUsedPercent = getQuotaWindowUsedPercent(sparkPrimaryWindow)
  if (sparkPrimaryUsedPercent !== undefined) display.spark_primary_used_percent = sparkPrimaryUsedPercent
  const sparkPrimaryResetAt = getQuotaWindowResetAt(sparkPrimaryWindow)
  if (sparkPrimaryResetAt !== undefined) display.spark_primary_reset_at = sparkPrimaryResetAt
  const sparkPrimaryResetSeconds = getQuotaWindowResetSeconds(sparkPrimaryWindow)
  if (sparkPrimaryResetSeconds !== undefined) display.spark_primary_reset_seconds = sparkPrimaryResetSeconds
  if (typeof sparkPrimaryWindow?.window_minutes === 'number') {
    display.spark_primary_window_minutes = sparkPrimaryWindow.window_minutes
  }

  const sparkSecondaryWindow = getQuotaWindow(quota, 'spark:weekly')
    ?? getQuotaWindow(quota, 'spark:7d')
    ?? getQuotaWindow(quota, 'spark_weekly')
  const sparkSecondaryUsedPercent = getQuotaWindowUsedPercent(sparkSecondaryWindow)
  if (sparkSecondaryUsedPercent !== undefined) display.spark_secondary_used_percent = sparkSecondaryUsedPercent
  const sparkSecondaryResetAt = getQuotaWindowResetAt(sparkSecondaryWindow)
  if (sparkSecondaryResetAt !== undefined) display.spark_secondary_reset_at = sparkSecondaryResetAt
  const sparkSecondaryResetSeconds = getQuotaWindowResetSeconds(sparkSecondaryWindow)
  if (sparkSecondaryResetSeconds !== undefined) display.spark_secondary_reset_seconds = sparkSecondaryResetSeconds
  if (typeof sparkSecondaryWindow?.window_minutes === 'number') {
    display.spark_secondary_window_minutes = sparkSecondaryWindow.window_minutes
  }

  return Object.keys(display).length > 0 ? display : null
}

function hasCodexQuotaDisplayData(key: EndpointAPIKey): boolean {
  return getCodexQuotaWindowsForDisplay(key).length > 0
}

function getKiroQuotaDisplay(key: EndpointAPIKey): KiroUpstreamMetadata | null {
  const quota = getQuotaSnapshotForProvider(key, 'kiro')
  if (!quota) return null

  const display: KiroUpstreamMetadata = {}
  const updatedAt = getQuotaSnapshotUpdatedAt(quota)
  if (updatedAt !== undefined) display.updated_at = updatedAt
  if (quota.plan_type) display.subscription_title = quota.plan_type

  if (String(quota.code || '').trim().toLowerCase() === 'banned') {
    display.is_banned = true
    if (quota.reason) display.ban_reason = quota.reason
    if (updatedAt !== undefined) display.banned_at = updatedAt
  }

  const usageWindow =
    getQuotaWindow(quota, 'usage')
    ?? getQuotaWindowByScope(quota, 'account')[0]
    ?? null
  if (usageWindow) {
    const usedPercent = getQuotaWindowUsedPercent(usageWindow)
    if (usedPercent !== undefined) display.usage_percentage = usedPercent
    if (typeof usageWindow.used_value === 'number') display.current_usage = usageWindow.used_value
    if (typeof usageWindow.limit_value === 'number') display.usage_limit = usageWindow.limit_value
    if (typeof usageWindow.remaining_value === 'number') display.remaining = usageWindow.remaining_value

    const nextResetAt =
      getQuotaWindowResetAt(usageWindow)
      ?? (() => {
        const resetSeconds = getQuotaWindowResetSeconds(usageWindow)
        if (updatedAt === undefined || resetSeconds === undefined) return undefined
        return updatedAt + resetSeconds
      })()
    if (nextResetAt !== undefined) display.next_reset_at = nextResetAt
  }

  return Object.keys(display).length > 0 ? display : null
}

function hasKiroQuotaDisplayData(key: EndpointAPIKey): boolean {
  const kiro = getKiroQuotaDisplay(key)
  return !!kiro && (kiro.usage_percentage !== undefined || kiro.usage_limit !== undefined)
}

type GrokQuotaDisplay = GrokUpstreamMetadata & {
  usage_percentage?: number
  usage_limit?: number
  current_usage?: number
  remaining?: number
  next_reset_at?: number
}

function getGrokQuotaDisplay(key: EndpointAPIKey): GrokQuotaDisplay | null {
  const quota = getQuotaSnapshotForProvider(key, 'grok')
  if (!quota) return null

  const display: GrokQuotaDisplay = {}
  const updatedAt = getQuotaSnapshotUpdatedAt(quota)
  if (updatedAt !== undefined) display.updated_at = updatedAt
  if (quota.plan_type) display.plan_type = quota.plan_type
  if (quota.pool_tier) display.pool_tier = quota.pool_tier

  const code = String(quota.code || '').trim().toLowerCase()
  if (code === 'banned' || code === 'forbidden') {
    display.is_banned = true
    if (quota.reason) display.ban_reason = quota.reason
  }

  const usageWindow =
    getQuotaWindow(quota, 'usage')
    ?? getQuotaWindowByScope(quota, 'account')[0]
    ?? getQuotaWindowByScope(quota, 'model')
      .map(window => ({
        window,
        remainingPercent: getQuotaWindowRemainingPercent(window),
      }))
      .filter((item): item is { window: QuotaWindowSnapshot, remainingPercent: number } => item.remainingPercent !== undefined)
      .sort((a, b) => a.remainingPercent - b.remainingPercent)[0]?.window
    ?? null
  if (usageWindow) {
    const usedPercent = getQuotaWindowUsedPercent(usageWindow)
    if (usedPercent !== undefined) display.usage_percentage = usedPercent
    if (typeof usageWindow.used_value === 'number') display.current_usage = usageWindow.used_value
    if (typeof usageWindow.limit_value === 'number') display.usage_limit = usageWindow.limit_value
    if (typeof usageWindow.remaining_value === 'number') display.remaining = usageWindow.remaining_value

    const nextResetAt =
      getQuotaWindowResetAt(usageWindow)
      ?? (() => {
        const resetSeconds = getQuotaWindowResetSeconds(usageWindow)
        if (updatedAt === undefined || resetSeconds === undefined) return undefined
        return updatedAt + resetSeconds
      })()
    if (nextResetAt !== undefined) display.next_reset_at = nextResetAt
  }

  return Object.keys(display).length > 0 ? display : null
}

function hasGrokQuotaDisplayData(key: EndpointAPIKey): boolean {
  const grok = getGrokQuotaDisplay(key)
  return !!grok && (grok.usage_percentage !== undefined || grok.usage_limit !== undefined)
}

type ChatGPTWebQuotaDisplay = ChatGPTWebUpstreamMetadata & {
  image_quota_remaining_percent?: number
  image_quota_used_percent?: number
}

function getChatGPTWebQuotaDisplay(key: EndpointAPIKey): ChatGPTWebQuotaDisplay | null {
  const quota = getQuotaSnapshotForProvider(key, 'chatgpt_web')
  if (!quota) return null

  const display: ChatGPTWebQuotaDisplay = {}
  const updatedAt = getQuotaSnapshotUpdatedAt(quota)
  if (updatedAt !== undefined) display.updated_at = updatedAt
  if (quota.plan_type) display.plan_type = quota.plan_type
  if (quota.code === 'exhausted' || quota.code === 'banned') display.image_quota_blocked = true

  const imageWindow =
    getQuotaWindow(quota, 'image_gen')
    ?? getQuotaWindowByScope(quota, 'account')[0]
    ?? null
  if (imageWindow) {
    const remainingValue = typeof imageWindow.remaining_value === 'number' ? imageWindow.remaining_value : undefined
    const limitValue = typeof imageWindow.limit_value === 'number' ? imageWindow.limit_value : undefined
    const usedValue = typeof imageWindow.used_value === 'number' ? imageWindow.used_value : undefined
    const remainingPercent = getQuotaWindowRemainingPercent(imageWindow)
    const usedPercent = getQuotaWindowUsedPercent(imageWindow)

    if (remainingValue !== undefined) display.image_quota_remaining = remainingValue
    if (limitValue !== undefined) display.image_quota_total = limitValue
    if (usedValue !== undefined) display.image_quota_used = usedValue
    if (remainingPercent !== undefined) display.image_quota_remaining_percent = remainingPercent
    if (usedPercent !== undefined) display.image_quota_used_percent = usedPercent
    if (typeof imageWindow.reset_at === 'number') display.image_quota_reset_at = imageWindow.reset_at
    if (typeof imageWindow.reset_seconds === 'number') {
      const resetAt = updatedAt === undefined ? undefined : updatedAt + imageWindow.reset_seconds
      if (resetAt !== undefined && display.image_quota_reset_at === undefined) {
        display.image_quota_reset_at = resetAt
      }
    }
  }

  return Object.keys(display).length > 0 ? display : null
}

function hasChatGPTWebQuotaDisplayData(key: EndpointAPIKey): boolean {
  const display = getChatGPTWebQuotaDisplay(key)
  return !!display && (
    display.image_quota_remaining_percent !== undefined
    || display.image_quota_total !== undefined
    || display.image_quota_used !== undefined
  )
}

function getChatGPTWebQuotaUsedPercent(key: EndpointAPIKey): number {
  const display = getChatGPTWebQuotaDisplay(key)
  if (!display) return 0
  if (typeof display.image_quota_used_percent === 'number') return display.image_quota_used_percent
  if (typeof display.image_quota_remaining_percent === 'number') {
    return Math.max(100 - display.image_quota_remaining_percent, 0)
  }
  return 0
}

function getChatGPTWebQuotaRemainingPercent(key: EndpointAPIKey): number {
  const display = getChatGPTWebQuotaDisplay(key)
  if (!display) return 0
  if (typeof display.image_quota_remaining_percent === 'number') return display.image_quota_remaining_percent
  if (typeof display.image_quota_used_percent === 'number') {
    return Math.max(100 - display.image_quota_used_percent, 0)
  }
  return 0
}

function formatChatGPTWebUsage(value: number | null | undefined): string {
  if (value === undefined || value === null) return '-'
  if (Math.abs(value - Math.round(value)) < 1e-6) {
    return String(Math.round(value))
  }
  return value.toFixed(1)
}

function isKiroBannedKey(key: EndpointAPIKey): boolean {
  const quota = getQuotaSnapshotForProvider(key, 'kiro')
  return String(quota?.code || '').trim().toLowerCase() === 'banned'
}

// 格式化封禁/禁止时间（后端返回秒级时间戳，Kiro/Antigravity 通用）
function formatBanTimestamp(timestamp: number | undefined): string {
  if (!timestamp) return ''
  const date = new Date(timestamp * 1000)
  return date.toLocaleString('zh-CN', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function isAntigravityForbiddenKey(key: EndpointAPIKey): boolean {
  const quota = getQuotaSnapshotForProvider(key, 'antigravity')
  return String(quota?.code || '').trim().toLowerCase() === 'forbidden'
}

function getAntigravityForbiddenReason(key: EndpointAPIKey): string | undefined {
  const quota = getQuotaSnapshotForProvider(key, 'antigravity')
  return quota?.reason || undefined
}

function getAntigravityForbiddenAt(key: EndpointAPIKey): number | undefined {
  return getQuotaSnapshotUpdatedAt(getQuotaSnapshotForProvider(key, 'antigravity'))
}

function getAntigravityQuotaUpdatedAt(key: EndpointAPIKey): number | undefined {
  return getQuotaSnapshotUpdatedAt(getQuotaSnapshotForProvider(key, 'antigravity'))
}

// 格式化 Kiro 更新时间
const formatKiroUpdatedAt = formatUpdatedAt

// 格式化 Kiro 使用量（带单位）
function formatKiroUsage(value: number | undefined): string {
  if (value === undefined || value === null) return '-'
  if (value >= 1000000) {
    return `${(value / 1000000).toFixed(1)}M`
  }
  if (value >= 1000) {
    return `${(value / 1000).toFixed(1)}K`
  }
  return value.toFixed(1)
}

// 格式化 Kiro 重置时间
function formatKiroResetTime(timestamp: number | null | undefined): string {
  if (!timestamp) return ''
  // timestamp 可能是毫秒或秒，需要判断
  const ts = timestamp > 1e12 ? timestamp : timestamp * 1000
  const now = Date.now()
  const diff = ts - now

  if (diff <= 0) {
    return t('providerDetail.reset')
  }

  const days = Math.floor(diff / (1000 * 60 * 60 * 24))
  const hours = Math.floor((diff % (1000 * 60 * 60 * 24)) / (1000 * 60 * 60))

  if (days > 0) {
    return t('providerDetail.daysHoursLater', { days, hours })
  }

  const minutes = Math.floor((diff % (1000 * 60 * 60)) / (1000 * 60))
  if (hours > 0) {
    return t('providerDetail.hoursMinutesLater', { hours, minutes })
  }

  return t('providerDetail.minutesLater', { minutes })
}

// 格式化 Kiro 订阅类型显示
function formatKiroSubscription(title: string | undefined): string {
  if (!title) return ''
  // 简化显示：KIRO PRO+ -> Pro+, KIRO FREE -> Free（首字母大写，与 Codex 保持一致）
  const upper = title.toUpperCase()
  if (upper.includes('POWER')) return 'Power'
  if (upper.includes('PRO+')) return 'Pro+'
  if (upper.includes('PRO')) return 'Pro'
  if (upper.includes('FREE')) return 'Free'
  return title
}

function getKiroSubscriptionTitle(key: EndpointAPIKey): string | undefined {
  return getKiroQuotaDisplay(key)?.subscription_title
}

function getKiroSubscriptionBadgeLabel(key: EndpointAPIKey): string {
  return formatKiroSubscription(getKiroSubscriptionTitle(key))
}

function shouldShowKiroSubscriptionBadge(key: EndpointAPIKey): boolean {
  if (provider.value?.provider_type !== 'kiro') return false

  const kiroLabel = getKiroSubscriptionBadgeLabel(key)
  if (!kiroLabel) return false

  const oauthPlanLabel = formatOAuthPlanType(key.oauth_plan_type)
  if (!oauthPlanLabel) return true

  return oauthPlanLabel.trim().toLowerCase() !== kiroLabel.trim().toLowerCase()
}

function shouldAutoRefreshCodexQuota(): boolean {
  if (provider.value?.provider_type !== 'codex') return false
  const now = Math.floor(Date.now() / 1000)

  for (const { key } of allKeys.value) {
    if (!key.is_active) continue

    if (isTokenExpiringSoon(key, now)) return true

    // 只要有一个活跃 key 没有配额数据，就刷新一次
    if (!hasCodexQuotaDisplayData(key)) {
      return true
    }
    // 配额数据超过 5 分钟未更新，也触发刷新
    const updatedAt = getCodexQuotaDisplay(key)?.updated_at
    if (typeof updatedAt !== 'number' || (now - updatedAt) > AUTO_QUOTA_REFRESH_STALE_SECONDS) {
      return true
    }
  }

  return false
}

// 检查 OAuth Token 是否即将过期（Codex / Antigravity / Kiro / ChatGPT Web）
function isTokenExpiringSoon(key: EndpointAPIKey, now: number): boolean {
  const oauthCode = String(key.status_snapshot?.oauth?.code || '').trim().toLowerCase()
  if (oauthCode && oauthCode !== 'valid' && oauthCode !== 'expiring') {
    return false
  }
  return typeof key.oauth_expires_at === 'number'
    && (key.oauth_expires_at - now) <= AUTO_TOKEN_REFRESH_SKEW_SECONDS
}

function shouldAutoRefreshAntigravityQuota(): boolean {
  if (provider.value?.provider_type !== 'antigravity') return false
  const now = Math.floor(Date.now() / 1000)

  for (const { key } of allKeys.value) {
    if (!key.is_active) continue

    if (isTokenExpiringSoon(key, now)) return true

    // 只要有一个活跃 key 没有配额/为空/过期，就刷新一次（接口会批量刷新所有活跃 key）
    if (!hasAntigravityQuotaDisplayData(key)) {
      return true
    }
    const updatedAt = getAntigravityQuotaUpdatedAt(key)
    if (typeof updatedAt !== 'number' || (now - updatedAt) > AUTO_QUOTA_REFRESH_STALE_SECONDS) {
      return true
    }
  }

  return false
}

function shouldAutoRefreshKiroQuota(): boolean {
  if (provider.value?.provider_type !== 'kiro') return false
  const now = Math.floor(Date.now() / 1000)

  for (const { key } of allKeys.value) {
    if (!key.is_active) continue

    if (isTokenExpiringSoon(key, now)) return true

    // 只要有一个活跃 key 没有配额数据，就刷新一次
    if (!hasKiroQuotaDisplayData(key)) {
      return true
    }
    // 配额数据超过 5 分钟未更新，也触发刷新
    const updatedAt = getKiroQuotaDisplay(key)?.updated_at
    if (typeof updatedAt !== 'number' || (now - updatedAt) > AUTO_QUOTA_REFRESH_STALE_SECONDS) {
      return true
    }
  }

  return false
}

function shouldAutoRefreshGrokQuota(): boolean {
  if (provider.value?.provider_type !== 'grok') return false
  const now = Math.floor(Date.now() / 1000)

  for (const { key } of allKeys.value) {
    if (!key.is_active) continue

    if (isTokenExpiringSoon(key, now)) return true

    if (!hasGrokQuotaDisplayData(key)) {
      return true
    }

    const updatedAt = getGrokQuotaDisplay(key)?.updated_at
    if (typeof updatedAt !== 'number' || (now - updatedAt) > AUTO_QUOTA_REFRESH_STALE_SECONDS) {
      return true
    }
  }

  return false
}

function shouldAutoRefreshChatGPTWebQuota(): boolean {
  if (provider.value?.provider_type !== 'chatgpt_web') return false
  const now = Math.floor(Date.now() / 1000)

  for (const { key } of allKeys.value) {
    if (!key.is_active) continue

    if (isTokenExpiringSoon(key, now)) return true

    if (!hasChatGPTWebQuotaDisplayData(key)) {
      return true
    }

    const updatedAt = getChatGPTWebQuotaDisplay(key)?.updated_at
    if (typeof updatedAt !== 'number' || (now - updatedAt) > AUTO_QUOTA_REFRESH_STALE_SECONDS) {
      return true
    }
  }

  return false
}

function defaultQuotaSnapshot(): QuotaStatusSnapshot {
  return {
    code: 'unknown',
    exhausted: false,
    usage_ratio: null,
    updated_at: null,
    reset_seconds: null,
    plan_type: null,
  }
}

function wrapQuotaMetadataForProvider(
  providerType: string,
  metadata: Record<string, unknown> | undefined,
): UpstreamMetadata | null {
  if (!metadata) return null
  if (providerType in metadata) {
    return metadata as UpstreamMetadata
  }
  return { [providerType]: metadata } as UpstreamMetadata
}

// 将配额刷新结果就地应用到现有 key 上，避免重新拉列表导致分页重置
function applyQuotaResults(
  results: { key_id: string; status: string; metadata?: Record<string, unknown>; quota_snapshot?: QuotaStatusSnapshot }[],
): number {
  const providerType = provider.value?.provider_type
  if (!providerType) return 0

  let applied = 0
  for (const r of results) {
    const target = providerKeys.value.find(k => k.id === r.key_id)
    if (!target) continue

    let changed = false
    const wrappedMetadata = wrapQuotaMetadataForProvider(providerType, r.metadata)
    if (wrappedMetadata) {
      target.upstream_metadata = { ...target.upstream_metadata, ...wrappedMetadata } as typeof target.upstream_metadata
      changed = true
    }

    if (r.quota_snapshot) {
      target.status_snapshot = {
        oauth: target.status_snapshot?.oauth ?? {
          code: 'none',
          label: null,
          reason: null,
          expires_at: null,
          invalid_at: null,
          source: null,
          requires_reauth: false,
          expiring_soon: false,
        },
        account: target.status_snapshot?.account ?? {
          code: 'ok',
          label: null,
          reason: null,
          blocked: false,
          source: null,
          recoverable: false,
        },
        quota: {
          ...defaultQuotaSnapshot(),
          ...(target.status_snapshot?.quota ?? {}),
          ...r.quota_snapshot,
        },
      }
      changed = true
    }

    if (changed) {
      applied += 1
    }
  }
  return applied
}

// 通用的自动刷新配额函数（支持 Codex、Antigravity、Kiro 和 ChatGPT Web）
async function autoRefreshQuotaInBackground(options: { ignoreCooldown?: boolean } = {}) {
  const providerId = props.providerId
  if (!providerId) return
  if (providerReadOnly.value) return
  if (refreshingQuota.value) return

  const providerType = provider.value?.provider_type
  if (providerType !== 'codex' && providerType !== 'antigravity' && providerType !== 'kiro' && providerType !== 'chatgpt_web' && providerType !== 'grok') return

  // 检查是否需要刷新
  let shouldRefresh = false
  if (providerType === 'codex') {
    shouldRefresh = shouldAutoRefreshCodexQuota()
  } else if (providerType === 'antigravity') {
    shouldRefresh = shouldAutoRefreshAntigravityQuota()
  } else if (providerType === 'kiro') {
    shouldRefresh = shouldAutoRefreshKiroQuota()
  } else if (providerType === 'grok') {
    shouldRefresh = shouldAutoRefreshGrokQuota()
  } else if (providerType === 'chatgpt_web') {
    shouldRefresh = shouldAutoRefreshChatGPTWebQuota()
  }
  if (!shouldRefresh) return
  if (!options.ignoreCooldown && isProviderQuotaAutoRefreshCoolingDown(providerId)) return

  let hadCachedQuota = false
  if (providerType === 'codex') {
    hadCachedQuota = allKeys.value.some(({ key }) => key.is_active && hasCodexQuotaDisplayData(key))
  } else if (providerType === 'antigravity') {
    hadCachedQuota = allKeys.value.some(({ key }) => key.is_active && hasAntigravityQuotaDisplayData(key))
  } else if (providerType === 'kiro') {
    hadCachedQuota = allKeys.value.some(({ key }) => key.is_active && hasKiroQuotaDisplayData(key))
  } else if (providerType === 'grok') {
    hadCachedQuota = allKeys.value.some(({ key }) => key.is_active && hasGrokQuotaDisplayData(key))
  } else if (providerType === 'chatgpt_web') {
    hadCachedQuota = allKeys.value.some(({ key }) => key.is_active && hasChatGPTWebQuotaDisplayData(key))
  }

  refreshingQuota.value = true
  markProviderQuotaAutoRefreshAttempt(providerId)
  try {
    const result = await refreshProviderQuota(providerId)
    const applied = applyQuotaResults(result.results)
    if (result.success <= 0 && applied === 0 && !hadCachedQuota && providerType === 'antigravity') {
      showError(t('providerDetail.noQuotaInfo'), t('providerDetail.notice'))
    }
  } catch (err: unknown) {
    if (!hadCachedQuota && providerType === 'antigravity') {
      showError(parseApiError(err, t('providerDetail.quotaRefreshFailed')), t('providerDetail.error'))
    }
  } finally {
    refreshingQuota.value = false
  }
}

async function openAntigravityQuotaDialog(key: EndpointAPIKey) {
  antigravityQuotaDialogKey.value = key
  antigravityQuotaDialogOpen.value = true

  // 没有配额数据时主动获取
  if (!hasAntigravityQuotaDisplayData(key)) {
    if (isKeyReadOnly(key)) return
    if (refreshingQuota.value) return
    refreshingQuota.value = true
    try {
      const providerId = props.providerId
      if (!providerId) return
      const result = await refreshProviderQuota(providerId)
      applyQuotaResults(result.results)
      // 更新弹窗引用的 key 数据
      const updated = allKeys.value.find(({ key: k }) => k.id === key.id)
      if (updated) {
        antigravityQuotaDialogKey.value = updated.key
      }
    } catch {
      // 静默失败，弹窗会显示"暂无配额数据"
    } finally {
      refreshingQuota.value = false
    }
  }
}

async function handleKeyChanged() {
  await Promise.all([loadEndpoints(), loadMappingPreview()])
  emit('refresh')
  // 添加/修改 key 后自动获取 Antigravity 配额（新 key 的 upstream_metadata 为空）
  void autoRefreshQuotaInBackground({ ignoreCooldown: true })
}

// 切换密钥启用状态
async function toggleKeyActive(key: EndpointAPIKey) {
  if (togglingKeyId.value) return
  if (isKeyReadOnly(key)) {
    showProviderReadOnly()
    return
  }

  togglingKeyId.value = key.id
  try {
    const newStatus = !key.is_active
    await updateProviderKey(key.id, { is_active: newStatus })
    key.is_active = newStatus
    showSuccess(newStatus ? t('providerDetail.keyEnabled') : t('providerDetail.keyDisabled'))
    emit('refresh')
  } catch (err: unknown) {
    showError(parseApiError(err, t('providerDetail.operationFailed')), t('providerDetail.error'))
  } finally {
    togglingKeyId.value = null
  }
}

// ===== Key 级别代理配置 =====

/** 获取 Key 当前代理节点的名称（用于显示） */
function getKeyProxyNodeName(key: EndpointAPIKey): string | null {
  if (!key.proxy?.node_id) return null
  const node = proxyNodesStore.nodes.find(n => n.id === key.proxy?.node_id)
  return node ? node.name : `${key.proxy.node_id.slice(0, 8)  }...`
}

/** 切换代理 Popover 的打开/关闭状态 */
function handleProxyPopoverToggle(keyId: string, open: boolean) {
  const key = providerKeys.value.find(item => item.id === keyId)
  if (open && key && isKeyReadOnly(key)) {
    showProviderReadOnly()
    proxyPopoverOpenKeyId.value = null
    return
  }
  proxyPopoverOpenKeyId.value = open ? keyId : null
  if (open) {
    proxyNodesStore.ensureLoaded()
  }
}

/** 设置 Key 的代理节点 */
async function setKeyProxy(key: EndpointAPIKey, nodeId: string) {
  if (isKeyReadOnly(key)) {
    showProviderReadOnly()
    return
  }
  savingProxyKeyId.value = key.id
  try {
    await updateProviderKey(key.id, {
      proxy: { node_id: nodeId, enabled: true },
    })
    key.proxy = { node_id: nodeId, enabled: true }
    proxyPopoverOpenKeyId.value = null
    showSuccess(t('providerDetail.proxySet'))
    emit('refresh')
  } catch (err: unknown) {
    showError(parseApiError(err, t('providerDetail.proxySetFailed')), t('providerDetail.error'))
  } finally {
    savingProxyKeyId.value = null
  }
}

/** 清除 Key 的代理节点（回退到 Provider 级别代理） */
async function clearKeyProxy(key: EndpointAPIKey) {
  if (isKeyReadOnly(key)) {
    showProviderReadOnly()
    return
  }
  savingProxyKeyId.value = key.id
  try {
    await updateProviderKey(key.id, { proxy: null })
    key.proxy = null
    proxyPopoverOpenKeyId.value = null
    showSuccess(t('providerDetail.accountProxyCleared'))
    emit('refresh')
  } catch (err: unknown) {
    showError(parseApiError(err, t('providerDetail.proxyClearFailed')), t('providerDetail.error'))
  } finally {
    savingProxyKeyId.value = null
  }
}

// ===== 模型事件处理 =====
// 处理编辑模型
function handleEditModel(model: Model) {
  if (isModelReadOnly(model)) {
    showProviderReadOnly()
    return
  }
  editingModel.value = model
  modelFormDialogOpen.value = true
}

// 处理打开批量关联对话框
function handleBatchAssign() {
  if (providerReadOnly.value) {
    showProviderReadOnly()
    return
  }
  batchAssignDialogOpen.value = true
}

// 处理打开上游模型导入对话框
function handleImportUpstream() {
  if (providerReadOnly.value) {
    showProviderReadOnly()
    return
  }
  upstreamModelsDialogOpen.value = true
}

// 处理批量关联完成
async function handleBatchAssignChanged() {
  await Promise.all([loadEndpoints(), loadMappingPreview()])
  emit('refresh')
}

// 处理上游模型导入完成
async function handleUpstreamModelsImported() {
  upstreamModelsDialogOpen.value = false
  await Promise.all([loadEndpoints(), loadMappingPreview()])
  emit('refresh')
}

// 处理模型映射变更
async function handleModelMappingChanged() {
  await Promise.all([loadEndpoints(), loadMappingPreview()])
  emit('refresh')
}

// 处理模型保存完成
async function handleModelSaved() {
  editingModel.value = null
  await Promise.all([loadEndpoints(), loadMappingPreview()])
  emit('refresh')
}

// ===== 点击编辑优先级 =====
function startEditPriority(key: EndpointAPIKey) {
  if (isKeyReadOnly(key)) {
    showProviderReadOnly()
    return
  }
  editingPriorityKey.value = key.id
  editingPriorityValue.value = key.internal_priority ?? 0
  prioritySaving.value = false
  nextTick(() => {
    // v-for 中的 ref 是数组，取第一个元素
    const input = Array.isArray(priorityInputRef.value) ? priorityInputRef.value[0] : priorityInputRef.value
    input?.focus()
    input?.select()
  })
}

function cancelEditPriority() {
  editingPriorityKey.value = null
  prioritySaving.value = false
}

function handlePriorityKeydown(e: KeyboardEvent, key: EndpointAPIKey) {
  if (e.key === 'Enter') {
    e.preventDefault()
    e.stopPropagation()
    if (!prioritySaving.value) {
      prioritySaving.value = true
      savePriority(key)
    }
  } else if (e.key === 'Escape') {
    e.preventDefault()
    cancelEditPriority()
  }
}

function handlePriorityBlur(key: EndpointAPIKey) {
  // 如果已经在保存中（Enter触发），不重复保存
  if (prioritySaving.value) return
  savePriority(key)
}

async function savePriority(key: EndpointAPIKey) {
  if (isKeyReadOnly(key)) {
    showProviderReadOnly()
    cancelEditPriority()
    return
  }
  const keyId = editingPriorityKey.value
  const newPriority = parseInt(String(editingPriorityValue.value), 10) || 0

  if (!keyId || newPriority < 0) {
    cancelEditPriority()
    return
  }

  // 如果优先级没有变化，直接取消编辑
  if (key.internal_priority === newPriority) {
    cancelEditPriority()
    return
  }

  cancelEditPriority()

  try {
    await updateProviderKey(keyId, { internal_priority: newPriority })
    showSuccess(t('providerDetail.priorityUpdated'))
    // 更新本地数据 - 更新 providerKeys 中的数据
    const keyToUpdate = providerKeys.value.find(k => k.id === keyId)
    if (keyToUpdate) {
      keyToUpdate.internal_priority = newPriority
    }
    // 重新排序
    providerKeys.value.sort((a, b) => (a.internal_priority ?? 0) - (b.internal_priority ?? 0))
    emit('refresh')
  } catch (err: unknown) {
    showError(parseApiError(err, t('providerDetail.priorityUpdateFailed')), t('providerDetail.error'))
  }
}

// ===== 点击编辑倍率 =====
function startEditMultiplier(key: EndpointAPIKey, format: string) {
  if (isKeyReadOnly(key)) {
    showProviderReadOnly()
    return
  }
  editingMultiplierKey.value = key.id
  editingMultiplierFormat.value = format
  editingMultiplierValue.value = getKeyRateMultiplier(key, format)
  multiplierSaving.value = false
  nextTick(() => {
    const input = Array.isArray(multiplierInputRef.value) ? multiplierInputRef.value[0] : multiplierInputRef.value
    input?.focus()
    input?.select()
  })
}

function cancelEditMultiplier() {
  editingMultiplierKey.value = null
  editingMultiplierFormat.value = null
}

function handleMultiplierKeydown(e: KeyboardEvent, key: EndpointAPIKey, format: string) {
  if (e.key === 'Enter') {
    e.preventDefault()
    e.stopPropagation()
    saveMultiplier(key, format)
  } else if (e.key === 'Escape') {
    e.preventDefault()
    multiplierSaving.value = true // 阻止 blur 触发保存
    cancelEditMultiplier()
  }
}

function handleMultiplierBlur(key: EndpointAPIKey, format: string) {
  if (multiplierSaving.value) return
  saveMultiplier(key, format)
}

async function saveMultiplier(key: EndpointAPIKey, format: string) {
  if (isKeyReadOnly(key)) {
    showProviderReadOnly()
    cancelEditMultiplier()
    multiplierSaving.value = false
    return
  }
  // 防止重复调用（Enter 触发后阻止 blur 再次进入）
  if (multiplierSaving.value) return
  multiplierSaving.value = true

  const keyId = editingMultiplierKey.value
  const newMultiplier = parseFloat(String(editingMultiplierValue.value))

  // 验证输入有效性
  if (!keyId || isNaN(newMultiplier)) {
    showError(t('providerDetail.invalidMultiplier'))
    cancelEditMultiplier()
    multiplierSaving.value = false
    return
  }

  // 验证合理范围
  if (newMultiplier <= 0 || newMultiplier > 100) {
    showError(t('providerDetail.multiplierRange'))
    cancelEditMultiplier()
    multiplierSaving.value = false
    return
  }

  // 如果倍率没有变化,直接取消编辑（使用精度容差比较浮点数）
  const currentMultiplier = getKeyRateMultiplier(key, format)
  if (Math.abs(currentMultiplier - newMultiplier) < 0.0001) {
    cancelEditMultiplier()
    multiplierSaving.value = false
    return
  }

  cancelEditMultiplier()

  try {
    // 构建 rate_multipliers 对象
    const rateMultipliers = { ...(key.rate_multipliers || {}) }
    rateMultipliers[format] = newMultiplier

    await updateProviderKey(keyId, { rate_multipliers: rateMultipliers })
    showSuccess(t('providerDetail.multiplierUpdated'))

    // 更新本地数据
    const keyToUpdate = providerKeys.value.find(k => k.id === keyId)
    if (keyToUpdate) {
      keyToUpdate.rate_multipliers = rateMultipliers
    }
    emit('refresh')
  } catch (err: unknown) {
    showError(parseApiError(err, t('providerDetail.multiplierUpdateFailed')), t('providerDetail.error'))
  } finally {
    multiplierSaving.value = false
  }
}

// ===== 密钥列表拖拽排序 =====
function handleKeyDragStart(event: DragEvent, index: number) {
  keyDragState.value.isDragging = true
  keyDragState.value.draggedIndex = index
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData('text/plain', String(index))
  }
}

function handleKeyDragEnd() {
  keyDragState.value.isDragging = false
  keyDragState.value.draggedIndex = null
  keyDragState.value.targetIndex = null
}

function handleKeyDragOver(event: DragEvent, index: number) {
  event.preventDefault()
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = 'move'
  }
  if (keyDragState.value.draggedIndex !== index) {
    keyDragState.value.targetIndex = index
  }
}

function handleKeyDragLeave() {
  keyDragState.value.targetIndex = null
}

async function handleKeyDrop(event: DragEvent, targetIndex: number) {
  event.preventDefault()
  if (providerReadOnly.value) {
    showProviderReadOnly()
    handleKeyDragEnd()
    return
  }

  const draggedIndex = keyDragState.value.draggedIndex
  if (draggedIndex === null || draggedIndex === targetIndex) {
    handleKeyDragEnd()
    return
  }

  const keys = allKeys.value.map(item => item.key)
  if (draggedIndex < 0 || draggedIndex >= keys.length || targetIndex < 0 || targetIndex >= keys.length) {
    handleKeyDragEnd()
    return
  }

  const draggedKey = keys[draggedIndex]
  const targetKey = keys[targetIndex]
  if (isKeyReadOnly(draggedKey) || isKeyReadOnly(targetKey)) {
    showProviderReadOnly()
    handleKeyDragEnd()
    return
  }
  const draggedPriority = draggedKey.internal_priority ?? 0
  const targetPriority = targetKey.internal_priority ?? 0

  // 如果是同组内拖拽（同优先级），忽略操作
  if (draggedPriority === targetPriority) {
    handleKeyDragEnd()
    return
  }

  handleKeyDragEnd()

  try {
    // 记录每个 key 的原始优先级
    const originalPriorityMap = new Map<string, number>()
    keys.forEach(k => {
      originalPriorityMap.set(k.id, k.internal_priority ?? 0)
    })

    // 重排数组：将被拖动项移到目标位置
    const items = [...keys]
    items.splice(draggedIndex, 1)
    items.splice(targetIndex, 0, draggedKey)

    // 按新顺序分配优先级：被拖动项单独成组，其他同组项保持在一起
    const groupNewPriority = new Map<number, number>()
    let currentPriority = 1
    const newPriorityMap = new Map<string, number>()

    items.forEach(key => {
      const originalPriority = originalPriorityMap.get(key.id) ?? 0

      if (key === draggedKey) {
        // 被拖动的项单独成组
        newPriorityMap.set(key.id, currentPriority)
        currentPriority++
      } else {
        if (groupNewPriority.has(originalPriority)) {
          // 同组的其他项使用相同的新优先级
          newPriorityMap.set(key.id, groupNewPriority.get(originalPriority) ?? currentPriority)
        } else {
          // 新组，分配新优先级
          groupNewPriority.set(originalPriority, currentPriority)
          newPriorityMap.set(key.id, currentPriority)
          currentPriority++
        }
      }
    })

    // 更新所有优先级发生变化的 key
    const updatePromises = keys.map(key => {
      const oldPriority = key.internal_priority ?? 0
      const newPriority = newPriorityMap.get(key.id)
      if (newPriority !== undefined && oldPriority !== newPriority) {
        return updateProviderKey(key.id, { internal_priority: newPriority })
      }
      return Promise.resolve()
    })

    await Promise.all(updatePromises)
    showSuccess(t('providerDetail.priorityUpdated'))
    await loadEndpoints()
    emit('refresh')
  } catch (err: unknown) {
    showError(parseApiError(err, t('providerDetail.priorityUpdateFailed')), t('providerDetail.error'))
    await loadEndpoints()
  }
}

// 获取密钥的 API 格式列表（按指定顺序排序）
function getKeyApiFormats(key: EndpointAPIKey, endpoint?: ProviderEndpointWithKeys): string[] {
  const providerType = provider.value?.provider_type
  let formats: string[] = []

  if (
    providerType
    && isOAuthAccountProviderType(providerType)
    && isOAuthManagedCredential(key)
  ) {
    formats = [...availableKeyApiFormats.value]
  } else if (key.api_formats && key.api_formats.length > 0) {
    formats = [...key.api_formats]
  } else if (endpoint) {
    formats = [endpoint.api_format]
  }
  // 使用统一的排序函数
  return sortApiFormats(formats)
}

// 获取密钥在指定 API 格式下的成本倍率
function getKeyRateMultiplier(key: EndpointAPIKey, format: string): number {
  if (key.rate_multipliers && key.rate_multipliers[format] !== undefined) {
    return key.rate_multipliers[format]
  }
  return 1.0
}

// OAuth 订阅类型格式化
function formatOAuthPlanType(planType?: string | null): string {
  if (!planType) return ''
  const labels: Record<string, string> = {
    plus: 'Plus',
    pro: 'Pro',
    free: 'Free',
    paid: 'Paid',
    team: 'Team',
    enterprise: 'Enterprise',
    ultra: 'Ultra',
    basic: 'Basic',
    super: 'Super',
    heavy: 'Heavy',
  }
  return labels[planType.toLowerCase()] || planType
}

// Codex 剩余额度样式（基于已用百分比计算剩余）
function getQuotaRemainingClass(usedPercent: number): string {
  const remaining = 100 - usedPercent
  if (remaining <= 10) return 'text-red-600 dark:text-red-400'
  if (remaining <= 30) return 'text-yellow-600 dark:text-yellow-400'
  return 'text-green-600 dark:text-green-400'
}

// Codex 剩余额度进度条颜色
function getQuotaRemainingBarColor(usedPercent: number): string {
  const remaining = 100 - usedPercent
  if (remaining <= 10) return 'bg-red-500 dark:bg-red-400'
  if (remaining <= 30) return 'bg-yellow-500 dark:bg-yellow-400'
  return 'bg-green-500 dark:bg-green-400'
}

interface AntigravityQuotaItem {
  model: string
  label: string
  usedPercent: number
  remainingPercent: number
  resetSeconds: number | null
}

function hasAntigravityQuotaData(metadata: UpstreamMetadata | null | undefined): boolean {
  const quotaByModel = metadata?.antigravity?.quota_by_model
  return !!quotaByModel && typeof quotaByModel === 'object' && Object.keys(quotaByModel).length > 0
}

function hasAntigravityQuotaDisplayData(key: EndpointAPIKey): boolean {
  const quota = getQuotaSnapshotForProvider(key, 'antigravity')
  if (Array.isArray(quota?.windows) && quota.windows.length > 0) {
    return true
  }
  return hasAntigravityQuotaData(key.upstream_metadata)
}

function formatUpdatedAt(updatedAt: number): string {
  if (!updatedAt || typeof updatedAt !== 'number') return ''
  const now = Math.floor(Date.now() / 1000)
  const diff = now - updatedAt
  if (diff <= 60) return t('providerDetail.justUpdated')
  const minutes = Math.floor(diff / 60)
  if (minutes < 60) return t('providerDetail.minutesAgoUpdated', { count: minutes })
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return t('providerDetail.hoursAgoUpdated', { count: hours })
  const days = Math.floor(hours / 24)
  return t('providerDetail.daysAgoUpdated', { count: days })
}

// 兼容旧函数名
const formatCodexUpdatedAt = formatUpdatedAt
const formatAntigravityUpdatedAt = formatUpdatedAt

function secondsUntilReset(resetTime: string): number | null {
  if (!resetTime) return null
  const ts = Date.parse(resetTime)
  if (Number.isNaN(ts)) return null
  const diff = Math.floor((ts - Date.now()) / 1000)
  return diff > 0 ? diff : 0
}

function getAntigravityQuotaItems(metadata: UpstreamMetadata | null | undefined): AntigravityQuotaItem[] {
  const quotaByModel = metadata?.antigravity?.quota_by_model
  if (!quotaByModel || typeof quotaByModel !== 'object') return []

  const items: AntigravityQuotaItem[] = []
  for (const [model, rawInfo] of Object.entries(quotaByModel)) {
    if (!model) continue
    const info: Partial<AntigravityModelQuota> = rawInfo || {}

    let usedPercent = Number(info.used_percent)
    if (!Number.isFinite(usedPercent)) {
      const remainingFraction = Number(info.remaining_fraction)
      if (Number.isFinite(remainingFraction)) {
        usedPercent = (1 - remainingFraction) * 100
      } else {
        continue
      }
    }

    if (usedPercent < 0) usedPercent = 0
    if (usedPercent > 100) usedPercent = 100

    const remainingPercent = Math.max(100 - usedPercent, 0)

    let resetSeconds: number | null = null
    if (typeof info.reset_time === 'string' && info.reset_time.trim()) {
      resetSeconds = secondsUntilReset(info.reset_time.trim())
    }

    items.push({
      model,
      label: model,
      usedPercent,
      remainingPercent,
      resetSeconds,
    })
  }

  // 按“最紧张”（已用最多）优先排序，便于快速定位额度风险；完整列表通过滚动展示
  items.sort((a, b) => (b.usedPercent - a.usedPercent) || a.model.localeCompare(b.model))
  return items
}

function getAntigravityQuotaItemsFromSnapshot(key: EndpointAPIKey): AntigravityQuotaItem[] {
  const quota = getQuotaSnapshotForProvider(key, 'antigravity')
  const windows = getQuotaWindowByScope(quota, 'model')
  if (!quota || windows.length === 0) return []

  const items = windows
    .map((window) => {
      const model = String(window.model || window.label || window.code || '').trim()
      if (!model) return null

      const usedPercent = getQuotaWindowUsedPercent(window)
      const remainingPercent = getQuotaWindowRemainingPercent(window)
      if (usedPercent === undefined && remainingPercent === undefined) {
        return null
      }

      const normalizedUsedPercent =
        usedPercent !== undefined
          ? usedPercent
          : Math.max(100 - (remainingPercent ?? 0), 0)
      const normalizedRemainingPercent =
        remainingPercent !== undefined
          ? remainingPercent
          : Math.max(100 - normalizedUsedPercent, 0)

      return {
        model,
        label: String(window.label || window.model || model),
        usedPercent: normalizedUsedPercent,
        remainingPercent: normalizedRemainingPercent,
        resetSeconds: getQuotaWindowLiveResetSeconds(quota, window),
      } satisfies AntigravityQuotaItem
    })
    .filter((item): item is AntigravityQuotaItem => item !== null)

  items.sort((a, b) => (b.usedPercent - a.usedPercent) || a.model.localeCompare(b.model))
  return items
}

// Antigravity 配额分组定义（按匹配优先级排列，具体规则在前）
interface AntigravityQuotaGroup {
  key: string
  label: string
  match: (model: string) => boolean
}

const ANTIGRAVITY_QUOTA_GROUPS: AntigravityQuotaGroup[] = [
  { key: 'claude', label: 'Claude', match: m => m.includes('claude') },
  { key: 'gemini-2.5', label: 'Gemini 2.5', match: m => m.includes('gemini-2.5') || m.includes('gemini-2-5') },
  { key: 'gemini-3', label: 'Gemini 3', match: m => m.includes('gemini-3') && !m.includes('image') },
  { key: 'gemini-3-image', label: 'Gemini 3 Image', match: m => m.includes('gemini-3') && m.includes('image') },
]

interface AntigravityQuotaSummaryItem {
  key: string
  label: string
  usedPercent: number       // 组内最高已用百分比（最紧张）
  remainingPercent: number  // 100 - usedPercent
  resetSeconds: number | null
}

function getAntigravityQuotaSummary(metadata: UpstreamMetadata | null | undefined): AntigravityQuotaSummaryItem[] {
  const items = getAntigravityQuotaItems(metadata)
  if (!items.length) return []

  // 将每个模型归入分组
  const groupMap = new Map<string, { label: string, maxUsed: number, resetSeconds: number | null }>()

  for (const item of items) {
    const model = item.model.toLowerCase()
    const group = ANTIGRAVITY_QUOTA_GROUPS.find(g => g.match(model))
    if (!group) continue

    const existing = groupMap.get(group.key)
    if (!existing) {
      groupMap.set(group.key, {
        label: group.label,
        maxUsed: item.usedPercent,
        resetSeconds: item.resetSeconds,
      })
    } else {
      if (item.usedPercent > existing.maxUsed) {
        existing.maxUsed = item.usedPercent
      }
      if (existing.resetSeconds === null) {
        existing.resetSeconds = item.resetSeconds
      } else if (item.resetSeconds !== null && item.resetSeconds < existing.resetSeconds) {
        existing.resetSeconds = item.resetSeconds
      }
    }
  }

  // 按 ANTIGRAVITY_QUOTA_GROUPS 定义的顺序输出
  const result: AntigravityQuotaSummaryItem[] = []
  for (const group of ANTIGRAVITY_QUOTA_GROUPS) {
    const data = groupMap.get(group.key)
    if (!data) continue
    result.push({
      key: group.key,
      label: data.label,
      usedPercent: data.maxUsed,
      remainingPercent: Math.max(100 - data.maxUsed, 0),
      resetSeconds: data.resetSeconds,
    })
  }
  return result
}

function getAntigravityQuotaSummaryForKey(key: EndpointAPIKey): AntigravityQuotaSummaryItem[] {
  const snapshotItems = getAntigravityQuotaItemsFromSnapshot(key)
  if (snapshotItems.length > 0) {
    const groupMap = new Map<string, { label: string, maxUsed: number, resetSeconds: number | null }>()

    for (const item of snapshotItems) {
      const model = item.model.toLowerCase()
      const group = ANTIGRAVITY_QUOTA_GROUPS.find(g => g.match(model))
      if (!group) continue

      const existing = groupMap.get(group.key)
      if (!existing) {
        groupMap.set(group.key, {
          label: group.label,
          maxUsed: item.usedPercent,
          resetSeconds: item.resetSeconds,
        })
      } else {
        if (item.usedPercent > existing.maxUsed) {
          existing.maxUsed = item.usedPercent
        }
        if (existing.resetSeconds === null) {
          existing.resetSeconds = item.resetSeconds
        } else if (item.resetSeconds !== null && item.resetSeconds < existing.resetSeconds) {
          existing.resetSeconds = item.resetSeconds
        }
      }
    }

    const result: AntigravityQuotaSummaryItem[] = []
    for (const group of ANTIGRAVITY_QUOTA_GROUPS) {
      const data = groupMap.get(group.key)
      if (!data) continue
      result.push({
        key: group.key,
        label: data.label,
        usedPercent: data.maxUsed,
        remainingPercent: Math.max(100 - data.maxUsed, 0),
        resetSeconds: data.resetSeconds,
      })
    }
    return result
  }

  return getAntigravityQuotaSummary(key.upstream_metadata)
}

function getResetCountdownText(
  resetAt: number | null | undefined,
  resetSecs: number | null | undefined,
  updatedAt: number | null | undefined,
  usedPercent: number | null | undefined
): string {
  const status = getCodexResetCountdown(
    resetAt,
    resetSecs,
    updatedAt,
    countdownTick.value,
    toCodexRemainingPercent(usedPercent)
  )
  if (!status) return ''
  return status.isExpired ? status.text : t('providerDetail.resetAfter', { time: status.text })
}

function getResetCountdownClass(
  resetAt: number | null | undefined,
  resetSecs: number | null | undefined,
  updatedAt: number | null | undefined,
  usedPercent: number | null | undefined
): string {
  const status = getCodexResetCountdown(
    resetAt,
    resetSecs,
    updatedAt,
    countdownTick.value,
    toCodexRemainingPercent(usedPercent)
  )
  if (!status || status.isExpired) return 'text-muted-foreground/70'
  if (status.isCritical) return 'text-destructive font-medium animate-pulse'
  if (status.isUrgent) return 'text-amber-500 dark:text-amber-400'
  return 'text-muted-foreground/70'
}

function toCodexRemainingPercent(usedPercent: number | null | undefined): number | null {
  const normalizedUsed = Number(usedPercent)
  if (!Number.isFinite(normalizedUsed)) return null
  const clampedUsed = Math.min(Math.max(normalizedUsed, 0), 100)
  return Math.max(100 - clampedUsed, 0)
}

function shouldStartCodexResetCountdown(usedPercent: number | null | undefined): boolean {
  const remainingPercent = toCodexRemainingPercent(usedPercent)
  if (remainingPercent == null) return true
  return remainingPercent < 100
}

// 格式化重置时间
function formatResetTime(seconds: number): string {
  const days = Math.floor(seconds / 86400)
  const hours = Math.floor((seconds % 86400) / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)

  if (days > 0) {
    return t('providerDetailExtra.daysHours', { days, hours })
  }
  if (hours > 0) {
    return t('providerDetailExtra.hoursMinutes', { hours, minutes })
  }
  return t('providerDetailExtra.minutes', { minutes })
}

// OAuth 订阅类型样式
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

// OAuth 状态信息（包括失效和过期）
function getKeyOAuthExpires(key: EndpointAPIKey) {
  return getOAuthStatusDisplayWithFallback(key, countdownTick.value)
}

function getOAuthRefreshButtonTitle(key: EndpointAPIKey): string {
  return resolveOAuthRefreshButtonTitle(key, countdownTick.value)
}

// OAuth 状态的 title 提示
function getOAuthStatusTitle(key: EndpointAPIKey): string {
  const accountTitle = getAccountStatusTitle(key)
  if (accountTitle && isAccountLevelBlock(key)) {
    return accountTitle
  }
  return resolveOAuthStatusTitle(key, countdownTick.value)
}

// 健康度颜色
function getHealthScoreColor(score: number): string {
  if (score >= 0.8) return 'text-green-600 dark:text-green-400'
  if (score >= 0.5) return 'text-yellow-600 dark:text-yellow-400'
  return 'text-red-600 dark:text-red-400'
}

function getHealthScoreBarColor(score: number): string {
  if (score >= 0.8) return 'bg-green-500 dark:bg-green-400'
  if (score >= 0.5) return 'bg-yellow-500 dark:bg-yellow-400'
  return 'bg-red-500 dark:bg-red-400'
}

function getKeySchedulingState(key: EndpointAPIKey): NonNullable<EndpointAPIKey['scheduling_state']> {
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
  if (!key.is_active) return 'disabled'
  return 'available'
}

function getKeySchedulingLabel(key: EndpointAPIKey): string {
  if (key.scheduling_label) return key.scheduling_label
  switch (getKeySchedulingState(key)) {
    case 'disabled':
      return t('providerDetailExtra.schedulingDisabled')
    case 'invalid':
      return t('providerDetailExtra.schedulingInvalid')
    case 'blocked':
      return t('providerDetailExtra.schedulingAnomaly')
    case 'quota_exhausted':
      return t('providerDetailExtra.schedulingQuotaExhausted')
    case 'temporary_unavailable':
      return t('providerDetailExtra.schedulingUnavailable')
    default:
      return t('providerDetailExtra.schedulingAvailable')
  }
}

function getKeySchedulingBadgeVariant(key: EndpointAPIKey): 'default' | 'secondary' | 'destructive' | 'outline' {
  const state = getKeySchedulingState(key)
  if (state === 'invalid' || state === 'blocked') return 'destructive'
  if (state === 'disabled') return 'secondary'
  return 'outline'
}

function getKeySchedulingTitle(key: EndpointAPIKey): string {
  const parts = [t('providerDetailExtra.statusLabel', { status: getKeySchedulingLabel(key) })]
  if (key.scheduling_reason_label) {
    parts.push(t('providerDetailExtra.reasonLabel', { reason: key.scheduling_reason_label }))
  }
  if (key.circuit_breaker_open) {
    parts.push(getKeyCircuitBreakerTitle(key))
  }
  return parts.join('\n')
}

function isKeyRecoverable(key: EndpointAPIKey): boolean {
  return Boolean(
    key.circuit_breaker_open
    || (key.health_score !== undefined && key.health_score < 0.5)
  )
}

function getOpenCircuitEntries(key: EndpointAPIKey): Array<[string, NonNullable<EndpointAPIKey['circuit_breaker_by_format']>[string]]> {
  return Object.entries(key.circuit_breaker_by_format || {})
    .filter(([, value]) => value?.open === true)
}

function getKeyCircuitBreakerTitle(key: EndpointAPIKey): string {
  const entries = getOpenCircuitEntries(key)
  if (entries.length === 0) return t('providerDetailExtra.historicalCircuitRecord')
  const parts = entries.map(([format, value]) => {
    const label = formatApiFormatShort(format)
    const reason = value.reason ? t('providerDetailExtra.reasonLabel', { reason: value.reason }) : t('providerDetailExtra.consecutiveFailures')
    return [label, reason]
      .filter(Boolean)
      .join(' / ')
  })
  parts.push(t('providerDetailExtra.historicalHealthHint'))
  return parts.join('\n')
}

function getRecoverKeyTitle(key: EndpointAPIKey): string {
  if (key.circuit_breaker_open) {
    return t('providerDetailExtra.clearHistoricalHealth')
  }
  return t('providerDetailExtra.refreshHealth')
}

// 获取自动获取模型状态的 title 提示
function getAutoFetchStatusTitle(key: EndpointAPIKey): string {
  const parts: string[] = [t('providerDetailExtra.autoModelsEnabled')]

  if (key.last_models_fetch_at) {
    const date = new Date(key.last_models_fetch_at)
    parts.push(t('providerDetailExtra.lastSync', { time: date.toLocaleString() }))
  }

  if (key.last_models_fetch_error) {
    parts.push(t('providerDetailExtra.errorLabel', { error: key.last_models_fetch_error }))
  }

  return parts.join('\n')
}

// 检查指定格式是否存在历史健康记录
function isFormatCircuitOpen(key: EndpointAPIKey, format: string): boolean {
  if (!key.circuit_breaker_by_format) return false
  const formatData = key.circuit_breaker_by_format[format]
  return formatData?.open === true
}

// 加载系统级格式转换配置
async function loadSystemFormatConversionConfig() {
  try {
    const result = await adminApi.getSystemConfig('enable_format_conversion')
    systemFormatConversionEnabled.value = result.value === true
  } catch {
    // 获取失败时默认为关闭
    systemFormatConversionEnabled.value = false
  }
}

// 加载 Provider 信息
async function loadProvider() {
  if (!props.providerId) return
  const requestId = ++providerLoadRequestId
  const shouldShowSpinner = !provider.value || provider.value.id !== props.providerId

  try {
    if (shouldShowSpinner) {
      loading.value = true
    }
    // 系统级格式转换配置只影响一个图标状态，不应阻塞详情抽屉首屏。
    void loadSystemFormatConversionConfig()
    const providerData = await getProvider(props.providerId)
    if (requestId !== providerLoadRequestId) return
    provider.value = providerData
    keyPageSize.value = getProviderKeysPageSize(providerData.provider_type)

    if (!provider.value) {
      throw new Error(t('providerDetailExtra.providerMissing'))
    }
  } catch (err: unknown) {
    if (requestId !== providerLoadRequestId) return
    showError(parseApiError(err, t('providerDetailExtra.loadFailed')), t('providerDetail.error'))
  } finally {
    if (requestId === providerLoadRequestId && shouldShowSpinner) {
      loading.value = false
    }
  }
}

async function loadProviderKeysPage(
  page = currentKeyPage.value,
  options: { silent?: boolean } = {},
) {
  if (!props.providerId) return
  const providerId = props.providerId
  const requestId = ++keysLoadRequestId
  if (!options.silent) loadingProviderKeys.value = true

  try {
    const result = await getProviderKeysPage(providerId, {
      page,
      page_size: keyPageSize.value,
    })
    if (requestId !== keysLoadRequestId || props.providerId !== providerId) return

    const nextTotalPages = Math.max(1, Math.ceil(result.total / result.page_size))
    if (result.keys.length === 0 && result.total > 0 && result.page > nextTotalPages) {
      await loadProviderKeysPage(nextTotalPages, options)
      return
    }

    providerKeys.value = result.keys
    providerKeysTotal.value = result.total
    currentKeyPage.value = Math.min(result.page, nextTotalPages)
    keyPageSize.value = result.page_size
    syncCurrentSelections(endpoints.value, result.keys)
  } catch (err: unknown) {
    if (requestId !== keysLoadRequestId || props.providerId !== providerId) return
    if (!options.silent) {
      providerKeys.value = []
      providerKeysTotal.value = 0
      syncCurrentSelections(endpoints.value, [])
      showError(parseApiError(err, t('providerDetailExtra.loadKeysFailed')), t('providerDetail.error'))
    }
  } finally {
    if (requestId === keysLoadRequestId && !options.silent) {
      loadingProviderKeys.value = false
    }
  }
}

function startConcurrencyRefresh() {
  if (concurrencyRefreshTimer) return
  concurrencyRefreshTimer = setInterval(() => {
    // 交互式加载优先，避免静默刷新抢占请求序号后让加载状态无法结束。
    if (props.open && props.providerId && !loadingProviderKeys.value) {
      void loadProviderKeysPage(currentKeyPage.value, { silent: true })
    }
  }, 3000)
}

function stopConcurrencyRefresh() {
  if (!concurrencyRefreshTimer) return
  clearInterval(concurrencyRefreshTimer)
  concurrencyRefreshTimer = null
}

onUnmounted(() => {
  stopConcurrencyRefresh()
})

// 加载端点列表
async function loadEndpoints() {
  if (!props.providerId) return
  const providerId = props.providerId
  const requestId = ++endpointsLoadRequestId
  loadingProviderEndpoints.value = true
  loadingProviderKeys.value = true
  loadingProviderModels.value = true

  const sortEndpoints = (items: ProviderEndpoint[]): ProviderEndpointWithKeys[] => {
    return [...items].sort((a, b) => {
      const aIdx = API_FORMAT_ORDER.indexOf(a.api_format)
      const bIdx = API_FORMAT_ORDER.indexOf(b.api_format)
      if (aIdx === -1 && bIdx === -1) return 0
      if (aIdx === -1) return 1
      if (bIdx === -1) return -1
      return aIdx - bIdx
    })
  }

  const endpointsPromise = getProviderEndpoints(providerId)
    .then((endpointsList) => {
      if (requestId !== endpointsLoadRequestId) return
      const sortedEndpoints = sortEndpoints(endpointsList)
      endpoints.value = sortedEndpoints
      syncCurrentSelections(sortedEndpoints, providerKeys.value)
    })
    .catch((err: unknown) => {
      if (requestId !== endpointsLoadRequestId) return
      endpoints.value = []
      syncCurrentSelections([], providerKeys.value)
      showError(parseApiError(err, t('providerDetailExtra.loadEndpointsFailed')), t('providerDetail.error'))
    })
    .finally(() => {
      if (requestId === endpointsLoadRequestId) {
        loadingProviderEndpoints.value = false
      }
    })

  const providerKeysPromise = loadProviderKeysPage(currentKeyPage.value)

  const modelsPromise = getProviderModels(providerId)
    .catch(() => [])
    .then((modelsResult) => {
      if (requestId !== endpointsLoadRequestId) return
      providerModels.value = modelsResult
    })
    .finally(() => {
      if (requestId === endpointsLoadRequestId) {
        loadingProviderModels.value = false
      }
    })

  await Promise.allSettled([endpointsPromise, providerKeysPromise, modelsPromise])
}

// 加载映射预览（独立于 loadEndpoints，不阻塞首屏渲染）
async function loadMappingPreview() {
  if (!props.providerId) return
  const requestId = ++mappingPreviewLoadRequestId
  loadingProviderMappingPreview.value = true
  try {
    const preview = await getProviderMappingPreview(props.providerId)
    if (requestId !== mappingPreviewLoadRequestId) return
    providerMappingPreview.value = preview
  } catch {
    if (requestId !== mappingPreviewLoadRequestId) return
    providerMappingPreview.value = null
  } finally {
    if (requestId === mappingPreviewLoadRequestId) {
      loadingProviderMappingPreview.value = false
    }
  }
}

// 添加 ESC 键监听
useEscapeKey(() => {
  if (props.open) {
    handleClose()
  }
}, {
  disableOnInput: true,
  once: false
})
</script>

<style scoped>
/* 抽屉过渡动画 */
.drawer-enter-active,
.drawer-leave-active {
  transition: opacity 0.3s ease;
}

.drawer-enter-active .relative,
.drawer-leave-active .relative {
  transition: transform 0.3s ease;
}

.drawer-enter-from,
.drawer-leave-to {
  opacity: 0;
}

.drawer-enter-from .relative {
  transform: translateX(100%);
}

.drawer-leave-to .relative {
  transform: translateX(100%);
}

.drawer-enter-to .relative,
.drawer-leave-from .relative {
  transform: translateX(0);
}

/* 轻量滚动条（用于 Antigravity 模型配额等小区域） */
.custom-scrollbar::-webkit-scrollbar {
  width: 4px;
}
.custom-scrollbar::-webkit-scrollbar-track {
  background: transparent;
}
.custom-scrollbar::-webkit-scrollbar-thumb {
  background-color: hsl(var(--muted-foreground) / 0.2);
  border-radius: 4px;
}
.custom-scrollbar::-webkit-scrollbar-thumb:hover {
  background-color: hsl(var(--muted-foreground) / 0.4);
}
</style>
