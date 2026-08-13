<template>
  <PageContainer>
    <div class="relative flex gap-6">
      <!-- 主内容 -->
      <div class="flex-1 min-w-0">
        <PageHeader
          :title="t('systemSettings.title')"
          :description="t('systemSettings.description')"
        />

        <Tabs v-model="activeTab" class="mt-6">
          <TabsList class="grid w-full grid-cols-2 gap-1 sm:grid-cols-3 lg:grid-cols-6">
            <TabsTrigger v-for="tab in settingsTabs" :key="tab.value" :value="tab.value">
              {{ tab.label }}
            </TabsTrigger>
          </TabsList>
          <div class="mt-6 space-y-6">
          <!-- 站点信息 -->
          <SiteInfoSection
            v-show="activeTab === 'site'"
            id="section-site-info"
            :site-name="systemConfig.site_name"
            :site-subtitle="systemConfig.site_subtitle"
            :contact-us-format="systemConfig.contact_us_format"
            :contact-us-content="systemConfig.contact_us_content"
            :loading="siteInfoLoading"
            :has-changes="hasSiteInfoChanges"
            @save="saveSiteInfo"
            @update:site-name="systemConfig.site_name = $event"
            @update:site-subtitle="systemConfig.site_subtitle = $event"
            @update:contact-us-format="systemConfig.contact_us_format = $event"
            @update:contact-us-content="systemConfig.contact_us_content = $event"
          />

          <!-- 数据管理 -->
          <DataManagementSection
            v-show="activeTab === 'data'"
            id="section-data-mgmt"
            :config-export-loading="exportLoading"
            :config-import-loading="importLoading"
            :users-export-loading="exportUsersLoading"
            :users-import-loading="importUsersLoading"
            :aggregate-export-loading="exportAggregateLoading"
            :aggregate-import-loading="importAggregateLoading"
            @export="handleDataExport"
            @file-select="handleDataFileSelect"
          />

          <!-- 网络代理 -->
          <ProxyConfigSection
            v-show="activeTab === 'network'"
            id="section-proxy"
            :proxy-node-id="systemConfig.system_proxy_node_id"
            :online-nodes="proxyNodesStore.onlineNodes"
            :all-nodes="proxyNodesStore.nodes"
            :loading="proxyConfigLoading"
            :has-changes="hasProxyConfigChanges"
            @save="saveProxyConfig"
            @update:proxy-node-id="systemConfig.system_proxy_node_id = $event"
          />

          <!-- 基础配置 -->
          <BasicConfigSection
            v-show="activeTab === 'site'"
            id="section-basic"
            :default-user-initial-gift-usd="systemConfig.default_user_initial_gift_usd"
            :rate-limit-per-minute="systemConfig.rate_limit_per_minute"
            :enable-registration="systemConfig.enable_registration"
            :password-policy-level="systemConfig.password_policy_level"
            :turnstile-enabled="systemConfig.turnstile_enabled"
            :turnstile-site-key="systemConfig.turnstile_site_key"
            :turnstile-secret-key="systemConfig.turnstile_secret_key"
            :turnstile-secret-configured="systemConfig.turnstile_secret_key_is_set"
            :turnstile-allowed-hostnames-str="turnstileAllowedHostnamesStr"
            :referral-enabled="systemConfig.referral_enabled"
            :referral-reward-mode="systemConfig.referral_reward_mode"
            :referral-recharge-percent="systemConfig.referral_recharge_percent"
            :referral-headcount-amount-usd="systemConfig.referral_headcount_amount_usd"
            :referral-headcount-trigger="systemConfig.referral_headcount_trigger"
            :registration-privacy-policy-enabled="systemConfig.registration_privacy_policy_enabled"
            :registration-privacy-policy-format="systemConfig.registration_privacy_policy_format"
            :registration-privacy-policy-content="systemConfig.registration_privacy_policy_content"
            :registration-privacy-policy-version="systemConfig.registration_privacy_policy_version"
            :auto-delete-expired-keys="systemConfig.auto_delete_expired_keys"
            :enable-format-conversion="systemConfig.enable_format_conversion"
            :enable-openai-image-sync-heartbeat="systemConfig.enable_openai_image_sync_heartbeat"
            :loading="basicConfigLoading"
            :has-changes="hasBasicConfigChanges"
            @save="saveBasicConfig"
            @update:default-user-initial-gift-usd="systemConfig.default_user_initial_gift_usd = $event"
            @update:rate-limit-per-minute="systemConfig.rate_limit_per_minute = $event"
            @update:enable-registration="systemConfig.enable_registration = $event"
            @update:password-policy-level="systemConfig.password_policy_level = $event"
            @update:turnstile-enabled="systemConfig.turnstile_enabled = $event"
            @update:turnstile-site-key="systemConfig.turnstile_site_key = $event"
            @update:turnstile-secret-key="systemConfig.turnstile_secret_key = $event"
            @update:turnstile-allowed-hostnames-str="turnstileAllowedHostnamesStr = $event"
            @clear-turnstile-secret="clearTurnstileSecret"
            @update:referral-enabled="systemConfig.referral_enabled = $event"
            @update:referral-reward-mode="systemConfig.referral_reward_mode = $event"
            @update:referral-recharge-percent="systemConfig.referral_recharge_percent = $event"
            @update:referral-headcount-amount-usd="systemConfig.referral_headcount_amount_usd = $event"
            @update:referral-headcount-trigger="systemConfig.referral_headcount_trigger = $event"
            @update:registration-privacy-policy-enabled="systemConfig.registration_privacy_policy_enabled = $event"
            @update:registration-privacy-policy-format="systemConfig.registration_privacy_policy_format = $event"
            @update:registration-privacy-policy-content="systemConfig.registration_privacy_policy_content = $event"
            @update:registration-privacy-policy-version="systemConfig.registration_privacy_policy_version = $event"
            @update:auto-delete-expired-keys="systemConfig.auto_delete_expired_keys = $event"
            @update:enable-format-conversion="systemConfig.enable_format_conversion = $event"
            @update:enable-openai-image-sync-heartbeat="systemConfig.enable_openai_image_sync_heartbeat = $event"
          />

          <!-- 请求记录配置 -->
          <RequestLogSection
            v-show="activeTab === 'network'"
            id="section-request-log"
            :request-record-level="systemConfig.request_record_level"
            :max-request-body-size-k-b="maxRequestBodySizeKB"
            :max-response-body-size-k-b="maxResponseBodySizeKB"
            :sensitive-headers-str="sensitiveHeadersStr"
            :loading="logConfigLoading"
            :has-changes="hasLogConfigChanges"
            @save="saveLogConfig"
            @update:request-record-level="systemConfig.request_record_level = $event"
            @update:max-request-body-size-k-b="maxRequestBodySizeKB = $event"
            @update:max-response-body-size-k-b="maxResponseBodySizeKB = $event"
            @update:sensitive-headers-str="sensitiveHeadersStr = $event"
          />

          <!-- 内容审查 / 账号保护 -->
          <ContentModerationSection
            v-show="activeTab === 'security'"
            id="section-content-moderation"
            :config="systemConfig.content_moderation_account_protection"
            :loading="contentModerationLoading"
            :has-changes="hasContentModerationChanges"
            @save="saveContentModerationConfig"
            @update:config="systemConfig.content_moderation_account_protection = $event"
          />

          <!-- Provider 高级设置 -->
          <ProviderAdvancedSection
            v-show="activeTab === 'provider'"
            id="section-provider-advanced"
            :enabled="systemConfig.codex_oauth_identity_convergence_enabled"
            :loading="systemConfigLoading"
            :saving="providerAdvancedConfigLoading"
            :load-error="!systemConfigLoading && !providerAdvancedConfigReady"
            :has-changes="hasProviderAdvancedConfigChanges"
            @save="saveProviderAdvancedConfig"
            @update:enabled="systemConfig.codex_oauth_identity_convergence_enabled = $event"
          />

          <!-- 请求记录清理策略 -->
          <CleanupPolicySection
            v-show="activeTab === 'data'"
            id="section-cleanup"
            :enable-auto-cleanup="systemConfig.enable_auto_cleanup"
            :detail-log-retention-days="systemConfig.detail_log_retention_days"
            :compressed-log-retention-days="systemConfig.compressed_log_retention_days"
            :header-retention-days="systemConfig.header_retention_days"
            :log-retention-days="systemConfig.log_retention_days"
            :cleanup-batch-size="systemConfig.cleanup_batch_size"
            :audit-log-retention-days="systemConfig.audit_log_retention_days"
            :request-candidates-retention-days="systemConfig.request_candidates_retention_days"
            :request-candidates-cleanup-batch-size="systemConfig.request_candidates_cleanup_batch_size"
            :proxy-node-metrics-1m-retention-days="systemConfig.proxy_node_metrics_1m_retention_days"
            :proxy-node-metrics-1h-retention-days="systemConfig.proxy_node_metrics_1h_retention_days"
            :proxy-node-metrics-cleanup-batch-size="systemConfig.proxy_node_metrics_cleanup_batch_size"
            :loading="cleanupConfigLoading"
            :has-changes="hasCleanupConfigChanges"
            @save="saveCleanupConfig"
            @toggle-auto-cleanup="handleAutoCleanupToggle"
            @update:detail-log-retention-days="systemConfig.detail_log_retention_days = $event"
            @update:compressed-log-retention-days="systemConfig.compressed_log_retention_days = $event"
            @update:header-retention-days="systemConfig.header_retention_days = $event"
            @update:log-retention-days="systemConfig.log_retention_days = $event"
            @update:cleanup-batch-size="systemConfig.cleanup_batch_size = $event"
            @update:audit-log-retention-days="systemConfig.audit_log_retention_days = $event"
            @update:request-candidates-retention-days="systemConfig.request_candidates_retention_days = $event"
            @update:request-candidates-cleanup-batch-size="systemConfig.request_candidates_cleanup_batch_size = $event"
            @update:proxy-node-metrics-1m-retention-days="systemConfig.proxy_node_metrics_1m_retention_days = $event"
            @update:proxy-node-metrics-1h-retention-days="systemConfig.proxy_node_metrics_1h_retention_days = $event"
            @update:proxy-node-metrics-cleanup-batch-size="systemConfig.proxy_node_metrics_cleanup_batch_size = $event"
          />

          <!-- 定时任务 -->
          <ScheduledTasksSection
            v-show="activeTab === 'data'"
            id="section-scheduled"
            :scheduled-tasks="scheduledTasks"
          />

          <!-- 系统版本信息 -->
          <SystemInfoSection
            v-show="activeTab === 'diagnostics'"
            id="section-sysinfo"
            :system-version="systemVersion"
          />
          </div>
        </Tabs>
      </div>

    </div>

    <!-- 导入配置对话框 -->
    <ConfigImportDialog
      :import-dialog-open="importDialogOpen"
      :import-result-dialog-open="importResultDialogOpen"
      :import-preview="importPreview"
      :import-result="importResult"
      :merge-mode="mergeMode"
      :merge-mode-select-open="mergeModeSelectOpen"
      :import-loading="importLoading"
      @confirm="confirmImport"
      @update:import-dialog-open="importDialogOpen = $event"
      @update:import-result-dialog-open="importResultDialogOpen = $event"
      @update:merge-mode="mergeMode = $event"
      @update:merge-mode-select-open="mergeModeSelectOpen = $event"
    />

    <!-- 用户数据导入对话框 -->
    <UsersImportDialog
      :import-users-dialog-open="importUsersDialogOpen"
      :import-users-result-dialog-open="importUsersResultDialogOpen"
      :import-users-preview="importUsersPreview"
      :import-users-result="importUsersResult"
      :users-merge-mode="usersMergeMode"
      :users-merge-mode-select-open="usersMergeModeSelectOpen"
      :import-users-loading="importUsersLoading"
      @confirm="confirmImportUsers"
      @update:import-users-dialog-open="importUsersDialogOpen = $event"
      @update:import-users-result-dialog-open="importUsersResultDialogOpen = $event"
      @update:users-merge-mode="usersMergeMode = $event"
      @update:users-merge-mode-select-open="usersMergeModeSelectOpen = $event"
    />

    <!-- 聚合数据导入对话框 -->
    <AggregateImportDialog
      :aggregate-import-dialog-open="aggregateImportDialogOpen"
      :aggregate-import-result-dialog-open="aggregateImportResultDialogOpen"
      :aggregate-import-preview="aggregateImportPreview"
      :aggregate-import-result="aggregateImportResult"
      :aggregate-merge-mode="aggregateMergeMode"
      :aggregate-merge-mode-select-open="aggregateMergeModeSelectOpen"
      :import-aggregate-loading="importAggregateLoading"
      @confirm="confirmImportAggregate"
      @update:aggregate-import-dialog-open="aggregateImportDialogOpen = $event"
      @update:aggregate-import-result-dialog-open="aggregateImportResultDialogOpen = $event"
      @update:aggregate-merge-mode="aggregateMergeMode = $event"
      @update:aggregate-merge-mode-select-open="aggregateMergeModeSelectOpen = $event"
    />
  </PageContainer>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { PageHeader, PageContainer } from '@/components/layout'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui'
import { useProxyNodesStore } from '@/stores/proxy-nodes'

// Composables
import { useSystemConfig } from './system-settings/composables/useSystemConfig'
import { useConfigExportImport } from './system-settings/composables/useConfigExportImport'
import { useScheduledTasks } from './system-settings/composables/useScheduledTasks'

// Section components
import SiteInfoSection from './system-settings/SiteInfoSection.vue'
import DataManagementSection from './system-settings/DataManagementSection.vue'
import ProxyConfigSection from './system-settings/ProxyConfigSection.vue'
import BasicConfigSection from './system-settings/BasicConfigSection.vue'
import RequestLogSection from './system-settings/RequestLogSection.vue'
import ContentModerationSection from './system-settings/ContentModerationSection.vue'
import ProviderAdvancedSection from './system-settings/ProviderAdvancedSection.vue'
import CleanupPolicySection from './system-settings/CleanupPolicySection.vue'
import ScheduledTasksSection from './system-settings/ScheduledTasksSection.vue'
import SystemInfoSection from './system-settings/SystemInfoSection.vue'

// Dialog components
import ConfigImportDialog from './system-settings/ConfigImportDialog.vue'
import UsersImportDialog from './system-settings/UsersImportDialog.vue'
import AggregateImportDialog from './system-settings/AggregateImportDialog.vue'

const proxyNodesStore = useProxyNodesStore()
const { t } = useI18n()

const settingsTabs = [
  { value: 'site', label: t('systemSettings.site') },
  { value: 'security', label: t('systemSettings.security') },
  { value: 'network', label: t('systemSettings.network') },
  { value: 'provider', label: t('systemSettings.providerAdvanced') },
  { value: 'data', label: t('systemSettings.data') },
  { value: 'diagnostics', label: t('systemSettings.diagnostics') },
]
const activeTab = ref('site')

// System config composable
const {
  systemConfig,
  systemVersion,
  siteInfoLoading,
  proxyConfigLoading,
  basicConfigLoading,
  logConfigLoading,
  contentModerationLoading,
  cleanupConfigLoading,
  providerAdvancedConfigLoading,
  providerAdvancedConfigReady,
  systemConfigLoading,
  hasSiteInfoChanges,
  hasProxyConfigChanges,
  hasBasicConfigChanges,
  hasLogConfigChanges,
  hasContentModerationChanges,
  hasCleanupConfigChanges,
  hasProviderAdvancedConfigChanges,
  maxRequestBodySizeKB,
  maxResponseBodySizeKB,
  sensitiveHeadersStr,
  turnstileAllowedHostnamesStr,
  loadSystemConfig,
  loadSystemVersion,
  saveSiteInfo,
  saveProxyConfig,
  saveBasicConfig,
  clearTurnstileSecret,
  saveLogConfig,
  saveContentModerationConfig,
  saveProviderAdvancedConfig,
  saveCleanupConfig,
  handleAutoCleanupToggle,
} = useSystemConfig()

// 数据导出/导入 composable
const {
  exportLoading,
  importLoading,
  importDialogOpen,
  importResultDialogOpen,
  importPreview,
  importResult,
  mergeMode,
  mergeModeSelectOpen,
  handleExportConfig,
  handleConfigFileSelect,
  confirmImport,
  exportUsersLoading,
  importUsersLoading,
  importUsersDialogOpen,
  importUsersResultDialogOpen,
  importUsersPreview,
  importUsersResult,
  usersMergeMode,
  usersMergeModeSelectOpen,
  handleExportUsers,
  handleUsersFileSelect,
  confirmImportUsers,
  exportAggregateLoading,
  importAggregateLoading,
  aggregateImportDialogOpen,
  aggregateImportResultDialogOpen,
  aggregateImportPreview,
  aggregateImportResult,
  aggregateMergeMode,
  aggregateMergeModeSelectOpen,
  handleExportAggregate,
  handleAggregateFileSelect,
  confirmImportAggregate,
} = useConfigExportImport(systemConfig)

type DataManagementKind = 'config' | 'users' | 'aggregate'

function handleDataExport(kind: DataManagementKind) {
  if (kind === 'config') {
    handleExportConfig()
  } else if (kind === 'users') {
    handleExportUsers()
  } else {
    handleExportAggregate()
  }
}

function handleDataFileSelect(kind: DataManagementKind, event: Event) {
  if (kind === 'config') {
    handleConfigFileSelect(event)
  } else if (kind === 'users') {
    handleUsersFileSelect(event)
  } else {
    handleAggregateFileSelect(event)
  }
}

// Scheduled tasks composable
const {
  scheduledTasks,
  initPreviousValues,
} = useScheduledTasks(systemConfig)

onMounted(async () => {
  await Promise.all([
    loadSystemConfig(),
    loadSystemVersion(),
    proxyNodesStore.ensureLoaded(),
  ])
  // 配置加载完成后初始化定时任务的原始值
  initPreviousValues()
})
</script>
