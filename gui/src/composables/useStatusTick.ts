// SPDX-License-Identifier: AGPL-3.0-only
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

interface StatusData {
  cpu: number
  memory: number
  network: string
}

const status = ref<StatusData>({
  cpu: 0,
  memory: 0,
  network: 'unknown',
})

let unlisten: UnlistenFn | null = null

export function useStatusTick() {
  const start = () => {}
  const stop = () => {}

  onMounted(async () => {
    unlisten = await listen<StatusData>('status-tick', (e) => {
      status.value = e.payload
    })
  })

  onUnmounted(() => {
    unlisten?.()
    unlisten = null
  })

  return {
    status: computed(() => status.value),
    start,
    stop,
  }
}
