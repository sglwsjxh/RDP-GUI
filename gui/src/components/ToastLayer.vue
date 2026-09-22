<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script setup lang="ts">
import { ref, onMounted, onUnmounted, provide } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { ToastEvent } from '../types/events'

interface Toast {
  id: number
  message: string
  type: 'info' | 'success' | 'warning' | 'error'
}
const toasts = ref<Toast[]>([])
let idCounter = 0

const addToast = (message: string, type: Toast['type'] = 'info') => {
  const id = ++idCounter
  toasts.value.push({ id, message, type })
  setTimeout(() => removeToast(id), 3000)
}

const removeToast = (id: number) => {
  const idx = toasts.value.findIndex(t => t.id === id)
  if (idx >= 0) toasts.value.splice(idx, 1)
}

const toastSymbol = Symbol('toast')
const provideToast = () => ({ addToast, removeToast })
provide(toastSymbol, provideToast())

let unlisten: UnlistenFn | null = null

onMounted(async () => {
  unlisten = await listen<ToastEvent>('toast', (e) => {
    addToast(e.payload.message, e.payload.type)
  })
})
onUnmounted(() => {
  unlisten?.()
  unlisten = null
})
</script>

<template>
  <div class="toast-layer" aria-live="polite">
    <div class="toast" v-for="t in toasts" :key="t.id" :class="t.type">
      {{ t.message }}
    </div>
  </div>
</template>

<style scoped>
.toast-layer {
  position: fixed;
  bottom: 16px;
  right: 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  z-index: 2000;
  pointer-events: none;
}
.toast {
  pointer-events: auto;
  padding: 10px 16px;
  border-radius: 6px;
  font-size: 13px;
  max-width: 320px;
  animation: slideIn 0.2s ease;
  box-shadow: 0 4px 12px rgba(0,0,0,0.15);
}
.toast.info {
  background: var(--status-info-bg);
  color: var(--status-info-fg);
}
.toast.success {
  background: var(--status-ok-bg);
  color: var(--status-ok-fg);
}
.toast.warning {
  background: var(--status-warn-bg);
  color: var(--status-warn-fg);
}
.toast.error {
  background: var(--status-error-bg);
  color: var(--status-error-fg);
}
@keyframes slideIn {
  from { opacity: 0; transform: translateX(20px) }
  to { opacity: 1; transform: translateX(0) }
}
</style>
