<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script setup lang="ts">
interface Props {
  title: string
  message: string
  confirmText?: string
  cancelText?: string
  variant?: 'danger' | 'warning' | 'info'
}
const props = withDefaults(defineProps<Props>(), {
  confirmText: '确定',
  cancelText: '取消',
  variant: 'info',
})

interface Emits {
  confirm: []
  close: []
}
const emit = defineEmits<Emits>()
</script>

<template>
  <div class="dialog-confirm">
    <section class="card">
      <h4 class="card-title">{{ props.title }}</h4>
      <p class="message">{{ props.message }}</p>
    </section>
    <footer class="actions">
      <button class="btn" @click="emit('close')">{{ props.cancelText }}</button>
      <button class="btn" :class="{ primary: props.variant !== 'danger', danger: props.variant === 'danger' }" @click="emit('confirm')">{{ props.confirmText }}</button>
    </footer>
  </div>
</template>

<style scoped>
.dialog-confirm {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.card {
  border: 1px solid var(--border-primary);
  border-radius: 8px;
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.card-title {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
}
.message {
  margin: 0;
  font-size: 13px;
  color: var(--text-primary);
  line-height: 1.5;
  white-space: pre-line;
}
.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.btn {
  padding: 6px 14px;
  background: var(--bg-primary);
  border: 1px solid var(--border-primary);
  border-radius: 4px;
  color: var(--text-primary);
  cursor: pointer;
  font-size: 13px;
}
.btn:hover {
  background: var(--bg-hover);
}
.btn.primary {
  background: var(--accent-primary);
  border-color: var(--accent-primary);
  color: #fff;
}
.btn.danger {
  background: var(--status-error-fg);
  border-color: var(--status-error-fg);
  color: #fff;
}
</style>
