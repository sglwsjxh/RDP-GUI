<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script setup lang="ts">
import { ref } from 'vue'

export interface CheckItem {
  name: string
  status: 'ok' | 'fail' | 'warn'
  detail: string
}

interface Props {
  checks: CheckItem[]
  version: string
  diagnostic: string
}
const props = defineProps<Props>()

interface Emits {
  runFixes: [payload: { disableTermWrap: boolean }]
  close: []
}
const emit = defineEmits<Emits>()

const disableTermWrap = ref(false)

const copy = async (text: string) => {
  try {
    await navigator.clipboard.writeText(text)
  } catch {
    // 剪贴板不可用时静默
  }
}
</script>

<template>
  <div class="dialog-env">
    <h3 class="dialog-title">环境检查 / 修复 — 桌面分身前置条件</h3>

    <section class="card">
      <h4 class="card-title">环境检查</h4>
      <p class="card-subtitle">桌面分身运行前置条件检测</p>
      <table class="checks">
        <thead>
          <tr>
            <th>检查项</th>
            <th>状态</th>
            <th>详情</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="c in props.checks" :key="c.name">
            <td class="name">{{ c.name }}</td>
            <td class="status" :class="c.status">
              {{ c.status === 'ok' ? '✓ 通过' : c.status === 'warn' ? '✗ 错误' : '✗ 失败' }}
            </td>
            <td class="detail">{{ c.detail }}</td>
          </tr>
        </tbody>
      </table>
    </section>

    <section class="card home-card">
      <h4 class="card-title">⚠ 家庭版需要安装 RDP Wrapper</h4>
      <p class="card-subtitle">Windows Home 缺少 RDP 主机功能，需第三方解锁层</p>
      <p class="hint">
        Windows 家庭版不提供 RDP 主机，桌面分身功能需要 RDP Wrapper 第三方解锁层。<br />
        安装步骤：① 下载任一社区维护的 RDP Wrapper → ② 解压到 C:\Program Files\RDP Wrapper\ →<br />
        ③ 以管理员运行 rdpWrapper.exe -install → ④ 确认 rdpwrap.ini 含本机 termsrv.dll 版本段。<br />
        AkiSpace 不会自动下载运行第三方二进制，请按需从可信来源获取。
      </p>
      <p class="version-line">
        本机 termsrv.dll 版本: {{ props.version }}
        （在 rdpwrap.ini 中需找到 [10.0.xxx.xxxx] 段落）
      </p>
      <div class="links">
        <a
          href="https://github.com/sergiye/rdpwrapper"
          target="_blank"
          rel="noopener"
        >
          ① 打开 sergiye/rdpWrapper (C# 推荐)
        </a>
        <a
          href="https://github.com/sebaxakerhtc/rdpwrap"
          target="_blank"
          rel="noopener"
        >
          ② 打开 sebaxakerhtc/rdpwrap (Delphi fork)
        </a>
      </div>
      <div class="copy-row">
        <button type="button" class="mini-btn" @click="copy(props.version)">
          ③ 复制版本号（用于搜索 rdpwrap.ini）
        </button>
        <button type="button" class="mini-btn" @click="copy(props.diagnostic)">
          ④ 复制诊断信息（用于 GitHub 反馈）
        </button>
      </div>
    </section>

    <section class="card fix-card">
      <h4 class="card-title">AkiSpace 环境修复</h4>
      <p class="confirm-text">将执行以下操作（需要管理员权限，会弹出 UAC 提示）：</p>
      <ul class="fix-list">
        <li>✗ 保留 RDP Wrapper hook（标准 RDP 模式必需）</li>
        <li>✓ 启用 RDP（fDenyTSConnections=0）</li>
        <li>✓ 允许多会话（fSingleSessionPerUser=0）</li>
        <li>✓ 设置 StartRCM=1（家庭版修复）</li>
        <li>✓ 安全加固（TLS + 高加密 + NLA）</li>
        <li>✓ 添加防火墙回环规则（RDP 仅允许 127.0.0.1）</li>
        <li>✓ 防火墙阻断 3389 公网入站（删除默认 RDP 公开 allow）</li>
        <li>✓ 重启 TermService 服务（会踢掉现有 RDP 会话）</li>
      </ul>
      <p class="confirm-sub">
        注意：RDP Wrapper 本身（rdpwrap.dll）需按本对话框顶部的指引手动安装。<br />
        如果你想在标准 RDP 模式下也禁用 TermWrap，请先在「设置」中切换到「子会话」模式，<br />
        或勾选下方的「同时禁用 TermWrap」选项。
      </p>
      <label class="check-row">
        <input v-model="disableTermWrap" type="checkbox" />
        <span>同时禁用 TermWrap（覆盖默认行为）</span>
      </label>
      <p class="confirm-text">是否继续？</p>
    </section>

    <footer class="actions">
      <button class="btn" @click="emit('close')">取消</button>
      <button class="btn primary" @click="emit('runFixes', { disableTermWrap })">
        继续
      </button>
    </footer>
  </div>
</template>

<style scoped>
.dialog-env {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
}
.dialog-title {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
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
.checks {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.checks th,
.checks td {
  text-align: left;
  padding: 4px 6px;
  border-bottom: 1px solid var(--border-primary);
}
.checks th {
  color: var(--text-secondary);
  font-weight: 600;
}
.checks .name {
  width: 30%;
}
.checks .status.ok {
  color: #2e9e4f;
}
.checks .status.fail {
  color: #d13438;
}
.checks .status.warn {
  color: #c29e3d;
}
.hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.7;
}
.version-line {
  margin: 0;
  font-size: 12px;
  font-weight: 600;
  font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
  color: var(--accent-primary);
}
.links {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.links a {
  font-size: 13px;
  color: var(--accent-primary);
}
.copy-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}
.mini-btn {
  padding: 3px 10px;
  background: var(--bg-primary);
  border: 1px solid var(--border-primary);
  border-radius: 4px;
  color: var(--text-primary);
  cursor: pointer;
  font-size: 12px;
}
.mini-btn:hover {
  background: var(--bg-hover);
}
.confirm-text {
  margin: 0;
  font-size: 13px;
}
.fix-list {
  margin: 0;
  padding: 0;
  list-style: none;
  font-size: 12px;
  color: var(--text-secondary);
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.confirm-sub {
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary);
}
.check-row {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
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
