<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useViewerRect } from '../composables/useInvokeRect'

const rootEl = ref<HTMLElement | null>(null)

onMounted(() => {
  if (rootEl.value) useViewerRect(rootEl.value)
})
</script>

<template>
  <div ref="rootEl" class="viewer-placeholder">
    <div class="placeholder-content">
      <span class="icon">🖥</span>
      <p>RDP 查看器占位</p>
      <p class="hint">连接后将显示远程桌面</p>
    </div>
  </div>
</template>

<style scoped>
.viewer-placeholder {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-tertiary);
}
.placeholder-content {
  text-align: center;
  color: var(--text-secondary);
}
.icon {
  font-size: 48px;
  display: block;
  margin-bottom: 12px;
}
.hint {
  font-size: 12px;
  margin-top: 8px;
}
</style>
