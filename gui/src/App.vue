<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import TopBar from './components/TopBar.vue'
import ViewerPlaceholder from './components/ViewerPlaceholder.vue'
import ModalShell from './components/ModalShell.vue'
import ToastLayer from './components/ToastLayer.vue'
import SettingsDialog from './components/dialogs/SettingsDialog.vue'
import EnvCheckDialog from './components/dialogs/EnvCheckDialog.vue'
import SelftestDialog from './components/dialogs/SelftestDialog.vue'
import PasswordDialog from './components/dialogs/PasswordDialog.vue'
import ConfirmDialog from './components/dialogs/ConfirmDialog.vue'
import { getSettings, setSettings } from './commands/settings'
import type { SettingsData } from './types/settings'
import { checkEnvironment, fixEnvironment } from './commands/environment'
import { useSettings } from './composables/useSettings'
import { useConnection } from './composables/useConnection'
import { useStatusTick } from './composables/useStatusTick'
import { useTheme } from './composables/useTheme'

const settingsStore = useSettings()
settingsStore.load()

const connection = useConnection()
const tick = useStatusTick()
const theme = useTheme()
// 保持引用，避免 tree-shaking 警告
void tick
void theme

// 全量设置（SettingsData，Tauri 侧存储）
const settings = ref<SettingsData | null>(null)

onMounted(async () => {
  try {
    settings.value = (await getSettings()) as unknown as SettingsData
  } catch {
    settings.value = null
  }
})

// 各 dialog 开关
const showSettings = ref(false)
const showEnvCheck = ref(false)
const showSelftest = ref(false)
const showPassword = ref(false)
const showConfirm = ref(false)

// 环境检查数据
const envChecks = ref<Array<{ name: string; status: 'ok' | 'fail' | 'warn'; detail: string }>>([])
const envVersion = ref('')
const envDiagnostic = ref('')

// 确认框场景
type ConfirmAction = 'terminate' | 'disconnect' | 'gameMouse' | 'launchProgram'
const confirmScene = ref<ConfirmAction>('terminate')

const confirmConfig = () => {
  switch (confirmScene.value) {
    case 'disconnect':
      // 源文件无独立断开确认 MessageBox（断开直接执行），保留中性文案
      return { title: '断开连接', message: '确定要断开当前 RDP 连接吗？', variant: 'warning' as const, confirmText: '断开', cancelText: '取消' }
    case 'gameMouse':
      return { title: 'AkiSpace — 游戏鼠标', message: '回放 Agent 未连接。游戏鼠标模式需要分身侧运行回放 Agent（--agent 模式）。\n请先在分身会话中启动 Agent，或在 Agent 落地前使用标准 RDP 鼠标。', variant: 'warning' as const, confirmText: '确定', cancelText: '取消' }
    case 'launchProgram':
      return { title: '启动程序', message: '确定要在子会话中启动配置的程序吗？', variant: 'info' as const, confirmText: '启动', cancelText: '取消' }
    default:
      // 原版 MainForm.cs:743 {sid.Value} 代入会话 ID；接线未导出 sessionId，暂用 {sid} 占位
      return { title: 'AkiSpace', message: '确定要终止子会话（ID {sid}）吗？其中的程序将全部关闭。', variant: 'danger' as const, confirmText: '确定', cancelText: '取消' }
  }
}

// SettingsDialog 事件
const handleSettingsSave = async (data: SettingsData) => {
  try {
    await setSettings(data as unknown as Partial<SettingsData>)
    settings.value = data
    if (data.theme) {
      const name = data.theme as 'light' | 'dark' | 'sepia' | 'high-contrast' | 'blue-light' | 'custom'
      await theme.set(name)
    }
    showSettings.value = false
  } catch {
    // 保存失败保留 dialog 打开
  }
}

// EnvCheckDialog 事件
const openEnvCheck = async () => {
  showEnvCheck.value = true
  try {
    const result = await checkEnvironment()
    envVersion.value = result.tauriVersion
    envDiagnostic.value = JSON.stringify(result.checks, null, 2)
    envChecks.value = Object.entries(result.checks).map(([name, ok]) => ({
      name,
      status: ok ? ('ok' as const) : ('fail' as const),
      detail: ok ? '通过' : '未通过',
    }))
  } catch {
    envChecks.value = [{ name: '环境检查', status: 'fail' as const, detail: '获取结果失败' }]
    envVersion.value = ''
    envDiagnostic.value = ''
  }
}

const handleEnvFix = async (_payload: { disableTermWrap: boolean }) => {
  try {
    await fixEnvironment()
    showEnvCheck.value = false
    openEnvCheck()
  } catch {
    // 修复失败保留 dialog 打开
  }
}

// SelftestDialog 事件
const handleSelftestClose = () => {
  showSelftest.value = false
}

// PasswordDialog 事件
const handlePasswordConfirm = (password: string) => {
  if (settings.value) {
    setSettings({ ...settings.value, clonePassword: password } as unknown as Partial<SettingsData>).catch(() => {})
  }
  showPassword.value = false
}

// ConfirmDialog 事件
const handleConfirm = async () => {
  const action = confirmScene.value
  showConfirm.value = false
  switch (action) {
    case 'disconnect':
      connection.disconnect()
      break
    case 'gameMouse':
      try {
        await invoke('cmd_game_mouse_toggle')
      } catch {
        // 切换失败保留静默，toast 由后端发送
      }
      break
    case 'launchProgram':
      try {
        await invoke('cmd_launch_program')
      } catch {
        // 启动失败保留静默，toast 由后端发送
      }
      break
    case 'terminate':
      try {
        await invoke('cmd_terminate')
      } catch {
        // 终止失败保留静默，toast 由后端发送
      }
      break
  }
}

// TopBar 事件
const handleOpenSettings = () => {
  showSettings.value = true
}
const handleOpenSelftest = () => {
  showSelftest.value = true
}
const handleTerminate = () => {
  confirmScene.value = 'terminate'
  showConfirm.value = true
}
const handleGameMouseToggle = () => {
  confirmScene.value = 'gameMouse'
  showConfirm.value = true
}
const handleLaunchProgram = () => {
  confirmScene.value = 'launchProgram'
  showConfirm.value = true
}
</script>

<template>
  <div class="app">
    <TopBar
      @open-settings="handleOpenSettings"
      @open-env-check="openEnvCheck"
      @open-selftest="handleOpenSelftest"
      @terminate="handleTerminate"
      @game-mouse-toggle="handleGameMouseToggle"
      @launch-program="handleLaunchProgram"
    />
    <ViewerPlaceholder />

    <ModalShell v-if="showSettings" title="设置" @close="showSettings = false">
      <SettingsDialog
        v-if="settings"
        :initial-settings="settings"
        @save="handleSettingsSave"
        @close="showSettings = false"
      />
    </ModalShell>

    <ModalShell v-if="showEnvCheck" title="环境检查" @close="showEnvCheck = false">
      <EnvCheckDialog
        :checks="envChecks"
        :version="envVersion"
        :diagnostic="envDiagnostic"
        @run-fixes="handleEnvFix"
        @close="showEnvCheck = false"
      />
    </ModalShell>

    <ModalShell v-if="showSelftest" title="自检" @close="showSelftest = false">
      <SelftestDialog @close="handleSelftestClose" />
    </ModalShell>

    <ModalShell v-if="showPassword" title="密码" @close="showPassword = false">
      <PasswordDialog @confirm="handlePasswordConfirm" @close="showPassword = false" />
    </ModalShell>

    <ModalShell v-if="showConfirm" :title="confirmConfig().title" @close="showConfirm = false">
      <ConfirmDialog
        :title="confirmConfig().title"
        :message="confirmConfig().message"
        :confirm-text="confirmConfig().confirmText"
        :cancel-text="confirmConfig().cancelText"
        :variant="confirmConfig().variant"
        @confirm="handleConfirm"
        @close="showConfirm = false"
      />
    </ModalShell>

    <ToastLayer />
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: var(--bg-primary);
  color: var(--text-primary);
}
</style>
