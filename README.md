# AkiSpace — 家庭版 Windows 的桌面分身 / RDP 前置修复工具

AkiSpace — Desktop clone & RDP pre-flight fixer for Windows Home editions.

---

## 功能概览 / Feature Overview

| 模式 | 说明 |
|------|------|
| **GUI** (`akispace-gui`) | Tauri + Vue 3 前端，托管 mstscax ActiveX 控件，建立到子会话或标准 RDP 的连接，转发相对鼠标流 |
| **Agent** (`akispace-agent`) | 无 UI 控制台程序，运行在子会话内，经命名管道接收鼠标批次并用 `SendInput` 回放 |
| **修环境** (`--fix-env`) | 管理员控制台分支，按顺序应用 9 步修复（注册表、防火墙、TermService 重启、启用子会话等） |

核心能力：
- **家庭版子会话模式**：`WTSEnableChildSessions` + `IMsRdpExtendedSettings::ConnectToChildSession`，不依赖 TCP 监听器，与 RDP Wrapper 互斥
- **标准 RDP 模式**：配合 RDP Wrapper 解锁多会话，`--fix-env` 可选禁用 Wrapper hook
- **二进制帧协议**：5 字节头（长度 LE + 类型），载荷 ≤ 1 MB，nonce 32 字节握手 + 恒定时间比较，fail-closed
- **命名管道 ACL**：当前用户 + SYSTEM + 可选克隆账户 SID，DACL 在流创建时刻烧死
- **设置持久化**：`%APPDATA%\AkiSpace\settings.json`，17 字段，DPAPI 三态密码（`DPAPI:` 前缀），原子写 `.tmp → Move(overwrite)`
- **自检 15 编号块**：`cargo run --bin akispace-selftest --no-default-features`，退出码 0 = 全过（含 SKIP 语义）

---

## 快速开始 / Quick Start

### 开发模式 / Dev
```bash
# 前端依赖 + Tauri dev（热重载，端口 1420）
npm --prefix gui install && npx tauri dev
```

### 前端构建检查 / Frontend Build Check
```bash
cd gui && npm run build
# 产物：gui/dist，供 tauri.conf.json "frontendDist" 使用
```

### 全量测试 / Full Test Suite
```bash
cargo test
# lib 34 项（environment×4, ipc::frame×9, launcher×1, ipc::frame::tests×...）
```

### 自检 / Selftest（无 GUI feature）
```bash
cargo run --bin akispace-selftest --no-default-features
# 退出码 0 = 全部通过（块 13 需管理员/真机时 SKIP，不计失败）
```

### Agent 发布构建 / Agent Release Build
```bash
cargo build --release --bin akispace-agent --no-default-features
# 产物：target/release/akispace-agent.exe（单文件，无 GUI 依赖）
```

### 修环境 / Fix Environment（需管理员）
```bash
cargo run -- --fix-env
# 写注册表、防火墙、重启 TermService、启用子会话 —— 生产机慎用
```

---

## 架构与目录 / Architecture & Layout

```
rdp-gui/
├── Cargo.toml                    # 单 package，3 bin + lib，gui feature 门控 tauri
├── main.rs                       # akispace-gui 入口，argv 分支 --fix-env
├── tauri.conf.json               # 前端 dist=gui/dist，devUrl=localhost:1420
├── build.rs                      # #[cfg(feature="gui")] 门控 tauri_build::build()
├── modules/
│   ├── lib.rs                    # lib 根，pub mod ipc/session/environment/logging/fix_env
│   ├── bin/
│   │   ├── agent.rs              # 命名管道客户端：nonce 握手 → verdict → RelativeMouseBatch 回放
│   │   └── selftest.rs           # 15 编号块自检（块 1–5/14/14b/14c/14d=帧协议，6–13=环境探测）
│   ├── ipc.rs                    # 帧编解码、管道 ACL、nonce 握手、Tauri 命令注册
│   ├── session.rs                # WTS 子会话、端口、监听、Wrapper、termsrv 版本探测
│   ├── environment.rs            # 10 检查 + 9 步修复（ApplyAllFixes 顺序固定）
│   ├── logging.rs                # panic hook + tracing（文件日志 %LOCALAPPDATA%\AkiSpace\logs\）
│   └── fix_env.rs                # --fix-env 入口，复用 environment::ApplyAllFixes
├── gui/
│   ├── package.json              # Vue 3.5 + TS + Vite，scripts: dev/build/preview/tauri
│   ├── src/
│   │   ├── main.ts               # createApp + tauri plugin
│   │   ├── App.vue               # 根布局：TopBar + Viewer 容器
│   │   ├── components/
│   │   │   ├── TopBar.vue        # 连接/断开/终止/游戏鼠标/启动程序（按钮未全接线）
│   │   │   └── ViewerPlaceholder.vue  # RDP 画面占位区（上报物理像素坐标）
│   │   ├── composables/          # useTheme/useSettings/useConnection/useStatusTick/useInvokeRect
│   │   ├── commands/             # Tauri invoke 封装
│   │   └── types/                # 前端事件接口（connection-state/status-tick/toast）
│   └── vite.config.ts
└── .omo/notepads/...             # 设计决策记录（emit-bridge.md 等）
```

关键数据流：
```
主会话 RawInput(相对鼠标) → MouseForwarder 累积(10ms/换向刷) → PipeServer(命名管道) → 子会话 Agent PipeClient → AgentRunner(8ms 子组) → SendInput 注入
```

---

## 依赖 / Dependencies

| 类别 | 关键 crate / 包 | 版本/备注 |
|------|----------------|-----------|
| Windows API | `windows` | 0.62.2，30+ feature flags（Win32_Foundation, Win32_System_RemoteDesktop, Win32_System_Com, …） |
| 异步运行时 | `tokio` | 1.40, full features |
| 序列化 | `serde` / `serde_json` | 1.0 |
| 日志 | `tracing` / `tracing-subscriber` | 0.1 / 0.3 (env-filter) |
| 加密/随机 | `rand` 0.8, `uuid` 1.8 (v4+serde) |
| 管道/通道 | `crossbeam-channel` 0.5 |
| Tauri (可选) | `tauri` 2.11.6, `tauri-plugin-single-instance` 2.2, `tauri-plugin-global-shortcut` 2.2 | 仅 `gui` feature 开启 |
| 前端 | Vue 3.5, TypeScript 6, Vite 8, @tauri-apps/api 2, @tauri-apps/cli 2 | `gui/package.json` |

**最低 Rust**：1.85+（edition 2021）  
**平台**：仅 Windows（依赖 `windows` crate 与 wtsapi32/mstscax/Task Scheduler COM）

---

## 已知问题 / Known Issues（实证，不美化）

1. **`connection-state` 事件 `sessionId` 恒 `null`**  
   后端 `emit_connection_state` 硬编码 `sessionId: null`，前端 `connect` invoke 返回值自持 sessionId。

2. **家庭版真机矩阵未验证**  
   AtlAxWin 遮挡 / `RunEx` / WTS 子会话在 24H2/25H2 上的行为 —— DESIGN §8 待办。

3. **浏览器点击矩阵验收未跑**  
   开发环境无 Chrome；`npm run build` / `vue-tsc --noEmit` 全过但 UI 行为待真机。

4. **Selftest 块 13 需管理员 SKIP；`BuildFirewallAddRule` 纯函数未抽**  
   `test_apply_fixes_wrapper_hook_skip()` 为空函数 + `ponytail:` 注释；C# 原块 13 断言 `ApplyAllFixes(false)` 保留 wrapper hook 的纯函数逻辑尚未在 Rust 侧暴露。

---

## License

AGPL-3.0-only — 与源文件头一致。  
复刻自 C# 版 AkiSpace（AkiroMusic/AkiSpace）。