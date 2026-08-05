<template>
  <Dialog
    :model-value="internalOpen"
    :title="t('endpointForm.title')"
    :description="t('endpointForm.description', { name: provider?.name || '' })"
    :icon="Settings"
    size="2xl"
    @update:model-value="handleDialogUpdate"
  >
    <div class="flex flex-col gap-4">
      <!-- 已有端点列表（可滚动） -->
      <div
        v-if="localEndpoints.length > 0"
        class="space-y-3 max-h-[50vh] overflow-y-auto scrollbar-hide"
      >
        <Label class="text-muted-foreground">{{ t('endpointForm.configuredEndpoints') }}</Label>

        <!-- 端点卡片列表 -->
        <div class="space-y-3">
          <div
            v-for="endpoint in localEndpoints"
            :key="endpoint.id"
            class="rounded-lg border bg-card"
            :class="{ 'opacity-60': !endpoint.is_active }"
          >
            <!-- 卡片头部：格式名称 + 状态 + 操作 -->
            <div class="flex items-center justify-between px-4 py-2.5 bg-muted/30 border-b">
              <div class="flex items-center gap-3">
                <span class="font-medium">{{ formatApiFormat(endpoint.api_format) }}</span>
                <Badge
                  v-if="!endpoint.is_active"
                  variant="secondary"
                  class="text-xs"
                >
                  {{ t('endpointForm.disabled') }}
                </Badge>
              </div>
              <div class="flex items-center gap-1.5">
                <!-- 格式转换按钮 -->
                <span
                  class="mr-1"
                  :title="isEndpointFormatConversionDisabled ? formatConversionDisabledTooltip : (endpoint.format_acceptance_config?.enabled ? t('endpointForm.disableConversion') : t('endpointForm.enableConversion'))"
                >
                  <Button
                    variant="ghost"
                    size="icon"
                    :class="`h-7 w-7 ${endpoint.format_acceptance_config?.enabled ? 'text-primary' : ''} ${isEndpointFormatConversionDisabled ? 'opacity-50' : ''}`"
                    :disabled="togglingFormatEndpointId === endpoint.id || isEndpointFormatConversionDisabled"
                    @click="handleToggleFormatConversion(endpoint)"
                  >
                    <Shuffle class="w-3.5 h-3.5" />
                  </Button>
                </span>
                <!-- 端点代理 -->
                <Popover
                  :open="endpointProxyPopoverOpen[endpoint.id] || false"
                  @update:open="(open: boolean) => handleEndpointProxyPopoverToggle(endpoint.id, open)"
                >
                  <PopoverTrigger as-child>
                    <Button
                      variant="ghost"
                      size="icon"
                      class="h-7 w-7"
                      :class="endpointProxyNodeId(endpoint) ? 'text-blue-500' : ''"
                      :disabled="savingEndpointId === endpoint.id"
                      :title="getEndpointProxyTitle(endpoint)"
                    >
                      <Globe class="w-3.5 h-3.5" />
                    </Button>
                  </PopoverTrigger>
                  <PopoverContent
                    class="w-72 p-3 !z-[90]"
                    side="bottom"
                    align="end"
                  >
                    <div class="space-y-2">
                      <div class="flex items-center justify-between">
                        <span class="text-xs font-medium">{{ t('endpointForm.endpointProxy') }}</span>
                        <Button
                          v-if="endpointProxyNodeId(endpoint)"
                          variant="ghost"
                          size="sm"
                          class="h-6 px-2 text-[10px] text-muted-foreground"
                          :disabled="savingEndpointId === endpoint.id"
                          @click="clearEndpointProxy(endpoint)"
                        >
                          {{ t('endpointForm.clear') }}
                        </Button>
                      </div>
                      <ProxyNodeSelect
                        :model-value="endpointProxyNodeId(endpoint)"
                        trigger-class="h-8"
                        @update:model-value="setEndpointProxy(endpoint, $event)"
                      />
                      <p class="text-[10px] text-muted-foreground">
                        {{ endpointProxyNodeId(endpoint) ? t('endpointForm.endpointProxyActive') : t('endpointForm.endpointProxyFallback') }}
                      </p>
                    </div>
                  </PopoverContent>
                </Popover>
                <!-- 上游流式三态按钮 -->
                <Button
                  variant="ghost"
                  size="icon"
                  :class="getUpstreamStreamButtonClass(endpoint)"
                  :title="getUpstreamStreamTooltip(endpoint)"
                  :disabled="savingEndpointId === endpoint.id || isUpstreamStreamPolicyLocked(endpoint)"
                  @click="handleCycleUpstreamStream(endpoint)"
                >
                  <Radio class="w-3.5 h-3.5" />
                </Button>
                <!-- 启用/停用 -->
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7"
                  :title="endpoint.is_active ? t('endpointForm.disable') : t('endpointForm.enable')"
                  :disabled="togglingEndpointId === endpoint.id"
                  @click="handleToggleEndpoint(endpoint)"
                >
                  <Power class="w-3.5 h-3.5" />
                </Button>
                <!-- 删除 -->
                <Button
                  v-if="!isFixedProvider"
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7 hover:text-destructive"
                  :title="t('endpointForm.delete')"
                  :disabled="deletingEndpointId === endpoint.id"
                  @click="handleDeleteEndpoint(endpoint)"
                >
                  <Trash2 class="w-3.5 h-3.5" />
                </Button>
              </div>
            </div>

            <!-- 卡片内容 -->
            <div class="p-4 space-y-4">
              <!-- URL 配置区 -->
              <div class="flex items-end gap-3">
                <div class="flex-1 min-w-0 grid grid-cols-3 gap-3">
                  <div class="col-span-2 space-y-1.5">
                    <Label class="text-xs text-muted-foreground">Base URL</Label>
                    <Input
                      :model-value="getEndpointEditState(endpoint.id)?.url ?? endpoint.base_url"
                      :placeholder="provider?.website || 'https://api.example.com'"
                      :disabled="isFixedProvider"
                      @update:model-value="(v) => updateEndpointField(endpoint.id, 'url', v)"
                    />
                  </div>
                  <div class="space-y-1.5">
                    <Label class="text-xs text-muted-foreground">{{ t('endpointForm.customPath') }}</Label>
                    <Input
                      :model-value="getDisplayedPath(endpoint)"
                      :placeholder="getDefaultPath(endpoint.api_format, endpoint.base_url) || t('endpointForm.defaultPathPlaceholder')"
                      :disabled="isFixedProvider"
                      @update:model-value="(v) => updateEndpointField(endpoint.id, 'path', v)"
                    />
                  </div>
                </div>
                <!-- 保存/撤销按钮（URL/路径有修改时显示） -->
                <div
                  v-if="hasUrlChanges(endpoint)"
                  class="flex items-center gap-1 shrink-0"
                >
                  <Button
                    variant="ghost"
                    size="icon"
                    class="h-9 w-9"
                    :title="t('endpointForm.save')"
                    :disabled="savingEndpointId === endpoint.id"
                    @click="saveEndpoint(endpoint)"
                  >
                    <Check class="w-4 h-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    class="h-9 w-9"
                    :title="t('endpointForm.reset')"
                    @click="resetEndpointChanges(endpoint)"
                  >
                    <RotateCcw class="w-4 h-4" />
                  </Button>
                </div>
              </div>

              <div
                v-if="isOpenAiImageEndpoint(endpoint)"
                class="rounded-md border bg-muted/20 p-3 space-y-2"
              >
                <div class="flex items-center justify-between gap-4">
                  <div class="min-w-0">
                    <div class="text-xs font-medium">
                      {{ t('endpointForm.imageTransportMode', '图片请求传输方式') }}
                    </div>
                    <p class="text-[11px] text-muted-foreground mt-0.5">
                      {{ getOpenAiImageTransportMode(endpoint) === 'images_passthrough'
                        ? t('endpointForm.imageTransportPassthroughHint', '直接调用上游 Images API；自定义路径优先，未填写时沿用 /v1/images/generations 或 /v1/images/edits。')
                        : t('endpointForm.imageTransportBridgeHint', '将 Images 请求转换为 Responses 图片工具调用，保持现有兼容行为。') }}
                    </p>
                  </div>
                  <Select
                    :model-value="getOpenAiImageTransportMode(endpoint)"
                    :disabled="savingEndpointId === endpoint.id"
                    @update:model-value="(value) => handleOpenAiImageTransportModeChange(endpoint, String(value))"
                  >
                    <SelectTrigger class="w-[190px] h-8 text-xs shrink-0">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="responses_bridge">
                        {{ t('endpointForm.imageTransportResponsesBridge', 'Responses 图片桥接') }}
                      </SelectItem>
                      <SelectItem value="images_passthrough">
                        {{ t('endpointForm.imageTransportImagesPassthrough', 'Images API 原样透传') }}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </div>

              <EndpointStreamFailoverSettings
                v-if="isOpenAiResponsesEndpoint(endpoint)"
                :model-value="getStreamFailoverState(endpoint)"
                :saving="savingEndpointId === endpoint.id"
                :changed="hasStreamFailoverChanges(endpoint)"
                :validation-message="getStreamFailoverValidationMessage(endpoint)"
                @update:model-value="(value) => updateStreamFailoverState(endpoint.id, value)"
                @save="saveStreamFailover(endpoint)"
                @reset="resetStreamFailover(endpoint)"
              />

              <!-- 请求/响应规则（请求头、请求体和响应头规则） -->
              <Collapsible v-model:open="endpointRulesExpanded[endpoint.id]">
                <div class="flex items-center gap-2">
                  <!-- 有规则时显示可折叠的触发器 -->
                  <CollapsibleTrigger
                    v-if="getTotalRulesCount(endpoint) > 0"
                    as-child
                  >
                    <button
                      type="button"
                      class="flex items-center gap-2 py-1.5 px-2 -mx-2 rounded-md hover:bg-muted/50 transition-colors"
                    >
                      <ChevronRight
                        class="w-4 h-4 transition-transform text-muted-foreground"
                        :class="{ 'rotate-90': endpointRulesExpanded[endpoint.id] }"
                      />
                      <span class="text-sm font-medium">{{ t('endpointForm.requestResponseRules') }}</span>
                      <Badge
                        variant="secondary"
                        class="text-xs"
                      >
                        {{ t('endpointForm.ruleCount', { count: getTotalRulesCount(endpoint) }) }}
                      </Badge>
                    </button>
                  </CollapsibleTrigger>
                  <!-- 没有规则时只显示标题 -->
                  <span
                    v-else
                    class="text-sm text-muted-foreground py-1.5"
                  >
                    {{ t('endpointForm.requestResponseRules') }}
                  </span>
                  <div class="flex-1" />
                  <div class="flex items-center gap-1 shrink-0">
                    <Button
                      v-if="hasRulePanelChanges(endpoint)"
                      variant="ghost"
                      size="icon"
                      class="h-7 w-7"
                      :title="t('endpointForm.saveRules')"
                      :disabled="savingEndpointId === endpoint.id"
                      @click="saveEndpoint(endpoint)"
                    >
                      <Save class="w-3.5 h-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      class="h-7 text-xs px-2"
                      :title="isEndpointRulesJsonMode(endpoint.id) ? t('endpointForm.formView') : t('endpointForm.jsonView')"
                      @click="toggleEndpointRulesJsonMode(endpoint)"
                    >
                      <Code2 class="w-3 h-3 mr-1" />
                      {{ isEndpointRulesJsonMode(endpoint.id) ? t('endpointForm.form') : 'JSON' }}
                    </Button>
                    <Button
                      v-if="isEndpointRulesJsonMode(endpoint.id)"
                      variant="ghost"
                      size="sm"
                      class="h-7 px-2 text-xs"
                      :title="t('endpointForm.formatJson')"
                      @click="formatEndpointRulesJson(endpoint.id)"
                    >
                      <AlignLeft class="w-3 h-3 mr-1" />
                      {{ t('endpointForm.format') }}
                    </Button>
                    <Button
                      v-if="!isEndpointRulesJsonMode(endpoint.id)"
                      variant="ghost"
                      size="sm"
                      class="h-7 text-xs px-2"
                      :title="t('endpointForm.addHeaderRule')"
                      @click="handleAddEndpointRule(endpoint.id)"
                    >
                      <Plus class="w-3 h-3 mr-1" />
                      {{ t('endpointForm.requestHeader') }}
                    </Button>
                    <Button
                      v-if="!isEndpointRulesJsonMode(endpoint.id)"
                      variant="ghost"
                      size="sm"
                      class="h-7 text-xs px-2"
                      :title="t('endpointForm.addBodyRule')"
                      @click="handleAddEndpointBodyRule(endpoint.id)"
                    >
                      <Plus class="w-3 h-3 mr-1" />
                      {{ t('endpointForm.requestBody') }}
                    </Button>
                    <Button
                      v-if="!isEndpointRulesJsonMode(endpoint.id)"
                      variant="ghost"
                      size="sm"
                      class="h-7 text-xs px-2"
                      :title="t('endpointForm.addResponseHeaderRule')"
                      @click="handleAddEndpointResponseRule(endpoint.id)"
                    >
                      <Plus class="w-3 h-3 mr-1" />
                      {{ t('endpointForm.responseHeader') }}
                    </Button>
                    <Button
                      v-if="isFixedProvider && hasDefaultBodyRules(endpoint.api_format)"
                      variant="ghost"
                      size="sm"
                      class="h-7 text-xs px-2"
                      :title="t('endpointForm.resetBodyRules')"
                      :disabled="resettingDefaultRulesEndpointId === endpoint.id"
                      @click="handleResetBodyRulesToDefault(endpoint)"
                    >
                      <RotateCcw class="w-3 h-3 mr-1" />
                      {{ t('endpointForm.resetBodyRules') }}
                    </Button>
                  </div>
                </div>
                <CollapsibleContent class="pt-3">
                  <div
                    v-if="isEndpointRulesJsonMode(endpoint.id)"
                    class="space-y-2"
                  >
                    <Textarea
                      :model-value="getEndpointRulesJsonDraft(endpoint)"
                      class="min-h-[220px] font-mono text-xs leading-relaxed"
                      spellcheck="false"
                      placeholder="{ &quot;header_rules&quot;: [], &quot;body_rules&quot;: [], &quot;response_header_rules&quot;: [] }"
                      @update:model-value="(value) => updateEndpointRulesJsonDraft(endpoint.id, value)"
                    />
                    <div
                      v-if="endpointRulesJsonError[endpoint.id]"
                      class="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive"
                    >
                      {{ endpointRulesJsonError[endpoint.id] }}
                    </div>
                  </div>
                  <div
                    v-else
                    class="space-y-2"
                  >
                    <div
                      v-if="getEndpointRulesCount(endpoint) > 1 || getEndpointBodyRulesCount(endpoint) > 1 || getEndpointResponseRulesCount(endpoint) > 1"
                      class="flex items-center gap-1.5 text-xs text-muted-foreground px-2"
                    >
                      <GripVertical class="w-3.5 h-3.5" />
                      <span>{{ t('endpointForm.dragHint') }}</span>
                    </div>
                    <!-- 请求头规则列表 - 主题色边框 -->
                    <template
                      v-for="(rule, index) in getEndpointEditRules(endpoint.id)"
                      :key="`header-${index}`"
                    >
                      <div
                        class="flex items-center gap-1.5 px-2 py-1.5 rounded-md border-l-4 border-primary/60 bg-muted/30"
                        :class="[
                          !rule.enabled ? 'opacity-60 border-primary/25 bg-muted/20' : '',
                          isHeaderRuleDragging(endpoint.id, index) ? 'opacity-60 border-primary bg-primary/5' : '',
                          isHeaderRuleDragOver(endpoint.id, index) ? 'ring-1 ring-primary/40 bg-primary/10' : ''
                        ]"
                        @dragover.prevent="handleHeaderRuleDragOver(endpoint.id, index)"
                        @dragleave="handleHeaderRuleDragLeave(endpoint.id, index)"
                        @drop.prevent="handleHeaderRuleDrop(endpoint.id, index)"
                      >
                        <button
                          type="button"
                          class="h-7 w-6 shrink-0 inline-flex items-center justify-center rounded-sm text-muted-foreground/60 hover:text-muted-foreground hover:bg-muted cursor-grab active:cursor-grabbing"
                          :title="t('endpointForm.dragSort')"
                          draggable="true"
                          @dragstart="(e) => handleHeaderRuleDragStart(endpoint.id, index, e)"
                          @dragend="() => handleHeaderRuleDragEnd(endpoint.id)"
                        >
                          <GripVertical class="w-3.5 h-3.5" />
                        </button>
                        <span
                          class="text-[10px] font-semibold text-primary shrink-0"
                          :title="t('endpointForm.requestHeader')"
                        >H</span>
                        <Switch
                          :model-value="rule.enabled"
                          class="shrink-0 scale-75 origin-center"
                          :title="rule.enabled ? t('endpointForm.disableRule', { type: t('endpointForm.requestHeader') }) : t('endpointForm.enableRule', { type: t('endpointForm.requestHeader') })"
                          @update:model-value="(v: boolean) => updateEndpointRuleEnabled(endpoint.id, index, v)"
                        />
                        <Select
                          :model-value="rule.action"
                          :open="ruleSelectOpen[`${endpoint.id}-${index}`]"
                          @update:model-value="(v) => updateEndpointRuleAction(endpoint.id, index, v as 'set' | 'drop' | 'rename')"
                          @update:open="(v) => handleRuleSelectOpen(endpoint.id, index, v)"
                        >
                          <SelectTrigger class="w-[88px] h-7 text-xs shrink-0">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="set">
                              {{ t('endpointForm.set') }}
                            </SelectItem>
                            <SelectItem value="drop">
                              {{ t('endpointForm.drop') }}
                            </SelectItem>
                            <SelectItem value="rename">
                              {{ t('endpointForm.rename') }}
                            </SelectItem>
                          </SelectContent>
                        </Select>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-7 w-7 shrink-0"
                          :class="rule.condition ? 'text-primary' : ''"
                          :title="t('endpointForm.condition')"
                          @click="toggleEndpointRuleCondition(endpoint.id, index)"
                        >
                          <Filter class="w-3 h-3" />
                        </Button>
                        <template v-if="rule.action === 'set'">
                          <Input
                            :model-value="rule.key"
                            :placeholder="t('endpointFormExtra.namePlaceholder')"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointRuleField(endpoint.id, index, 'key', v)"
                          />
                          <span class="text-muted-foreground text-xs">=</span>
                          <Input
                            :model-value="rule.value"
                            :placeholder="t('endpointFormExtra.valuePlaceholder')"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointRuleField(endpoint.id, index, 'value', v)"
                          />
                        </template>
                        <template v-else-if="rule.action === 'drop'">
                          <Input
                            :model-value="rule.key"
                            :placeholder="t('endpointFormExtra.nameToDeletePlaceholder')"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointRuleField(endpoint.id, index, 'key', v)"
                          />
                        </template>
                        <template v-else-if="rule.action === 'rename'">
                          <Input
                            :model-value="rule.from"
                            :placeholder="t('endpointFormExtra.oldNamePlaceholder')"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointRuleField(endpoint.id, index, 'from', v)"
                          />
                          <span class="text-muted-foreground text-xs">→</span>
                          <Input
                            :model-value="rule.to"
                            :placeholder="t('endpointFormExtra.newNamePlaceholder')"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointRuleField(endpoint.id, index, 'to', v)"
                          />
                        </template>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-7 w-7 shrink-0"
                          @click="removeEndpointRule(endpoint.id, index)"
                        >
                          <X class="w-3 h-3" />
                        </Button>
                      </div>
                      <EndpointConditionEditor
                        v-if="rule.condition"
                        :model-value="rule.condition"
                        :path-hint="t('endpointFormExtra.requestBodyPathHint')"
                        removable
                        @update:model-value="(condition) => updateEndpointRuleCondition(endpoint.id, index, condition)"
                        @remove="clearEndpointRuleCondition(endpoint.id, index)"
                      />
                    </template>

                    <!-- 响应头规则列表 -->
                    <template
                      v-for="(rule, index) in getEndpointEditResponseRules(endpoint.id)"
                      :key="`response-header-${index}`"
                    >
                      <div
                        class="flex items-center gap-1.5 px-2 py-1.5 rounded-md border-l-4 border-sky-500/60 bg-muted/30"
                        :class="[
                          !rule.enabled ? 'opacity-60 border-sky-500/25 bg-muted/20' : '',
                          isResponseRuleDragging(endpoint.id, index) ? 'opacity-60 border-sky-500 bg-sky-500/5' : '',
                          isResponseRuleDragOver(endpoint.id, index) ? 'ring-1 ring-sky-500/40 bg-sky-500/10' : ''
                        ]"
                        @dragover.prevent="handleResponseRuleDragOver(endpoint.id, index)"
                        @dragleave="handleResponseRuleDragLeave(endpoint.id, index)"
                        @drop.prevent="handleResponseRuleDrop(endpoint.id, index)"
                      >
                        <button
                          type="button"
                          class="h-7 w-6 shrink-0 inline-flex items-center justify-center rounded-sm text-muted-foreground/60 hover:text-muted-foreground hover:bg-muted cursor-grab active:cursor-grabbing"
                          :title="t('endpointForm.dragSort')"
                          draggable="true"
                          @dragstart="(e) => handleResponseRuleDragStart(endpoint.id, index, e)"
                          @dragend="() => handleResponseRuleDragEnd(endpoint.id)"
                        >
                          <GripVertical class="w-3.5 h-3.5" />
                        </button>
                        <span
                          class="text-[10px] font-semibold text-sky-600 dark:text-sky-400 shrink-0"
                          :title="t('endpointForm.responseHeader')"
                        >R</span>
                        <Switch
                          :model-value="rule.enabled"
                          class="shrink-0 scale-75 origin-center"
                          :title="rule.enabled ? t('endpointForm.disableRule', { type: t('endpointForm.responseHeader') }) : t('endpointForm.enableRule', { type: t('endpointForm.responseHeader') })"
                          @update:model-value="(v: boolean) => updateEndpointResponseRuleEnabled(endpoint.id, index, v)"
                        />
                        <Select
                          :model-value="rule.action"
                          :open="responseRuleSelectOpen[`${endpoint.id}-${index}`]"
                          @update:model-value="(v) => updateEndpointResponseRuleAction(endpoint.id, index, v as 'set' | 'drop' | 'rename')"
                          @update:open="(v) => handleResponseRuleSelectOpen(endpoint.id, index, v)"
                        >
                          <SelectTrigger class="w-[88px] h-7 text-xs shrink-0">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="set">
                              {{ t('endpointForm.set') }}
                            </SelectItem>
                            <SelectItem value="drop">
                              {{ t('endpointForm.drop') }}
                            </SelectItem>
                            <SelectItem value="rename">
                              {{ t('endpointForm.rename') }}
                            </SelectItem>
                          </SelectContent>
                        </Select>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-7 w-7 shrink-0"
                          :class="rule.condition ? 'text-primary' : ''"
                          :title="t('endpointForm.condition')"
                          @click="toggleEndpointResponseRuleCondition(endpoint.id, index)"
                        >
                          <Filter class="w-3 h-3" />
                        </Button>
                        <template v-if="rule.action === 'set'">
                          <Input
                            :model-value="rule.key"
                            :placeholder="t('endpointFormExtra.responseHeaderNamePlaceholder')"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointResponseRuleField(endpoint.id, index, 'key', v)"
                          />
                          <span class="text-muted-foreground text-xs">=</span>
                          <Input
                            :model-value="rule.value"
                            :placeholder="t('endpointFormExtra.valuePlaceholder')"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointResponseRuleField(endpoint.id, index, 'value', v)"
                          />
                        </template>
                        <template v-else-if="rule.action === 'drop'">
                          <Input
                            :model-value="rule.key"
                            :placeholder="t('endpointFormExtra.responseHeaderToDeletePlaceholder')"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointResponseRuleField(endpoint.id, index, 'key', v)"
                          />
                        </template>
                        <template v-else-if="rule.action === 'rename'">
                          <Input
                            :model-value="rule.from"
                            :placeholder="t('endpointFormExtra.oldNamePlaceholder')"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointResponseRuleField(endpoint.id, index, 'from', v)"
                          />
                          <span class="text-muted-foreground text-xs">→</span>
                          <Input
                            :model-value="rule.to"
                            :placeholder="t('endpointFormExtra.newNamePlaceholder')"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointResponseRuleField(endpoint.id, index, 'to', v)"
                          />
                        </template>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-7 w-7 shrink-0"
                          @click="removeEndpointResponseRule(endpoint.id, index)"
                        >
                          <X class="w-3 h-3" />
                        </Button>
                      </div>
                      <EndpointConditionEditor
                        v-if="rule.condition"
                        :model-value="rule.condition"
                        :path-hint="t('endpointFormExtra.responseBodyPathHint')"
                        removable
                        @update:model-value="(condition) => updateEndpointResponseRuleCondition(endpoint.id, index, condition)"
                        @remove="clearEndpointResponseRuleCondition(endpoint.id, index)"
                      />
                    </template>

                    <div
                      v-if="getEndpointEditBodyRules(endpoint.id).length > 0"
                      class="flex items-center gap-1 text-xs text-muted-foreground px-2"
                    >
                      <span><code class="bg-muted px-1 rounded">.</code> {{ t('endpointForm.nestedField') }} / <code class="bg-muted px-1 rounded">[N]</code> {{ t('endpointForm.arrayIndex') }} / <code class="bg-muted px-1 rounded">[*]</code> {{ t('endpointForm.wildcard') }}; {{ t('endpointForm.valueJson') }}</span>
                      <div class="flex-1" />
                      <Popover
                        :open="bodyRuleHelpOpenEndpointId === endpoint.id"
                        @update:open="(v: boolean) => setBodyRuleHelpOpen(endpoint.id, v)"
                      >
                        <PopoverTrigger as-child>
                          <button
                            type="button"
                            class="shrink-0 h-6 w-6 inline-flex items-center justify-center rounded-md hover:bg-muted/60"
                            :title="t('endpointForm.ruleHelp')"
                            :aria-label="t('endpointForm.ruleHelp')"
                          >
                            <HelpCircle class="w-3.5 h-3.5 text-muted-foreground/60" />
                          </button>
                        </PopoverTrigger>
                        <PopoverContent
                          side="bottom"
                          align="end"
                          :side-offset="6"
                          class="w-80 p-3 !z-[90]"
                        >
                          <div class="text-xs space-y-2">
                            <div>
                              <div class="font-medium mb-0.5">
                                {{ t('endpointForm.pathSyntax') }}
                              </div>
                              <div class="text-muted-foreground">
                                <code>metadata.user_id</code> {{ t('endpointForm.nestedField') }}<br>
                                <code>messages[0].content</code> {{ t('endpointForm.arrayIndex') }}<br>
                                <code>tools[*].name</code> {{ t('endpointForm.wildcardAll') }}<br>
                                <code>tools[0-4].name</code> {{ t('endpointForm.range') }}<br>
                                <code>config\.v1.key</code> {{ t('endpointForm.escapedDot') }}
                              </div>
                            </div>
                            <div>
                              <div class="font-medium mb-0.5">
                                {{ t('endpointForm.valueFormat') }}
                              </div>
                              <div class="text-muted-foreground">
                                <code>123</code> {{ t('endpointForm.number') }} / <code>"text"</code> {{ t('endpointForm.string') }} / <code>true</code> {{ t('endpointForm.boolean') }}<br>
                                <code>{"k":"v"}</code> {{ t('endpointForm.object') }} / <code>[1,2]</code> {{ t('endpointForm.array') }} / <code>null</code><br>
                                <code v-pre>{{$original}}</code> {{ t('endpointForm.originalValue') }}
                              </div>
                            </div>
                            <div>
                              <div class="font-medium mb-0.5">
                                {{ t('endpointForm.conditionOperators') }}
                              </div>
                              <div class="text-muted-foreground">
                                <code>eq</code> <code>neq</code> {{ t('endpointFormExtra.operatorEquality') }}<br>
                                <code>gt</code> <code>lt</code> <code>gte</code> <code>lte</code> {{ t('endpointFormExtra.operatorComparison') }}<br>
                                <code>starts_with</code> <code>ends_with</code> <code>contains</code> {{ t('endpointFormExtra.operatorStringMatch') }}<br>
                                <code>matches</code> {{ t('endpointFormExtra.operatorRegex') }}<br>
                                <code>exists</code> <code>not_exists</code> {{ t('endpointFormExtra.operatorExistence') }}<br>
                                <code>in</code> {{ t('endpointFormExtra.operatorIn') }} <code>["a","b"]</code><br>
                                <code>type_is</code> {{ t('endpointFormExtra.operatorType') }} (string/number/boolean/array/object/null)<br>
                                {{ t('endpointFormExtra.itemPathHint') }} <code>$item.xxx</code><br>
                                {{ t('endpointFormExtra.conditionSourceHint') }} <code>{{ t('endpointForm.requestBody') }}</code>/<code>{{ t('endpointForm.requestHeader') }}</code>, <code>ALL</code>/<code>ANY</code>
                              </div>
                            </div>
                            <div class="text-muted-foreground">
                              {{ t('endpointFormExtra.ruleOrderHint') }}
                            </div>
                            <div class="text-muted-foreground">
                              {{ t('endpointFormExtra.conditionDefaultHint') }} <code>{{ t('endpointForm.requestHeader') }}</code> {{ t('endpointFormExtra.conditionHeaderHint') }}
                            </div>
                          </div>
                        </PopoverContent>
                      </Popover>
                    </div>

                    <!-- 请求体规则列表 - 次要色边框 -->
                    <template
                      v-for="(rule, index) in getEndpointEditBodyRules(endpoint.id)"
                      :key="`body-${index}`"
                    >
                      <div
                        class="flex items-center gap-1.5 px-2 py-1.5 rounded-md border-l-4 border-muted-foreground/40 bg-muted/30"
                        :class="[
                          !rule.enabled ? 'opacity-60 border-muted-foreground/25 bg-muted/20' : '',
                          isBodyRuleDragging(endpoint.id, index) ? 'opacity-60 border-muted-foreground/70 bg-muted/50' : '',
                          isBodyRuleDragOver(endpoint.id, index) ? 'ring-1 ring-muted-foreground/40 bg-muted/40' : ''
                        ]"
                        @dragover.prevent="handleBodyRuleDragOver(endpoint.id, index)"
                        @dragleave="handleBodyRuleDragLeave(endpoint.id, index)"
                        @drop.prevent="handleBodyRuleDrop(endpoint.id, index)"
                      >
                        <button
                          type="button"
                          class="h-7 w-6 shrink-0 inline-flex items-center justify-center rounded-sm text-muted-foreground/60 hover:text-muted-foreground hover:bg-muted cursor-grab active:cursor-grabbing"
                          :title="t('endpointForm.dragSort')"
                          draggable="true"
                          @dragstart="(e) => handleBodyRuleDragStart(endpoint.id, index, e)"
                          @dragend="() => handleBodyRuleDragEnd(endpoint.id)"
                        >
                          <GripVertical class="w-3.5 h-3.5" />
                        </button>
                        <span
                          class="text-[10px] font-semibold text-muted-foreground shrink-0"
                          :title="t('endpointForm.requestBody')"
                        >B</span>
                        <Switch
                          :model-value="rule.enabled"
                          class="shrink-0 scale-75 origin-center"
                          :title="rule.enabled ? t('endpointForm.disableRule', { type: t('endpointForm.requestBody') }) : t('endpointForm.enableRule', { type: t('endpointForm.requestBody') })"
                          @update:model-value="(v: boolean) => updateEndpointBodyRuleEnabled(endpoint.id, index, v)"
                        />
                        <Select
                          :model-value="rule.action"
                          :open="bodyRuleSelectOpen[`${endpoint.id}-${index}`]"
                          @update:model-value="(v: string) => updateEndpointBodyRuleAction(endpoint.id, index, v as BodyRuleAction)"
                          @update:open="(v) => handleBodyRuleSelectOpen(endpoint.id, index, v)"
                        >
                          <SelectTrigger class="w-[88px] h-7 text-xs shrink-0">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="set">
                              {{ t('endpointForm.set') }}
                            </SelectItem>
                            <SelectItem value="drop">
                              {{ t('endpointForm.drop') }}
                            </SelectItem>
                            <SelectItem value="rename">
                              {{ t('endpointForm.rename') }}
                            </SelectItem>
                            <SelectItem value="append">
                              {{ t('endpointForm.append') }}
                            </SelectItem>
                            <SelectItem value="insert">
                              {{ t('endpointForm.insert') }}
                            </SelectItem>
                            <SelectItem value="regex_replace">
                              {{ t('endpointForm.regexReplace') }}
                            </SelectItem>
                          </SelectContent>
                        </Select>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-7 w-7 shrink-0"
                          :class="rule.condition ? 'text-primary' : ''"
                          :title="t('endpointForm.condition')"
                          @click="toggleBodyRuleCondition(endpoint.id, index)"
                        >
                          <Filter class="w-3 h-3" />
                        </Button>
                        <template v-if="rule.action === 'set'">
                          <Input
                            :model-value="rule.path"
                            :placeholder="t('endpointFormExtra.fieldPathExamplePlaceholder')"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'path', v)"
                          />
                          <span class="text-muted-foreground text-xs">=</span>
                          <Input
                            :model-value="rule.value"
                            placeholder="123 / &quot;text&quot; / {{$original}}"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'value', v)"
                          />
                          <CheckCircle
                            class="w-4 h-4 shrink-0"
                            :class="getBodySetValueValidation(rule) === true ? 'text-green-600' : getBodySetValueValidation(rule) === false ? 'text-destructive' : 'text-muted-foreground/40'"
                            :title="getBodySetValueValidationTip(rule)"
                          />
                        </template>
                        <template v-else-if="rule.action === 'drop'">
                          <Input
                            :model-value="rule.path"
                            :placeholder="t('endpointFormExtra.fieldPathToDeletePlaceholder')"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'path', v)"
                          />
                        </template>
                        <template v-else-if="rule.action === 'rename'">
                          <Input
                            :model-value="rule.from"
                            :placeholder="t('endpointFormExtra.oldPathPlaceholder')"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'from', v)"
                          />
                          <span class="text-muted-foreground text-xs">→</span>
                          <Input
                            :model-value="rule.to"
                            :placeholder="t('endpointFormExtra.newPathPlaceholder')"
                            size="sm"
                            class="flex-1 min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'to', v)"
                          />
                        </template>
                        <template v-else-if="rule.action === 'append'">
                          <Input
                            :model-value="rule.path"
                            :placeholder="t('endpointFormExtra.arrayPathExamplePlaceholder')"
                            size="sm"
                            class="flex-[2] min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'path', v)"
                          />
                          <span class="text-muted-foreground text-xs">+=</span>
                          <Input
                            :model-value="rule.value"
                            :placeholder="t('endpointFormExtra.jsonValuePlaceholder')"
                            size="sm"
                            class="flex-[3] min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'value', v)"
                          />
                          <CheckCircle
                            class="w-4 h-4 shrink-0"
                            :class="getBodySetValueValidation(rule) === true ? 'text-green-600' : getBodySetValueValidation(rule) === false ? 'text-destructive' : 'text-muted-foreground/40'"
                            :title="getBodySetValueValidationTip(rule)"
                          />
                        </template>
                        <template v-else-if="rule.action === 'insert'">
                          <Input
                            :model-value="rule.path"
                            :placeholder="t('endpointFormExtra.arrayPathExamplePlaceholder')"
                            size="sm"
                            class="flex-[2] min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'path', v)"
                          />
                          <Input
                            :model-value="rule.index"
                            :placeholder="t('endpointFormExtra.positionPlaceholder')"
                            size="sm"
                            class="w-14 h-7 text-xs shrink-0"
                            :title="t('endpointFormExtra.positionHint')"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'index', v)"
                          />
                          <Input
                            :model-value="rule.value"
                            :placeholder="t('endpointFormExtra.jsonValuePlaceholder')"
                            size="sm"
                            class="flex-[3] min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'value', v)"
                          />
                          <CheckCircle
                            class="w-4 h-4 shrink-0"
                            :class="getBodySetValueValidation(rule) === true ? 'text-green-600' : getBodySetValueValidation(rule) === false ? 'text-destructive' : 'text-muted-foreground/40'"
                            :title="getBodySetValueValidationTip(rule)"
                          />
                        </template>
                        <template v-else-if="rule.action === 'regex_replace'">
                          <Input
                            :model-value="rule.path"
                            :placeholder="t('endpointFormExtra.fieldPathPlaceholder')"
                            size="sm"
                            class="flex-[2] min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'path', v)"
                          />
                          <Input
                            :model-value="rule.pattern"
                            :placeholder="t('endpointFormExtra.regexPlaceholder')"
                            size="sm"
                            class="flex-[2] min-w-0 h-7 text-xs font-mono"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'pattern', v)"
                          />
                          <span class="text-muted-foreground text-xs">→</span>
                          <Input
                            :model-value="rule.replacement"
                            :placeholder="t('endpointFormExtra.replacementPlaceholder')"
                            size="sm"
                            class="flex-[2] min-w-0 h-7 text-xs"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'replacement', v)"
                          />
                          <Input
                            :model-value="rule.flags"
                            placeholder="ims"
                            size="sm"
                            class="w-12 h-7 text-xs shrink-0 font-mono"
                            :title="t('endpointFormExtra.regexFlagsHint')"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'flags', v)"
                          />
                          <Input
                            :model-value="rule.count"
                            :placeholder="t('endpointFormExtra.allPlaceholder')"
                            size="sm"
                            class="w-14 h-7 text-xs shrink-0"
                            :title="t('endpointFormExtra.replaceCountHint')"
                            @update:model-value="(v) => updateEndpointBodyRuleField(endpoint.id, index, 'count', v)"
                          />
                          <CheckCircle
                            class="w-4 h-4 shrink-0"
                            :class="getRegexPatternValidation(rule) === true ? 'text-green-600' : getRegexPatternValidation(rule) === false ? 'text-destructive' : 'text-muted-foreground/40'"
                            :title="getRegexPatternValidationTip(rule)"
                          />
                        </template>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-7 w-7 shrink-0"
                          @click="removeEndpointBodyRule(endpoint.id, index)"
                        >
                          <X class="w-3 h-3" />
                        </Button>
                      </div>
                      <EndpointConditionEditor
                        v-if="rule.condition"
                        :model-value="rule.condition"
                        :path-hint="getBodyRuleConditionPathPlaceholder(rule.path)"
                        removable
                        @update:model-value="(condition) => updateEndpointBodyRuleCondition(endpoint.id, index, condition)"
                        @remove="clearEndpointBodyRuleCondition(endpoint.id, index)"
                      />
                    </template>
                  </div>
                </CollapsibleContent>
              </Collapsible>
            </div>
          </div>
        </div>
      </div>

      <!-- 添加新端点 -->
      <div
        v-if="!isFixedProvider && availableFormats.length > 0"
        class="rounded-lg border border-dashed p-3"
      >
        <!-- 卡片头部：API 格式选择 + 添加按钮 -->
        <div class="flex items-center justify-between px-4 py-2.5 bg-muted/30 border-b border-dashed">
          <Select
            v-model="newEndpoint.api_format"
            :open="formatSelectOpen"
            @update:open="handleFormatSelectOpen"
          >
            <SelectTrigger class="h-auto w-auto gap-1.5 !border-0 bg-transparent !shadow-none p-0 font-medium rounded-none flex-row-reverse !ring-0 !ring-offset-0 !outline-none [&>svg]:h-4 [&>svg]:w-4 [&>svg]:opacity-70">
              <SelectValue :placeholder="t('endpointForm.chooseFormat')" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="format in availableFormats"
                :key="format.value"
                :value="format.value"
              >
                {{ format.label }}
              </SelectItem>
            </SelectContent>
          </Select>
          <Button
            variant="outline"
            size="sm"
            class="h-7 px-3"
            :disabled="!newEndpoint.api_format || (!newEndpoint.base_url?.trim() && !provider?.website?.trim()) || addingEndpoint"
            @click="handleAddEndpoint"
          >
            {{ t('endpointForm.add') }}
          </Button>
        </div>
        <!-- 卡片内容：URL 配置 -->
        <div class="p-4">
          <div class="flex items-end gap-3">
            <div class="flex-1 min-w-0 grid grid-cols-3 gap-3">
              <div class="col-span-2 space-y-1.5">
                <Label class="text-xs text-muted-foreground">Base URL</Label>
                <Input
                  v-model="newEndpoint.base_url"
                  size="sm"
                  :placeholder="provider?.website || 'https://api.example.com'"
                />
              </div>
              <div class="space-y-1.5">
                <Label class="text-xs text-muted-foreground">{{ t('endpointForm.customPath') }}</Label>
                <Input
                  v-model="newEndpoint.custom_path"
                  size="sm"
                  :placeholder="newEndpointDefaultPath || t('endpointForm.defaultPathPlaceholder')"
                />
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- 空状态 -->
      <div
        v-if="localEndpoints.length === 0 && availableFormats.length === 0"
        class="text-center py-8 text-muted-foreground"
      >
        <p>{{ t('endpointForm.allConfigured') }}</p>
      </div>
    </div>

    <template #footer>
      <Button
        variant="outline"
        @click="handleClose"
      >
        {{ t('endpointForm.close') }}
      </Button>
    </template>
  </Dialog>

  <!-- 删除端点确认弹窗 -->
  <AlertDialog
    :model-value="deleteConfirmOpen"
    :title="t('endpointForm.deleteTitle')"
    :description="deleteConfirmDescription"
    :confirm-text="t('endpointForm.delete')"
    :cancel-text="t('endpointForm.cancel')"
    type="danger"
    @update:model-value="deleteConfirmOpen = $event"
    @confirm="confirmDeleteEndpoint"
    @cancel="deleteConfirmOpen = false"
  />
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  Dialog,
  Button,
  Input,
  Textarea,
  Label,
  Badge,
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
  Switch,
  Collapsible,
  CollapsibleTrigger,
  CollapsibleContent,
  Popover,
  PopoverTrigger,
  PopoverContent,
} from '@/components/ui'
import { Settings, Trash2, Check, X, Power, ChevronRight, Plus, Shuffle, RotateCcw, Radio, CheckCircle, Save, Filter, HelpCircle, GripVertical, Globe, Code2, AlignLeft } from 'lucide-vue-next'
import { useToast } from '@/composables/useToast'
import { parseApiError } from '@/utils/errorParser'
import { log } from '@/utils/logger'
import AlertDialog from '@/components/common/AlertDialog.vue'
import EndpointConditionEditor from './EndpointConditionEditor.vue'
import EndpointStreamFailoverSettings from './EndpointStreamFailoverSettings.vue'
import ProxyNodeSelect from './ProxyNodeSelect.vue'
import { getDefaultEndpointPath, normalizeEndpointApiFormat } from './endpoint-default-paths'
import {
  buildEndpointStreamFailoverUpdate,
  endpointStreamFailoverStateChanged,
  initEndpointStreamFailoverState,
  isOpenAiResponsesStreamFailoverEndpoint,
  validateEndpointStreamFailoverState,
  type EndpointStreamFailoverState,
  type EndpointStreamFailoverValidationError,
} from './endpoint-stream-failover'
import { useProxyNodesStore } from '@/stores/proxy-nodes'
import {
  createEndpoint,
  getDefaultBodyRules,
  updateEndpoint,
  deleteEndpoint,
  type ProviderEndpoint,
  type ProviderWithEndpointsSummary,
  type HeaderRule,
  type BodyRule,
  type BodyRuleRegexReplace,
} from '@/api/endpoints'
import { adminApi } from '@/api/admin'
import { formatApiFormat } from '@/api/endpoints/types/api-format'
import {
  conditionEquals,
  conditionToEditable,
  createEmptyConditionLeaf,
  editableConditionToApi,
  getBodyRuleConditionPathPlaceholder,
  type EditableConditionNode,
  validateEditableCondition,
} from './endpoint-rule-condition'

// 编辑用的规则类型（统一的可编辑结构）
interface EditableRule {
  action: 'set' | 'drop' | 'rename'
  enabled: boolean
  key: string      // set/drop 用
  value: string    // set 用
  from: string     // rename 用
  to: string       // rename 用
  condition: EditableConditionNode | null
}

// 编辑用的请求体规则类型
type BodyRuleAction = 'set' | 'drop' | 'rename' | 'append' | 'insert' | 'regex_replace'

interface EditableBodyRule {
  action: BodyRuleAction
  enabled: boolean
  path: string     // set/drop/append/insert/regex_replace 用
  value: string    // set/append/insert 用（JSON 格式）
  from: string     // rename 用
  to: string       // rename 用
  index: string    // insert 用（字符串输入，保存时解析为 int）
  pattern: string  // regex_replace 用
  replacement: string // regex_replace 用
  flags: string    // regex_replace 用（i/m/s）
  count: string    // regex_replace 用（空=默认全部；0=全部）
  condition: EditableConditionNode | null
}

// 端点编辑状态（仅 URL、路径、规则，格式转换是直接保存的）
interface EndpointEditState {
  url: string
  path: string
  upstreamStreamPolicy: string
  streamFailover: EndpointStreamFailoverState
  rules: EditableRule[]
  responseRules: EditableRule[]
  bodyRules: EditableBodyRule[]
}

interface EndpointRulesJsonPayload {
  header_rules: HeaderRule[]
  body_rules: BodyRule[]
  response_header_rules: HeaderRule[]
}

const props = defineProps<{
  modelValue: boolean
  provider: ProviderWithEndpointsSummary | null
  endpoints?: ProviderEndpoint[]
  systemFormatConversionEnabled?: boolean
  providerFormatConversionEnabled?: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  'endpointCreated': []
  'endpointUpdated': []
}>()

const { t } = useI18n()

// 计算端点级格式转换是否应该被禁用
const isEndpointFormatConversionDisabled = computed(() => {
  return props.systemFormatConversionEnabled || props.providerFormatConversionEnabled
})

// 获取禁用提示
const formatConversionDisabledTooltip = computed(() => {
  if (props.systemFormatConversionEnabled) {
    return t('endpointFormExtra.closeSystemFirst')
  }
  if (props.providerFormatConversionEnabled) {
    return t('endpointFormExtra.closeProviderFirst')
  }
  return ''
})

const { success, error: showError } = useToast()
const proxyNodesStore = useProxyNodesStore()

// 规则 Select 的展开状态（与 Collapsible 分开管理）
const ruleSelectOpen = ref<Record<string, boolean>>({})
const responseRuleSelectOpen = ref<Record<string, boolean>>({})

// 打开规则选择器时关闭其他所有下拉
function handleRuleSelectOpen(endpointId: string, index: number, open: boolean) {
  if (open) {
    formatSelectOpen.value = false
    // 关闭其他 Select
    Object.keys(ruleSelectOpen.value).forEach(key => {
      ruleSelectOpen.value[key] = false
    })
    Object.keys(responseRuleSelectOpen.value).forEach(key => {
      responseRuleSelectOpen.value[key] = false
    })
  }
  ruleSelectOpen.value[`${endpointId}-${index}`] = open
}

// 打开响应头规则选择器时关闭其他所有下拉
function handleResponseRuleSelectOpen(endpointId: string, index: number, open: boolean) {
  if (open) {
    formatSelectOpen.value = false
    Object.keys(ruleSelectOpen.value).forEach(key => {
      ruleSelectOpen.value[key] = false
    })
    Object.keys(responseRuleSelectOpen.value).forEach(key => {
      responseRuleSelectOpen.value[key] = false
    })
    Object.keys(bodyRuleSelectOpen.value).forEach(key => {
      bodyRuleSelectOpen.value[key] = false
    })
  }
  responseRuleSelectOpen.value[`${endpointId}-${index}`] = open
}

// 打开格式选择器时关闭其他所有下拉
function handleFormatSelectOpen(open: boolean) {
  if (open) {
    // 关闭所有规则 Select
    Object.keys(ruleSelectOpen.value).forEach(key => {
      ruleSelectOpen.value[key] = false
    })
    Object.keys(responseRuleSelectOpen.value).forEach(key => {
      responseRuleSelectOpen.value[key] = false
    })
    Object.keys(bodyRuleSelectOpen.value).forEach(key => {
      bodyRuleSelectOpen.value[key] = false
    })
  }
  formatSelectOpen.value = open
}

// 打开请求体规则选择器时关闭其他所有下拉
function handleBodyRuleSelectOpen(endpointId: string, index: number, open: boolean) {
  if (open) {
    formatSelectOpen.value = false
    // 关闭所有 Select
    Object.keys(ruleSelectOpen.value).forEach(key => {
      ruleSelectOpen.value[key] = false
    })
    Object.keys(responseRuleSelectOpen.value).forEach(key => {
      responseRuleSelectOpen.value[key] = false
    })
    Object.keys(bodyRuleSelectOpen.value).forEach(key => {
      bodyRuleSelectOpen.value[key] = false
    })
  }
  bodyRuleSelectOpen.value[`${endpointId}-${index}`] = open
}

function clearHeaderRuleSelectOpen(endpointId: string) {
  Object.keys(ruleSelectOpen.value).forEach((key) => {
    if (key.startsWith(`${endpointId}-`)) {
      delete ruleSelectOpen.value[key]
    }
  })
}

function clearResponseRuleSelectOpen(endpointId: string) {
  Object.keys(responseRuleSelectOpen.value).forEach((key) => {
    if (key.startsWith(`${endpointId}-`)) {
      delete responseRuleSelectOpen.value[key]
    }
  })
}

function clearBodyRuleSelectOpen(endpointId: string) {
  Object.keys(bodyRuleSelectOpen.value).forEach((key) => {
    if (key.startsWith(`${endpointId}-`)) {
      delete bodyRuleSelectOpen.value[key]
    }
  })
}

function isHeaderRuleDragging(endpointId: string, index: number): boolean {
  return headerRuleDraggedIndex.value[endpointId] === index
}

function isHeaderRuleDragOver(endpointId: string, index: number): boolean {
  return headerRuleDragOverIndex.value[endpointId] === index
}

function isBodyRuleDragging(endpointId: string, index: number): boolean {
  return bodyRuleDraggedIndex.value[endpointId] === index
}

function isResponseRuleDragging(endpointId: string, index: number): boolean {
  return responseRuleDraggedIndex.value[endpointId] === index
}

function isBodyRuleDragOver(endpointId: string, index: number): boolean {
  return bodyRuleDragOverIndex.value[endpointId] === index
}

function isResponseRuleDragOver(endpointId: string, index: number): boolean {
  return responseRuleDragOverIndex.value[endpointId] === index
}

function clearHeaderRuleDragState(endpointId: string) {
  headerRuleDraggedIndex.value[endpointId] = null
  headerRuleDragOverIndex.value[endpointId] = null
}

function clearResponseRuleDragState(endpointId: string) {
  responseRuleDraggedIndex.value[endpointId] = null
  responseRuleDragOverIndex.value[endpointId] = null
}

function clearBodyRuleDragState(endpointId: string) {
  bodyRuleDraggedIndex.value[endpointId] = null
  bodyRuleDragOverIndex.value[endpointId] = null
}

function handleHeaderRuleDragStart(endpointId: string, index: number, event: DragEvent) {
  const rules = getEndpointEditRules(endpointId)
  if (!rules[index]) return

  headerRuleDraggedIndex.value[endpointId] = index
  headerRuleDragOverIndex.value[endpointId] = null
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData('text/plain', `header:${endpointId}:${index}`)
  }
}

function handleHeaderRuleDragOver(endpointId: string, index: number) {
  const dragged = headerRuleDraggedIndex.value[endpointId]
  if (dragged === null || dragged === undefined || dragged === index) return
  headerRuleDragOverIndex.value[endpointId] = index
}

function handleHeaderRuleDragLeave(endpointId: string, index: number) {
  if (headerRuleDragOverIndex.value[endpointId] === index) {
    headerRuleDragOverIndex.value[endpointId] = null
  }
}

function handleHeaderRuleDrop(endpointId: string, targetIndex: number) {
  const dragIndex = headerRuleDraggedIndex.value[endpointId]
  clearHeaderRuleDragState(endpointId)
  if (dragIndex === null || dragIndex === undefined || dragIndex === targetIndex) return

  const rules = getEndpointEditRules(endpointId)
  if (dragIndex < 0 || dragIndex >= rules.length || targetIndex < 0 || targetIndex >= rules.length) return

  const [draggedRule] = rules.splice(dragIndex, 1)
  rules.splice(targetIndex, 0, draggedRule)
  clearHeaderRuleSelectOpen(endpointId)
}

function handleHeaderRuleDragEnd(endpointId: string) {
  clearHeaderRuleDragState(endpointId)
}

function handleResponseRuleDragStart(endpointId: string, index: number, event: DragEvent) {
  const rules = getEndpointEditResponseRules(endpointId)
  if (!rules[index]) return

  responseRuleDraggedIndex.value[endpointId] = index
  responseRuleDragOverIndex.value[endpointId] = null
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData('text/plain', `response:${endpointId}:${index}`)
  }
}

function handleResponseRuleDragOver(endpointId: string, index: number) {
  const dragged = responseRuleDraggedIndex.value[endpointId]
  if (dragged === null || dragged === undefined || dragged === index) return
  responseRuleDragOverIndex.value[endpointId] = index
}

function handleResponseRuleDragLeave(endpointId: string, index: number) {
  if (responseRuleDragOverIndex.value[endpointId] === index) {
    responseRuleDragOverIndex.value[endpointId] = null
  }
}

function handleResponseRuleDrop(endpointId: string, targetIndex: number) {
  const dragIndex = responseRuleDraggedIndex.value[endpointId]
  clearResponseRuleDragState(endpointId)
  if (dragIndex === null || dragIndex === undefined || dragIndex === targetIndex) return

  const rules = getEndpointEditResponseRules(endpointId)
  if (dragIndex < 0 || dragIndex >= rules.length || targetIndex < 0 || targetIndex >= rules.length) return

  const [draggedRule] = rules.splice(dragIndex, 1)
  rules.splice(targetIndex, 0, draggedRule)
  clearResponseRuleSelectOpen(endpointId)
}

function handleResponseRuleDragEnd(endpointId: string) {
  clearResponseRuleDragState(endpointId)
}

function handleBodyRuleDragStart(endpointId: string, index: number, event: DragEvent) {
  const rules = getEndpointEditBodyRules(endpointId)
  if (!rules[index]) return

  bodyRuleDraggedIndex.value[endpointId] = index
  bodyRuleDragOverIndex.value[endpointId] = null
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData('text/plain', `body:${endpointId}:${index}`)
  }
}

function handleBodyRuleDragOver(endpointId: string, index: number) {
  const dragged = bodyRuleDraggedIndex.value[endpointId]
  if (dragged === null || dragged === undefined || dragged === index) return
  bodyRuleDragOverIndex.value[endpointId] = index
}

function handleBodyRuleDragLeave(endpointId: string, index: number) {
  if (bodyRuleDragOverIndex.value[endpointId] === index) {
    bodyRuleDragOverIndex.value[endpointId] = null
  }
}

function handleBodyRuleDrop(endpointId: string, targetIndex: number) {
  const dragIndex = bodyRuleDraggedIndex.value[endpointId]
  clearBodyRuleDragState(endpointId)
  if (dragIndex === null || dragIndex === undefined || dragIndex === targetIndex) return

  const rules = getEndpointEditBodyRules(endpointId)
  if (dragIndex < 0 || dragIndex >= rules.length || targetIndex < 0 || targetIndex >= rules.length) return

  const [draggedRule] = rules.splice(dragIndex, 1)
  rules.splice(targetIndex, 0, draggedRule)
  clearBodyRuleSelectOpen(endpointId)
}

function handleBodyRuleDragEnd(endpointId: string) {
  clearBodyRuleDragState(endpointId)
}

// 状态
const addingEndpoint = ref(false)
const savingEndpointId = ref<string | null>(null)
const resettingDefaultRulesEndpointId = ref<string | null>(null)
const deletingEndpointId = ref<string | null>(null)
const togglingEndpointId = ref<string | null>(null)
const togglingFormatEndpointId = ref<string | null>(null)
const formatSelectOpen = ref(false)
const endpointProxyPopoverOpen = ref<Record<string, boolean>>({})

// 删除确认弹窗状态
const deleteConfirmOpen = ref(false)
const endpointToDelete = ref<ProviderEndpoint | null>(null)

// 请求规则折叠状态
const endpointRulesExpanded = ref<Record<string, boolean>>({})
const endpointRulesJsonMode = ref<Record<string, boolean>>({})
const endpointRulesJsonDraft = ref<Record<string, string>>({})
const endpointRulesJsonError = ref<Record<string, string | null>>({})
const endpointRulesJsonDirty = ref<Record<string, boolean>>({})

// 请求体规则 Select 的展开状态
const bodyRuleSelectOpen = ref<Record<string, boolean>>({})

// 请求体规则说明 Popover 的展开状态
const bodyRuleHelpOpenEndpointId = ref<string | null>(null)

// 规则拖拽状态（按 endpoint 维度）
const headerRuleDraggedIndex = ref<Record<string, number | null>>({})
const headerRuleDragOverIndex = ref<Record<string, number | null>>({})
const responseRuleDraggedIndex = ref<Record<string, number | null>>({})
const responseRuleDragOverIndex = ref<Record<string, number | null>>({})
const bodyRuleDraggedIndex = ref<Record<string, number | null>>({})
const bodyRuleDragOverIndex = ref<Record<string, number | null>>({})

function setBodyRuleHelpOpen(endpointId: string, open: boolean) {
  bodyRuleHelpOpenEndpointId.value = open ? endpointId : null
}

// 每个端点的编辑状态（内联编辑）
const endpointEditStates = ref<Record<string, EndpointEditState>>({})
const defaultBodyRulesByFormat = ref<Record<string, BodyRule[]>>({})
const defaultBodyRulesLoaded = ref<Record<string, boolean>>({})
const loadingDefaultBodyRulesByFormat = ref<Record<string, boolean>>({})

// 系统保留的 header 名称（不允许用户设置）
const RESERVED_HEADERS = new Set([
  'authorization',
  'x-api-key',
  'x-goog-api-key',
  'content-type',
  'content-length',
  'host',
])

const RESERVED_RESPONSE_HEADERS = new Set([
  'content-length',
])

const RESPONSE_HEADER_RULES_CONFIG_KEY = 'response_header_rules'
const RESPONSE_HEADER_RULES_CAMEL_CONFIG_KEY = 'responseHeaderRules'

// 系统保留的 body 字段名（不允许用户设置）
const RESERVED_BODY_FIELDS = new Set([
  'stream',
])

const BODY_RULE_JSON_ACTIONS = new Set(['set', 'drop', 'rename', 'append', 'insert', 'regex_replace'])
const CONDITION_JSON_OPS = new Set(['eq', 'neq', 'gt', 'lt', 'gte', 'lte', 'starts_with', 'ends_with', 'contains', 'matches', 'exists', 'not_exists', 'in', 'type_is'])
const CONDITION_JSON_SOURCES = new Set(['body', 'current', 'original', 'request_headers', 'headers'])

function isJsonObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function readJsonRulesArray(root: Record<string, unknown>, key: keyof EndpointRulesJsonPayload, label: string): { value: unknown[]; error: string | null } {
  const raw = root[key]
  if (raw === undefined || raw === null) return { value: [], error: null }
  if (!Array.isArray(raw)) return { value: [], error: t('endpointFormExtra.jsonArrayOrNull', { label }) }
  return { value: raw, error: null }
}

function validateJsonCondition(rule: Record<string, unknown>, label: string, index: number): string | null {
  const raw = rule.condition
  if (raw === undefined || raw === null) return null
  if (!isJsonObject(raw)) return t('endpointFormExtra.conditionObjectError', { label, index: index + 1 })
  const shapeError = validateJsonConditionShape(raw, t('endpointFormExtra.ruleFieldLabel', { label, index: index + 1, key: 'condition' }))
  if (shapeError) return shapeError
  const editable = conditionToEditable(raw as unknown as BodyRule['condition'])
  const err = validateEditableCondition(editable)
  return err ? t('endpointFormExtra.ruleErrorWithMessage', { label, index: index + 1, message: err }) : null
}

function validateJsonConditionShape(condition: Record<string, unknown>, label: string): string | null {
  if (Object.prototype.hasOwnProperty.call(condition, 'all')) {
    if (!Array.isArray(condition.all)) return t('endpointFormExtra.conditionArrayError', { label: `${label}.all` })
    for (let i = 0; i < condition.all.length; i++) {
      const child = condition.all[i]
      if (!isJsonObject(child)) return t('endpointFormExtra.conditionObjectPathError', { label: `${label}.all[${i}]` })
      const err = validateJsonConditionShape(child, `${label}.all[${i}]`)
      if (err) return err
    }
    return null
  }
  if (Object.prototype.hasOwnProperty.call(condition, 'any')) {
    if (!Array.isArray(condition.any)) return t('endpointFormExtra.conditionArrayError', { label: `${label}.any` })
    for (let i = 0; i < condition.any.length; i++) {
      const child = condition.any[i]
      if (!isJsonObject(child)) return t('endpointFormExtra.conditionObjectPathError', { label: `${label}.any[${i}]` })
      const err = validateJsonConditionShape(child, `${label}.any[${i}]`)
      if (err) return err
    }
    return null
  }

  if (typeof condition.path !== 'string') return t('endpointFormExtra.conditionStringError', { label: `${label}.path` })
  if (typeof condition.op !== 'string' || !CONDITION_JSON_OPS.has(condition.op)) {
    return t('endpointFormExtra.conditionInvalidError', { label: `${label}.op` })
  }
  if (condition.source !== undefined && (typeof condition.source !== 'string' || !CONDITION_JSON_SOURCES.has(condition.source))) {
    return t('endpointFormExtra.conditionInvalidError', { label: `${label}.source` })
  }
  return null
}

function requireJsonString(rule: Record<string, unknown>, key: string, label: string, index: number): string | null {
  return typeof rule[key] === 'string' ? null : t('endpointFormExtra.ruleStringError', { label, index: index + 1, key })
}

function validateHeaderRuleJson(rule: unknown, label: string, index: number): string | null {
  if (!isJsonObject(rule)) return t('endpointFormExtra.ruleObjectError', { label, index: index + 1 })
  if (rule.enabled !== undefined && typeof rule.enabled !== 'boolean') {
    return t('endpointFormExtra.ruleBooleanError', { label, index: index + 1, key: 'enabled' })
  }
  const action = rule.action
  if (action !== 'set' && action !== 'drop' && action !== 'rename') {
    return t('endpointFormExtra.headerActionError', { label, index: index + 1 })
  }
  if (action === 'set') {
    return requireJsonString(rule, 'key', label, index)
      || requireJsonString(rule, 'value', label, index)
      || validateJsonCondition(rule, label, index)
  }
  if (action === 'drop') {
    return requireJsonString(rule, 'key', label, index)
      || validateJsonCondition(rule, label, index)
  }
  return requireJsonString(rule, 'from', label, index)
    || requireJsonString(rule, 'to', label, index)
    || validateJsonCondition(rule, label, index)
}

function validateBodyRuleJson(rule: unknown, label: string, index: number): string | null {
  if (!isJsonObject(rule)) return t('endpointFormExtra.ruleObjectError', { label, index: index + 1 })
  if (rule.enabled !== undefined && typeof rule.enabled !== 'boolean') {
    return t('endpointFormExtra.ruleBooleanError', { label, index: index + 1, key: 'enabled' })
  }
  const action = typeof rule.action === 'string' ? rule.action : ''
  if (!BODY_RULE_JSON_ACTIONS.has(action)) {
    return t('endpointFormExtra.ruleInvalidError', { label, index: index + 1, key: 'action' })
  }

  if (action === 'set' || action === 'append') {
    return requireJsonString(rule, 'path', label, index)
      || (Object.prototype.hasOwnProperty.call(rule, 'value') ? null : t('endpointFormExtra.ruleRequiredError', { label, index: index + 1, key: 'value' }))
      || validateJsonCondition(rule, label, index)
  }
  if (action === 'drop') {
    return requireJsonString(rule, 'path', label, index)
      || validateJsonCondition(rule, label, index)
  }
  if (action === 'rename') {
    return requireJsonString(rule, 'from', label, index)
      || requireJsonString(rule, 'to', label, index)
      || validateJsonCondition(rule, label, index)
  }
  if (action === 'insert') {
    if (requireJsonString(rule, 'path', label, index)) return requireJsonString(rule, 'path', label, index)
    if (!Number.isInteger(rule.index)) return t('endpointFormExtra.ruleIntegerError', { label, index: index + 1, key: 'index' })
    if (!Object.prototype.hasOwnProperty.call(rule, 'value')) return t('endpointFormExtra.ruleRequiredError', { label, index: index + 1, key: 'value' })
    return validateJsonCondition(rule, label, index)
  }
  if (action === 'regex_replace') {
    if (requireJsonString(rule, 'path', label, index)) return requireJsonString(rule, 'path', label, index)
    if (requireJsonString(rule, 'pattern', label, index)) return requireJsonString(rule, 'pattern', label, index)
    if (typeof rule.replacement !== 'string') return t('endpointFormExtra.ruleStringError', { label, index: index + 1, key: 'replacement' })
    if (rule.flags !== undefined && typeof rule.flags !== 'string') return t('endpointFormExtra.ruleStringError', { label, index: index + 1, key: 'flags' })
    if (rule.count !== undefined && !Number.isInteger(rule.count)) return t('endpointFormExtra.ruleIntegerError', { label, index: index + 1, key: 'count' })
    return validateJsonCondition(rule, label, index)
  }
  return t('endpointFormExtra.ruleInvalidError', { label, index: index + 1, key: 'action' })
}

function parseEndpointRulesJsonDraft(draft: string): { value: EndpointRulesJsonPayload | null; error: string | null } {
  const raw = draft.trim()
  if (!raw) {
    return { value: { header_rules: [], body_rules: [], response_header_rules: [] }, error: null }
  }

  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch (error: unknown) {
    return { value: null, error: error instanceof Error ? error.message : t('endpointFormExtra.invalidJson') }
  }
  if (!isJsonObject(parsed)) return { value: null, error: t('endpointFormExtra.rulesJsonObject') }

  const header = readJsonRulesArray(parsed, 'header_rules', 'header_rules')
  if (header.error) return { value: null, error: header.error }
  const body = readJsonRulesArray(parsed, 'body_rules', 'body_rules')
  if (body.error) return { value: null, error: body.error }
  const response = readJsonRulesArray(parsed, 'response_header_rules', 'response_header_rules')
  if (response.error) return { value: null, error: response.error }

  for (let i = 0; i < header.value.length; i++) {
    const error = validateHeaderRuleJson(header.value[i], 'header_rules ', i)
    if (error) return { value: null, error }
  }
  for (let i = 0; i < body.value.length; i++) {
    const error = validateBodyRuleJson(body.value[i], 'body_rules ', i)
    if (error) return { value: null, error }
  }
  for (let i = 0; i < response.value.length; i++) {
    const error = validateHeaderRuleJson(response.value[i], 'response_header_rules ', i)
    if (error) return { value: null, error }
  }

  return {
    value: {
      header_rules: header.value as HeaderRule[],
      body_rules: body.value as BodyRule[],
      response_header_rules: response.value as HeaderRule[],
    },
    error: null,
  }
}

function applyEndpointRulesJsonDraft(
  endpointId: string,
  options: { notify?: boolean; notifyError?: boolean } = {},
): boolean {
  const notifyError = options.notifyError !== false
  const parsed = parseEndpointRulesJsonDraft(endpointRulesJsonDraft.value[endpointId] ?? '')
  if (!parsed.value) {
    endpointRulesJsonError.value[endpointId] = parsed.error
    if (notifyError) showError(parsed.error || t('endpointForm.invalidRulesJson'))
    return false
  }

  const state = ensureEndpointEditState(endpointId)
  if (!state) {
    endpointRulesJsonError.value[endpointId] = t('endpointForm.editStateUnavailable')
    if (notifyError) showError(t('endpointForm.editStateUnavailable'))
    return false
  }

  state.rules = editableHeaderRulesFromRules(parsed.value.header_rules)
  state.bodyRules = editableBodyRulesFromRules(parsed.value.body_rules)
  state.responseRules = editableHeaderRulesFromRules(parsed.value.response_header_rules)

  clearHeaderRuleSelectOpen(endpointId)
  clearResponseRuleSelectOpen(endpointId)
  clearBodyRuleSelectOpen(endpointId)
  clearHeaderRuleDragState(endpointId)
  clearResponseRuleDragState(endpointId)
  clearBodyRuleDragState(endpointId)

  const validationError = getHeaderValidationErrorForEndpoint(endpointId)
    || getResponseHeaderValidationErrorForEndpoint(endpointId)
    || getBodyValidationErrorForEndpoint(endpointId)
  if (validationError) {
    endpointRulesJsonError.value[endpointId] = validationError
    if (notifyError) showError(validationError)
    return false
  }

  endpointRulesJsonDraft.value[endpointId] = stringifyEndpointRulesJsonPayload(parsed.value)
  endpointRulesJsonError.value[endpointId] = null
  endpointRulesJsonDirty.value[endpointId] = false
  if (options.notify !== false) success(t('endpointForm.jsonRulesApplied'))
  return true
}

// {{$original}} 占位符处理
const ORIGINAL_PLACEHOLDER = '{{$original}}'
const ORIGINAL_SENTINEL = '__AETHER_ORIGINAL__'

// 将 {{$original}} 替换为合法 JSON 以便 JSON.parse 校验
// 处理三种写法：裸占位符 {{$original}}、带引号 "{{$original}}"、引号内拼接 "prefix_{{$original}}_suffix"
function prepareValueForJsonParse(raw: string): string {
  // Step 1: 纯文本替换占位符为 sentinel
  const result = raw.replaceAll(ORIGINAL_PLACEHOLDER, ORIGINAL_SENTINEL)

  // Step 2: 尝试直接 parse（占位符在引号内时已经是合法 JSON）
  try { JSON.parse(result); return result } catch { /* sentinel not in valid JSON position */ }

  // Step 3: 有裸 sentinel 不在引号内，需要扫描并补引号
  let out = ''
  let inStr = false
  let i = 0
  while (i < result.length) {
    if (result[i] === '\\' && inStr) {
      out += result[i] + (result[i + 1] || '')
      i += 2
      continue
    }
    if (result[i] === '"') {
      inStr = !inStr
      out += result[i]
      i++
      continue
    }
    if (!inStr && result.startsWith(ORIGINAL_SENTINEL, i)) {
      out += `"${  ORIGINAL_SENTINEL  }"`
      i += ORIGINAL_SENTINEL.length
      continue
    }
    out += result[i]
    i++
  }
  return out
}

// 递归还原: 将 sentinel 字符串还原为 {{$original}}
function restoreOriginalPlaceholder(value: unknown): unknown {
  if (typeof value === 'string') {
    if (value === ORIGINAL_SENTINEL) return ORIGINAL_PLACEHOLDER
    if (value.includes(ORIGINAL_SENTINEL)) {
      return value.replaceAll(ORIGINAL_SENTINEL, ORIGINAL_PLACEHOLDER)
    }
    return value
  }
  if (Array.isArray(value)) return value.map(item => restoreOriginalPlaceholder(item))
  if (value !== null && typeof value === 'object') {
    const result: Record<string, unknown> = {}
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) result[k] = restoreOriginalPlaceholder(v)
    return result
  }
  return value
}

function parseBodyRulePathParts(path: string): string[] | null {
  const raw = path.trim()
  if (!raw) return null

  const parts: string[] = []
  let current = ''
  for (let i = 0; i < raw.length; i++) {
    const ch = raw[i]
    // 支持 \. 转义字面量点号；其他反斜杠组合按字面量保留
    if (ch === '\\' && i + 1 < raw.length && raw[i + 1] === '.') {
      current += '.'
      i++
      continue
    }
    if (ch === '.') {
      if (!current) return null // 禁止空段：.a / a. / a..b
      parts.push(current)
      current = ''
      continue
    }
    current += ch
  }
  if (!current) return null
  parts.push(current)
  return parts
}

function initBodyRuleSetValueForEditor(value: unknown): { value: string } {
  if (value === undefined) return { value: '' }

  // 所有值都用 JSON 格式回显
  try {
    return { value: JSON.stringify(value) }
  } catch {
    return { value: String(value) }
  }
}

// 内部状态
const internalOpen = computed(() => props.modelValue)

const isFixedProvider = computed(() => {
  const t = props.provider?.provider_type
  return !!t && t !== 'custom' && t !== 'claude_code_api'
})

// 新端点表单
const newEndpoint = ref({
  api_format: '',
  base_url: '',
  custom_path: '',
})

// API 格式列表
const apiFormats = ref<Array<{ value: string; label: string; default_path: string }>>([])

// 本地端点列表
const localEndpoints = ref<ProviderEndpoint[]>([])

// 可用的格式（未添加的）
const availableFormats = computed(() => {
  const existingFormats = localEndpoints.value.map(e => e.api_format)
  return apiFormats.value.filter(f => !existingFormats.includes(f.value))
})

// 删除确认弹窗描述
const deleteConfirmDescription = computed(() => {
  if (!endpointToDelete.value) return ''
  const formatLabel = formatApiFormat(endpointToDelete.value.api_format)
  return t('endpointFormExtra.deleteConfirm', { format: formatLabel })
})

function defaultBodyRulesCacheKey(apiFormat: string): string {
  const providerType = (props.provider?.provider_type || '').toLowerCase()
  return providerType ? `${apiFormat}:${providerType}` : apiFormat
}

function hasDefaultBodyRules(apiFormat: string): boolean {
  const cacheKey = defaultBodyRulesCacheKey(apiFormat)
  if (!defaultBodyRulesLoaded.value[cacheKey]) return false
  return (defaultBodyRulesByFormat.value[cacheKey]?.length || 0) > 0
}

async function loadDefaultBodyRulesForFormat(apiFormat: string, force = false): Promise<BodyRule[]> {
  if (!apiFormat) return []
  const providerType = (props.provider?.provider_type || '').toLowerCase()
  const cacheKey = defaultBodyRulesCacheKey(apiFormat)
  if (!force && defaultBodyRulesLoaded.value[cacheKey]) {
    return defaultBodyRulesByFormat.value[cacheKey] || []
  }
  if (loadingDefaultBodyRulesByFormat.value[cacheKey]) {
    return defaultBodyRulesByFormat.value[cacheKey] || []
  }

  loadingDefaultBodyRulesByFormat.value[cacheKey] = true
  try {
    const response = await getDefaultBodyRules(apiFormat, providerType || undefined)
    const rules = response.body_rules || []
    defaultBodyRulesByFormat.value[cacheKey] = rules
    defaultBodyRulesLoaded.value[cacheKey] = true
    return rules
  } catch (error: unknown) {
    defaultBodyRulesByFormat.value[cacheKey] = []
    defaultBodyRulesLoaded.value[cacheKey] = true
    log.warn('加载默认请求体规则失败', { apiFormat, error })
    return []
  } finally {
    loadingDefaultBodyRulesByFormat.value[cacheKey] = false
  }
}

async function preloadDefaultBodyRules(endpoints: ProviderEndpoint[]): Promise<void> {
  const formats = Array.from(new Set(endpoints.map(e => e.api_format).filter(Boolean)))
  await Promise.all(formats.map(fmt => loadDefaultBodyRulesForFormat(fmt)))
}

// 获取指定 API 格式的默认路径
function getDefaultPath(apiFormat: string, baseUrl?: string): string {
  const providerType = (props.provider?.provider_type || '').toLowerCase()
  return getDefaultEndpointPath({
    apiFormat,
    providerType,
    baseUrl,
    apiFormats: apiFormats.value,
  })
}

function getDisplayedPath(endpoint: ProviderEndpoint): string {
  if (isFixedProvider.value) {
    return getDefaultPath(endpoint.api_format, endpoint.base_url)
  }
  return getEndpointEditState(endpoint.id)?.path ?? (endpoint.custom_path || '')
}

// 读取端点的上游流式策略（endpoint.config.upstream_stream_policy）
function getEndpointUpstreamStreamPolicy(endpoint: ProviderEndpoint): string {
  const cfg = endpoint.config || {}
  const raw = (cfg.upstream_stream_policy ?? cfg.upstreamStreamPolicy ?? cfg.upstream_stream) as unknown
  if (raw === null || raw === undefined) return 'auto'
  if (typeof raw === 'boolean') return raw ? 'force_stream' : 'force_non_stream'
  const s = String(raw).trim().toLowerCase()
  if (!s || s === 'auto' || s === 'follow' || s === 'client' || s === 'default') return 'auto'
  if (s === 'force_stream' || s === 'stream' || s === 'sse' || s === 'true' || s === '1') return 'force_stream'
  if (s === 'force_non_stream' || s === 'force_sync' || s === 'non_stream' || s === 'sync' || s === 'false' || s === '0') return 'force_non_stream'
  return 'auto'
}

function endpointProxyNodeId(endpoint: ProviderEndpoint): string {
  if (endpoint.proxy?.enabled === false) return ''
  return endpoint.proxy?.node_id?.trim() || ''
}

function getEndpointProxyNodeName(endpoint: ProviderEndpoint): string {
  const nodeId = endpointProxyNodeId(endpoint)
  if (!nodeId) return t('endpointFormExtra.unknownNode')
  const node = proxyNodesStore.nodes.find(n => n.id === nodeId)
  return node ? node.name : `${nodeId.slice(0, 8)}...`
}

function getEndpointProxyTitle(endpoint: ProviderEndpoint): string {
  const nodeId = endpointProxyNodeId(endpoint)
  return nodeId
    ? t('endpointFormExtra.endpointProxyLabel', { name: getEndpointProxyNodeName(endpoint) })
    : t('endpointFormExtra.setEndpointProxy')
}

function handleEndpointProxyPopoverToggle(endpointId: string, open: boolean) {
  endpointProxyPopoverOpen.value[endpointId] = open
  if (open) {
    proxyNodesStore.ensureLoaded()
  }
}

function replaceLocalEndpoint(updated: ProviderEndpoint) {
  localEndpoints.value = localEndpoints.value.map(endpoint =>
    endpoint.id === updated.id ? updated : endpoint,
  )
}

async function setEndpointProxy(endpoint: ProviderEndpoint, nodeId: string) {
  const normalizedNodeId = nodeId.trim()
  if (!normalizedNodeId) return

  savingEndpointId.value = endpoint.id
  try {
    const updated = await updateEndpoint(endpoint.id, {
      proxy: { node_id: normalizedNodeId, enabled: true },
    })
    replaceLocalEndpoint(updated)
    endpointProxyPopoverOpen.value[endpoint.id] = false
    success(t('endpointForm.proxyUpdated'))
    emit('endpointUpdated')
  } catch (error: unknown) {
    showError(parseApiError(error, t('endpointForm.proxyUpdateFailed')), t('endpointForm.error'))
  } finally {
    savingEndpointId.value = null
  }
}

async function clearEndpointProxy(endpoint: ProviderEndpoint) {
  savingEndpointId.value = endpoint.id
  try {
    const updated = await updateEndpoint(endpoint.id, { proxy: null })
    replaceLocalEndpoint(updated)
    endpointProxyPopoverOpen.value[endpoint.id] = false
    success(t('endpointForm.proxyCleared'))
    emit('endpointUpdated')
  } catch (error: unknown) {
    showError(parseApiError(error, t('endpointForm.proxyClearFailed')), t('endpointForm.error'))
  } finally {
    savingEndpointId.value = null
  }
}

function emptyHeaderRule(): EditableRule {
  return { action: 'set', enabled: true, key: '', value: '', from: '', to: '', condition: null }
}

function editableHeaderRulesFromRules(rules: HeaderRule[] | null | undefined): EditableRule[] {
  if (!Array.isArray(rules)) return []
  const editableRules: EditableRule[] = []
  for (const rule of rules) {
    if (rule.action === 'set') {
      editableRules.push({ ...emptyHeaderRule(), action: 'set', enabled: rule.enabled !== false, key: rule.key, value: rule.value || '', condition: conditionToEditable(rule.condition) })
    } else if (rule.action === 'drop') {
      editableRules.push({ ...emptyHeaderRule(), action: 'drop', enabled: rule.enabled !== false, key: rule.key, condition: conditionToEditable(rule.condition) })
    } else if (rule.action === 'rename') {
      editableRules.push({ ...emptyHeaderRule(), action: 'rename', enabled: rule.enabled !== false, from: rule.from, to: rule.to, condition: conditionToEditable(rule.condition) })
    }
  }
  return editableRules
}

function emptyBodyRule(action: BodyRuleAction = 'set'): EditableBodyRule {
  return {
    action,
    enabled: true,
    path: '',
    value: '',
    from: '',
    to: '',
    index: '',
    pattern: '',
    replacement: '',
    flags: '',
    count: '',
    condition: null,
  }
}

function editableBodyRulesFromRules(rules: BodyRule[] | null | undefined): EditableBodyRule[] {
  const bodyRules: EditableBodyRule[] = []
  if (!Array.isArray(rules)) return bodyRules

  for (const rule of rules) {
    if (rule.action === 'set') {
      const { value } = initBodyRuleSetValueForEditor(rule.value)
      bodyRules.push({ ...emptyBodyRule('set'), enabled: rule.enabled !== false, path: rule.path, value, condition: conditionToEditable(rule.condition) })
    } else if (rule.action === 'drop') {
      bodyRules.push({ ...emptyBodyRule('drop'), enabled: rule.enabled !== false, path: rule.path, condition: conditionToEditable(rule.condition) })
    } else if (rule.action === 'rename') {
      bodyRules.push({ ...emptyBodyRule('rename'), enabled: rule.enabled !== false, from: rule.from, to: rule.to, condition: conditionToEditable(rule.condition) })
    } else if (rule.action === 'append') {
      const { value } = initBodyRuleSetValueForEditor(rule.value)
      bodyRules.push({ ...emptyBodyRule('append'), enabled: rule.enabled !== false, path: rule.path || '', value, condition: conditionToEditable(rule.condition) })
    } else if (rule.action === 'insert') {
      const { value } = initBodyRuleSetValueForEditor(rule.value)
      bodyRules.push({ ...emptyBodyRule('insert'), enabled: rule.enabled !== false, path: rule.path || '', value, index: String(rule.index ?? ''), condition: conditionToEditable(rule.condition) })
    } else if (rule.action === 'regex_replace') {
      bodyRules.push({
        ...emptyBodyRule('regex_replace'),
        enabled: rule.enabled !== false,
        path: rule.path || '',
        pattern: rule.pattern || '',
        replacement: rule.replacement || '',
        flags: rule.flags || '',
        count: rule.count === undefined || rule.count === null ? '' : String(rule.count),
        condition: conditionToEditable(rule.condition),
      })
    }
  }
  return bodyRules
}

// 初始化端点的编辑状态
function initEndpointEditState(endpoint: ProviderEndpoint): EndpointEditState {
  const rules = editableHeaderRulesFromRules(endpoint.header_rules)
  const responseRules = editableHeaderRulesFromRules(getEndpointResponseHeaderRules(endpoint))
  const bodyRules = editableBodyRulesFromRules(endpoint.body_rules)

  return {
    url: endpoint.base_url,
    path: endpoint.custom_path || '',
    upstreamStreamPolicy: getEndpointUpstreamStreamPolicy(endpoint),
    streamFailover: initEndpointStreamFailoverState(endpoint),
    rules,
    responseRules,
    bodyRules,
  }
}

function isOpenAiResponsesEndpoint(endpoint: ProviderEndpoint): boolean {
  return isOpenAiResponsesStreamFailoverEndpoint(endpoint.api_format)
}

const OPENAI_IMAGE_TRANSPORT_MODE_CONFIG_KEY = 'openai_image_transport_mode'

function isOpenAiImageEndpoint(endpoint: ProviderEndpoint): boolean {
  return normalizeEndpointApiFormat(endpoint.api_format) === 'openai:image'
}

function getOpenAiImageTransportMode(endpoint: ProviderEndpoint): 'responses_bridge' | 'images_passthrough' {
  const value = endpoint.config?.[OPENAI_IMAGE_TRANSPORT_MODE_CONFIG_KEY]
  return typeof value === 'string' && value.trim().toLowerCase() === 'images_passthrough'
    ? 'images_passthrough'
    : 'responses_bridge'
}

async function handleOpenAiImageTransportModeChange(endpoint: ProviderEndpoint, value: string) {
  const nextMode = value === 'images_passthrough' ? 'images_passthrough' : 'responses_bridge'
  if (nextMode === getOpenAiImageTransportMode(endpoint)) return

  savingEndpointId.value = endpoint.id
  try {
    const config: Record<string, unknown> = { ...(endpoint.config || {}) }
    delete config[OPENAI_IMAGE_TRANSPORT_MODE_CONFIG_KEY]
    if (nextMode === 'images_passthrough') {
      config[OPENAI_IMAGE_TRANSPORT_MODE_CONFIG_KEY] = nextMode
    }
    const updated = await updateEndpoint(endpoint.id, {
      config: Object.keys(config).length > 0 ? config : null,
    })
    replaceLocalEndpoint(updated)
    success(nextMode === 'images_passthrough'
      ? t('endpointForm.imageTransportPassthroughEnabled', '已切换为 Images API 原样透传')
      : t('endpointForm.imageTransportBridgeEnabled', '已切换为 Responses 图片桥接'))
    emit('endpointUpdated')
  } catch (error: unknown) {
    showError(parseApiError(error, t('endpointForm.operationFailed')), t('endpointForm.error'))
  } finally {
    savingEndpointId.value = null
  }
}

function getStreamFailoverState(endpoint: ProviderEndpoint): EndpointStreamFailoverState {
  return endpointEditStates.value[endpoint.id]?.streamFailover
    ?? initEndpointStreamFailoverState(endpoint)
}

function updateStreamFailoverState(
  endpointId: string,
  value: EndpointStreamFailoverState,
) {
  const state = ensureEndpointEditState(endpointId)
  if (state) state.streamFailover = { ...value }
}

function hasStreamFailoverChanges(endpoint: ProviderEndpoint): boolean {
  const state = endpointEditStates.value[endpoint.id]
  return state
    ? endpointStreamFailoverStateChanged(endpoint, state.streamFailover)
    : false
}

function streamFailoverValidationMessage(
  error: EndpointStreamFailoverValidationError,
): string {
  return t(`streamFailoverUi.validation.${error}`)
}

function getStreamFailoverValidationMessage(endpoint: ProviderEndpoint): string | null {
  const state = endpointEditStates.value[endpoint.id]
  if (!state) return null
  const validationError = validateEndpointStreamFailoverState(state.streamFailover)
  return validationError ? streamFailoverValidationMessage(validationError) : null
}

function resetStreamFailover(endpoint: ProviderEndpoint) {
  const state = ensureEndpointEditState(endpoint.id)
  if (state) state.streamFailover = initEndpointStreamFailoverState(endpoint)
}

async function saveStreamFailover(endpoint: ProviderEndpoint) {
  const state = ensureEndpointEditState(endpoint.id)
  if (!state) return
  const validationError = validateEndpointStreamFailoverState(state.streamFailover)
  if (validationError) {
    showError(streamFailoverValidationMessage(validationError))
    return
  }

  savingEndpointId.value = endpoint.id
  try {
    const updated = await updateEndpoint(
      endpoint.id,
      buildEndpointStreamFailoverUpdate(endpoint, state.streamFailover),
    )
    Object.assign(endpoint, updated)
    state.streamFailover = initEndpointStreamFailoverState(updated)
    success(t('streamFailoverUi.saved'))
    emit('endpointUpdated')
  } catch (error: unknown) {
    showError(parseApiError(error, t('streamFailoverUi.saveFailed')), t('endpointForm.error'))
  } finally {
    savingEndpointId.value = null
  }
}

// 获取端点的编辑状态
function getEndpointEditState(endpointId: string): EndpointEditState | undefined {
  return endpointEditStates.value[endpointId]
}

function ensureEndpointEditState(endpointId: string): EndpointEditState | null {
  if (!endpointEditStates.value[endpointId]) {
    const endpoint = localEndpoints.value.find(e => e.id === endpointId)
    if (endpoint) {
      endpointEditStates.value[endpointId] = initEndpointEditState(endpoint)
    }
  }
  return endpointEditStates.value[endpointId] ?? null
}

function isEndpointRulesJsonMode(endpointId: string): boolean {
  return endpointRulesJsonMode.value[endpointId] === true
}

function buildEndpointRulesJsonPayload(endpointId: string): EndpointRulesJsonPayload {
  const state = ensureEndpointEditState(endpointId)
  return {
    header_rules: state ? (rulesToHeaderRules(state.rules) ?? []) : [],
    body_rules: state ? (rulesToBodyRules(state.bodyRules) ?? []) : [],
    response_header_rules: state ? (rulesToHeaderRules(state.responseRules) ?? []) : [],
  }
}

function stringifyEndpointRulesJsonPayload(payload: EndpointRulesJsonPayload): string {
  return JSON.stringify(payload, null, 2)
}

function refreshEndpointRulesJsonDraft(endpointId: string) {
  endpointRulesJsonDraft.value[endpointId] = stringifyEndpointRulesJsonPayload(
    buildEndpointRulesJsonPayload(endpointId),
  )
  endpointRulesJsonError.value[endpointId] = null
  endpointRulesJsonDirty.value[endpointId] = false
}

function enterEndpointRulesJsonMode(endpoint: ProviderEndpoint) {
  ensureEndpointEditState(endpoint.id)
  refreshEndpointRulesJsonDraft(endpoint.id)
  endpointRulesJsonMode.value[endpoint.id] = true
  endpointRulesExpanded.value[endpoint.id] = true
}

function toggleEndpointRulesJsonMode(endpoint: ProviderEndpoint) {
  if (isEndpointRulesJsonMode(endpoint.id)) {
    if (endpointRulesJsonDirty.value[endpoint.id] && !applyEndpointRulesJsonDraft(endpoint.id, { notify: false })) {
      return
    }
    endpointRulesJsonMode.value[endpoint.id] = false
    return
  }
  enterEndpointRulesJsonMode(endpoint)
}

function getEndpointRulesJsonDraft(endpoint: ProviderEndpoint): string {
  if (endpointRulesJsonDraft.value[endpoint.id] === undefined) {
    endpointRulesJsonDraft.value[endpoint.id] = stringifyEndpointRulesJsonPayload(
      buildEndpointRulesJsonPayload(endpoint.id),
    )
  }
  return endpointRulesJsonDraft.value[endpoint.id]
}

function updateEndpointRulesJsonDraft(endpointId: string, value: string) {
  endpointRulesJsonDraft.value[endpointId] = value
  endpointRulesJsonDirty.value[endpointId] = true
  endpointRulesJsonError.value[endpointId] = null
}

function formatEndpointRulesJson(endpointId: string) {
  const currentDraft = endpointRulesJsonDraft.value[endpointId] ?? ''
  const parsed = parseEndpointRulesJsonDraft(endpointRulesJsonDraft.value[endpointId] ?? '')
  if (!parsed.value) {
    endpointRulesJsonError.value[endpointId] = parsed.error
    return
  }
  const formattedDraft = stringifyEndpointRulesJsonPayload(parsed.value)
  endpointRulesJsonDraft.value[endpointId] = formattedDraft
  endpointRulesJsonError.value[endpointId] = null
  if (formattedDraft !== currentDraft) {
    endpointRulesJsonDirty.value[endpointId] = true
  }
}

// 更新端点字段
function updateEndpointField(endpointId: string, field: 'url' | 'path', value: string) {
  ensureEndpointEditState(endpointId)
  if (endpointEditStates.value[endpointId]) {
    endpointEditStates.value[endpointId][field] = value
  }
}

// 获取端点的编辑规则
function getEndpointEditRules(endpointId: string): EditableRule[] {
  const state = endpointEditStates.value[endpointId]
  if (state) {
    return state.rules
  }
  // 从原始端点加载
  const endpoint = localEndpoints.value.find(e => e.id === endpointId)
  if (endpoint) {
    const newState = initEndpointEditState(endpoint)
    endpointEditStates.value[endpointId] = newState
    return newState.rules
  }
  return []
}

function getEndpointResponseHeaderRules(endpoint: ProviderEndpoint): HeaderRule[] {
  const raw = (endpoint as ProviderEndpoint & { response_header_rules?: unknown }).response_header_rules
    ?? endpoint.config?.[RESPONSE_HEADER_RULES_CONFIG_KEY]
    ?? endpoint.config?.[RESPONSE_HEADER_RULES_CAMEL_CONFIG_KEY]
  return Array.isArray(raw) ? (raw as HeaderRule[]) : []
}

function getEndpointEditResponseRules(endpointId: string): EditableRule[] {
  const state = endpointEditStates.value[endpointId]
  if (state) {
    return state.responseRules
  }
  const endpoint = localEndpoints.value.find(e => e.id === endpointId)
  if (endpoint) {
    const newState = initEndpointEditState(endpoint)
    endpointEditStates.value[endpointId] = newState
    return newState.responseRules
  }
  return []
}

// 添加规则（同时自动展开折叠）
function handleAddEndpointRule(endpointId: string) {
  const rules = getEndpointEditRules(endpointId)
  rules.push(emptyHeaderRule())
  // 自动展开折叠
  endpointRulesExpanded.value[endpointId] = true
}

function handleAddEndpointResponseRule(endpointId: string) {
  const rules = getEndpointEditResponseRules(endpointId)
  rules.push(emptyHeaderRule())
  endpointRulesExpanded.value[endpointId] = true
}

// 删除规则
function removeEndpointRule(endpointId: string, index: number) {
  const rules = getEndpointEditRules(endpointId)
  rules.splice(index, 1)
  clearHeaderRuleDragState(endpointId)
  clearHeaderRuleSelectOpen(endpointId)
}

function removeEndpointResponseRule(endpointId: string, index: number) {
  const rules = getEndpointEditResponseRules(endpointId)
  rules.splice(index, 1)
  clearResponseRuleDragState(endpointId)
  clearResponseRuleSelectOpen(endpointId)
}

// 更新规则类型
function updateEndpointRuleAction(endpointId: string, index: number, action: 'set' | 'drop' | 'rename') {
  const rules = getEndpointEditRules(endpointId)
  if (rules[index]) {
    const currentCondition = rules[index].condition
    const currentEnabled = rules[index].enabled
    rules[index] = { ...emptyHeaderRule(), action, enabled: currentEnabled, condition: currentCondition }
  }
}

function updateEndpointResponseRuleAction(endpointId: string, index: number, action: 'set' | 'drop' | 'rename') {
  const rules = getEndpointEditResponseRules(endpointId)
  if (rules[index]) {
    const currentCondition = rules[index].condition
    const currentEnabled = rules[index].enabled
    rules[index] = { ...emptyHeaderRule(), action, enabled: currentEnabled, condition: currentCondition }
  }
}

function updateEndpointRuleEnabled(endpointId: string, index: number, enabled: boolean) {
  const rules = getEndpointEditRules(endpointId)
  if (rules[index]) {
    rules[index].enabled = enabled
  }
}

function updateEndpointResponseRuleEnabled(endpointId: string, index: number, enabled: boolean) {
  const rules = getEndpointEditResponseRules(endpointId)
  if (rules[index]) {
    rules[index].enabled = enabled
  }
}

// 更新规则字段
function updateEndpointRuleField(endpointId: string, index: number, field: 'key' | 'value' | 'from' | 'to', value: string) {
  const rules = getEndpointEditRules(endpointId)
  if (rules[index]) {
    rules[index][field] = value
  }
}

function updateEndpointResponseRuleField(endpointId: string, index: number, field: 'key' | 'value' | 'from' | 'to', value: string) {
  const rules = getEndpointEditResponseRules(endpointId)
  if (rules[index]) {
    rules[index][field] = value
  }
}

function toggleEndpointRuleCondition(endpointId: string, index: number) {
  const rules = getEndpointEditRules(endpointId)
  if (rules[index]) {
    rules[index].condition = rules[index].condition ? null : createEmptyConditionLeaf()
  }
}

function toggleEndpointResponseRuleCondition(endpointId: string, index: number) {
  const rules = getEndpointEditResponseRules(endpointId)
  if (rules[index]) {
    rules[index].condition = rules[index].condition ? null : createEmptyConditionLeaf()
  }
}

function updateEndpointRuleCondition(endpointId: string, index: number, condition: EditableConditionNode) {
  const rules = getEndpointEditRules(endpointId)
  if (rules[index]) {
    rules[index].condition = condition
  }
}

function updateEndpointResponseRuleCondition(endpointId: string, index: number, condition: EditableConditionNode) {
  const rules = getEndpointEditResponseRules(endpointId)
  if (rules[index]) {
    rules[index].condition = condition
  }
}

function clearEndpointRuleCondition(endpointId: string, index: number) {
  const rules = getEndpointEditRules(endpointId)
  if (rules[index]) {
    rules[index].condition = null
  }
}

function clearEndpointResponseRuleCondition(endpointId: string, index: number) {
  const rules = getEndpointEditResponseRules(endpointId)
  if (rules[index]) {
    rules[index].condition = null
  }
}

// 验证规则 key（针对特定端点）
function validateRuleKeyForEndpoint(endpointId: string, key: string, index: number): string | null {
  const trimmedKey = key.trim().toLowerCase()
  if (!trimmedKey) return null

  if (RESERVED_HEADERS.has(trimmedKey)) {
    return t('endpointFormExtra.reservedRequestHeader', { name: key })
  }

  const rules = getEndpointEditRules(endpointId)
  const currentRule = rules[index]
  if (currentRule && !currentRule.enabled) return null
  const duplicate = rules.findIndex(
    (r, i) => i !== index && r.enabled && (
      ((r.action === 'set' || r.action === 'drop') && r.key.trim().toLowerCase() === trimmedKey) ||
      (r.action === 'rename' && r.to.trim().toLowerCase() === trimmedKey)
    )
  )
  if (duplicate >= 0) {
    return t('endpointFormExtra.duplicateRequestHeader')
  }

  return null
}

// 验证 rename from
function validateRenameFromForEndpoint(endpointId: string, from: string, index: number): string | null {
  const trimmedFrom = from.trim().toLowerCase()
  if (!trimmedFrom) return null

  const rules = getEndpointEditRules(endpointId)
  const currentRule = rules[index]
  if (currentRule && !currentRule.enabled) return null
  const duplicate = rules.findIndex(
    (r, i) => i !== index && r.enabled &&
      ((r.action === 'set' && r.key.trim().toLowerCase() === trimmedFrom) ||
       (r.action === 'drop' && r.key.trim().toLowerCase() === trimmedFrom) ||
       (r.action === 'rename' && r.from.trim().toLowerCase() === trimmedFrom))
  )
  if (duplicate >= 0) {
    return t('endpointFormExtra.requestHeaderAlreadyHandled')
  }

  return null
}

// 验证 rename to
function validateRenameToForEndpoint(endpointId: string, to: string, index: number): string | null {
  const trimmedTo = to.trim().toLowerCase()
  if (!trimmedTo) return null

  if (RESERVED_HEADERS.has(trimmedTo)) {
    return t('endpointFormExtra.reservedRequestHeader', { name: to })
  }

  const rules = getEndpointEditRules(endpointId)
  const currentRule = rules[index]
  if (currentRule && !currentRule.enabled) return null
  const duplicate = rules.findIndex(
    (r, i) => i !== index && r.enabled &&
      ((r.action === 'set' && r.key.trim().toLowerCase() === trimmedTo) ||
       (r.action === 'rename' && r.to.trim().toLowerCase() === trimmedTo))
  )
  if (duplicate >= 0) {
    return t('endpointFormExtra.duplicateRequestHeader')
  }

  return null
}

// 获取端点的请求头规则数量（有效的规则）
function getEndpointRulesCount(endpoint: ProviderEndpoint): number {
  const state = endpointEditStates.value[endpoint.id]
  if (state) {
    return state.rules.filter(r => {
      if (r.action === 'set' || r.action === 'drop') return r.key.trim()
      if (r.action === 'rename') return r.from.trim() && r.to.trim()
      return false
    }).length
  }
  return endpoint.header_rules?.length || 0
}

function getEndpointResponseRulesCount(endpoint: ProviderEndpoint): number {
  const state = endpointEditStates.value[endpoint.id]
  if (state) {
    return state.responseRules.filter(r => {
      if (r.action === 'set' || r.action === 'drop') return r.key.trim()
      if (r.action === 'rename') return r.from.trim() && r.to.trim()
      return false
    }).length
  }
  return getEndpointResponseHeaderRules(endpoint).length
}

// 检查端点是否有任何规则（包括正在编辑的空规则）
function _hasAnyRules(endpoint: ProviderEndpoint): boolean {
  const state = endpointEditStates.value[endpoint.id]
  if (state) {
    return state.rules.length > 0
  }
  return (endpoint.header_rules?.length || 0) > 0
}

// ========== 请求体规则相关函数 ==========

// 获取端点的编辑请求体规则
function getEndpointEditBodyRules(endpointId: string): EditableBodyRule[] {
  const state = endpointEditStates.value[endpointId]
  if (state) {
    return state.bodyRules
  }
  // 从原始端点加载
  const endpoint = localEndpoints.value.find(e => e.id === endpointId)
  if (endpoint) {
    const newState = initEndpointEditState(endpoint)
    endpointEditStates.value[endpointId] = newState
    return newState.bodyRules
  }
  return []
}

// 添加请求体规则（同时自动展开折叠）
function handleAddEndpointBodyRule(endpointId: string) {
  const rules = getEndpointEditBodyRules(endpointId)
  rules.push(emptyBodyRule('set'))
  // 自动展开折叠
  endpointRulesExpanded.value[endpointId] = true
}

// 删除请求体规则
function removeEndpointBodyRule(endpointId: string, index: number) {
  const rules = getEndpointEditBodyRules(endpointId)
  rules.splice(index, 1)
  clearBodyRuleDragState(endpointId)
  clearBodyRuleSelectOpen(endpointId)
}

// 更新请求体规则类型
function updateEndpointBodyRuleAction(endpointId: string, index: number, action: BodyRuleAction) {
  const rules = getEndpointEditBodyRules(endpointId)
  if (rules[index]) {
    const currentCondition = rules[index].condition
    const currentEnabled = rules[index].enabled
    rules[index] = { ...emptyBodyRule(action), enabled: currentEnabled, condition: currentCondition }
  }
}

function updateEndpointBodyRuleEnabled(endpointId: string, index: number, enabled: boolean) {
  const rules = getEndpointEditBodyRules(endpointId)
  if (rules[index]) {
    rules[index].enabled = enabled
  }
}

// 更新请求体规则字段
function updateEndpointBodyRuleField(endpointId: string, index: number, field: 'path' | 'value' | 'from' | 'to' | 'index' | 'pattern' | 'replacement' | 'flags' | 'count', value: string) {
  const rules = getEndpointEditBodyRules(endpointId)
  if (rules[index]) {
    rules[index][field] = value
  }
}

function toggleBodyRuleCondition(endpointId: string, index: number) {
  const rules = getEndpointEditBodyRules(endpointId)
  if (rules[index]) {
    rules[index].condition = rules[index].condition ? null : createEmptyConditionLeaf()
  }
}

function updateEndpointBodyRuleCondition(endpointId: string, index: number, condition: EditableConditionNode) {
  const rules = getEndpointEditBodyRules(endpointId)
  if (rules[index]) {
    rules[index].condition = condition
  }
}

function clearEndpointBodyRuleCondition(endpointId: string, index: number) {
  const rules = getEndpointEditBodyRules(endpointId)
  if (rules[index]) {
    rules[index].condition = null
  }
}

// 验证请求体规则 path（针对特定端点）
function validateBodyRulePathForEndpoint(endpointId: string, path: string, index: number): string | null {
  const raw = path.trim()
  if (!raw) return null

  // 基础格式校验；对含 [N] 的路径，取方括号前的部分做 dot 校验
  const dotPart = raw.includes('[') ? raw.slice(0, raw.indexOf('[')) : raw
  const parts = dotPart ? parseBodyRulePathParts(dotPart) : [raw.split('[')[0] || raw]
  if (!parts) {
    return t('endpointFormExtra.invalidPath')
  }

  // 提取顶层 key（去除数组索引部分）
  const topKey = (parts[0] || '').trim().toLowerCase()
  if (RESERVED_BODY_FIELDS.has(topKey)) {
    return t('endpointFormExtra.reservedTopLevelField', { name: parts[0] })
  }

  const normalizedPath = raw.toLowerCase()

  const rules = getEndpointEditBodyRules(endpointId)
  const currentRule = rules[index]
  if (currentRule && !currentRule.enabled) return null
  // 任意一方启用了条件，则不视为冲突（条件可能互斥，真正冲突在运行时处理）
  const duplicate = rules.findIndex(
    (r, i) => i !== index && r.enabled && !currentRule.condition && !r.condition && (
      ((r.action === 'set' || r.action === 'drop') && r.path.trim().toLowerCase() === normalizedPath) ||
      (r.action === 'rename' && r.to.trim().toLowerCase() === normalizedPath)
    )
  )
  if (duplicate >= 0) {
    return t('endpointFormExtra.duplicateFieldPath')
  }

  return null
}

// 验证请求体 rename from
function validateBodyRenameFromForEndpoint(endpointId: string, from: string, index: number): string | null {
  const raw = from.trim()
  if (!raw) return null

  const parts = parseBodyRulePathParts(raw)
  if (!parts) {
    return t('endpointFormExtra.invalidPathDetailed')
  }

  const topKey = (parts[0] || '').trim().toLowerCase()
  if (RESERVED_BODY_FIELDS.has(topKey)) {
    return t('endpointFormExtra.reservedTopLevelField', { name: parts[0] })
  }

  const normalizedFrom = raw.toLowerCase()

  const rules = getEndpointEditBodyRules(endpointId)
  const currentRule = rules[index]
  if (currentRule && !currentRule.enabled) return null
  const duplicate = rules.findIndex(
    (r, i) => i !== index && r.enabled && !currentRule.condition && !r.condition &&
      ((r.action === 'set' && r.path.trim().toLowerCase() === normalizedFrom) ||
       (r.action === 'drop' && r.path.trim().toLowerCase() === normalizedFrom) ||
       (r.action === 'rename' && r.from.trim().toLowerCase() === normalizedFrom))
  )
  if (duplicate >= 0) {
    return t('endpointFormExtra.pathAlreadyHandled')
  }

  return null
}

// 验证请求体 rename to
function validateBodyRenameToForEndpoint(endpointId: string, to: string, index: number): string | null {
  const raw = to.trim()
  if (!raw) return null

  const parts = parseBodyRulePathParts(raw)
  if (!parts) {
    return t('endpointFormExtra.invalidPathDetailed')
  }

  const topKey = (parts[0] || '').trim().toLowerCase()
  if (RESERVED_BODY_FIELDS.has(topKey)) {
    return t('endpointFormExtra.reservedTopLevelField', { name: parts[0] })
  }

  const normalizedTo = raw.toLowerCase()

  const rules = getEndpointEditBodyRules(endpointId)
  const currentRule = rules[index]
  if (currentRule && !currentRule.enabled) return null
  const duplicate = rules.findIndex(
    (r, i) => i !== index && r.enabled && !currentRule.condition && !r.condition &&
      ((r.action === 'set' && r.path.trim().toLowerCase() === normalizedTo) ||
       (r.action === 'rename' && r.to.trim().toLowerCase() === normalizedTo))
  )
  if (duplicate >= 0) {
    return t('endpointFormExtra.duplicateFieldPath')
  }

  return null
}

function validateBodySetValue(rule: EditableBodyRule): string | null {
  if (rule.action !== 'set' && rule.action !== 'append' && rule.action !== 'insert') return null

  const raw = rule.value.trim()
  if (!raw) return t('endpointFormExtra.valueRequired')
  try {
    JSON.parse(prepareValueForJsonParse(raw))
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err)
    return t('endpointFormExtra.jsonFormatError', { message: msg })
  }
  return null
}

// 获取值验证状态：true=有效, false=无效, null=空
function getBodySetValueValidation(rule: EditableBodyRule): boolean | null {
  if (rule.action !== 'set' && rule.action !== 'append' && rule.action !== 'insert') return null
  const raw = rule.value.trim()
  if (!raw) return null
  try {
    JSON.parse(prepareValueForJsonParse(raw))
    return true
  } catch {
    return false
  }
}

// 正则表达式验证状态：true=有效, false=无效, null=空
function getRegexPatternValidation(rule: EditableBodyRule): boolean | null {
  if (rule.action !== 'regex_replace') return null
  const pattern = rule.pattern.trim()
  if (!pattern) return null
  try {
    new RegExp(pattern)
    // 校验 flags
    const flags = rule.flags.trim()
    if (flags) {
      const validFlags = new Set(['i', 'm', 's'])
      for (const f of flags) {
        if (!validFlags.has(f)) return false
      }
    }
    return true
  } catch {
    return false
  }
}

// 获取正则验证提示
function getRegexPatternValidationTip(rule: EditableBodyRule): string {
  const validation = getRegexPatternValidation(rule)
  if (validation === null) return t('endpointFormExtra.enterRegex')
  if (validation === true) return t('endpointFormExtra.validRegex')
  try {
    new RegExp(rule.pattern.trim())
    // 正则有效但 flags 无效
    return t('endpointFormExtra.invalidRegexFlags')
  } catch (err: unknown) {
    return err instanceof Error ? err.message : String(err)
  }
}

// 获取验证提示
function getBodySetValueValidationTip(rule: EditableBodyRule): string {
  const validation = getBodySetValueValidation(rule)
  if (validation === null) return t('endpointFormExtra.validateJson')
  if (validation === true) {
    const parsed = restoreOriginalPlaceholder(JSON.parse(prepareValueForJsonParse(rule.value.trim())))
    const type = Array.isArray(parsed) ? t('endpointForm.array') : typeof parsed === 'object' && parsed !== null ? t('endpointForm.object') : typeof parsed === 'string' ? t('endpointForm.string') : typeof parsed === 'number' ? t('endpointForm.number') : typeof parsed === 'boolean' ? t('endpointForm.boolean') : 'null'
    return t('endpointFormExtra.validJson', { type })
  }
  try {
    JSON.parse(prepareValueForJsonParse(rule.value.trim()))
    return ''
  } catch (err: unknown) {
    return err instanceof Error ? err.message : String(err)
  }
}

function isStrictIntegerString(value: string): boolean {
  return /^-?\d+$/.test(value.trim())
}

function isStrictNonNegativeIntegerString(value: string): boolean {
  return /^\d+$/.test(value.trim())
}

// 判断请求体规则是否有效（必填字段已填写）
function isBodyRuleEffective(r: EditableBodyRule): boolean {
  switch (r.action) {
    case 'set':
    case 'drop':
      return !!r.path.trim()
    case 'rename':
      return !!(r.from.trim() && r.to.trim())
    case 'append':
      return !!r.path.trim()
    case 'insert':
      return !!(r.path.trim() && r.index.trim())
    case 'regex_replace':
      return !!(r.path.trim() && r.pattern.trim())
    default:
      return false
  }
}

// 获取端点的请求体规则数量（有效的规则）
function getEndpointBodyRulesCount(endpoint: ProviderEndpoint): number {
  const state = endpointEditStates.value[endpoint.id]
  if (state) {
    return state.bodyRules.filter(isBodyRuleEffective).length
  }
  return endpoint.body_rules?.length || 0
}

// 检查端点是否有任何请求体规则（包括正在编辑的空规则）
function _hasAnyBodyRules(endpoint: ProviderEndpoint): boolean {
  const state = endpointEditStates.value[endpoint.id]
  if (state) {
    return state.bodyRules.length > 0
  }
  return (endpoint.body_rules?.length || 0) > 0
}

// 获取端点的总规则数量（请求头 + 请求体 + 响应头）
function getTotalRulesCount(endpoint: ProviderEndpoint): number {
  return getEndpointRulesCount(endpoint) + getEndpointBodyRulesCount(endpoint) + getEndpointResponseRulesCount(endpoint)
}

// 格式化请求头规则的显示标签
function _formatHeaderRuleLabel(rule: EditableRule): string {
  if (rule.action === 'set') {
    if (!rule.key) return t('endpointFormExtra.notSet')
    return `${rule.key}=${rule.value || '...'}`
  } else if (rule.action === 'drop') {
    if (!rule.key) return t('endpointFormExtra.notSet')
    return `-${rule.key}`
  } else if (rule.action === 'rename') {
    if (!rule.from || !rule.to) return t('endpointFormExtra.notSet')
    return `${rule.from}→${rule.to}`
  }
  return t('endpointFormExtra.unknown')
}

// 格式化请求体规则的显示标签
function _formatBodyRuleLabel(rule: EditableBodyRule): string {
  if (rule.action === 'set') {
    if (!rule.path) return t('endpointFormExtra.notSet')
    return `${rule.path}=${rule.value || '...'}`
  } else if (rule.action === 'drop') {
    if (!rule.path) return t('endpointFormExtra.notSet')
    return `-${rule.path}`
  } else if (rule.action === 'rename') {
    if (!rule.from || !rule.to) return t('endpointFormExtra.notSet')
    return `${rule.from}→${rule.to}`
  } else if (rule.action === 'append') {
    if (!rule.path) return t('endpointFormExtra.notSet')
    return `${rule.path}[]+=${rule.value || '...'}`
  } else if (rule.action === 'insert') {
    if (!rule.path) return t('endpointFormExtra.notSet')
    const idx = rule.index?.trim() || t('endpointFormExtra.endPosition')
    return `${rule.path}[${idx}]+=${rule.value || '...'}`
  } else if (rule.action === 'regex_replace') {
    if (!rule.path || !rule.pattern) return t('endpointFormExtra.notSet')
    const flags = rule.flags.trim()
    const count = rule.count.trim()
    return `${rule.path}: s/${rule.pattern}/${rule.replacement || ''}/${flags}${count ? ` ×${count}` : ''}`
  }
  return t('endpointFormExtra.unknown')
}

// 检查端点请求体规则是否有修改
function hasBodyRulesChanges(endpoint: ProviderEndpoint): boolean {
  const state = endpointEditStates.value[endpoint.id]
  if (!state) return false

  const originalRules = endpoint.body_rules || []
  const editedRules = state.bodyRules.filter(isBodyRuleEffective)
  if (editedRules.length !== originalRules.length) return true
  for (let i = 0; i < editedRules.length; i++) {
    const edited = editedRules[i]
    const original = originalRules[i]
    if (!original) return true
    if (edited.action !== original.action) return true
    if (edited.enabled !== (original.enabled !== false)) return true
    if (edited.action === 'set' && original.action === 'set') {
      const baseline = initBodyRuleSetValueForEditor(original.value)
      if (edited.path !== original.path) return true
      if (edited.value !== baseline.value) return true
    } else if (edited.action === 'drop' && original.action === 'drop') {
      if (edited.path !== original.path) return true
    } else if (edited.action === 'rename' && original.action === 'rename') {
      if (edited.from !== original.from || edited.to !== original.to) return true
    } else if (edited.action === 'append' && original.action === 'append') {
      const baseline = initBodyRuleSetValueForEditor(original.value)
      if (edited.path !== original.path) return true
      if (edited.value !== baseline.value) return true
    } else if (edited.action === 'insert' && original.action === 'insert') {
      const baseline = initBodyRuleSetValueForEditor(original.value)
      if (edited.path !== original.path) return true
      if (edited.index !== String(original.index ?? '')) return true
      if (edited.value !== baseline.value) return true
    } else if (edited.action === 'regex_replace' && original.action === 'regex_replace') {
      if (edited.path !== original.path) return true
      if (edited.pattern !== (original.pattern ?? '')) return true
      if (edited.replacement !== (original.replacement ?? '')) return true
      if (edited.flags !== (original.flags ?? '')) return true
      if (edited.count !== (original.count === undefined || original.count === null ? '' : String(original.count))) return true
    }
    if (!conditionEquals(edited.condition, conditionToEditable(original.condition))) return true
  }
  return false
}

// 将可编辑请求体规则数组转换为 API 需要的 BodyRule[]
function rulesToBodyRules(rules: EditableBodyRule[]): BodyRule[] | null {
  const result: BodyRule[] = []

  for (const rule of rules) {
    const condition = editableConditionToApi(rule.condition)
    const common = { ...(rule.enabled ? {} : { enabled: false }), ...(condition ? { condition } : {}) }
    if (rule.action === 'set' && rule.path.trim()) {
      let value: unknown = rule.value
      try { value = restoreOriginalPlaceholder(JSON.parse(prepareValueForJsonParse(rule.value.trim()))) } catch { value = rule.value }
      result.push({ action: 'set', path: rule.path.trim(), value, ...common })
    } else if (rule.action === 'drop' && rule.path.trim()) {
      result.push({ action: 'drop', path: rule.path.trim(), ...common })
    } else if (rule.action === 'rename' && rule.from.trim() && rule.to.trim()) {
      result.push({ action: 'rename', from: rule.from.trim(), to: rule.to.trim(), ...common })
    } else if (rule.action === 'append' && rule.path.trim()) {
      let value: unknown = rule.value
      try { value = restoreOriginalPlaceholder(JSON.parse(prepareValueForJsonParse(rule.value.trim()))) } catch { value = rule.value }
      result.push({ action: 'append', path: rule.path.trim(), value, ...common })
    } else if (rule.action === 'insert' && rule.path.trim()) {
      let value: unknown = rule.value
      try { value = restoreOriginalPlaceholder(JSON.parse(prepareValueForJsonParse(rule.value.trim()))) } catch { value = rule.value }
      if (!isStrictIntegerString(rule.index)) continue
      const idx = parseInt(rule.index.trim(), 10)
      result.push({ action: 'insert', path: rule.path.trim(), index: idx, value, ...common })
    } else if (rule.action === 'regex_replace' && rule.path.trim() && rule.pattern.trim()) {
      const entry: BodyRuleRegexReplace = {
        action: 'regex_replace',
        path: rule.path.trim(),
        pattern: rule.pattern,
        replacement: rule.replacement || '',
        ...(rule.flags.trim() ? { flags: rule.flags.trim() } : {}),
        ...(isStrictNonNegativeIntegerString(rule.count) ? { count: parseInt(rule.count.trim(), 10) } : {}),
      }
      result.push({ ...entry, ...common })
    }
  }

  return result.length > 0 ? result : null
}

function getBodyValidationErrorForEndpoint(endpointId: string): string | null {
  const rules = getEndpointEditBodyRules(endpointId)
  for (let i = 0; i < rules.length; i++) {
    const rule = rules[i]
    if (!rule.enabled) continue
    const prefix = t('endpointFormExtra.bodyRulePrefix', { index: i + 1 })

    if (rule.action === 'set' || rule.action === 'drop') {
      const pathErr = validateBodyRulePathForEndpoint(endpointId, rule.path, i)
      if (pathErr) return `${prefix}${pathErr}`
      if (rule.action === 'set') {
        const valueErr = validateBodySetValue(rule)
        if (valueErr) return `${prefix}${valueErr}`
      }
    } else if (rule.action === 'rename') {
      const fromErr = validateBodyRenameFromForEndpoint(endpointId, rule.from, i)
      if (fromErr) return `${prefix}${fromErr}`
      const toErr = validateBodyRenameToForEndpoint(endpointId, rule.to, i)
      if (toErr) return `${prefix}${toErr}`
    } else if (rule.action === 'append') {
      const pathErr = validateBodyRulePathForEndpoint(endpointId, rule.path, i)
      if (pathErr) return `${prefix}${pathErr}`
      const valueErr = validateBodySetValue(rule)
      if (valueErr) return `${prefix}${valueErr}`
    } else if (rule.action === 'insert') {
      const pathErr = validateBodyRulePathForEndpoint(endpointId, rule.path, i)
      if (pathErr) return `${prefix}${pathErr}`
      const indexStr = rule.index.trim()
      if (!indexStr) return `${prefix}${t('endpointFormExtra.insertPositionRequired')}`
      if (!isStrictIntegerString(indexStr)) return `${prefix}${t('endpointFormExtra.positionInteger')}`
      const valueErr = validateBodySetValue(rule)
      if (valueErr) return `${prefix}${valueErr}`
    } else if (rule.action === 'regex_replace') {
      const pathErr = validateBodyRulePathForEndpoint(endpointId, rule.path, i)
      if (pathErr) return `${prefix}${pathErr}`
      if (!rule.pattern.trim()) return `${prefix}${t('endpointFormExtra.regexRequired')}`
      try {
        new RegExp(rule.pattern.trim())
      } catch (err: unknown) {
        return `${prefix}${t('endpointFormExtra.regexInvalid', { message: err instanceof Error ? err.message : String(err) })}`
      }
      const flags = rule.flags.trim()
      if (flags) {
        const validFlags = new Set(['i', 'm', 's'])
        for (const f of flags) {
          if (!validFlags.has(f)) return `${prefix}${t('endpointFormExtra.flagsInvalidChar', { char: f })}`
        }
      }
      const count = rule.count.trim()
      if (count) {
        if (!isStrictNonNegativeIntegerString(count)) return `${prefix}${t('endpointFormExtra.replaceCountInteger')}`
      }
    }

    const conditionErr = validateEditableCondition(rule.condition)
    if (conditionErr) return `${prefix}${conditionErr}`
  }
  return null
}

// 检查请求体规则是否有验证错误
function _hasBodyValidationErrorsForEndpoint(endpointId: string): boolean {
  return !!getBodyValidationErrorForEndpoint(endpointId)
}

// 检查端点 URL/路径是否有修改
function hasUrlChanges(endpoint: ProviderEndpoint): boolean {
  const state = endpointEditStates.value[endpoint.id]
  if (!state) return false
  if (state.url !== endpoint.base_url) return true
  if (state.path !== (endpoint.custom_path || '')) return true
  // 注：upstreamStreamPolicy 现在由头部按钮直接保存，无需在此检查
  return false
}

// 检查端点规则是否有修改
function editableHeaderRulesChanged(edited: EditableRule[], originalRules: HeaderRule[]): boolean {
  const editedRules = edited.filter(r => {
    if (r.action === 'set' || r.action === 'drop') return r.key.trim()
    if (r.action === 'rename') return r.from.trim() && r.to.trim()
    return false
  })
  if (editedRules.length !== originalRules.length) return true
  for (let i = 0; i < editedRules.length; i++) {
    const edited = editedRules[i]
    const original = originalRules[i]
    if (!original) return true
    if (edited.action !== original.action) return true
    if (edited.enabled !== (original.enabled !== false)) return true
    if (edited.action === 'set' && original.action === 'set') {
      if (edited.key !== original.key || edited.value !== (original.value || '')) return true
    } else if (edited.action === 'drop' && original.action === 'drop') {
      if (edited.key !== original.key) return true
    } else if (edited.action === 'rename' && original.action === 'rename') {
      if (edited.from !== original.from || edited.to !== original.to) return true
    }
    if (!conditionEquals(edited.condition, conditionToEditable(original.condition))) return true
  }
  return false
}

function hasRulesChanges(endpoint: ProviderEndpoint): boolean {
  const state = endpointEditStates.value[endpoint.id]
  if (!state) return false
  return editableHeaderRulesChanged(state.rules, endpoint.header_rules || [])
}

function hasResponseHeaderRulesChanges(endpoint: ProviderEndpoint): boolean {
  const state = endpointEditStates.value[endpoint.id]
  if (!state) return false
  return editableHeaderRulesChanged(state.responseRules, getEndpointResponseHeaderRules(endpoint))
}

function hasRulePanelChanges(endpoint: ProviderEndpoint): boolean {
  return hasRulesChanges(endpoint)
    || hasBodyRulesChanges(endpoint)
    || hasResponseHeaderRulesChanges(endpoint)
    || (isEndpointRulesJsonMode(endpoint.id) && endpointRulesJsonDirty.value[endpoint.id] === true)
}

// 检查端点是否有修改（URL、路径或规则）
// 注：当前模板直接使用各子函数，此聚合函数保留供未来使用
function _hasEndpointChanges(endpoint: ProviderEndpoint): boolean {
  return hasUrlChanges(endpoint) || hasRulePanelChanges(endpoint)
}

// 重置端点修改
function resetEndpointChanges(endpoint: ProviderEndpoint) {
  endpointEditStates.value[endpoint.id] = initEndpointEditState(endpoint)
  if (isEndpointRulesJsonMode(endpoint.id)) {
    refreshEndpointRulesJsonDraft(endpoint.id)
  } else {
    endpointRulesJsonError.value[endpoint.id] = null
    endpointRulesJsonDirty.value[endpoint.id] = false
  }
}

async function handleResetBodyRulesToDefault(endpoint: ProviderEndpoint) {
  resettingDefaultRulesEndpointId.value = endpoint.id
  try {
    const defaultRules = await loadDefaultBodyRulesForFormat(endpoint.api_format, true)
    if (!defaultRules.length) {
      showError(t('endpointForm.noDefaultBodyRules'))
      return
    }

    if (!endpointEditStates.value[endpoint.id]) {
      endpointEditStates.value[endpoint.id] = initEndpointEditState(endpoint)
    }
    const state = endpointEditStates.value[endpoint.id]
    if (!state) return

    const resetState = initEndpointEditState({
      ...endpoint,
      body_rules: defaultRules,
    })
    state.bodyRules = resetState.bodyRules
    endpointRulesExpanded.value[endpoint.id] = (state.rules.length + state.responseRules.length + state.bodyRules.length) > 0
    clearBodyRuleDragState(endpoint.id)
    clearBodyRuleSelectOpen(endpoint.id)
    if (isEndpointRulesJsonMode(endpoint.id)) {
      refreshEndpointRulesJsonDraft(endpoint.id)
    }
    success(t('endpointForm.bodyRulesReset'))
  } catch (error: unknown) {
    showError(parseApiError(error, t('endpointForm.resetFailed')), t('endpointForm.error'))
  } finally {
    resettingDefaultRulesEndpointId.value = null
  }
}

// 将可编辑规则数组转换为 API 需要的 HeaderRule[]
function rulesToHeaderRules(rules: EditableRule[]): HeaderRule[] | null {
  const result: HeaderRule[] = []

  for (const rule of rules) {
    const condition = editableConditionToApi(rule.condition)
    const common = { ...(rule.enabled ? {} : { enabled: false }), ...(condition ? { condition } : {}) }
    if (rule.action === 'set' && rule.key.trim()) {
      result.push({ action: 'set', key: rule.key.trim(), value: rule.value, ...common })
    } else if (rule.action === 'drop' && rule.key.trim()) {
      result.push({ action: 'drop', key: rule.key.trim(), ...common })
    } else if (rule.action === 'rename' && rule.from.trim() && rule.to.trim()) {
      result.push({ action: 'rename', from: rule.from.trim(), to: rule.to.trim(), ...common })
    }
  }

  return result.length > 0 ? result : null
}

function endpointConfigWithResponseHeaderRules(endpoint: ProviderEndpoint, rules: HeaderRule[] | null): Record<string, unknown> | null {
  const merged: Record<string, unknown> = { ...(endpoint.config || {}) }
  delete merged[RESPONSE_HEADER_RULES_CONFIG_KEY]
  delete merged[RESPONSE_HEADER_RULES_CAMEL_CONFIG_KEY]
  if (rules && rules.length > 0) {
    merged[RESPONSE_HEADER_RULES_CONFIG_KEY] = rules
  }
  return Object.keys(merged).length > 0 ? merged : null
}

function getHeaderValidationErrorForEndpoint(endpointId: string): string | null {
  const rules = getEndpointEditRules(endpointId)
  for (let i = 0; i < rules.length; i++) {
    const rule = rules[i]
    if (!rule.enabled) continue
    const prefix = t('endpointFormExtra.headerRulePrefix', { index: i + 1 })
    if (rule.action === 'set' || rule.action === 'drop') {
      const err = validateRuleKeyForEndpoint(endpointId, rule.key, i)
      if (err) return `${prefix}${err}`
    } else if (rule.action === 'rename') {
      const fromErr = validateRenameFromForEndpoint(endpointId, rule.from, i)
      if (fromErr) return `${prefix}${fromErr}`
      const toErr = validateRenameToForEndpoint(endpointId, rule.to, i)
      if (toErr) return `${prefix}${toErr}`
    }
    const conditionErr = validateEditableCondition(rule.condition)
    if (conditionErr) return `${prefix}${conditionErr}`
  }
  return null
}

function validateResponseHeaderNameForEndpoint(endpointId: string, name: string, index: number, field: 'key' | 'from' | 'to'): string | null {
  const trimmedName = name.trim().toLowerCase()
  if (!trimmedName) return null

  if ((field === 'key' || field === 'to') && RESERVED_RESPONSE_HEADERS.has(trimmedName)) {
    return t('endpointFormExtra.reservedResponseHeader', { name })
  }

  const rules = getEndpointEditResponseRules(endpointId)
  const currentRule = rules[index]
  if (currentRule && !currentRule.enabled) return null
  const duplicate = rules.findIndex(
    (r, i) => i !== index && r.enabled && (
      ((r.action === 'set' || r.action === 'drop') && r.key.trim().toLowerCase() === trimmedName) ||
      (r.action === 'rename' && (field === 'from'
        ? r.from.trim().toLowerCase() === trimmedName
        : r.to.trim().toLowerCase() === trimmedName))
    )
  )
  if (duplicate >= 0) {
    return field === 'from' ? t('endpointFormExtra.responseHeaderAlreadyHandled') : t('endpointFormExtra.duplicateResponseHeader')
  }

  return null
}

function getResponseHeaderValidationErrorForEndpoint(endpointId: string): string | null {
  const rules = getEndpointEditResponseRules(endpointId)
  for (let i = 0; i < rules.length; i++) {
    const rule = rules[i]
    if (!rule.enabled) continue
    const prefix = t('endpointFormExtra.responseHeaderRulePrefix', { index: i + 1 })
    if (rule.action === 'set' || rule.action === 'drop') {
      const err = validateResponseHeaderNameForEndpoint(endpointId, rule.key, i, 'key')
      if (err) return `${prefix}${err}`
    } else if (rule.action === 'rename') {
      const fromErr = validateResponseHeaderNameForEndpoint(endpointId, rule.from, i, 'from')
      if (fromErr) return `${prefix}${fromErr}`
      const toErr = validateResponseHeaderNameForEndpoint(endpointId, rule.to, i, 'to')
      if (toErr) return `${prefix}${toErr}`
    }
    const conditionErr = validateEditableCondition(rule.condition)
    if (conditionErr) return `${prefix}${conditionErr}`
  }
  return null
}

// 新端点选择的格式的默认路径
const newEndpointDefaultPath = computed(() => {
  // 使用填写的 base_url 或 provider 的 website 来判断是否是 Codex 端点
  const baseUrl = newEndpoint.value.base_url || props.provider?.website || ''
  return getDefaultPath(newEndpoint.value.api_format, baseUrl)
})

// 加载 API 格式列表
const loadApiFormats = async () => {
  try {
    const response = await adminApi.getApiFormats()
    apiFormats.value = response.formats
  } catch (error) {
    log.error('加载API格式失败:', error)
  }
}

onMounted(() => {
  loadApiFormats()
})

// 监听 props 变化
watch(() => props.modelValue, (open) => {
  bodyRuleHelpOpenEndpointId.value = null
  ruleSelectOpen.value = {}
  responseRuleSelectOpen.value = {}
  bodyRuleSelectOpen.value = {}
  headerRuleDraggedIndex.value = {}
  headerRuleDragOverIndex.value = {}
  responseRuleDraggedIndex.value = {}
  responseRuleDragOverIndex.value = {}
  bodyRuleDraggedIndex.value = {}
  bodyRuleDragOverIndex.value = {}
  endpointRulesJsonMode.value = {}
  endpointRulesJsonDraft.value = {}
  endpointRulesJsonError.value = {}
  endpointRulesJsonDirty.value = {}
  if (open) {
    localEndpoints.value = [...(props.endpoints || [])]
    // 清空编辑状态，重新从端点加载
    endpointEditStates.value = {}
    endpointRulesExpanded.value = {}
    // 初始化每个端点的编辑状态，有规则时默认展开
    for (const endpoint of localEndpoints.value) {
      endpointEditStates.value[endpoint.id] = initEndpointEditState(endpoint)
      // 有规则时默认展开
      const hasRules = (endpoint.header_rules?.length || 0) + getEndpointResponseHeaderRules(endpoint).length + (endpoint.body_rules?.length || 0) > 0
      endpointRulesExpanded.value[endpoint.id] = hasRules
    }
    void preloadDefaultBodyRules(localEndpoints.value)
  } else {
    // 关闭对话框时完全清空新端点表单
    newEndpoint.value = { api_format: '', base_url: '', custom_path: '' }
  }
}, { immediate: true })

watch(() => props.endpoints, (endpoints) => {
  if (props.modelValue) {
    localEndpoints.value = [...(endpoints || [])]
    // 初始化新添加端点的编辑状态
    for (const endpoint of localEndpoints.value) {
      if (!endpointEditStates.value[endpoint.id]) {
        endpointEditStates.value[endpoint.id] = initEndpointEditState(endpoint)
      }
      if (isEndpointRulesJsonMode(endpoint.id) && !endpointRulesJsonDirty.value[endpoint.id]) {
        refreshEndpointRulesJsonDraft(endpoint.id)
      }
    }
    const newFormats = localEndpoints.value
      .filter(e => e.api_format && !defaultBodyRulesLoaded.value[defaultBodyRulesCacheKey(e.api_format)])
      .map(e => ({ api_format: e.api_format }) as ProviderEndpoint)
    if (newFormats.length) {
      void preloadDefaultBodyRules(newFormats)
    }
  }
}, { deep: true })

// 保存端点
async function saveEndpoint(endpoint: ProviderEndpoint) {
  if (isEndpointRulesJsonMode(endpoint.id) && endpointRulesJsonDirty.value[endpoint.id]) {
    if (!applyEndpointRulesJsonDraft(endpoint.id, { notify: false })) return
  }

  const state = endpointEditStates.value[endpoint.id]
  if (!state || !state.url) return

  // 检查规则是否有验证错误
  const headerErr = getHeaderValidationErrorForEndpoint(endpoint.id)
  if (headerErr) {
    showError(headerErr)
    return
  }

  const responseHeaderErr = getResponseHeaderValidationErrorForEndpoint(endpoint.id)
  if (responseHeaderErr) {
    showError(responseHeaderErr)
    return
  }

  // 检查请求体规则是否有验证错误
  const bodyErr = getBodyValidationErrorForEndpoint(endpoint.id)
  if (bodyErr) {
    showError(bodyErr)
    return
  }

  savingEndpointId.value = endpoint.id
  try {
    // 仅提交变更字段，避免 fixed provider 因 base_url/custom_path 被锁定而更新失败
    const payload: Record<string, unknown> = {}

    if (!isFixedProvider.value) {
      if (state.url !== endpoint.base_url) payload.base_url = state.url
      if (state.path !== (endpoint.custom_path || '')) payload.custom_path = state.path || null
    }

    if (hasRulesChanges(endpoint)) payload.header_rules = rulesToHeaderRules(state.rules)
    if (hasResponseHeaderRulesChanges(endpoint)) {
      payload.config = endpointConfigWithResponseHeaderRules(
        endpoint,
        rulesToHeaderRules(state.responseRules),
      )
    }
    if (hasBodyRulesChanges(endpoint)) payload.body_rules = rulesToBodyRules(state.bodyRules)

    // 注：upstreamStreamPolicy 现在由头部按钮直接保存，不在此处处理

    if (Object.keys(payload).length === 0) return

    await updateEndpoint(endpoint.id, payload)
    success(t('endpointForm.updated'))
    emit('endpointUpdated')
  } catch (error: unknown) {
    showError(parseApiError(error, t('endpointForm.updateFailed')), t('endpointForm.error'))
  } finally {
    savingEndpointId.value = null
  }
}

// 切换格式转换（直接保存）
async function handleToggleFormatConversion(endpoint: ProviderEndpoint) {
  const currentEnabled = endpoint.format_acceptance_config?.enabled || false
  const newEnabled = !currentEnabled

  togglingFormatEndpointId.value = endpoint.id
  try {
    await updateEndpoint(endpoint.id, {
      format_acceptance_config: newEnabled ? { enabled: true } : null,
    })
    success(newEnabled ? t('endpointForm.conversionEnabled') : t('endpointForm.conversionDisabled'))
    emit('endpointUpdated')
  } catch (error: unknown) {
    showError(parseApiError(error, t('endpointForm.operationFailed')), t('endpointForm.error'))
  } finally {
    togglingFormatEndpointId.value = null
  }
}

// 获取上游流式按钮的当前状态（优先使用编辑状态）
function getCurrentUpstreamStreamPolicy(endpoint: ProviderEndpoint): string {
  if (isUpstreamStreamPolicyLocked(endpoint)) return 'force_stream'
  const state = endpointEditStates.value[endpoint.id]
  return state?.upstreamStreamPolicy ?? getEndpointUpstreamStreamPolicy(endpoint)
}

function isUpstreamStreamPolicyLocked(endpoint: ProviderEndpoint): boolean {
  return (props.provider?.provider_type || '').toLowerCase() === 'codex'
    && normalizeEndpointApiFormat(endpoint.api_format) === 'openai:responses'
}

// 获取上游流式按钮的样式类
function getUpstreamStreamButtonClass(endpoint: ProviderEndpoint): string {
  if (isUpstreamStreamPolicyLocked(endpoint)) {
    return 'h-7 w-7 text-primary/70 cursor-not-allowed'
  }
  const policy = getCurrentUpstreamStreamPolicy(endpoint)
  const base = 'h-7 w-7'
  if (policy === 'force_stream') return `${base} text-primary`
  if (policy === 'force_non_stream') return `${base} text-destructive`
  return `${base} text-muted-foreground` // auto - 跟随请求，淡色显示
}

// 获取上游流式按钮的提示文字
function getUpstreamStreamTooltip(endpoint: ProviderEndpoint): string {
  if (isUpstreamStreamPolicyLocked(endpoint)) return t('endpointFormExtra.streamLocked')
  const policy = getCurrentUpstreamStreamPolicy(endpoint)
  if (policy === 'force_stream') return t('endpointFormExtra.forceStreamHint')
  if (policy === 'force_non_stream') return t('endpointFormExtra.forceNonStreamHint')
  return t('endpointFormExtra.followRequestHint')
}

// 循环切换上游流式策略并直接保存
async function handleCycleUpstreamStream(endpoint: ProviderEndpoint) {
  if (isUpstreamStreamPolicyLocked(endpoint)) return

  const currentPolicy = getCurrentUpstreamStreamPolicy(endpoint)
  let nextPolicy: string
  let nextLabel: string

  // 循环：auto -> force_stream -> force_non_stream -> auto
  if (currentPolicy === 'auto') {
    nextPolicy = 'force_stream'
    nextLabel = t('endpointFormExtra.forceStream')
  } else if (currentPolicy === 'force_stream') {
    nextPolicy = 'force_non_stream'
    nextLabel = t('endpointFormExtra.forceNonStream')
  } else {
    nextPolicy = 'auto'
    nextLabel = t('endpointFormExtra.followRequest')
  }

  savingEndpointId.value = endpoint.id
  try {
    const merged: Record<string, unknown> = { ...(endpoint.config || {}) }
    // 清理旧的 key
    delete merged.upstream_stream_policy
    delete merged.upstreamStreamPolicy
    delete merged.upstream_stream

    if (nextPolicy !== 'auto') {
      merged.upstream_stream_policy = nextPolicy
    }

    await updateEndpoint(endpoint.id, {
      config: Object.keys(merged).length > 0 ? merged : null,
    })

    // 更新本地编辑状态
    if (endpointEditStates.value[endpoint.id]) {
      endpointEditStates.value[endpoint.id].upstreamStreamPolicy = nextPolicy
    }

    success(t('endpointForm.switchedTo', { label: nextLabel }))
    emit('endpointUpdated')
  } catch (error: unknown) {
    showError(parseApiError(error, t('endpointForm.operationFailed')), t('endpointForm.error'))
  } finally {
    savingEndpointId.value = null
  }
}

// 添加端点
async function handleAddEndpoint() {
  if (!props.provider || !newEndpoint.value.api_format) return

  // 如果没有输入 base_url，使用提供商的 website 作为默认值
  const baseUrl = newEndpoint.value.base_url || props.provider.website
  if (!baseUrl) {
    showError(t('endpointForm.baseUrlRequired'))
    return
  }

  addingEndpoint.value = true
  try {
    await createEndpoint(props.provider.id, {
      provider_id: props.provider.id,
      api_format: newEndpoint.value.api_format,
      base_url: baseUrl,
      custom_path: newEndpoint.value.custom_path || undefined,
      is_active: true,
    })
    success(t('endpointForm.added', { format: formatApiFormat(newEndpoint.value.api_format) }))
    // 重置表单，保留 URL
    newEndpoint.value = { api_format: '', base_url: baseUrl, custom_path: '' }
    emit('endpointCreated')
  } catch (error: unknown) {
    showError(parseApiError(error, t('endpointForm.addFailed')), t('endpointForm.error'))
  } finally {
    addingEndpoint.value = false
  }
}

// 切换端点启用状态
async function handleToggleEndpoint(endpoint: ProviderEndpoint) {
  togglingEndpointId.value = endpoint.id
  try {
    const newStatus = !endpoint.is_active
    await updateEndpoint(endpoint.id, { is_active: newStatus })
    success(newStatus ? t('endpointForm.enabledSuccess') : t('endpointForm.disabledSuccess'))
    emit('endpointUpdated')
  } catch (error: unknown) {
    showError(parseApiError(error, t('endpointForm.operationFailed')), t('endpointForm.error'))
  } finally {
    togglingEndpointId.value = null
  }
}

// 删除端点 - 打开确认弹窗
function handleDeleteEndpoint(endpoint: ProviderEndpoint) {
  endpointToDelete.value = endpoint
  deleteConfirmOpen.value = true
}

// 确认删除端点
async function confirmDeleteEndpoint() {
  if (!endpointToDelete.value) return

  const endpoint = endpointToDelete.value
  deleteConfirmOpen.value = false
  deletingEndpointId.value = endpoint.id

  try {
    await deleteEndpoint(endpoint.id)
    success(t('endpointForm.deleted', { format: formatApiFormat(endpoint.api_format) }))
    emit('endpointUpdated')
  } catch (error: unknown) {
    showError(parseApiError(error, t('endpointForm.deleteFailed')), t('endpointForm.error'))
  } finally {
    deletingEndpointId.value = null
    endpointToDelete.value = null
  }
}

// 关闭对话框
function handleDialogUpdate(value: boolean) {
  emit('update:modelValue', value)
}

function handleClose() {
  emit('update:modelValue', false)
}
</script>
