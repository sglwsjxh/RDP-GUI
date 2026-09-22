// SPDX-License-Identifier: AGPL-3.0-only
import { ref, computed } from 'vue'

interface Settings {
  theme: string
  language: string
  autoConnect: boolean
  rememberPassword: boolean
}

const defaultSettings: Settings = {
  theme: 'light',
  language: 'zh-CN',
  autoConnect: false,
  rememberPassword: false,
}

const settings = ref<Settings>({ ...defaultSettings })

export function useSettings() {
  const load = () => {
    const saved = localStorage.getItem('akispace-settings')
    if (saved) {
      try {
        settings.value = { ...defaultSettings, ...JSON.parse(saved) }
      } catch {
        settings.value = { ...defaultSettings }
      }
    }
  }

  const save = () => {
    localStorage.setItem('akispace-settings', JSON.stringify(settings.value))
  }

  const reset = () => {
    settings.value = { ...defaultSettings }
    save()
  }

  return {
    settings: computed(() => settings.value),
    load,
    save,
    reset,
  }
}
