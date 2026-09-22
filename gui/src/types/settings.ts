// SPDX-License-Identifier: AGPL-3.0-only
export interface SettingsData {
  desktopWidth: number
  desktopHeight: number
  colorDepth: number
  gameMouseModeEnabled: boolean
  audioRedirected: boolean
  smartSizing: boolean
  sendSystemShortcutsToRemote: boolean
  rdpPort: number
  autoConnect: boolean
  logoffOnExit: boolean
  launchProgramPath?: string
  connectionMode: 'standardRdp' | 'childSession'
  cloneUsername: string
  clonePassword: string
  minimizeToTray: boolean
  showPerformance: boolean
  enableGlobalHotkey: boolean
  theme: string
}

export interface SettingsState {
  data: SettingsData
  loaded: boolean
}
