<template>
  <div class="container mx-auto px-4 py-8">
    <h2 class="text-2xl font-bold text-foreground mb-6">
      {{ t('userSettings.title') }}
    </h2>

    <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
      <!-- 左侧：个人信息和密码 -->
      <div class="lg:col-span-2 space-y-6">
        <!-- 基本信息 -->
        <Card class="p-6">
          <form
            class="space-y-4"
            @submit.prevent="updateProfile"
          >
            <div class="flex items-center justify-between">
              <h3 class="text-lg font-medium text-foreground">
                {{ t('userSettings.basic') }}
              </h3>
              <Button
                type="submit"
                :disabled="savingProfile || !hasProfileChanges"
                class="shadow-none hover:shadow-none"
              >
                {{ savingProfile ? t('userSettings.saving') : t('userSettings.save') }}
              </Button>
            </div>

            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div>
                <Label for="username">{{ t('userSettings.username') }}</Label>
                <Input
                  id="username"
                  v-model="profileForm.username"
                  class="mt-1"
                />
              </div>
              <div>
                <Label for="avatar">{{ t('userSettings.avatar') }}</Label>
                <Input
                  id="avatar"
                  v-model="preferencesForm.avatar_url"
                  type="url"
                  class="mt-1"
                />
              </div>
            </div>

            <div>
              <Label for="bio">{{ t('userSettings.bio') }}</Label>
              <Textarea
                id="bio"
                v-model="preferencesForm.bio"
                rows="3"
                class="mt-1"
              />
            </div>

            <!-- 邮箱字段：当系统配置了邮箱服务或用户已有邮箱时显示 -->
            <div
              v-if="emailConfigured || profileForm.email"
              class="grid grid-cols-1 md:grid-cols-2 gap-4"
            >
              <div>
                <Label for="email">{{ t('userSettings.email') }}</Label>
                <Input
                  id="email"
                  v-model="profileForm.email"
                  type="email"
                  class="mt-1"
                  :disabled="!emailConfigured"
                />
                <p
                  v-if="!emailConfigured && profileForm.email"
                  class="mt-1 text-xs text-muted-foreground"
                >
                  {{ t('userSettings.emailUnavailable') }}
                </p>
              </div>
            </div>
          </form>
        </Card>

        <Card class="p-6">
          <div class="flex items-center justify-between mb-4">
            <div>
              <h3 class="text-lg font-medium text-foreground">
                {{ t('userSettings.privacy') }}
              </h3>
              <p class="text-sm text-muted-foreground mt-1">
                {{ t('userSettings.privacyHint') }}
              </p>
            </div>
            <Button
              variant="outline"
              :disabled="savingFeatureSettings || !hasFeatureSettingsChanges"
              @click="updateFeatureSettings"
            >
              {{ savingFeatureSettings ? t('userSettings.saving') : t('userSettings.save') }}
            </Button>
          </div>
          <div class="space-y-4">
            <div class="flex items-center justify-between gap-4 rounded-lg border border-border/60 bg-muted/30 px-4 py-3">
              <div>
                <Label class="text-sm font-medium">{{ t('userSettings.enabledByDefault') }}</Label>
                <p class="mt-1 text-xs text-muted-foreground">
                  {{ t('userSettings.enabledHint') }}
                </p>
              </div>
              <Switch v-model="featureSettingsForm.chatPiiRedactionEnabled" />
            </div>
            <div class="flex items-center justify-between gap-4 rounded-lg border border-border/60 bg-muted/30 px-4 py-3">
              <div>
                <Label class="text-sm font-medium">{{ t('userSettings.notice') }}</Label>
                <p class="mt-1 text-xs text-muted-foreground">
                  {{ t('userSettings.noticeHint') }}
                </p>
              </div>
              <Switch
                v-model="featureSettingsForm.chatPiiRedactionInjectNotice"
                :disabled="!featureSettingsForm.chatPiiRedactionEnabled"
              />
            </div>
          </div>
        </Card>

        <!-- 密码设置（LDAP 用户不显示） -->
        <Card
          v-if="profile?.auth_source !== 'ldap'"
          class="p-6"
        >
          <form
            class="space-y-4"
            @submit.prevent="changePassword"
          >
            <div class="flex items-center justify-between">
              <h3 class="text-lg font-medium text-foreground">
                {{ profile?.has_password ? t('userSettings.changePassword') : t('userSettings.setPassword') }}
              </h3>
              <Button
                type="submit"
                :disabled="changingPassword || !hasPasswordChanges"
                class="shadow-none hover:shadow-none"
              >
                {{ changingPassword ? t('userSettings.saving') : t('userSettings.save') }}
              </Button>
            </div>
            <div v-if="profile?.has_password">
              <Label for="old-password">{{ t('userSettings.currentPassword') }}</Label>
              <Input
                id="old-password"
                v-model="passwordForm.old_password"
                type="text"
                masked
                class="mt-1"
              />
            </div>
            <div>
              <Label for="new-password">{{ profile?.has_password ? t('userSettings.newPassword') : t('userSettings.password') }}</Label>
              <Input
                id="new-password"
                v-model="passwordForm.new_password"
                type="text"
                masked
                :placeholder="getPasswordPolicyPlaceholder(passwordPolicyLevel, translatePasswordPolicy)"
                class="mt-1"
              />
              <p
                v-if="passwordError"
                class="mt-1 text-xs text-destructive"
              >
                {{ passwordError }}
              </p>
              <p
                v-else
                class="mt-1 text-xs text-muted-foreground"
              >
                {{ passwordPolicyHint }}
              </p>
            </div>
            <div>
              <Label for="confirm-password">{{ profile?.has_password ? t('userSettings.confirmNewPassword') : t('userSettings.confirmPassword') }}</Label>
              <Input
                id="confirm-password"
                v-model="passwordForm.confirm_password"
                type="text"
                masked
                :placeholder="t('userSettings.enterPasswordAgain')"
                class="mt-1"
              />
              <p
                v-if="passwordForm.confirm_password && passwordForm.new_password !== passwordForm.confirm_password"
                class="mt-1 text-xs text-destructive"
              >
                {{ t('userSettings.passwordsMismatch') }}
              </p>
            </div>
          </form>
        </Card>

        <Card class="p-6">
          <div class="flex items-center justify-between mb-4">
            <div>
              <h3 class="text-lg font-medium text-foreground">
                {{ t('userSettings.sessions') }}
              </h3>
              <p class="text-sm text-muted-foreground mt-1">
                {{ t('userSettings.sessionsHint') }}
              </p>
            </div>
            <Button
              variant="outline"
              :disabled="sessionsLoading || otherSessionCount === 0 || sessionActionLoading === 'others'"
              @click="handleRevokeOtherSessions"
            >
              {{ sessionActionLoading === 'others' ? t('userSettings.processing') : t('userSettings.signOutOthers') }}
            </Button>
          </div>

          <div
            v-if="sessionsLoading"
            class="text-sm text-muted-foreground"
          >
            {{ t('userSettings.loadingSessions') }}
          </div>
          <div
            v-else-if="userSessions.length === 0"
            class="text-sm text-muted-foreground"
          >
            {{ t('userSettings.noSessions') }}
          </div>
          <div
            v-else
            class="space-y-3"
          >
            <div
              v-for="session in userSessions"
              :key="session.id"
              class="flex items-start justify-between gap-4 rounded-lg border border-border/60 bg-muted/20 p-4"
            >
              <div class="min-w-0">
                <div class="flex items-center gap-2 flex-wrap">
                  <template v-if="editingSessionId === session.id">
                    <Input
                      v-model="sessionLabelDraft"
                      size="sm"
                      class="h-8 w-56"
                      maxlength="120"
                      @keyup.enter="saveSessionLabel(session.id)"
                    />
                  </template>
                  <span
                    v-else
                    class="font-medium text-foreground"
                  >{{ session.device_label }}</span>
                  <Badge
                    v-if="session.is_current"
                    variant="secondary"
                  >
                    {{ t('userSettings.current') }}
                  </Badge>
                </div>
                <p class="mt-1 text-xs text-muted-foreground">
                  {{ formatSessionMeta(session) }}
                </p>
                <p class="mt-1 text-xs text-muted-foreground">
                  {{ t('userSettings.lastActive') }} {{ formatDate(session.last_seen_at || session.created_at) }}
                  <span v-if="session.ip_address"> · IP {{ session.ip_address }}</span>
                </p>
              </div>
              <div class="flex items-center gap-2">
                <template v-if="editingSessionId === session.id">
                  <Button
                    size="sm"
                    :disabled="sessionActionLoading === session.id || !sessionLabelDraft.trim()"
                    @click="saveSessionLabel(session.id)"
                  >
                    {{ sessionActionLoading === session.id ? t('userSettings.saving') : t('userSettings.save') }}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    :disabled="sessionActionLoading === session.id"
                    @click="cancelSessionLabelEdit"
                  >
                    {{ t('userSettings.cancel') }}
                  </Button>
                </template>
                <template v-else>
                  <Button
                    variant="outline"
                    size="sm"
                    :disabled="sessionActionLoading !== null"
                    @click="startSessionLabelEdit(session)"
                  >
                    {{ t('userSettings.rename') }}
                  </Button>
                  <Button
                    v-if="!session.is_current"
                    variant="outline"
                    size="sm"
                    :disabled="sessionActionLoading === session.id"
                    @click="handleRevokeSession(session.id)"
                  >
                    {{ sessionActionLoading === session.id ? t('userSettings.processing') : t('userSettings.signOut') }}
                  </Button>
                </template>
              </div>
            </div>
          </div>
        </Card>

        <!-- OAuth 绑定 -->
        <Card class="p-6">
          <h3 class="text-lg font-medium text-foreground mb-4">
            {{ t('userSettings.oauthBinding') }}
          </h3>

          <div
            v-if="profile?.auth_source === 'ldap'"
            class="text-sm text-muted-foreground"
          >
            {{ t('userSettings.oauthUnsupported') }}
          </div>

          <div
            v-else-if="oauthUnavailable"
            class="text-sm text-muted-foreground"
          >
            {{ t('userSettings.oauthUnavailable') }}
          </div>

          <div
            v-else
            class="space-y-4"
          >
            <!-- 合并已绑定和可绑定为卡片网格 -->
            <div
              v-if="oauthLinks.length === 0 && bindableProviders.length === 0"
              class="text-sm text-muted-foreground"
            >
              {{ t('userSettings.noOAuth') }}
            </div>
            <div
              v-else
              class="grid grid-cols-1 sm:grid-cols-2 gap-3"
            >
              <!-- 已绑定的 Provider -->
              <div
                v-for="link in oauthLinks"
                :key="link.provider_type"
                class="flex items-center justify-between gap-3 rounded-lg border border-border bg-muted/30 p-4"
              >
                <div class="flex items-center gap-3 min-w-0 flex-1">
                  <!-- eslint-disable vue/no-v-html -->
                  <div
                    class="oauth-icon shrink-0"
                    v-html="getOAuthIcon(link.provider_type)"
                  />
                  <!-- eslint-enable vue/no-v-html -->
                  <div class="min-w-0">
                    <div class="text-sm font-medium truncate">
                      {{ link.display_name }}
                    </div>
                    <div class="text-xs text-muted-foreground truncate">
                      {{ link.provider_username || link.provider_email || t('userSettings.bound') }}
                    </div>
                  </div>
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  :disabled="oauthActionLoading"
                  @click="handleUnbind(link.provider_type)"
                >
                  {{ t('userSettings.unbind') }}
                </Button>
              </div>

              <!-- 可绑定的 Provider -->
              <div
                v-for="p in bindableProviders"
                :key="p.provider_type"
                class="flex items-center justify-between gap-3 rounded-lg border border-dashed border-border p-4 hover:border-primary/50 transition-colors"
              >
                <div class="flex items-center gap-3 min-w-0 flex-1">
                  <!-- eslint-disable vue/no-v-html -->
                  <div
                    class="oauth-icon shrink-0"
                    v-html="getOAuthIcon(p.provider_type, p.icon_url)"
                  />
                  <!-- eslint-enable vue/no-v-html -->
                  <div class="min-w-0">
                    <div class="text-sm font-medium truncate">
                      {{ p.display_name }}
                    </div>
                    <div class="text-xs text-muted-foreground">
                      {{ t('userSettings.unbound') }}
                    </div>
                  </div>
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  :disabled="oauthActionLoading"
                  @click="handleBind(p.provider_type)"
                >
                  {{ t('userSettings.bind') }}
                </Button>
              </div>
            </div>
          </div>
        </Card>

        <!-- 偏好设置 -->
        <Card class="p-6">
          <h3 class="text-lg font-medium text-foreground mb-4">
            {{ t('userSettings.preferences') }}
          </h3>
          <div class="space-y-4">
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div>
                <Label for="theme">{{ t('userSettings.themeLabel') }}</Label>
                <Select
                  v-model="preferencesForm.theme"
                  v-model:open="themeSelectOpen"
                  @update:model-value="handleThemeChange"
                >
                  <SelectTrigger
                    id="theme"
                    class="mt-1"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="light">
                      {{ t('userSettings.light') }}
                    </SelectItem>
                    <SelectItem value="dark">
                      {{ t('userSettings.dark') }}
                    </SelectItem>
                    <SelectItem value="system">
                      {{ t('userSettings.system') }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div>
                <Label for="language">{{ t('userSettings.languageLabel') }}</Label>
                <Select
                  v-model="preferencesForm.language"
                  v-model:open="languageSelectOpen"
                  @update:model-value="handleLanguageChange"
                >
                  <SelectTrigger
                    id="language"
                    class="mt-1"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="zh-CN">
                      {{ t('userSettings.chinese') }}
                    </SelectItem>
                    <SelectItem value="en">
                      English
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div>
                <Label for="timezone">{{ t('userSettings.timezone') }}</Label>
                <Input
                  id="timezone"
                  v-model="preferencesForm.timezone"
                  placeholder="Asia/Shanghai"
                  class="mt-1"
                />
              </div>
            </div>

            <div class="space-y-3">
              <h4 class="font-medium text-foreground">
                {{ t('userSettings.notifications') }}
              </h4>
              <div class="space-y-3">
                <!-- 邮件通知：仅当系统配置了邮箱服务时显示 -->
                <div
                  v-if="emailConfigured"
                  class="flex items-center justify-between py-2 border-b border-border/40 last:border-0"
                >
                  <div class="flex-1">
                    <Label
                      for="email-notifications"
                      class="text-sm font-medium cursor-pointer"
                    >
                      {{ t('userSettings.emailNotifications') }}
                    </Label>
                    <p class="text-xs text-muted-foreground mt-1">
                      {{ t('userSettings.emailNotificationsHint') }}
                    </p>
                  </div>
                  <Switch
                    id="email-notifications"
                    v-model="preferencesForm.notifications.email"
                    @update:model-value="updatePreferences"
                  />
                </div>
                <div class="flex items-center justify-between py-2 border-b border-border/40 last:border-0">
                  <div class="flex-1">
                    <Label
                      for="usage-alerts"
                      class="text-sm font-medium cursor-pointer"
                    >
                      {{ t('userSettings.usageAlerts') }}
                    </Label>
                    <p class="text-xs text-muted-foreground mt-1">
                      {{ t('userSettings.usageAlertsHint') }}
                    </p>
                  </div>
                  <Switch
                    id="usage-alerts"
                    v-model="preferencesForm.notifications.usage_alerts"
                    @update:model-value="updatePreferences"
                  />
                </div>
                <div class="flex items-center justify-between py-2">
                  <div class="flex-1">
                    <Label
                      for="announcement-notifications"
                      class="text-sm font-medium cursor-pointer"
                    >
                      {{ t('userSettings.announcementNotifications') }}
                    </Label>
                    <p class="text-xs text-muted-foreground mt-1">
                      {{ t('userSettings.announcementNotificationsHint') }}
                    </p>
                  </div>
                  <Switch
                    id="announcement-notifications"
                    v-model="preferencesForm.notifications.announcements"
                    @update:model-value="updatePreferences"
                  />
                </div>
              </div>
            </div>
          </div>
        </Card>
      </div>

      <!-- 右侧：账户信息和使用量 -->
      <div class="space-y-6">
        <!-- 账户信息 -->
        <Card class="p-6">
          <h3 class="text-lg font-medium text-foreground mb-4">
            {{ t('userSettings.accountInfo') }}
          </h3>
          <div class="space-y-3">
            <div class="flex justify-between">
              <span class="text-muted-foreground">{{ t('userSettings.role') }}</span>
              <Badge :variant="profile?.role === 'admin' ? 'default' : 'secondary'">
                {{ profileRoleLabel }}
              </Badge>
            </div>
            <div class="flex justify-between">
              <span class="text-muted-foreground">{{ t('userSettings.accountStatus') }}</span>
              <span :class="profile?.is_active ? 'text-success' : 'text-destructive'">
                {{ profile?.is_active ? t('userSettings.active') : t('userSettings.disabled') }}
              </span>
            </div>
            <div class="flex justify-between">
              <span class="text-muted-foreground">{{ t('userSettings.registeredAt') }}</span>
              <span class="text-foreground">
                {{ formatDate(profile?.created_at) }}
              </span>
            </div>
            <div class="flex justify-between">
              <span class="text-muted-foreground">{{ t('userSettings.lastLogin') }}</span>
              <span class="text-foreground">
                {{ profile?.last_login_at ? formatDate(profile.last_login_at) : t('userSettings.notRecorded') }}
              </span>
            </div>
          </div>
        </Card>

        <!-- 钱包状态 -->
        <Card class="p-6">
          <h3 class="text-lg font-medium text-foreground mb-4">
            {{ t('userSettings.walletStatus') }}
          </h3>
          <div class="space-y-4">
            <div class="flex justify-between text-sm">
              <span class="text-muted-foreground">{{ t('userSettings.totalBalance') }}</span>
              <span class="text-foreground">
                <template v-if="isUnlimitedBilling()">
                  {{ t('userSettings.unlimited') }}
                </template>
                <template v-else>
                  {{ formatCurrency(profile?.billing?.balance || 0) }}
                </template>
              </span>
            </div>
            <div class="flex justify-between text-sm">
              <span class="text-muted-foreground">{{ t('userSettings.rechargeBalance') }}</span>
              <span class="text-foreground">{{ formatCurrency(profile?.billing?.recharge_balance || 0) }}</span>
            </div>
            <div class="flex justify-between text-sm">
              <span class="text-muted-foreground">{{ t('userSettings.giftBalance') }}</span>
              <span class="text-foreground">{{ formatCurrency(profile?.billing?.gift_balance || 0) }}</span>
            </div>
            <div class="flex justify-between text-sm">
              <span class="text-muted-foreground">{{ t('userSettings.totalConsumed') }}</span>
              <span class="text-foreground">{{ formatCurrency(profile?.billing?.total_consumed || 0) }}</span>
            </div>

            <div v-if="!isUnlimitedBilling()">
              <div class="flex justify-between text-sm mb-1">
                <span class="text-muted-foreground">{{ t('userSettings.consumedPercent') }}</span>
                <span class="text-foreground">{{ getBillingUsagePercentage().toFixed(1) }}%</span>
              </div>
              <div class="w-full bg-muted rounded-full h-2.5">
                <div
                  class="bg-success h-2.5 rounded-full"
                  :style="`width: ${getBillingUsagePercentage()}%`"
                />
              </div>
            </div>
          </div>
        </Card>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { meApi, type Profile } from '@/api/me'
import { type UserSession, formatSessionMeta } from '@/types/session'
import { authApi } from '@/api/auth'
import { oauthApi, type OAuthLinkInfo, type OAuthProviderInfo } from '@/api/oauth'
import { getClientDeviceId } from '@/utils/deviceId'
import { getOAuthIcon } from '@/utils/oauth-icons'
import { useDarkMode, type ThemeMode } from '@/composables/useDarkMode'
import {
  getPasswordPolicyHint,
  getPasswordPolicyPlaceholder,
  normalizePasswordPolicyLevel,
  validatePasswordByPolicy,
  type PasswordPolicyLevel,
} from '@/utils/passwordPolicy'
import Card from '@/components/ui/card.vue'
import Button from '@/components/ui/button.vue'
import Badge from '@/components/ui/badge.vue'
import Input from '@/components/ui/input.vue'
import Label from '@/components/ui/label.vue'
import Textarea from '@/components/ui/textarea.vue'
import Select from '@/components/ui/select.vue'
import SelectTrigger from '@/components/ui/select-trigger.vue'
import SelectValue from '@/components/ui/select-value.vue'
import SelectContent from '@/components/ui/select-content.vue'
import SelectItem from '@/components/ui/select-item.vue'
import Switch from '@/components/ui/switch.vue'
import { useToast } from '@/composables/useToast'
import { formatWalletCurrency as formatCurrency } from '@/utils/walletDisplay'
import { getApiUrl } from '@/utils/url'
import { log } from '@/utils/logger'
import { getErrorMessage, getErrorStatus } from '@/types/api-error'
import {
  mergeChatPiiRedactionFeatureSettings,
  readChatPiiRedactionFeatureSettings,
} from '@/utils/featureSettings'

const authStore = useAuthStore()
const route = useRoute()
const router = useRouter()
const { success, error: showError } = useToast()
const { setThemeMode } = useDarkMode()
const { t, locale } = useI18n()
const translatePasswordPolicy = (key: string, params?: Record<string, string | number>) => t(key, params ?? {})

const profile = ref<Profile | null>(null)
const userSessions = ref<UserSession[]>([])
const profileRoleLabel = computed(() => {
  if (profile.value?.role === 'admin') return t('userSettings.roleAdmin')
  if (profile.value?.role === 'audit_admin') return t('userSettings.roleAuditAdmin')
  return t('userSettings.roleUser')
})

const profileForm = ref({
  email: '',
  username: ''
})

const passwordForm = ref({
  old_password: '',
  new_password: '',
  confirm_password: ''
})

const preferencesForm = ref({
  avatar_url: '',
  bio: '',
  theme: 'light',
  language: 'zh-CN',
  timezone: 'Asia/Shanghai',
  notifications: {
    email: true,
    usage_alerts: true,
    announcements: true
  }
})

const featureSettingsForm = ref({
  chatPiiRedactionEnabled: false,
  chatPiiRedactionInjectNotice: true,
})

const savingProfile = ref(false)
const savingFeatureSettings = ref(false)
const changingPassword = ref(false)
const sessionsLoading = ref(false)
const sessionActionLoading = ref<string | null>(null)
const editingSessionId = ref<string | null>(null)
const sessionLabelDraft = ref('')
const passwordPolicyLevel = ref<PasswordPolicyLevel>('weak')
const themeSelectOpen = ref(false)
const languageSelectOpen = ref(false)

const oauthUnavailable = ref(false)
const oauthActionLoading = ref(false)
const oauthLinks = ref<OAuthLinkInfo[]>([])
const bindableProviders = ref<OAuthProviderInfo[]>([])
const emailConfigured = ref(false) // 系统是否配置了邮箱服务

// 原始值，用于检测是否有修改
const originalProfileForm = ref({ email: '', username: '' })
const originalPreferencesForm = ref({ avatar_url: '', bio: '' })
const originalFeatureSettingsForm = ref({ ...featureSettingsForm.value })

// 检测基本信息是否有修改
const hasProfileChanges = computed(() => {
  return (
    profileForm.value.username !== originalProfileForm.value.username ||
    profileForm.value.email !== originalProfileForm.value.email ||
    preferencesForm.value.avatar_url !== originalPreferencesForm.value.avatar_url ||
    preferencesForm.value.bio !== originalPreferencesForm.value.bio
  )
})

const hasFeatureSettingsChanges = computed(() => {
  return (
    featureSettingsForm.value.chatPiiRedactionEnabled !== originalFeatureSettingsForm.value.chatPiiRedactionEnabled ||
    featureSettingsForm.value.chatPiiRedactionInjectNotice !== originalFeatureSettingsForm.value.chatPiiRedactionInjectNotice
  )
})

const passwordPolicyHint = computed(() => getPasswordPolicyHint(passwordPolicyLevel.value, translatePasswordPolicy))
const passwordError = computed(() =>
  validatePasswordByPolicy(passwordForm.value.new_password, passwordPolicyLevel.value, translatePasswordPolicy)
)

// 检测密码表单是否有内容
const hasPasswordChanges = computed(() => {
  const hasPassword = profile.value?.has_password
  if (hasPassword) {
    // 已有密码：需要填写旧密码和新密码
    return !!(passwordForm.value.old_password && passwordForm.value.new_password && passwordForm.value.confirm_password)
  } else {
    // 设置密码：只需要填写新密码
    return !!(passwordForm.value.new_password && passwordForm.value.confirm_password)
  }
})

const otherSessionCount = computed(() => userSessions.value.filter((session) => !session.is_current).length)

function handleThemeChange(value: string) {
  preferencesForm.value.theme = value
  themeSelectOpen.value = false
  updatePreferences()

  // 使用 useDarkMode 统一切换主题
  setThemeMode(value as ThemeMode)
}

function handleLanguageChange(value: string) {
  preferencesForm.value.language = value
  languageSelectOpen.value = false
  updatePreferences()
}

onMounted(async () => {
  await loadProfile()
  await Promise.all([
    loadPreferences(),
    loadSessions(),
    loadOAuthBindings(),
    loadEmailConfigured(),
  ])
})

async function loadEmailConfigured() {
  try {
    const settings = await authApi.getRegistrationSettings()
    emailConfigured.value = !!settings.email_configured
    passwordPolicyLevel.value = normalizePasswordPolicyLevel(settings.password_policy_level)
  } catch {
    emailConfigured.value = false
    passwordPolicyLevel.value = 'weak'
  }
}

async function loadProfile() {
  try {
    profile.value = await meApi.getProfile()
    profileForm.value = {
      email: profile.value.email || '',
      username: profile.value.username
    }
    const redactionFeature = readChatPiiRedactionFeatureSettings(profile.value.feature_settings)
    featureSettingsForm.value = {
      chatPiiRedactionEnabled: redactionFeature.enabled,
      chatPiiRedactionInjectNotice: redactionFeature.inject_model_instruction,
    }
    // 保存原始值
    originalProfileForm.value = { ...profileForm.value }
    originalFeatureSettingsForm.value = { ...featureSettingsForm.value }
  } catch (error) {
    log.error('加载个人信息失败:', error)
    showError(t('userSettings.loadProfileFailed'))
  }
}

async function updateFeatureSettings() {
  savingFeatureSettings.value = true
  try {
    await meApi.updateProfile({
      feature_settings: mergeChatPiiRedactionFeatureSettings(profile.value?.feature_settings, {
        enabled: featureSettingsForm.value.chatPiiRedactionEnabled,
        inject_model_instruction: featureSettingsForm.value.chatPiiRedactionInjectNotice,
      }),
    })
    if (profile.value) {
      profile.value.feature_settings = mergeChatPiiRedactionFeatureSettings(profile.value.feature_settings, {
        enabled: featureSettingsForm.value.chatPiiRedactionEnabled,
        inject_model_instruction: featureSettingsForm.value.chatPiiRedactionInjectNotice,
      })
    }
    originalFeatureSettingsForm.value = { ...featureSettingsForm.value }
    success(t('userSettings.privacySaved'))
  } catch (err) {
    log.error('更新敏感信息保护设置失败:', err)
    showError(getErrorMessage(err), t('userSettings.privacySaveFailed'))
  } finally {
    savingFeatureSettings.value = false
  }
}

async function loadSessions() {
  sessionsLoading.value = true
  try {
    userSessions.value = await meApi.listSessions()
    if (editingSessionId.value) {
      const currentEditing = userSessions.value.find((session) => session.id === editingSessionId.value)
      if (!currentEditing) {
        cancelSessionLabelEdit()
      }
    }
  } catch (error) {
    log.error('加载登录设备失败:', error)
  } finally {
    sessionsLoading.value = false
  }
}

async function loadOAuthBindings() {
  oauthUnavailable.value = false
  oauthLinks.value = []
  bindableProviders.value = []

  // profile 加载失败时跳过
  if (!profile.value) {
    oauthUnavailable.value = true
    return
  }

  // LDAP 用户不支持绑定
  if (profile.value.auth_source === 'ldap') {
    return
  }

  try {
    const [links, providers] = await Promise.all([
      oauthApi.getMyLinks(),
      oauthApi.getBindableProviders(),
    ])
    oauthLinks.value = links
    bindableProviders.value = providers
  } catch (err: unknown) {
    if (getErrorStatus(err) === 503) {
      oauthUnavailable.value = true
      return
    }
    log.error('加载 OAuth 绑定信息失败:', err)
    oauthUnavailable.value = true
  }
}

function handleBind(providerType: string) {
  // 保存返回路径（OAuth callback 会读取）
  sessionStorage.setItem('redirectPath', route.fullPath)

  // 先获取一次性绑定令牌，再在新标签页打开（避免在 URL 中暴露 access_token）
  oauthActionLoading.value = true
  oauthApi.createBindToken(providerType)
    .then((bindToken) => {
      // getApiUrl 可能返回相对路径，需要拼接完整 URL
      const basePath = getApiUrl(`/api/user/oauth/${providerType}/bind`)
      const bindUrl = basePath.startsWith('http')
        ? new URL(basePath)
        : new URL(basePath, window.location.origin)
      bindUrl.searchParams.set('bind_token', bindToken)
      bindUrl.searchParams.set('client_device_id', getClientDeviceId())

      // 新标签页打开 OAuth 流程
      const newTab = window.open(bindUrl.toString(), '_blank')

      // 监听标签页关闭，刷新绑定状态
      if (newTab) {
        const MAX_WAIT_MS = 10 * 60 * 1000 // 10 分钟超时
        const startTime = Date.now()
        const checkClosed = setInterval(() => {
          if (newTab.closed || Date.now() - startTime > MAX_WAIT_MS) {
            clearInterval(checkClosed)
            oauthActionLoading.value = false
            loadOAuthBindings()
          }
        }, 500)
      } else {
        // 被浏览器阻止，回退到当前页面跳转
        oauthActionLoading.value = false
        window.location.href = bindUrl.toString()
      }
    })
    .catch((err) => {
      oauthActionLoading.value = false
      showError(getErrorMessage(err, t('userSettings.bindTokenFailed')))
    })
}

async function handleUnbind(providerType: string) {
  oauthActionLoading.value = true
  try {
    await oauthApi.unbind(providerType)
    success(t('userSettings.unbindSuccess'))
    await loadOAuthBindings()
  } catch (err) {
    showError(getErrorMessage(err, t('userSettings.unbindFailed')))
  } finally {
    oauthActionLoading.value = false
  }
}

async function loadPreferences() {
  try {
    const prefs = await meApi.getPreferences()

    // 主题以本地 localStorage 为准（useDarkMode 在应用启动时已初始化）
    // 这样可以避免刷新页面时主题被服务端旧值覆盖
    const { themeMode: currentThemeMode } = useDarkMode()
    const localTheme = currentThemeMode.value

    preferencesForm.value = {
      avatar_url: prefs.avatar_url || '',
      bio: prefs.bio || '',
      theme: localTheme,  // 使用本地主题，而非服务端返回值
      language: prefs.language || 'zh-CN',
      timezone: prefs.timezone || 'Asia/Shanghai',
      notifications: {
        email: prefs.notifications?.email ?? true,
        usage_alerts: prefs.notifications?.usage_alerts ?? true,
        announcements: prefs.notifications?.announcements ?? true
      }
    }

    // 保存原始值
    originalPreferencesForm.value = {
      avatar_url: preferencesForm.value.avatar_url,
      bio: preferencesForm.value.bio
    }

    // 如果本地主题和服务端不一致，同步到服务端（静默更新，不提示用户）
    const serverTheme = prefs.theme || 'light'
    if (localTheme !== serverTheme) {
      meApi.updatePreferences({ theme: localTheme }).catch(() => {
        // 静默失败，不影响用户体验
      })
    }
  } catch (error) {
    log.error('加载偏好设置失败:', error)
  }
}

async function updateProfile() {
  savingProfile.value = true
  try {
    await meApi.updateProfile(profileForm.value)

    // 同时更新偏好设置中的 avatar_url 和 bio
    await meApi.updatePreferences({
      avatar_url: preferencesForm.value.avatar_url || undefined,
      bio: preferencesForm.value.bio || undefined,
      theme: preferencesForm.value.theme,
      language: preferencesForm.value.language,
      timezone: preferencesForm.value.timezone || undefined,
      notifications: {
        email: preferencesForm.value.notifications.email,
        usage_alerts: preferencesForm.value.notifications.usage_alerts,
        announcements: preferencesForm.value.notifications.announcements
      }
    })

    // 更新原始值
    originalProfileForm.value = { ...profileForm.value }
    originalPreferencesForm.value = {
      avatar_url: preferencesForm.value.avatar_url,
      bio: preferencesForm.value.bio
    }

    success(t('userSettings.profileUpdated'))
    authStore.fetchCurrentUser()
  } catch (err) {
    log.error('更新个人信息失败:', err)
    showError(getErrorMessage(err), t('userSettings.profileUpdateFailed'))
  } finally {
    savingProfile.value = false
  }
}

async function changePassword() {
  if (passwordForm.value.new_password !== passwordForm.value.confirm_password) {
    showError(t('userSettings.passwordsMismatch'), t('userSettings.passwordError'))
    return
  }

  if (passwordError.value) {
    showError(passwordError.value, t('userSettings.passwordError'))
    return
  }

  const isSettingPassword = !profile.value?.has_password
  changingPassword.value = true
  try {
    await meApi.changePassword({
      old_password: isSettingPassword ? undefined : passwordForm.value.old_password,
      new_password: passwordForm.value.new_password
    })
    success(isSettingPassword ? t('userSettings.passwordSetSuccess') : t('userSettings.passwordChangeSuccess'))
    await authStore.logout()
    await router.replace('/')
  } catch (err) {
    log.error('修改密码失败:', err)
    const title = isSettingPassword ? t('userSettings.passwordSetFailed') : t('userSettings.passwordChangeFailed')
    const defaultMsg = isSettingPassword ? t('userSettings.tryAgainLater') : t('userSettings.checkCurrentPassword')
    showError(getErrorMessage(err, defaultMsg), title)
  } finally {
    changingPassword.value = false
  }
}

function startSessionLabelEdit(session: UserSession) {
  editingSessionId.value = session.id
  sessionLabelDraft.value = session.device_label
}

function cancelSessionLabelEdit() {
  editingSessionId.value = null
  sessionLabelDraft.value = ''
}

async function saveSessionLabel(sessionId: string) {
  const nextLabel = sessionLabelDraft.value.trim()
  if (!nextLabel) {
    showError(t('userSettings.deviceNameRequired'))
    return
  }

  sessionActionLoading.value = sessionId
  try {
    const updated = await meApi.updateSessionLabel(sessionId, nextLabel)
    userSessions.value = userSessions.value.map((session) =>
      session.id === sessionId ? updated : session
    )
    cancelSessionLabelEdit()
    success(t('userSettings.deviceNameUpdated'))
  } catch (error) {
    log.error('更新设备名称失败:', error)
    showError(getErrorMessage(error, t('userSettings.deviceNameUpdateFailed')))
  } finally {
    sessionActionLoading.value = null
  }
}

async function handleRevokeSession(sessionId: string) {
  sessionActionLoading.value = sessionId
  try {
    await meApi.revokeSession(sessionId)
    if (editingSessionId.value === sessionId) {
      cancelSessionLabelEdit()
    }
    success(t('userSettings.deviceSignedOut'))
    await loadSessions()
  } catch (error) {
    log.error('退出设备失败:', error)
    showError(getErrorMessage(error, t('userSettings.deviceSignOutFailed')))
  } finally {
    sessionActionLoading.value = null
  }
}

async function handleRevokeOtherSessions() {
  sessionActionLoading.value = 'others'
  try {
    const result = await meApi.revokeOtherSessions()
    success(result.revoked_count > 0
      ? t('userSettings.otherDevicesSignedOut', { count: result.revoked_count })
      : t('userSettings.noOtherDevices'))
    await loadSessions()
  } catch (error) {
    log.error('退出其他设备失败:', error)
    showError(getErrorMessage(error, t('userSettings.otherDevicesSignOutFailed')))
  } finally {
    sessionActionLoading.value = null
  }
}

async function updatePreferences() {
  try {
    await meApi.updatePreferences({
      avatar_url: preferencesForm.value.avatar_url || undefined,
      bio: preferencesForm.value.bio || undefined,
      theme: preferencesForm.value.theme,
      language: preferencesForm.value.language,
      timezone: preferencesForm.value.timezone || undefined,
      notifications: {
        email: preferencesForm.value.notifications.email,
        usage_alerts: preferencesForm.value.notifications.usage_alerts,
        announcements: preferencesForm.value.notifications.announcements
      }
    })
    success(t('userSettings.settingsSaved'))
  } catch (error) {
    log.error('更新偏好设置失败:', error)
    showError(t('userSettings.settingsSaveFailed'))
  }
}

function getBillingUsagePercentage(): number {
  const billing = profile.value?.billing
  if (!billing) return 0
  const consumed = billing.total_consumed || 0
  const denominator = consumed + (billing.balance || 0)
  if (denominator <= 0) return 0
  return Math.min(100, (consumed / denominator) * 100)
}

function isUnlimitedBilling(): boolean {
  return profile.value?.billing?.unlimited === true
}

function formatDate(dateString?: string): string {
  if (!dateString) return t('userSettings.notRecorded')
  return new Date(dateString).toLocaleDateString(locale.value, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit'
  })
}
</script>

<style scoped>
.oauth-icon {
  width: 24px;
  height: 24px;
}

.oauth-icon :deep(svg) {
  width: 100%;
  height: 100%;
}
</style>
