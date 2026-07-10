<template>
  <div class="space-y-6 pb-8">
    <!-- API Keys 表格 -->
    <Card
      variant="default"
      class="overflow-hidden"
    >
      <!-- 标题和操作栏 -->
      <div class="px-4 sm:px-6 py-3 sm:py-3.5 border-b border-border/60">
        <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3 sm:gap-4">
          <h3 class="text-sm sm:text-base font-semibold shrink-0">
            我的 API Keys
          </h3>

          <!-- 操作按钮 -->
          <div class="flex items-center gap-2">
            <!-- 新增 API Key 按钮 -->
            <Button
              variant="ghost"
              size="icon"
              class="h-8 w-8"
              title="创建新 API Key"
              @click="openCreateApiKeyDialog"
            >
              <Plus class="w-3.5 h-3.5" />
            </Button>

            <!-- 刷新按钮 -->
            <RefreshButton
              :loading="loading"
              @click="loadApiKeys"
            />
          </div>
        </div>
      </div>

      <!-- 加载状态 -->
      <div
        v-if="loading"
        class="flex items-center justify-center py-12"
      >
        <LoadingState message="加载中..." />
      </div>

      <!-- 空状态 -->
      <div
        v-else-if="apiKeys.length === 0"
        class="flex items-center justify-center py-12"
      >
        <EmptyState
          title="暂无 API 密钥"
          description="创建你的第一个 API 密钥开始使用"
          :icon="Key"
        >
          <template #actions>
            <Button
              size="lg"
              class="shadow-lg shadow-primary/20"
              @click="openCreateApiKeyDialog"
            >
              <Plus class="mr-2 h-4 w-4" />
              创建新 API Key
            </Button>
          </template>
        </EmptyState>
      </div>

      <!-- 桌面端表格 -->
      <div
        v-else
        class="hidden md:block overflow-x-auto"
      >
        <Table>
          <TableHeader>
            <TableRow class="border-b border-border/60 hover:bg-transparent">
              <TableHead class="min-w-[200px] h-12 font-semibold">
                密钥名称
              </TableHead>
              <TableHead class="min-w-[160px] h-12 font-semibold">
                密钥
              </TableHead>
              <TableHead class="min-w-[100px] h-12 font-semibold">
                费用(USD)
              </TableHead>
              <TableHead class="min-w-[100px] h-12 font-semibold">
                请求次数
              </TableHead>
              <TableHead class="min-w-[70px] h-12 font-semibold text-center">
                状态
              </TableHead>
              <TableHead class="min-w-[100px] h-12 font-semibold">
                最后使用
              </TableHead>
              <TableHead class="min-w-[80px] h-12 font-semibold text-center">
                操作
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow
              v-for="apiKey in paginatedApiKeys"
              :key="apiKey.id"
              class="border-b border-border/40 hover:bg-muted/30 transition-colors"
            >
              <!-- 密钥名称 -->
              <TableCell class="py-4">
                <div class="flex-1 min-w-0">
                  <div
                    class="text-sm font-semibold truncate"
                    :title="apiKey.name"
                  >
                    {{ apiKey.name }}
                  </div>
                  <div class="text-xs text-muted-foreground mt-0.5">
                    创建于 {{ formatDate(apiKey.created_at) }}
                  </div>
                  <div class="text-xs text-muted-foreground mt-0.5">
                    分组：{{ apiKey.group_name || '默认分组' }}
                  </div>
                </div>
              </TableCell>

              <!-- 密钥显示 -->
              <TableCell class="py-4">
                <div class="flex items-center gap-1.5">
                  <code class="text-xs font-mono text-muted-foreground bg-muted/30 px-2 py-1 rounded">
                    {{ apiKey.key_display || 'sk-••••••••' }}
                  </code>
                  <Button
                    variant="ghost"
                    size="icon"
                    class="h-6 w-6"
                    title="复制完整密钥"
                    @click="copyApiKey(apiKey)"
                  >
                    <Copy class="h-3.5 w-3.5" />
                  </Button>
                </div>
              </TableCell>

              <!-- 费用 -->
              <TableCell class="py-4">
                <span class="text-sm font-semibold text-amber-600 dark:text-amber-500">
                  ${{ (apiKey.total_cost_usd || 0).toFixed(4) }}
                </span>
              </TableCell>

              <!-- 请求次数 -->
              <TableCell class="py-4">
                <div class="flex items-center gap-1.5">
                  <Activity class="h-3.5 w-3.5 text-muted-foreground" />
                  <span class="text-sm font-medium text-foreground">
                    {{ formatNumber(apiKey.total_requests || 0) }}
                  </span>
                </div>
              </TableCell>

              <!-- 状态 -->
              <TableCell class="py-4 text-center">
                <div class="flex flex-col items-center gap-1">
                  <Badge
                    :variant="apiKey.is_active ? 'success' : 'secondary'"
                    class="h-5 px-2 py-0 text-[10px] font-medium"
                  >
                    {{ apiKey.is_active ? '活跃' : '禁用' }}
                  </Badge>
                  <Badge
                    v-if="apiKey.is_locked"
                    variant="warning"
                    class="h-5 px-2 py-0 text-[10px] font-medium"
                  >
                    已锁定
                  </Badge>
                  <Badge
                    variant="secondary"
                    class="h-5 px-2 py-0 text-[10px] font-medium"
                  >
                    {{ formatRateLimitSimple(apiKey.rate_limit) }}
                  </Badge>
                  <Badge
                    variant="secondary"
                    class="h-5 px-2 py-0 text-[10px] font-medium"
                  >
                    {{ formatConcurrentLimitSimple(apiKey.concurrent_limit) }}
                  </Badge>
                </div>
              </TableCell>

              <!-- 最后使用时间 -->
              <TableCell class="py-4 text-sm text-muted-foreground">
                {{ apiKey.last_used_at ? formatRelativeTime(apiKey.last_used_at) : '从未使用' }}
              </TableCell>

              <!-- 操作按钮 -->
              <TableCell class="py-4">
                <div class="flex justify-center gap-1">
                  <Button
                    variant="ghost"
                    size="icon"
                    class="h-8 w-8"
                    title="一键配置"
                    @click="openInstallDialog(apiKey)"
                  >
                    <Terminal class="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    class="h-8 w-8"
                    :title="apiKey.is_locked ? '已锁定' : '导入 CC Switch'"
                    :disabled="apiKey.is_locked"
                    @click="openCcSwitchDialog(apiKey)"
                  >
                    <ExternalLink class="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    class="h-8 w-8"
                    :title="apiKey.is_locked ? '已锁定' : '编辑'"
                    :disabled="apiKey.is_locked"
                    @click="openEditApiKeyDialog(apiKey)"
                  >
                    <SquarePen class="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    class="h-8 w-8"
                    :title="apiKey.is_locked ? '已锁定' : (apiKey.is_active ? '禁用' : '启用')"
                    :disabled="apiKey.is_locked"
                    @click="toggleApiKey(apiKey)"
                  >
                    <Power class="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    class="h-8 w-8"
                    :title="apiKey.is_locked ? '已锁定' : '删除'"
                    :disabled="apiKey.is_locked"
                    @click="confirmDelete(apiKey)"
                  >
                    <Trash2 class="h-4 w-4" />
                  </Button>
                </div>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>

      <!-- 移动端卡片列表 -->
      <div
        v-if="!loading && apiKeys.length > 0"
        class="md:hidden space-y-3 p-4"
      >
        <Card
          v-for="apiKey in paginatedApiKeys"
          :key="apiKey.id"
          variant="default"
          class="group hover:shadow-md hover:border-primary/30 transition-all duration-200"
        >
          <div class="p-4">
            <!-- 第一行：名称、状态、操作 -->
            <div class="flex items-center justify-between mb-2">
              <div class="flex items-center gap-2 min-w-0 flex-1">
                <h3 class="text-sm font-semibold text-foreground truncate">
                  {{ apiKey.name }}
                </h3>
                <Badge
                  :variant="apiKey.is_active ? 'success' : 'secondary'"
                  class="text-xs px-1.5 py-0"
                >
                  {{ apiKey.is_active ? '活跃' : '禁用' }}
                </Badge>
                <Badge
                  v-if="apiKey.is_locked"
                  variant="warning"
                  class="text-[10px] px-1.5 py-0"
                >
                  已锁定
                </Badge>
                <Badge
                  variant="secondary"
                  class="text-[10px] px-1.5 py-0"
                >
                  {{ formatRateLimitSimple(apiKey.rate_limit) }}
                </Badge>
                <Badge
                  variant="secondary"
                  class="text-[10px] px-1.5 py-0"
                >
                  {{ formatConcurrentLimitSimple(apiKey.concurrent_limit) }}
                </Badge>
              </div>
              <div class="flex items-center gap-0.5 flex-shrink-0">
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7"
                  title="一键配置"
                  @click="openInstallDialog(apiKey)"
                >
                  <Terminal class="h-3.5 w-3.5" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7"
                  :title="apiKey.is_locked ? '已锁定' : '导入 CC Switch'"
                  :disabled="apiKey.is_locked"
                  @click="openCcSwitchDialog(apiKey)"
                >
                  <ExternalLink class="h-3.5 w-3.5" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7"
                  :title="apiKey.is_locked ? '已锁定' : '编辑'"
                  :disabled="apiKey.is_locked"
                  @click="openEditApiKeyDialog(apiKey)"
                >
                  <SquarePen class="h-3.5 w-3.5" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7"
                  title="复制"
                  @click="copyApiKey(apiKey)"
                >
                  <Copy class="h-3.5 w-3.5" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7"
                  :title="apiKey.is_locked ? '已锁定' : (apiKey.is_active ? '禁用' : '启用')"
                  :disabled="apiKey.is_locked"
                  @click="toggleApiKey(apiKey)"
                >
                  <Power class="h-3.5 w-3.5" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7"
                  :title="apiKey.is_locked ? '已锁定' : '删除'"
                  :disabled="apiKey.is_locked"
                  @click="confirmDelete(apiKey)"
                >
                  <Trash2 class="h-3.5 w-3.5" />
                </Button>
              </div>
            </div>

            <!-- 第二行：密钥、时间、统计 -->
            <div class="space-y-1.5">
              <div class="flex items-center gap-2 text-xs">
                <code class="font-mono text-muted-foreground">{{ apiKey.key_display || 'sk-••••••••' }}</code>
                <span class="text-muted-foreground">•</span>
                <span class="text-muted-foreground">
                  {{ apiKey.last_used_at ? formatRelativeTime(apiKey.last_used_at) : '从未使用' }}
                </span>
              </div>
              <div class="text-xs text-muted-foreground">
                分组：{{ apiKey.group_name || '默认分组' }}
              </div>
              <div class="flex items-center gap-3 text-xs">
                <span class="text-amber-600 dark:text-amber-500 font-semibold">
                  ${{ (apiKey.total_cost_usd || 0).toFixed(4) }}
                </span>
                <span class="text-muted-foreground">•</span>
                <span class="text-foreground font-medium">
                  {{ formatNumber(apiKey.total_requests || 0) }} 次
                </span>
                <span class="text-muted-foreground">•</span>
                <span class="text-muted-foreground">
                  {{ formatRateLimitSimple(apiKey.rate_limit) }}
                </span>
              </div>
            </div>
          </div>
        </Card>
      </div>

      <!-- 分页 -->
      <Pagination
        v-if="apiKeys.length > 0"
        :current="currentPage"
        :total="apiKeys.length"
        :page-size="pageSize"
        cache-key="my-api-keys-page-size"
        @update:current="currentPage = $event"
        @update:page-size="pageSize = $event"
      />
    </Card>

    <!-- 创建 API 密钥对话框 -->
    <Dialog v-model="showCreateDialog">
      <template #header>
        <div class="border-b border-border px-6 py-4">
          <div class="flex items-center gap-3">
            <div class="flex h-9 w-9 items-center justify-center rounded-lg bg-primary/10 flex-shrink-0">
              <Key class="h-5 w-5 text-primary" />
            </div>
            <div class="flex-1 min-w-0">
              <h3 class="text-lg font-semibold text-foreground leading-tight">
                {{ editingApiKey ? '编辑 API 密钥' : '创建 API 密钥' }}
              </h3>
            </div>
          </div>
        </div>
      </template>

      <div class="space-y-4">
        <div class="space-y-2">
          <Label
            for="key-name"
            class="text-sm font-semibold"
          >密钥名称</Label>
          <Input
            id="key-name"
            v-model="newKeyName"
            placeholder="例如：生产环境密钥"
            class="h-11 border-border/60"
            autocomplete="off"
            required
          />
          <p class="text-xs text-muted-foreground">
            给密钥起一个有意义的名称方便识别
          </p>
        </div>

        <div class="space-y-2">
          <Label
            for="key-rate-limit"
            class="text-sm font-semibold"
          >速率限制 (请求/分钟)</Label>
          <Input
            id="key-rate-limit"
            :model-value="newKeyRateLimit ?? ''"
            type="number"
            min="0"
            max="10000"
            placeholder="留空不限"
            class="h-11 border-border/60"
            @update:model-value="(v) => newKeyRateLimit = parseNumberInput(v, { min: 0, max: 10000 })"
          />
          <p class="text-xs text-muted-foreground">
            留空不限
          </p>
        </div>

        <div class="space-y-2">
          <Label
            for="key-group"
            class="text-sm font-semibold"
          >使用分组</Label>
          <select
            id="key-group"
            v-model="selectedGroupId"
            class="h-11 w-full rounded-md border border-border/60 bg-background px-3 text-sm"
            :disabled="apiKeyGroups.length === 0"
          >
            <option
              v-if="apiKeyGroups.length === 0"
              value=""
            >
              暂无可用分组
            </option>
            <option
              v-for="group in apiKeyGroups"
              :key="group.id"
              :value="group.id"
            >
              {{ group.name }}{{ group.visibility === 'internal' ? '（内部分组）' : '' }}
            </option>
          </select>
          <p class="text-xs text-muted-foreground">
            分组决定这个 API Key 的按量可用范围、并发上限和钱包扣费倍率；已购买套餐按套餐自己的模型范围使用。
          </p>
        </div>

        <div class="space-y-2">
          <Label
            for="key-concurrent-limit"
            class="text-sm font-semibold"
          >并发限制</Label>
          <Input
            id="key-concurrent-limit"
            :model-value="newKeyConcurrentLimit ?? ''"
            type="number"
            min="0"
            max="10000"
            placeholder="0 = 不限并发"
            class="h-11 border-border/60"
            @update:model-value="(v) => newKeyConcurrentLimit = parseNumberInput(v, { min: 0, max: 10000 })"
          />
          <p class="text-xs text-muted-foreground">
            {{ editingApiKey ? '留空表示保持当前值，填 0 表示不限并发' : '留空表示不限并发，填 0 也表示不限并发' }}
          </p>
        </div>

        <div class="rounded-lg border border-border/60 bg-muted/30 p-4">
          <div class="flex items-center justify-between gap-4">
            <div>
              <Label class="text-sm font-semibold">敏感信息保护</Label>
              <p class="mt-1 text-xs text-muted-foreground">
                {{ keyRedactionMode === 'inherit' ? '默认跟随账户设置' : '管理员开启功能后生效' }}
              </p>
            </div>
            <div class="flex items-center gap-2">
              <Button
                size="sm"
                :variant="keyRedactionMode === 'inherit' ? 'default' : 'outline'"
                @click="keyRedactionMode = 'inherit'"
              >
                跟随账户
              </Button>
              <Button
                size="sm"
                :variant="keyRedactionMode === 'custom' ? 'default' : 'outline'"
                @click="keyRedactionMode = 'custom'"
              >
                单独配置
              </Button>
            </div>
          </div>
          <div
            v-if="keyRedactionMode === 'custom'"
            class="mt-4 flex items-center justify-between gap-4 border-t border-border/50 pt-4"
          >
            <div>
              <Label class="text-sm font-medium">启用保护</Label>
              <p class="mt-1 text-xs text-muted-foreground">
                只影响此 API Key
              </p>
            </div>
            <Switch v-model="newKeyRedactionEnabled" />
          </div>
          <div
            v-if="keyRedactionMode === 'custom' && newKeyRedactionEnabled"
            class="mt-4 flex items-center justify-between gap-4 border-t border-border/50 pt-4"
          >
            <div>
              <Label class="text-sm font-medium">占位符说明</Label>
              <p class="mt-1 text-xs text-muted-foreground">
                向模型说明占位符含义
              </p>
            </div>
            <Switch v-model="newKeyRedactionInjectNotice" />
          </div>
        </div>
      </div>

      <template #footer>
        <Button
          variant="outline"
          class="h-11 px-6"
          @click="closeApiKeyDialog"
        >
          取消
        </Button>
        <Button
          class="h-11 px-6 shadow-lg shadow-primary/20"
          :disabled="creating || apiKeyGroups.length === 0"
          @click="saveApiKey"
        >
          <Loader2
            v-if="creating"
            class="animate-spin h-4 w-4 mr-2"
          />
          {{ creating ? (editingApiKey ? '保存中...' : '创建中...') : (editingApiKey ? '保存' : '创建') }}
        </Button>
      </template>
    </Dialog>

    <!-- 新密钥创建成功对话框 -->
    <Dialog
      v-model="showKeyDialog"
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
                创建成功
              </h3>
              <p class="text-xs text-muted-foreground">
                请妥善保管, 切勿泄露给他人
              </p>
            </div>
          </div>
        </div>
      </template>

      <div class="space-y-4">
        <div class="space-y-2">
          <Label class="text-sm font-medium">API 密钥</Label>
          <div class="flex items-center gap-2">
            <Input
              type="text"
              :value="newKeyValue"
              readonly
              class="flex-1 font-mono text-sm bg-muted/50 h-11"
              @click="($event.target as HTMLInputElement)?.select()"
            />
            <Button
              class="h-11"
              @click="copyTextToClipboard(newKeyValue)"
            >
              复制
            </Button>
          </div>
        </div>
      </div>

      <template #footer>
        <Button
          class="h-10 px-5"
          @click="closeCreatedKeyDialog"
        >
          确定
        </Button>
      </template>
    </Dialog>

    <!-- 接入方式选择对话框 -->
    <Dialog
      v-model="showSetupChoiceDialog"
      size="lg"
    >
      <template #header>
        <div class="border-b border-border px-6 py-4">
          <div class="flex items-center gap-3">
            <div class="flex h-9 w-9 items-center justify-center rounded-lg bg-primary/10 flex-shrink-0">
              <Key class="h-5 w-5 text-primary" />
            </div>
            <div class="flex-1 min-w-0">
              <h3 class="text-lg font-semibold text-foreground leading-tight">
                选择接入方式
              </h3>
              <p class="text-xs text-muted-foreground truncate">
                当前密钥：{{ selectedSetupApiKey?.name || '未选择' }}
              </p>
            </div>
          </div>
        </div>
      </template>

      <div class="space-y-4">
        <div class="rounded-lg border border-border/60 bg-muted/30 p-3 text-xs text-muted-foreground">
          推荐优先导入 CC Switch；如果只想在一台机器上快速配置，也可以生成一次性命令。
        </div>

        <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <button
            type="button"
            class="group rounded-xl border border-primary/45 bg-primary/10 p-4 text-left transition hover:border-primary hover:bg-primary/15 focus:outline-none focus:ring-2 focus:ring-primary/35"
            @click="chooseSetupCcSwitch"
          >
            <span class="mb-3 inline-flex items-center rounded-full bg-primary px-2.5 py-1 text-[11px] font-semibold text-primary-foreground">
              推荐
            </span>
            <span class="flex items-center gap-2 text-base font-semibold text-foreground">
              <ExternalLink class="h-4 w-4 text-primary" />
              导入 CC Switch
            </span>
            <span class="mt-2 block text-sm leading-6 text-muted-foreground">
              适合已经用 CC Switch 管理服务的用户，导入后可以继续在 CC Switch 里切换和管理。
            </span>
          </button>

          <button
            type="button"
            class="group rounded-xl border border-border/70 bg-background p-4 text-left transition hover:border-primary/70 hover:bg-muted/30 focus:outline-none focus:ring-2 focus:ring-primary/25"
            @click="chooseSetupInstall"
          >
            <span class="mb-3 inline-flex items-center rounded-full border border-border px-2.5 py-1 text-[11px] font-semibold text-muted-foreground">
              命令配置
            </span>
            <span class="flex items-center gap-2 text-base font-semibold text-foreground">
              <Terminal class="h-4 w-4 text-primary" />
              一键配置
            </span>
            <span class="mt-2 block text-sm leading-6 text-muted-foreground">
              生成 15 分钟内有效的一次性命令，复制到目标机器执行，不会在命令里暴露原始密钥。
            </span>
          </button>
        </div>
      </div>

      <template #footer>
        <Button
          variant="outline"
          class="h-10 px-5"
          @click="showSetupChoiceDialog = false"
        >
          稍后再说
        </Button>
      </template>
    </Dialog>

    <!-- 一键配置对话框 -->
    <Dialog
      v-model="showInstallDialog"
      size="lg"
    >
      <template #header>
        <div class="border-b border-border px-6 py-4">
          <div class="flex items-center gap-3">
            <div class="flex h-9 w-9 items-center justify-center rounded-lg bg-primary/10 flex-shrink-0">
              <Terminal class="h-5 w-5 text-primary" />
            </div>
            <div class="flex-1 min-w-0">
              <h3 class="text-lg font-semibold text-foreground leading-tight">
                一键配置
              </h3>
              <p class="text-xs text-muted-foreground truncate">
                当前密钥：{{ selectedInstallApiKey?.name || '未选择' }}
              </p>
            </div>
          </div>
        </div>
      </template>

      <div class="space-y-5">
        <div class="rounded-lg border border-border/60 bg-muted/30 p-3 text-xs text-muted-foreground">
          选择要配置的工具和目标系统，Niffler 会生成 15 分钟内有效的一次性 install code。页面命令不会包含原始 API Key。
        </div>

        <div class="space-y-2">
          <Label class="text-sm font-semibold">目标工具</Label>
          <div class="grid grid-cols-1 sm:grid-cols-3 gap-2">
            <Button
              v-for="option in installCliOptions"
              :key="option.value"
              :variant="installCli === option.value ? 'default' : 'outline'"
              class="justify-start h-auto py-3"
              @click="selectInstallCli(option.value)"
            >
              {{ option.label }}
            </Button>
          </div>
        </div>

        <div class="space-y-2">
          <Label class="text-sm font-semibold">目标系统</Label>
          <div class="grid grid-cols-1 sm:grid-cols-3 gap-2">
            <Button
              v-for="option in installSystemOptions"
              :key="option.value"
              :variant="installSystem === option.value ? 'default' : 'outline'"
              class="justify-start h-auto py-3"
              @click="selectInstallSystem(option.value)"
            >
              {{ option.label }}
            </Button>
          </div>
        </div>

        <div class="space-y-2">
          <div class="flex items-center justify-between gap-2">
            <Label class="text-sm font-semibold">复制到目标机器执行</Label>
            <div class="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                class="gap-1.5"
                :disabled="installLoading || !installCommand"
                :title="installCopied ? '已复制' : '一键复制安装命令'"
                @click="copyInstallCommand"
              >
                <CheckCircle
                  v-if="installCopied"
                  class="h-3.5 w-3.5 text-emerald-600 dark:text-emerald-400"
                />
                <Copy
                  v-else
                  class="h-3.5 w-3.5"
                />
                {{ installCopied ? '已复制' : '一键复制' }}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                :disabled="installLoading || !selectedInstallApiKey"
                @click="refreshInstallCommand"
              >
                {{ installLoading ? '生成中...' : '重新生成' }}
              </Button>
            </div>
          </div>
          <div class="rounded-lg border border-border/60 bg-background overflow-hidden">
            <pre class="max-h-32 overflow-x-auto whitespace-pre-wrap break-all p-3 text-xs font-mono">{{ installCommand || '正在生成短命令...' }}</pre>
          </div>
          <p class="text-xs text-muted-foreground">
            {{ installCommandHint }}
          </p>
        </div>
      </div>

      <template #footer>
        <Button
          variant="outline"
          class="h-10 px-5"
          @click="showInstallDialog = false"
        >
          关闭
        </Button>
        <Button
          class="h-10 px-5 shadow-lg shadow-primary/20"
          :disabled="!installCommand || installLoading"
          @click="copyInstallCommand"
        >
          {{ installCopied ? '已复制' : '复制命令' }}
        </Button>
      </template>
    </Dialog>

    <!-- 导入 CC Switch 对话框 -->
    <Dialog
      v-model="showCcSwitchDialog"
      size="lg"
    >
      <template #header>
        <div class="border-b border-border px-6 py-4">
          <div class="flex items-center gap-3">
            <div class="flex h-9 w-9 items-center justify-center rounded-lg bg-primary/10 flex-shrink-0">
              <ExternalLink class="h-5 w-5 text-primary" />
            </div>
            <div class="flex-1 min-w-0">
              <h3 class="text-lg font-semibold text-foreground leading-tight">
                导入 CC Switch
              </h3>
              <p class="text-xs text-muted-foreground truncate">
                当前密钥：{{ selectedCcSwitchApiKey?.name || '未选择' }}
              </p>
            </div>
          </div>
        </div>
      </template>

      <div class="space-y-5">
        <div class="rounded-lg border border-border/60 bg-muted/30 p-3 text-xs text-muted-foreground">
          会使用当前页面地址生成导入链接。域名或 IP 以后变了，重新导入一次即可。
        </div>

        <div class="space-y-2">
          <Label class="text-sm font-semibold">导入到</Label>
          <div class="grid grid-cols-1 sm:grid-cols-3 gap-2">
            <Button
              v-for="option in ccSwitchAppOptions"
              :key="option.value"
              :variant="ccSwitchApp === option.value ? 'default' : 'outline'"
              class="justify-start h-auto py-3"
              @click="selectCcSwitchApp(option.value)"
            >
              <span class="flex flex-col items-start gap-0.5 text-left">
                <span>{{ option.label }}</span>
                <span class="text-xs opacity-70 font-normal">{{ option.description }}</span>
              </span>
            </Button>
          </div>
        </div>

        <div class="space-y-2">
          <Label
            for="ccswitch-provider-name"
            class="text-sm font-semibold"
          >名称</Label>
          <Input
            id="ccswitch-provider-name"
            v-model="ccSwitchProviderName"
            placeholder="Niffler"
            class="h-11 border-border/60"
            autocomplete="off"
          />
          <p class="text-xs text-muted-foreground">
            这个名称会显示在 CC Switch 里，方便区分不同密钥。
          </p>
        </div>

        <div class="space-y-2">
          <Label
            for="ccswitch-model"
            class="text-sm font-semibold"
          >主模型（可选）</Label>
          <Input
            id="ccswitch-model"
            v-model="ccSwitchModel"
            placeholder="例如：gpt-5.5"
            class="h-11 border-border/60"
            autocomplete="off"
          />
          <p
            v-if="ccSwitchApp === 'codex'"
            class="text-xs text-muted-foreground"
          >
            Codex 默认使用 {{ DEFAULT_CCSWITCH_CODEX_MODEL }}，思考等级 {{ DEFAULT_CCSWITCH_CODEX_REASONING_EFFORT }}。
          </p>
          <p
            v-else
            class="text-xs text-muted-foreground"
          >
            填写后，CC Switch 余额检查会按这个模型计算套餐额度；不填则显示账户总可用额度。
          </p>
        </div>

        <div class="rounded-lg border border-border/60 bg-background overflow-hidden">
          <div class="border-b border-border/60 px-3 py-2 text-xs font-semibold text-muted-foreground">
            将导入的服务地址
          </div>
          <pre class="max-h-24 overflow-x-auto whitespace-pre-wrap break-all p-3 text-xs font-mono">{{ ccSwitchEndpointPreview }}</pre>
        </div>

        <p class="text-xs text-muted-foreground">
          导入时会读取完整 API Key，并通过本机协议交给 CC Switch；余额检查会访问 /user/balance。
        </p>
      </div>

      <template #footer>
        <Button
          variant="outline"
          class="h-10 px-5"
          @click="showCcSwitchDialog = false"
        >
          取消
        </Button>
        <Button
          class="h-10 px-5 shadow-lg shadow-primary/20"
          :disabled="ccSwitchImportLoading || !selectedCcSwitchApiKey"
          @click="importToCcSwitch"
        >
          <Loader2
            v-if="ccSwitchImportLoading"
            class="animate-spin h-4 w-4 mr-2"
          />
          {{ ccSwitchImportLoading ? '准备中...' : '导入' }}
        </Button>
      </template>
    </Dialog>

    <!-- 删除确认对话框 -->
    <AlertDialog
      v-model="showDeleteDialog"
      type="danger"
      title="确认删除"
      :description="`确定要删除密钥 &quot;${keyToDelete?.name}&quot; 吗？此操作不可恢复。`"
      confirm-text="删除"
      :loading="deleting"
      @confirm="deleteApiKey"
      @cancel="showDeleteDialog = false"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, computed, watch } from 'vue'
import { meApi, type ApiKey, type ApiKeyGroupOption, type InstallSessionTargetSystem, type InstallTargetCli, type ApiKeyInstallSession } from '@/api/me'
import Card from '@/components/ui/card.vue'
import Button from '@/components/ui/button.vue'
import Input from '@/components/ui/input.vue'
import Label from '@/components/ui/label.vue'
import Badge from '@/components/ui/badge.vue'
import Switch from '@/components/ui/switch.vue'
import { Dialog, Pagination } from '@/components/ui'
import { LoadingState, AlertDialog, EmptyState } from '@/components/common'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow
} from '@/components/ui'
import RefreshButton from '@/components/ui/refresh-button.vue'
import { Plus, Key, Copy, Trash2, Loader2, Activity, CheckCircle, Power, SquarePen, Terminal, ExternalLink } from 'lucide-vue-next'
import { useToast } from '@/composables/useToast'
import { log } from '@/utils/logger'
import { parseApiError } from '@/utils/errorParser'
import { formatRateLimitSimple } from '@/utils/format'
import { parseNumberInput } from '@/utils/form'
import { getApiBaseOrigin } from '@/utils/url'
import { getErrorStatus } from '@/types/api-error'
import {
  buildCcSwitchImportUrl,
  ccSwitchEndpoint,
  DEFAULT_CCSWITCH_CODEX_MODEL,
  DEFAULT_CCSWITCH_CODEX_REASONING_EFFORT,
  type CcSwitchApp,
} from '@/features/api-keys/utils/ccswitchImport'
import {
  hasChatPiiRedactionFeatureSettings,
  mergeChatPiiRedactionFeatureSettings,
  readChatPiiRedactionFeatureSettings,
} from '@/utils/featureSettings'

const { success, error: showError } = useToast()

const installCliOptions: Array<{ value: InstallTargetCli; label: string }> = [
  { value: 'claude_code', label: 'Claude Code' },
  { value: 'codex_cli', label: 'Codex' },
  { value: 'gemini_cli', label: 'Gemini CLI' }
]

const installSystemOptions: Array<{ value: InstallSessionTargetSystem; label: string }> = [
  { value: 'macos', label: 'macOS' },
  { value: 'linux', label: 'Linux' },
  { value: 'windows', label: 'Windows' }
]

const ccSwitchAppOptions: Array<{ value: CcSwitchApp; label: string; description: string }> = [
  { value: 'claude', label: 'Claude Code', description: '根地址' },
  { value: 'codex', label: 'Codex', description: '自动加 /v1' },
  { value: 'gemini', label: 'Gemini CLI', description: '根地址' },
]

const apiKeys = ref<ApiKey[]>([])
const apiKeyGroups = ref<ApiKeyGroupOption[]>([])
const loading = ref(false)
const creating = ref(false)
const deleting = ref(false)

// 分页相关
const currentPage = ref(1)
const pageSize = ref(10)

const paginatedApiKeys = computed(() => {
  const start = (currentPage.value - 1) * pageSize.value
  return apiKeys.value.slice(start, start + pageSize.value)
})

const showCreateDialog = ref(false)
const showKeyDialog = ref(false)
const showDeleteDialog = ref(false)
const showSetupChoiceDialog = ref(false)
const showInstallDialog = ref(false)
const showCcSwitchDialog = ref(false)

const newKeyName = ref('')
const selectedGroupId = ref('')
const newKeyRateLimit = ref<number | undefined>(undefined)
const newKeyConcurrentLimit = ref<number | undefined>(undefined)
const keyRedactionMode = ref<'inherit' | 'custom'>('inherit')
const newKeyRedactionEnabled = ref(false)
const newKeyRedactionInjectNotice = ref(true)
const newKeyValue = ref('')
const keyToDelete = ref<ApiKey | null>(null)
const editingApiKey = ref<ApiKey | null>(null)
const selectedSetupApiKey = ref<ApiKey | null>(null)
const selectedInstallApiKey = ref<ApiKey | null>(null)
const pendingSetupApiKey = ref<ApiKey | null>(null)
const installCli = ref<InstallTargetCli>('claude_code')
const installSystem = ref<InstallSessionTargetSystem>('linux')
const installSession = ref<ApiKeyInstallSession | null>(null)
const installLoading = ref(false)
const installCopied = ref(false)
const selectedCcSwitchApiKey = ref<ApiKey | null>(null)
const ccSwitchApp = ref<CcSwitchApp>('claude')
const ccSwitchProviderName = ref('Niffler')
const ccSwitchModel = ref('')
const ccSwitchImportLoading = ref(false)
let installCopiedResetTimer: ReturnType<typeof setTimeout> | null = null

const installCommand = computed(() => {
  if (!installSession.value) return ''
  return installSystem.value === 'windows'
    ? installSession.value.powershell_command
    : installSession.value.unix_command
})

const installCommandHint = computed(() => {
  if (installSystem.value === 'windows') {
    return 'Windows 请在 PowerShell 中执行。install code 使用后立即失效，如需再次执行请重新生成。'
  }
  return 'macOS / Linux 请在 sh 兼容终端中执行。install code 使用后立即失效，如需再次执行请重新生成。'
})

const ccSwitchBaseUrl = ref(getApiBaseOrigin())

const ccSwitchEndpointPreview = computed(() => {
  return ccSwitchEndpoint(ccSwitchApp.value, ccSwitchBaseUrl.value)
})

onMounted(() => {
  installSystem.value = detectCurrentSystem()
  loadApiKeyGroups()
  loadApiKeys()
})

onBeforeUnmount(() => {
  resetInstallCopiedState()
})

watch(showInstallDialog, (isOpen) => {
  if (!isOpen) {
    resetInstallCopiedState()
  }
})

watch(showKeyDialog, (isOpen) => {
  if (!isOpen && pendingSetupApiKey.value) {
    closeCreatedKeyDialog()
  }
})

async function loadApiKeys() {
  loading.value = true
  try {
    apiKeys.value = await meApi.getApiKeys()
  } catch (error: unknown) {
    log.error('加载 API 密钥失败:', error)
    const status = getErrorStatus(error)
    if (status === undefined) {
      showError('无法连接到服务器，请检查后端服务是否运行')
    } else if (status === 401) {
      showError('认证失败，请重新登录')
    } else {
      showError(parseApiError(error, '加载 API 密钥失败'))
    }
  } finally {
    loading.value = false
  }
}

async function loadApiKeyGroups() {
  try {
    apiKeyGroups.value = await meApi.getApiKeyGroups()
    if (!selectedGroupId.value && apiKeyGroups.value.length > 0) {
      selectedGroupId.value = apiKeyGroups.value[0].id
    }
  } catch (error: unknown) {
    log.error('加载 API Key 分组失败:', error)
    showError(parseApiError(error, '加载 API Key 分组失败'))
  }
}

function clearInstallCopiedResetTimer() {
  if (installCopiedResetTimer) {
    clearTimeout(installCopiedResetTimer)
    installCopiedResetTimer = null
  }
}

function resetInstallCopiedState() {
  clearInstallCopiedResetTimer()
  installCopied.value = false
}

function openEditApiKeyDialog(apiKey: ApiKey) {
  const hasRedactionFeature = hasChatPiiRedactionFeatureSettings(apiKey.feature_settings)
  const redactionFeature = readChatPiiRedactionFeatureSettings(apiKey.feature_settings)
  editingApiKey.value = apiKey
  newKeyName.value = apiKey.name || ''
  selectedGroupId.value = apiKey.group_id || apiKeyGroups.value[0]?.id || ''
  newKeyRateLimit.value = apiKey.rate_limit ?? undefined
  newKeyConcurrentLimit.value = apiKey.concurrent_limit ?? undefined
  keyRedactionMode.value = hasRedactionFeature ? 'custom' : 'inherit'
  newKeyRedactionEnabled.value = redactionFeature.enabled
  newKeyRedactionInjectNotice.value = redactionFeature.inject_model_instruction
  showCreateDialog.value = true
}

function openCreateApiKeyDialog() {
  editingApiKey.value = null
  newKeyName.value = ''
  selectedGroupId.value = apiKeyGroups.value[0]?.id || ''
  newKeyRateLimit.value = undefined
  newKeyConcurrentLimit.value = undefined
  keyRedactionMode.value = 'inherit'
  newKeyRedactionEnabled.value = false
  newKeyRedactionInjectNotice.value = true
  showCreateDialog.value = true
}

function detectCurrentSystem(): InstallSessionTargetSystem {
  const platform = window.navigator.platform.toLowerCase()
  const userAgent = window.navigator.userAgent.toLowerCase()
  if (platform.includes('mac')) return 'macos'
  if (platform.includes('win') || userAgent.includes('windows')) return 'windows'
  return 'linux'
}

async function openInstallDialog(apiKey: ApiKey) {
  selectedInstallApiKey.value = apiKey
  installSession.value = null
  resetInstallCopiedState()
  showInstallDialog.value = true
  await refreshInstallCommand()
}

function openSetupChoiceDialog(apiKey: ApiKey) {
  selectedSetupApiKey.value = apiKey
  showSetupChoiceDialog.value = true
}

function chooseSetupCcSwitch() {
  if (!selectedSetupApiKey.value) return
  const apiKey = selectedSetupApiKey.value
  showSetupChoiceDialog.value = false
  openCcSwitchDialog(apiKey)
}

function chooseSetupInstall() {
  if (!selectedSetupApiKey.value) return
  const apiKey = selectedSetupApiKey.value
  showSetupChoiceDialog.value = false
  void openInstallDialog(apiKey)
}

async function selectInstallCli(value: InstallTargetCli) {
  installCli.value = value
  await refreshInstallCommand()
}

async function selectInstallSystem(value: InstallSessionTargetSystem) {
  installSystem.value = value
  await refreshInstallCommand()
}

async function refreshInstallCommand() {
  if (!selectedInstallApiKey.value) return
  installLoading.value = true
  installSession.value = null
  resetInstallCopiedState()
  try {
    installSession.value = await meApi.createApiKeyInstallSession(selectedInstallApiKey.value.id, {
      target_cli: installCli.value,
      target_system: installSystem.value,
    })
  } catch (error) {
    log.error('生成 CLI 安装命令失败:', error)
    showError(parseApiError(error, '生成 CLI 安装命令失败'))
  } finally {
    installLoading.value = false
  }
}

async function copyInstallCommand() {
  if (!installCommand.value) return
  const copied = await copyTextToClipboard(installCommand.value, false)
  if (!copied) return

  installCopied.value = true
  success('安装命令已复制到剪贴板')
  clearInstallCopiedResetTimer()
  installCopiedResetTimer = setTimeout(() => {
    installCopied.value = false
    installCopiedResetTimer = null
  }, 2000)
}

function openCcSwitchDialog(apiKey: ApiKey) {
  selectedCcSwitchApiKey.value = apiKey
  ccSwitchApp.value = 'claude'
  ccSwitchProviderName.value = `Niffler - ${apiKey.name || 'API Key'}`
  ccSwitchModel.value = ''
  showCcSwitchDialog.value = true
  void refreshCcSwitchBaseUrl()
}

function selectCcSwitchApp(value: CcSwitchApp) {
  ccSwitchApp.value = value
  if (value === 'codex') {
    ccSwitchModel.value = ccSwitchModel.value.trim() || DEFAULT_CCSWITCH_CODEX_MODEL
  } else {
    ccSwitchModel.value = ''
  }
}

async function refreshCcSwitchBaseUrl() {
  try {
    const response = await meApi.getPublicBaseUrl()
    const value = response.public_base_url?.trim().replace(/\/+$/, '')
    if (value) {
      ccSwitchBaseUrl.value = value
    }
  } catch (error) {
    log.warn('获取公开 API 地址失败，使用前端推断地址:', error)
  }
}

async function importToCcSwitch() {
  if (!selectedCcSwitchApiKey.value) return
  ccSwitchImportLoading.value = true
  try {
    await refreshCcSwitchBaseUrl()
    const response = await meApi.getFullApiKey(selectedCcSwitchApiKey.value.id)
    const deeplink = buildCcSwitchImportUrl({
      app: ccSwitchApp.value,
      baseUrl: ccSwitchBaseUrl.value,
      providerName: ccSwitchProviderName.value,
      apiKey: response.key,
      model: ccSwitchModel.value,
    })
    window.location.href = deeplink
    success('已打开 CC Switch 导入')
  } catch (error) {
    log.error('导入 CC Switch 失败:', error)
    showError(parseApiError(error, '导入 CC Switch 失败'))
  } finally {
    ccSwitchImportLoading.value = false
  }
}

function closeCreatedKeyDialog() {
  showKeyDialog.value = false
  const pending = pendingSetupApiKey.value
  pendingSetupApiKey.value = null
  if (pending) {
    openSetupChoiceDialog(pending)
  }
}

function closeApiKeyDialog() {
  showCreateDialog.value = false
  editingApiKey.value = null
  newKeyName.value = ''
  selectedGroupId.value = apiKeyGroups.value[0]?.id || ''
  newKeyRateLimit.value = undefined
  newKeyConcurrentLimit.value = undefined
  keyRedactionMode.value = 'inherit'
  newKeyRedactionEnabled.value = false
  newKeyRedactionInjectNotice.value = true
}

async function saveApiKey() {
  if (!newKeyName.value.trim()) {
    showError('请输入密钥名称')
    return
  }
  if (apiKeyGroups.value.length === 0 || !selectedGroupId.value) {
    showError('当前没有可用分组，请联系管理员')
    return
  }

  creating.value = true
  try {
    if (editingApiKey.value) {
      await meApi.updateApiKey(editingApiKey.value.id, {
        name: newKeyName.value,
        group_id: selectedGroupId.value || undefined,
        rate_limit: newKeyRateLimit.value ?? 0,
        concurrent_limit: newKeyConcurrentLimit.value,
        feature_settings: keyRedactionMode.value === 'custom'
          ? mergeChatPiiRedactionFeatureSettings(editingApiKey.value.feature_settings, {
                enabled: newKeyRedactionEnabled.value,
                inject_model_instruction: newKeyRedactionInjectNotice.value,
            })
          : null,
      })
      success('API 密钥更新成功')
    } else {
      const newKey = await meApi.createApiKey({
        name: newKeyName.value,
        group_id: selectedGroupId.value || undefined,
        rate_limit: newKeyRateLimit.value ?? 0,
        concurrent_limit: newKeyConcurrentLimit.value,
        ...(keyRedactionMode.value === 'custom'
          ? {
              feature_settings: mergeChatPiiRedactionFeatureSettings(null, {
                enabled: newKeyRedactionEnabled.value,
                inject_model_instruction: newKeyRedactionInjectNotice.value,
              }),
            }
          : {}),
      })
      newKeyValue.value = newKey.key || ''
      pendingSetupApiKey.value = newKey
      showKeyDialog.value = true
      success('API 密钥创建成功')
    }
    closeApiKeyDialog()
    await loadApiKeys()
  } catch (error) {
    log.error(editingApiKey.value ? '更新 API 密钥失败:' : '创建 API 密钥失败:', error)
    showError(editingApiKey.value ? '更新 API 密钥失败' : '创建 API 密钥失败')
  } finally {
    creating.value = false
  }
}

function confirmDelete(apiKey: ApiKey) {
  keyToDelete.value = apiKey
  showDeleteDialog.value = true
}

async function deleteApiKey() {
  if (!keyToDelete.value) return

  deleting.value = true
  try {
    await meApi.deleteApiKey(keyToDelete.value.id)
    apiKeys.value = apiKeys.value.filter(k => k.id !== keyToDelete.value?.id)
    showDeleteDialog.value = false
    success('API 密钥已删除')
  } catch (error) {
    log.error('删除 API 密钥失败:', error)
    showError('删除 API 密钥失败')
  } finally {
    deleting.value = false
    keyToDelete.value = null
  }
}

async function toggleApiKey(apiKey: ApiKey) {
  try {
    const updated = await meApi.toggleApiKey(apiKey.id)
    const index = apiKeys.value.findIndex(k => k.id === apiKey.id)
    if (index !== -1) {
      apiKeys.value[index].is_active = updated.is_active
    }
    success(updated.is_active ? '密钥已启用' : '密钥已禁用')
  } catch (error) {
    log.error('切换密钥状态失败:', error)
    showError('操作失败')
  }
}

async function copyApiKey(apiKey: ApiKey) {
  try {
    // 调用后端 API 获取完整密钥
    const response = await meApi.getFullApiKey(apiKey.id)
    const copied = await copyTextToClipboard(response.key, false) // 不显示内部提示
    if (copied) {
      success('完整密钥已复制到剪贴板')
    }
  } catch (error) {
    log.error('复制密钥失败:', error)
    showError('复制失败，请重试')
  }
}

async function copyTextToClipboard(text: string, showToast: boolean = true): Promise<boolean> {
  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text)
      if (showToast) success('已复制到剪贴板')
      return true
    } else {
      const textArea = document.createElement('textarea')
      textArea.value = text
      textArea.style.position = 'fixed'
      textArea.style.left = '-999999px'
      textArea.style.top = '-999999px'
      document.body.appendChild(textArea)
      textArea.focus()
      textArea.select()

      try {
        const successful = document.execCommand('copy')
        if (successful && showToast) {
          success('已复制到剪贴板')
        }
        if (successful) {
          return true
        } else {
          showError('复制失败，请手动复制')
          return false
        }
      } finally {
        document.body.removeChild(textArea)
      }
    }
  } catch (error) {
    log.error('复制失败:', error)
    showError('复制失败，请手动选择文本进行复制')
    return false
  }
}

function formatNumber(num: number | undefined | null): string {
  if (num === undefined || num === null) {
    return '0'
  }
  return num.toLocaleString('zh-CN')
}

function formatConcurrentLimitSimple(concurrentLimit?: number | null): string {
  if (concurrentLimit == null || concurrentLimit === 0) {
    return '不限并发'
  }
  return `${concurrentLimit} 并发`
}

function formatDate(dateString?: string | null): string {
  if (!dateString) return '未知'
  const date = new Date(dateString)
  if (Number.isNaN(date.getTime())) return '未知'
  return date.toLocaleDateString('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit'
  })
}

function formatRelativeTime(dateString: string): string {
  const date = new Date(dateString)
  if (Number.isNaN(date.getTime())) return '未知'
  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffMins = Math.floor(diffMs / (1000 * 60))
  const diffHours = Math.floor(diffMs / (1000 * 60 * 60))
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24))

  if (diffMins < 1) return '刚刚'
  if (diffMins < 60) return `${diffMins}分钟前`
  if (diffHours < 24) return `${diffHours}小时前`
  if (diffDays < 7) return `${diffDays}天前`

  return formatDate(dateString)
}

</script>
