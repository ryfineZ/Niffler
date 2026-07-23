<template>
  <PageContainer>
    <PageHeader
      title="模型后缀参数"
      description="允许通过模型名后缀覆盖推理参数或服务层级"
    >
      <template #actions>
        <div class="flex flex-wrap items-center justify-end gap-3">
          <div class="flex h-9 items-center gap-2 rounded-md border px-3">
            <span class="text-sm text-muted-foreground">模块状态</span>
            <Switch
              :model-value="moduleEnabled"
              :disabled="loading || moduleSaving"
              aria-label="启用模型后缀参数模块"
              @update:model-value="setModuleEnabled"
            />
          </div>
          <Button
            variant="outline"
            :disabled="loading || saving || moduleSaving"
            @click="loadConfig"
          >
            <RefreshCw
              class="w-4 h-4 mr-2"
              :class="{ 'animate-spin': loading }"
            />
            刷新
          </Button>
        </div>
      </template>
    </PageHeader>

    <div class="mt-6 space-y-5">
      <Card
        variant="default"
        class="p-6"
      >
        <ModelDirectivesPanel
          :config="modelDirectivesConfig"
          :loading="loading || saving || moduleSaving"
          @save="saveConfig"
        />
      </Card>
    </div>
  </PageContainer>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { RefreshCw } from 'lucide-vue-next'
import Button from '@/components/ui/button.vue'
import Card from '@/components/ui/card.vue'
import Switch from '@/components/ui/switch.vue'
import { PageContainer, PageHeader } from '@/components/layout'
import { adminApi } from '@/api/admin'
import { useModuleStore } from '@/stores/modules'
import { useToast } from '@/composables/useToast'
import { log } from '@/utils/logger'
import { getErrorMessage } from '@/types/api-error'
import ModelDirectivesPanel from './module-management/ModelDirectivesPanel.vue'
import {
  createDefaultModelDirectivesConfig,
  normalizeModelDirectivesConfig,
  type ModelDirectivesConfig,
} from './module-management/modelDirectivesConfig'

const { success, error } = useToast()
const moduleStore = useModuleStore()

const modelDirectivesConfig = ref<ModelDirectivesConfig>(createDefaultModelDirectivesConfig())
const loading = ref(false)
const saving = ref(false)
const moduleSaving = ref(false)
const moduleEnabled = ref(false)

async function loadConfig() {
  loading.value = true
  try {
    const [configResult, modulesResult] = await Promise.allSettled([
      adminApi.getSystemConfig('model_directives'),
      moduleStore.fetchModules(),
    ])
    if (modulesResult.status === 'fulfilled') {
      moduleEnabled.value = modulesResult.value.model_directives?.enabled === true
    } else {
      error('获取模型后缀参数模块状态失败')
      log.error('获取模型后缀参数模块状态失败:', modulesResult.reason)
    }
    if (configResult.status === 'rejected') throw configResult.reason
    const normalized = normalizeModelDirectivesConfig(configResult.value.value)
    modelDirectivesConfig.value = normalized
  } catch (err) {
    error('获取模型后缀参数配置失败')
    log.error('获取模型后缀参数配置失败:', err)
  } finally {
    loading.value = false
  }
}

async function saveConfig(nextConfig: ModelDirectivesConfig) {
  saving.value = true
  try {
    const normalized = normalizeModelDirectivesConfig(nextConfig)
    await adminApi.updateSystemConfig(
      'model_directives',
      normalized,
      '模型后缀参数配置'
    )
    modelDirectivesConfig.value = normalized
    success('模型后缀参数配置已保存')
  } catch (err) {
    error(getErrorMessage(err, '保存模型后缀参数配置失败'))
    log.error('保存模型后缀参数配置失败:', err)
  } finally {
    saving.value = false
  }
}

async function setModuleEnabled(value: boolean) {
  moduleSaving.value = true
  try {
    await moduleStore.setEnabled('model_directives', Boolean(value))
    moduleEnabled.value = moduleStore.modules.model_directives?.enabled === true
    success(moduleEnabled.value ? '模型后缀参数模块已启用' : '模型后缀参数模块已禁用')
  } catch (err) {
    error(getErrorMessage(err, '更新模型后缀参数模块状态失败'))
    log.error('更新模型后缀参数模块状态失败:', err)
  } finally {
    moduleSaving.value = false
  }
}

onMounted(() => {
  loadConfig()
})
</script>
