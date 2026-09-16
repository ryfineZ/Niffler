<template>
  <div>
    <div class="guide-eyebrow">{{ t('guide.faq.eyebrow') }}</div>
    <h1 class="mt-4">{{ t('guide.faq.title') }}</h1>
    <p class="mt-5 max-w-3xl text-lg">{{ t('guide.faq.subtitle') }}</p>
    <div class="mt-9 space-y-3">
      <div v-for="faq in faqs" :key="faq.id" class="border border-border/80 bg-background/70">
        <button class="flex w-full items-center justify-between gap-4 p-5 text-left" @click="openId = openId === faq.id ? '' : faq.id">
          <span class="font-semibold">{{ faq.question }}</span><ChevronDown class="h-4 w-4 shrink-0 transition" :class="openId === faq.id ? 'rotate-180 text-primary' : 'text-muted-foreground'" />
        </button>
        <div v-if="openId === faq.id" class="prose prose-sm dark:prose-invert max-w-none border-t border-border/60 px-5 py-5 text-muted-foreground" v-html="faq.answer"></div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { ChevronDown } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { marked } from 'marked'
import { usePortalBaseUrl } from '@/composables/usePortalBaseUrl'
import { sanitizeMarkdown } from '@/utils/sanitize'
const { t } = useI18n()
const { apiBaseUrl } = usePortalBaseUrl()
const openId = ref('')
const fallbackFaqs = computed(() => [1, 2, 3, 4, 5, 6].map(index => ({ id: `default-${index}`, question: t(`guide.faq.q${index}`), answer: sanitizeMarkdown(marked.parse(t(`guide.faq.a${index}`, { baseUrl: apiBaseUrl.value })) as string) })))
const faqs = fallbackFaqs
if (faqs.value.length) openId.value = faqs.value[0].id
</script>

<style scoped>.guide-eyebrow { color: hsl(var(--primary)); font-size: 11px; font-weight: 700; letter-spacing: 0.2em; text-transform: uppercase; }</style>
