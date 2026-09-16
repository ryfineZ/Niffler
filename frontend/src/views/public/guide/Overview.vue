<template>
  <div>
    <div class="guide-eyebrow">{{ t('guide.start.eyebrow') }}</div>
    <h1 class="mt-4">{{ t('guide.start.title') }}</h1>
    <p class="mt-5 max-w-3xl text-lg">{{ t('guide.start.subtitle') }}</p>

    <section class="mt-9 border border-primary/25 bg-primary/[0.05] p-5 sm:p-7">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <h2 class="!m-0 !text-2xl">{{ t('guide.start.beforeTitle') }}</h2>
        <span class="inline-flex items-center gap-2 border border-primary/25 bg-background/70 px-3 py-1.5 text-xs font-semibold text-primary">
          <Clock3 class="h-3.5 w-3.5" />{{ t('guide.start.estimatedTime') }}
        </span>
      </div>
      <p class="mt-3 text-sm">{{ t('guide.start.beforeDesc') }}</p>
      <div class="mt-5 grid gap-3 sm:grid-cols-2">
        <div v-for="item in readinessItems" :key="item.title" class="flex gap-3 border border-border/70 bg-background/65 p-4">
          <CheckCircle2 class="mt-0.5 h-4 w-4 shrink-0 text-emerald-500" />
          <div><strong class="text-sm">{{ item.title }}</strong><p class="mt-1 text-xs">{{ item.description }}</p></div>
        </div>
      </div>
    </section>

    <section id="quick-start-path">
      <h2>{{ t('guide.start.pathTitle') }}</h2>
      <p>{{ t('guide.start.pathDesc') }}</p>
      <div class="mt-5 space-y-3">
        <article v-for="(step, index) in steps" :key="step.title" class="flex flex-col gap-4 border border-border/80 bg-background/70 p-5 sm:flex-row sm:items-start">
          <span class="flex h-9 w-9 shrink-0 items-center justify-center bg-primary font-mono text-xs font-bold text-primary-foreground">{{ index + 1 }}</span>
          <div class="min-w-0 flex-1">
            <h3 class="!m-0">{{ step.title }}</h3>
            <p class="mt-2 text-sm">{{ step.description }}</p>
          </div>
          <RouterLink :to="step.to" class="inline-flex shrink-0 items-center gap-1.5 self-start border border-primary/35 px-3 py-2 text-xs font-semibold text-primary transition hover:bg-primary/10">
            {{ step.action }}<ArrowUpRight class="h-3.5 w-3.5" />
          </RouterLink>
        </article>
      </div>
    </section>

    <section id="connection-details">
      <h2>{{ t('guide.start.connectionTitle') }}</h2>
      <p>{{ t('guide.start.connectionDesc') }}</p>
      <div class="mt-5 overflow-hidden border border-border/80 bg-background/70">
        <div v-for="item in connectionItems" :key="item.label" class="grid gap-1 border-b border-border/60 px-4 py-4 last:border-b-0 sm:grid-cols-[140px_minmax(0,1fr)] sm:gap-4">
          <strong class="text-sm">{{ item.label }}</strong>
          <code class="min-w-0 break-all text-sm text-primary">{{ item.value }}</code>
        </div>
      </div>
      <p class="mt-3 text-xs">{{ t('guide.start.connectionHint', { baseUrl: apiBaseUrl }) }}</p>
    </section>

    <section>
      <h2>{{ t('guide.start.firstCall') }}</h2>
      <p>{{ t('guide.start.firstCallDesc') }}</p>
      <CustomerCodeBlock label="cURL" :code="curlExample" />
      <p class="text-sm">{{ t('guide.start.pythonHint') }}</p>
      <CustomerCodeBlock label="Python · OpenAI SDK" :code="pythonExample" />
      <div class="border-l-4 border-primary bg-primary/5 p-5 text-sm leading-7 text-muted-foreground">
        <strong class="text-foreground">{{ t('guide.start.keepSafeTitle') }}</strong><br>
        {{ t('guide.start.keepSafeDesc') }}
      </div>
    </section>

    <section>
      <h2>{{ t('guide.start.troubleshootTitle') }}</h2>
      <p>{{ t('guide.start.troubleshootDesc') }}</p>
      <div class="mt-5 grid gap-3 sm:grid-cols-2">
        <div v-for="item in troubleshootItems" :key="item.title" class="flex gap-3 border border-border/70 bg-background/65 p-4">
          <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0 text-amber-500" />
          <div><strong class="text-sm">{{ item.title }}</strong><p class="mt-1 text-xs">{{ item.description }}</p></div>
        </div>
      </div>
    </section>

    <section>
      <h2>{{ t('guide.start.whatNext') }}</h2>
      <div class="grid gap-3 sm:grid-cols-3">
        <RouterLink to="/guide/models" class="next-card"><Library class="h-5 w-5 text-primary" />{{ t('guide.nav.models') }}</RouterLink>
        <RouterLink to="/guide/clients" class="next-card"><Terminal class="h-5 w-5 text-primary" />{{ t('guide.nav.clients') }}</RouterLink>
        <RouterLink to="/guide/usage-billing" class="next-card"><CircleDollarSign class="h-5 w-5 text-primary" />{{ t('guide.nav.billing') }}</RouterLink>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import { AlertTriangle, ArrowUpRight, CheckCircle2, CircleDollarSign, Clock3, Library, Terminal } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { usePortalBaseUrl } from '@/composables/usePortalBaseUrl'
import CustomerCodeBlock from './components/CustomerCodeBlock.vue'

const { t } = useI18n()
const { apiBaseUrl } = usePortalBaseUrl()
const readinessItems = computed(() => [
  { title: t('guide.start.readyAccount'), description: t('guide.start.readyAccountDesc') },
  { title: t('guide.start.readyBalance'), description: t('guide.start.readyBalanceDesc') },
  { title: t('guide.start.readyModel'), description: t('guide.start.readyModelDesc') },
  { title: t('guide.start.readyTool'), description: t('guide.start.readyToolDesc') },
])
const steps = computed(() => [
  { title: t('guide.start.stepAccount'), description: t('guide.start.stepAccountDesc'), action: t('guide.start.openDashboard'), to: '/dashboard' },
  { title: t('guide.start.stepBalance'), description: t('guide.start.stepBalanceDesc'), action: t('guide.start.openWallet'), to: '/dashboard/wallet' },
  { title: t('guide.start.stepKey'), description: t('guide.start.stepKeyDesc'), action: t('guide.start.openKeys'), to: '/dashboard/api-keys' },
  { title: t('guide.start.stepModel'), description: t('guide.start.stepModelDesc'), action: t('guide.start.openModels'), to: '/models' },
])
const connectionItems = computed(() => [
  { label: t('guide.start.baseUrl'), value: apiBaseUrl.value },
  { label: t('guide.start.apiKey'), value: 'YOUR_NIFFLER_KEY' },
  { label: t('guide.start.modelId'), value: 'YOUR_MODEL_ID' },
])
const troubleshootItems = computed(() => [
  { title: t('guide.start.checkKey'), description: t('guide.start.checkKeyDesc') },
  { title: t('guide.start.checkBalance'), description: t('guide.start.checkBalanceDesc') },
  { title: t('guide.start.checkModel'), description: t('guide.start.checkModelDesc') },
  { title: t('guide.start.checkUrl'), description: t('guide.start.checkUrlDesc', { baseUrl: apiBaseUrl.value }) },
])
const curlExample = computed(() => `curl ${apiBaseUrl.value}/chat/completions -H "Authorization: Bearer YOUR_NIFFLER_KEY" -H "Content-Type: application/json" -d '{"model":"YOUR_MODEL_ID","messages":[{"role":"user","content":"用一句话介绍 Niffler"}]}'`)
const pythonExample = computed(() => `from openai import OpenAI

client = OpenAI(
    api_key="YOUR_NIFFLER_KEY",
    base_url="${apiBaseUrl.value}"
)

response = client.chat.completions.create(
    model="YOUR_MODEL_ID",
    messages=[{"role": "user", "content": "用一句话介绍 Niffler"}]
)

print(response.choices[0].message.content)`)
</script>

<style scoped>
.guide-eyebrow { color: hsl(var(--primary)); font-size: 11px; font-weight: 700; letter-spacing: 0.2em; text-transform: uppercase; }
.next-card { display: flex; align-items: center; gap: 0.75rem; border: 1px solid hsl(var(--border) / 0.8); background: hsl(var(--background) / 0.7); padding: 1rem; font-size: 0.875rem; font-weight: 600; transition: color 150ms ease, border-color 150ms ease; }
.next-card:hover { color: hsl(var(--primary)); border-color: hsl(var(--primary) / 0.5); }
</style>
