<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script setup lang="ts">
import { reactive } from 'vue'
import type { SettingsData } from '../../types/settings'

interface Props {
  initialSettings: SettingsData
}
const props = defineProps<Props>()

interface Emits {
  save: [settings: SettingsData]
  close: []
}
const emit = defineEmits<Emits>()

const form = reactive<SettingsData>({ ...props.initialSettings })

const save = () => {
  emit('save', { ...form })
}
</script>

<template>
  <div class="dialog-settings">
    <section class="card">
      <h4 class="card-title">连接配置</h4>
      <p class="card-subtitle">分身桌面的连接参数与模式</p>
      <div class="row">
        <label class="row-label" for="sd-mode">连接模式</label>
        <select id="sd-mode" v-model="form.connectionMode">
          <option value="standardRdp">标准RDP（不同用户）</option>
          <option value="childSession">子会话（同一用户）</option>
        </select>
      </div>
      <div class="row">
        <label class="row-label" for="sd-width">分身桌面宽度</label>
        <input id="sd-width" v-model.number="form.desktopWidth" type="number" min="0" />
        <span class="hint">像素，建议匹配显示器分辨率</span>
      </div>
      <div class="row">
        <label class="row-label" for="sd-height">分身桌面高度</label>
        <input id="sd-height" v-model.number="form.desktopHeight" type="number" min="0" />
        <span class="hint">像素，建议匹配显示器分辨率</span>
      </div>
      <div class="row">
        <label class="row-label" for="sd-depth">颜色深度</label>
        <input id="sd-depth" v-model.number="form.colorDepth" type="number" min="0" />
        <span class="hint">位，32位为真彩色</span>
      </div>
      <div class="row">
        <label class="row-label" for="sd-port">RDP 端口</label>
        <input id="sd-port" v-model.number="form.rdpPort" type="number" min="1" max="65535" />
        <span class="hint">仅非 3389 生效；保持 3389 则跟随系统 RDP 监听端口</span>
      </div>
      <div class="row check">
        <input id="sd-smart" v-model="form.smartSizing" type="checkbox" />
        <label class="check-label" for="sd-smart">缩放适应窗口 (Smart Sizing)</label>
      </div>
      <div class="row check">
        <input id="sd-shortcuts" v-model="form.sendSystemShortcutsToRemote" type="checkbox" />
        <label class="check-label" for="sd-shortcuts">系统快捷键发送到分身</label>
      </div>
      <div class="row check">
        <input id="sd-audio" v-model="form.audioRedirected" type="checkbox" />
        <label class="check-label" for="sd-audio">音频重定向到本机</label>
      </div>
    </section>

    <section class="card">
      <h4 class="card-title">行为设置</h4>
      <p class="card-subtitle">启动、托盘、热键与性能监控</p>
      <div class="row check">
        <input id="sd-game-mouse" v-model="form.gameMouseModeEnabled" type="checkbox" />
        <label class="check-label" for="sd-game-mouse">启用游戏鼠标模式（仅标准 RDP，需回放 Agent）</label>
      </div>
      <div class="row check">
        <input id="sd-auto" v-model="form.autoConnect" type="checkbox" />
        <label class="check-label" for="sd-auto">启动时自动连接分身</label>
      </div>
      <div class="row check">
        <input id="sd-logoff" v-model="form.logoffOnExit" type="checkbox" />
        <label class="check-label" for="sd-logoff">退出时终止子会话</label>
      </div>
      <div class="row check">
        <input id="sd-tray" v-model="form.minimizeToTray" type="checkbox" />
        <label class="check-label" for="sd-tray">最小化到系统托盘</label>
      </div>
      <div class="row check">
        <input id="sd-perf" v-model="form.showPerformance" type="checkbox" />
        <label class="check-label" for="sd-perf">状态栏显示性能监控 (CPU/内存)</label>
      </div>
      <div class="row check">
        <input id="sd-hotkey" v-model="form.enableGlobalHotkey" type="checkbox" />
        <label class="check-label" for="sd-hotkey">启用全局热键 (Ctrl+Shift+D 切换连接, Ctrl+Alt+Space 显示窗口)</label>
      </div>
    </section>

    <section class="card">
      <h4 class="card-title">分身账户</h4>
      <p class="card-subtitle">标准 RDP 模式下的登录凭据</p>
      <div class="row">
        <label class="row-label" for="sd-clone-user">分身账户用户名</label>
        <input id="sd-clone-user" v-model="form.cloneUsername" type="text" />
        <span class="hint">标准 RDP 模式必填，如 AkiSpaceUser</span>
      </div>
      <div class="row">
        <label class="row-label" for="sd-clone-pass">分身账户密码</label>
        <input id="sd-clone-pass" v-model="form.clonePassword" type="password" />
        <span class="hint">密码使用 DPAPI 加密存储；留空则首次连接标准 RDP 时询问</span>
      </div>
    </section>

    <section class="card">
      <h4 class="card-title">自动启动</h4>
      <p class="card-subtitle">连接成功后在分身中启动程序</p>
      <div class="row">
        <label class="row-label" for="sd-launch">连接后自动启动</label>
        <div class="path-input">
          <input id="sd-launch" v-model="form.launchProgramPath" type="text" />
          <button type="button" class="browse-btn" disabled>浏览...</button>
        </div>
      </div>
    </section>

    <footer class="actions">
      <button class="btn" @click="emit('close')">取消</button>
      <button class="btn primary" @click="save">保存</button>
    </footer>
  </div>
</template>

<style scoped>
.dialog-settings {
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
.card-subtitle {
  margin: 0;
  font-size: 11px;
  color: var(--text-tertiary);
}
.row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.row-label {
  flex: 0 0 150px;
  font-size: 13px;
}
.hint {
  flex: 1 1 100%;
  font-size: 11px;
  color: var(--text-tertiary);
}
.row.check {
  flex-wrap: nowrap;
}
.check-label {
  flex: 1;
  font-size: 13px;
}
.row input[type='number'],
.row input[type='text'],
.row input[type='password'],
.row select {
  flex: 1;
  min-width: 0;
  padding: 4px 8px;
  background: var(--bg-primary);
  border: 1px solid var(--border-primary);
  border-radius: 4px;
  color: var(--text-primary);
  font-size: 13px;
}
.path-input {
  flex: 1;
  display: flex;
  gap: 6px;
}
.browse-btn {
  flex: 0 0 auto;
  padding: 4px 10px;
  background: var(--bg-primary);
  border: 1px solid var(--border-primary);
  border-radius: 4px;
  color: var(--text-secondary);
  cursor: not-allowed;
  font-size: 12px;
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
</style>
