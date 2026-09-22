// SPDX-License-Identifier: AGPL-3.0-only
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'

interface ConnectionState {
  status: 'disconnected' | 'connecting' | 'connected' | 'error'
  host: string
  port: number
  username: string
}

interface ConnectionStatePayload extends Partial<ConnectionState> {
  sessionId?: string
}

const state = ref<ConnectionState>({
  status: 'disconnected',
  host: '',
  port: 3389,
  username: '',
})

let sessionId: string | null = null
let unlisten: UnlistenFn | null = null

export function useConnection() {
  const connect = async (config: Partial<ConnectionState>) => {
    state.value = { ...state.value, ...config, status: 'connecting' }
    try {
      const result: { sessionId: string } = await invoke('connect_rdp', { ...config })
      sessionId = result.sessionId
      state.value.status = 'connected'
    } catch {
      state.value.status = 'error'
    }
  }

  const disconnect = async () => {
    if (sessionId !== null) {
      try {
        await invoke('disconnect_rdp', { sessionId })
      } catch {
        // swallow
      }
    }
    sessionId = null
    state.value.status = 'disconnected'
  }

  onMounted(async () => {
    unlisten = await listen<ConnectionStatePayload>('connection-state', (e) => {
      const { status, host, port, username } = e.payload
      if (status !== undefined) state.value.status = status
      if (host !== undefined) state.value.host = host
      if (port !== undefined) state.value.port = port
      if (username !== undefined) state.value.username = username
    })
  })

  onUnmounted(() => {
    unlisten?.()
    unlisten = null
  })

  return {
    state: computed(() => state.value),
    connect,
    disconnect,
  }
}
