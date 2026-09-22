<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script setup lang="ts">
import { ref } from 'vue'

const emit = defineEmits<{
  confirm: [password: string]
  close: []
}>()

const password = ref('')

function submit() {
  emit('confirm', password.value)
}
</script>

<template>
  <div class="dialog-password">
    <h2>AkiSpace — 分身账户密码</h2>
    <p class="hint">标准 RDP 模式需要分身账户的密码。
AkiSpace 不再内置默认密码，请输入分身账户的密码
（将使用 DPAPI 加密保存在本机设置中）：</p>
    <input type="password" v-model="password" class="input" @keyup.enter="submit" />
    <div class="actions">
      <button class="btn secondary" @click="emit('close')">取消</button>
      <button class="btn primary" @click="submit">确定</button>
    </div>
  </div>
</template>

<style scoped>
.dialog-password {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 8px 0;
}
.dialog-password h2 {
  margin: 0;
  font-size: 15px;
  color: var(--text-primary);
}
.hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary);
  white-space: pre-line;
  line-height: 1.5;
}
.input {
  padding: 8px 12px;
  border: 1px solid var(--border-primary);
  border-radius: 4px;
  background: var(--bg-tertiary);
  color: var(--text-primary);
  font-size: 13px;
}
.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}
.btn {
  padding: 6px 16px;
  border: none;
  border-radius: 4px;
  font-size: 13px;
  cursor: pointer;
}
.btn.primary {
  background: var(--accent-primary);
  color: white;
}
.btn.secondary {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}
</style>
