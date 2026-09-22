// SPDX-License-Identifier: AGPL-3.0-only
import { invoke } from '@tauri-apps/api/core'

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

export async function getSettings(): Promise<SettingsData> {
  return invoke('cmd_get_settings')
}

export async function setSettings(data: Partial<SettingsData>): Promise<void> {
  return invoke('cmd_set_settings', { settings: data })
}

export async function resetSettings(): Promise<void> {
  return invoke('reset_settings')
}
