<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script setup lang="ts">
import { ref } from 'vue'
import { runSelftest } from '../../commands/selftest'
import type { SelftestResult } from '../../commands/selftest'

interface Emits {
  close: []
}
const emit = defineEmits<Emits>()

const running = ref(false)
const result = ref<SelftestResult | null>(null)
const error = ref('')

const run = async () => {
  if (running.value) return
  running.value = true
  result.value = null
  error.value = ''
  try {
    result.value = await runSelftest()
  } catch (e) {
    error.value = String(e)
  } finally {
    running.value = false
  }
}
</script>

<template>
  <div class="dialog-selftest">
    <section class="card">
      <h4 class="card-title">环境自检</h4>
      <div v-if="running" class="status">正在运行自检…</div>
      <div v-else-if="error" class="status err">自检失败：{{ error }}</div>
      <div v-else-if="!result" class="status muted">尚未运行</div>
      <template v-else>
        <ul class="checks">
          <li v-for="(t, i) in result.tests" :key="i" class="check">
            <span class="num">{{ String(i + 1).padStart(2, '0') }}</span>
            <span :class="['badge', t.passed ? 'pass' : 'fail']">{{ t.passed ? 'PASS' : 'FAIL' }}</span>
            <span class="name">{{ t.name }}</span>
            <span v-if="t.message" class="msg">{{ t.message }}</span>
          </li>
        </ul>
        <div class="summary">
          汇总：{{ result.tests.filter((t) => t.passed).length }} / {{ result.tests.length }} 通过
          <span :class="['summary-badge', result.passed ? 'pass' : 'fail']">
            退出码 {{ result.passed ? 0 : 1 }}
          </span>
        </div>
      </template>
    </section>

    <footer class="actions">
      <button class="btn" @click="emit('close')">关闭</button>
      <button class="btn primary" :disabled="running" @click="run">运行</button>
    </footer>
  </div>
</template>

<style scoped>
.dialog-selftest {
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
.status {
  font-size: 13px;
}
.status.err {
  color: #e5484d;
}
.status.muted {
  color: var(--text-secondary);
}
.checks {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.check {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}
.num {
  flex: 0 0 20px;
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
}
.name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.msg {
  color: var(--text-secondary);
  font-size: 12px;
}
.badge {
  flex: 0 0 auto;
  font-size: 11px;
  font-weight: 700;
  border-radius: 3px;
  padding: 1px 6px;
}
.badge.pass {
  color: #2da44e;
  background: rgba(45, 164, 78, 0.15);
}
.badge.fail {
  color: #e5484d;
  background: rgba(229, 72, 77, 0.15);
}
.summary {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}
.summary-badge {
  font-size: 11px;
  font-weight: 700;
  border-radius: 3px;
  padding: 1px 6px;
}
.summary-badge.pass {
  color: #2da44e;
  background: rgba(45, 164, 78, 0.15);
}
.summary-badge.fail {
  color: #e5484d;
  background: rgba(229, 72, 77, 0.15);
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
.btn:hover:not(:disabled) {
  background: var(--bg-hover);
}
.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.btn.primary {
  background: var(--accent-primary);
  border-color: var(--accent-primary);
  color: #fff;
}
</style>
