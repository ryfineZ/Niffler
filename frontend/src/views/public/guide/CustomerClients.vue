<template>
  <div>
    <div class="guide-eyebrow">{{ t('guide.clients.eyebrow') }}</div>
    <h1 class="mt-4">{{ t('guide.clients.title') }}</h1>
    <p class="mt-5 max-w-3xl text-lg">{{ t('guide.clients.subtitle') }}</p>

    <div class="mt-8 flex flex-wrap gap-2">
      <button v-for="client in clients" :key="client.id" class="border px-4 py-2 text-sm font-medium" :class="activeClient === client.id ? 'border-primary bg-primary/10 text-primary' : 'border-border bg-background/70'" @click="activeClient = client.id">{{ client.name }}</button>
    </div>

    <section class="mt-8 border border-border/80 bg-background/70 p-5 sm:p-7">
      <div class="flex items-center gap-3"><component :is="active.icon" class="h-6 w-6 text-primary" /><h2 class="!m-0">{{ active.name }}</h2></div>
      <p class="mt-4">{{ active.description }}</p>
      <div v-for="block in active.blocks" :key="block.label">
        <h3>{{ block.label }}</h3>
        <CustomerCodeBlock :label="block.file" :code="block.code" />
      </div>
    </section>

    <section>
      <h2>{{ t('guide.clients.securityTitle') }}</h2>
      <div class="border-l-4 border-amber-500 bg-amber-500/5 p-5 text-sm leading-7 text-muted-foreground">
        <strong class="text-foreground">{{ t('guide.clients.securityLead') }}</strong><br>{{ t('guide.clients.securityDesc') }}
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { Bot, Braces, Terminal } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { usePortalBaseUrl } from '@/composables/usePortalBaseUrl'
import CustomerCodeBlock from './components/CustomerCodeBlock.vue'
type ClientId = 'openai' | 'claude' | 'codex' | 'gemini'
const { t } = useI18n()
const { portalOrigin, apiBaseUrl } = usePortalBaseUrl()
const activeClient = ref<ClientId>('openai')
const clients = computed(() => [
  { id: 'openai' as const, name: 'OpenAI SDK', icon: Braces, description: t('guide.clients.openaiDesc'), blocks: [{ label: t('guide.clients.python'), file: 'app.py', code: `from openai import OpenAI

client = OpenAI(
    api_key="YOUR_NIFFLER_KEY",
    base_url="${apiBaseUrl.value}"
)

response = client.chat.completions.create(
    model="gpt-5.4",
    messages=[{"role": "user", "content": "Hello"}]
)` }] },
  { id: 'claude' as const, name: 'Claude Code', icon: Bot, description: t('guide.clients.claudeDesc'), blocks: [{ label: t('guide.clients.environment'), file: '~/.claude/settings.json', code: `{
  "env": {
    "ANTHROPIC_AUTH_TOKEN": "YOUR_NIFFLER_KEY",
    "ANTHROPIC_BASE_URL": "${portalOrigin.value}"
  }
}` }, { label: t('guide.clients.launch'), file: 'Terminal', code: 'claude' }] },
  { id: 'codex' as const, name: 'Codex CLI', icon: Terminal, description: t('guide.clients.codexDesc'), blocks: [{ label: t('guide.clients.providerConfig'), file: '~/.codex/config.toml', code: `model_provider = "niffler"
model = "gpt-5.4"
model_reasoning_effort = "high"

[model_providers.niffler]
name = "Niffler"
base_url = "${apiBaseUrl.value}"
wire_api = "responses"
requires_openai_auth = true` }, { label: t('guide.clients.authConfig'), file: '~/.codex/auth.json', code: `{
  "OPENAI_API_KEY": "YOUR_NIFFLER_KEY"
}` }] },
  { id: 'gemini' as const, name: 'Gemini CLI', icon: Bot, description: t('guide.clients.geminiDesc'), blocks: [{ label: t('guide.clients.environment'), file: '~/.gemini/.env', code: `GEMINI_API_KEY=YOUR_NIFFLER_KEY
GOOGLE_GEMINI_BASE_URL=${portalOrigin.value}
GEMINI_MODEL=gemini-3-pro` }, { label: t('guide.clients.settings'), file: '~/.gemini/settings.json', code: `{
  "security": {
    "auth": { "selectedType": "gemini-api-key" }
  }
}` }] },
])
const active = computed(() => clients.value.find(client => client.id === activeClient.value) || clients.value[0])
</script>

<style scoped>.guide-eyebrow { color: hsl(var(--primary)); font-size: 11px; font-weight: 700; letter-spacing: 0.2em; text-transform: uppercase; }</style>
