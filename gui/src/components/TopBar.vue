<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script setup lang="ts">
import StatusPill from './StatusPill.vue'
import { computed } from 'vue'
import { useTheme } from '../composables/useTheme'
import { useConnection } from '../composables/useConnection'
import { useStatusTick } from '../composables/useStatusTick'

type ThemeName = 'light' | 'dark' | 'sepia' | 'high-contrast' | 'blue-light' | 'custom'

const theme = useTheme()
const connection = useConnection()
const tick = useStatusTick()

const emit = defineEmits<{
  'open-settings': []
  'open-env-check': []
  'open-selftest': []
  'terminate': []
  'game-mouse-toggle': []
  'launch-program': []
}>()

const handleConnect = () => connection.connect({})
const handleDisconnect = () => connection.disconnect()
const handleTerminate = () => emit('terminate')
const handleGameMouseToggle = () => emit('game-mouse-toggle')
const handleLaunchProgram = () => emit('launch-program')

const connectDisabled = computed(() => connection.state.value.status === 'connecting')
const disconnectDisabled = computed(() => connection.state.value.status !== 'connected')
const gameMouseDisabled = computed(() => connection.state.value.status === 'disconnected')
const launchProgramDisabled = computed(() => connection.state.value.status === 'disconnected')
const terminateDisabled = computed(() => connection.state.value.status === 'disconnected')

const themes = theme.themes
const currentTheme = computed(() => theme.current.value)

const handleThemeChange = (e: Event) => {
  const target = e.target as HTMLSelectElement
  theme.set(target.value as ThemeName)
}

// 状态胶囊（文案逐字对齐 akispace/Forms/MainForm.cs）
const childSessionPill = computed(() => {
  const s = connection.state.value.status
  if (s === 'connected') return { label: '子会话: 活动', variant: 'ok' as const }
  if (s === 'connecting') return { label: '子会话: 检测中', variant: 'info' as const }
  return { label: '子会话: 无', variant: 'warn' as const }
})

const connectionPill = computed(() => {
  const s = connection.state.value.status
  if (s === 'connected') return { label: '连接: 已连接', variant: 'ok' as const }
  if (s === 'connecting') return { label: `连接: 正在连接 ${connection.state.value.host || '127.0.0.1'}:${connection.state.value.port} ...`, variant: 'info' as const }
  if (s === 'error') return { label: '连接: 连接失败', variant: 'error' as const }
  return { label: '连接: 未连接', variant: 'warn' as const }
})

const rdpUnlockPill = computed(() => {
  const s = connection.state.value.status
  if (s === 'connected') return { label: 'RDP 解锁: 已安装', variant: 'ok' as const }
  if (s === 'connecting') return { label: 'RDP 解锁: 检测中', variant: 'info' as const }
  if (s === 'error') return { label: 'RDP 解锁: 未安装', variant: 'error' as const }
  return { label: 'RDP 解锁: 检测中', variant: 'warn' as const }
})

// 原版格式 CPU: {x:F1}% | 内存: {n} MB（内存为 MB，非百分比）
const cpuMemoryPill = computed(() => ({ label: `CPU: ${tick.status.value.cpu.toFixed(1)}% | 内存: ${tick.status.value.memory} MB`, variant: 'ok' as const }))
</script>

<template>
  <header class="topbar">
    <div class="topbar-left">
      <button class="icon-btn" title="连接分身" :disabled="connectDisabled" @click="handleConnect">🔗</button>
      <button class="icon-btn" title="断开分身" :disabled="disconnectDisabled" @click="handleDisconnect">⛔</button>
      <button class="icon-btn" title="终止" :disabled="terminateDisabled" @click="handleTerminate">🛑</button>
      <button class="icon-btn" title="游戏鼠标" :disabled="gameMouseDisabled" @click="handleGameMouseToggle">🎮</button>
      <button class="icon-btn" title="启动程序" :disabled="launchProgramDisabled" @click="handleLaunchProgram">🚀</button>
      <button class="icon-btn" title="环境检查" @click="emit('open-env-check')">🔍</button>
      <button class="icon-btn" title="设置" @click="emit('open-settings')">⚙</button>
      <button class="icon-btn" title="自检" @click="emit('open-selftest')">🧪</button>
    </div>
    <div class="topbar-center">
      <span class="title">AkiSpace — 桌面分身</span>
    </div>
    <div class="topbar-right">
      <StatusPill :label="childSessionPill.label" :variant="childSessionPill.variant" />
      <StatusPill :label="connectionPill.label" :variant="connectionPill.variant" />
      <StatusPill :label="rdpUnlockPill.label" :variant="rdpUnlockPill.variant" />
      <StatusPill :label="cpuMemoryPill.label" :variant="cpuMemoryPill.variant" />
      <select class="theme-select" :value="currentTheme" @change="handleThemeChange">
        <option v-for="t in themes" :key="t" :value="t">{{ t }}</option>
      </select>
      <a class="github-link" href="https://github.com/AkiroMusic/AkiSpace" target="_blank" rel="noopener">GitHub: AkiroMusic/AkiSpace</a>
    </div>
  </header>
</template>

<style scoped>
.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 36px;
  padding: 0 12px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-primary);
  user-select: none;
}
.topbar-left, .topbar-right {
  display: flex;
  align-items: center;
  gap: 4px;
}
.topbar-center {
  flex: 1;
  text-align: center;
}
.title {
  font-weight: 600;
  font-size: 13px;
}
.icon-btn {
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 14px;
}
.icon-btn:hover {
  background: var(--bg-hover);
}
.icon-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.theme-select {
  padding: 4px 8px;
  border: 1px solid var(--border-primary);
  border-radius: 4px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  font-size: 12px;
}
.github-link {
  font-size: 12px;
  color: var(--text-secondary);
  text-decoration: none;
}
.github-link:hover {
  color: var(--text-primary);
}
</style>
