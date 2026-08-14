<script setup lang="ts">
import { cn } from '@/lib/utils'
import { computed } from 'vue'
import type { ClassValue } from 'clsx'

interface Props {
  class?: ClassValue
}

const props = defineProps<Props>()

const tableClass = computed(() =>
  cn('w-full caption-bottom text-sm', props.class)
)
</script>

<template>
  <div class="relative w-full overflow-auto">
    <table :class="tableClass">
      <slot />
    </table>
  </div>
</template>
