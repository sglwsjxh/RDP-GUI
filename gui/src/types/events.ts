// SPDX-License-Identifier: AGPL-3.0-only
export interface StatusTickEvent {
  cpu: number
  memory: number
  network: string
}

export interface ConnectionEvent {
  type: 'connected' | 'disconnected' | 'error'
  sessionId?: string
  message?: string
}

export interface SelftestEvent {
  type: 'started' | 'progress' | 'completed'
  current?: string
  total?: number
  result?: {
    passed: boolean
    tests: Array<{ name: string; passed: boolean; message?: string }>
  }
}

export interface ToastEvent {
  message: string
  type: 'info' | 'success' | 'warning' | 'error'
}
