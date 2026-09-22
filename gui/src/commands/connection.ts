// SPDX-License-Identifier: AGPL-3.0-only
import { invoke } from '@tauri-apps/api/core'

export interface ConnectConfig {
  host: string
  port: number
  username: string
  password?: string
}

export interface ConnectionInfo {
  sessionId: string
  width: number
  height: number
}

export interface ViewerRect {
  x: number
  y: number
  width: number
  height: number
}

export async function connectRdp(config: ConnectConfig): Promise<ConnectionInfo> {
  return invoke('cmd_connect', config as unknown as Record<string, unknown>)
}

export async function disconnectRdp(sessionId: string): Promise<void> {
  return invoke('cmd_disconnect', { sessionId })
}

export async function resizeSession(sessionId: string, width: number, height: number): Promise<void> {
  return invoke('resize_session', { sessionId, width, height })
}

export async function reportViewerRect(rect: ViewerRect): Promise<void> {
  return invoke('report_viewer_rect', rect as unknown as Record<string, unknown>)
}
