<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script setup lang="ts">
interface Props {
  label: string
  variant: 'ok' | 'warn' | 'error' | 'info'
}
const props = defineProps<Props>()
const variantClass = `pill-${props.variant}`
</script>

<template>
  <span :class="['status-pill', variantClass]">{{ label }}</span>
</template>

<style scoped>
.status-pill {
  display: inline-flex;
  align-items: center;
  padding: 2px 8px;
  border-radius: 12px;
  font-size: 11px;
  font-weight: 500;
}
.pill-ok {
  background: var(--status-ok-bg);
  color: var(--status-ok-fg);
}
.pill-warn {
  background: var(--status-warn-bg);
  color: var(--status-warn-fg);
}
.pill-error {
  background: var(--status-error-bg);
  color: var(--status-error-fg);
}
.pill-info {
  background: var(--status-info-bg);
  color: var(--status-info-fg);
}
</style>
