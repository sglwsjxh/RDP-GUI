// AGPL-3.0 许可证
import { ref, computed, provide, inject, onMounted } from 'vue'
import { getSettings, setSettings } from '../commands/settings'

type ThemeName = 'light' | 'dark' | 'sepia' | 'high-contrast' | 'blue-light' | 'custom'

const themeKey = Symbol('theme')

const themes: Record<ThemeName, Record<string, string>> = {
  light: {
    '--bg-primary': '#ffffff',
    '--bg-secondary': '#f5f5f5',
    '--bg-tertiary': '#eeeeee',
    '--bg-hover': '#e0e0e0',
    '--text-primary': '#1a1a1a',
    '--text-secondary': '#666666',
    '--border-primary': '#dddddd',
    '--accent-primary': '#0066cc',
    '--status-ok-bg': '#e8f5e9',
    '--status-ok-fg': '#2e7d32',
    '--status-warn-bg': '#fff3e0',
    '--status-warn-fg': '#ef6c00',
    '--status-error-bg': '#fbe9e7',
    '--status-error-fg': '#c62828',
    '--status-info-bg': '#e3f2fd',
    '--status-info-fg': '#1565c0',
  },
  dark: {
    '--bg-primary': '#1a1a1a',
    '--bg-secondary': '#242424',
    '--bg-tertiary': '#2d2d2d',
    '--bg-hover': '#3a3a3a',
    '--text-primary': '#e0e0e0',
    '--text-secondary': '#999999',
    '--border-primary': '#3d3d3d',
    '--accent-primary': '#4da6ff',
    '--status-ok-bg': '#1b5e20',
    '--status-ok-fg': '#81c784',
    '--status-warn-bg': '#e65100',
    '--status-warn-fg': '#ffb74d',
    '--status-error-bg': '#b71c1c',
    '--status-error-fg': '#ef9a9a',
    '--status-info-bg': '#0d47a1',
    '--status-info-fg': '#64b5f6',
  },
  sepia: {
    '--bg-primary': '#fdf6e3',
    '--bg-secondary': '#f5ebd3',
    '--bg-tertiary': '#eee8d5',
    '--bg-hover': '#e8dcc8',
    '--text-primary': '#3e2723',
    '--text-secondary': '#6d4c41',
    '--border-primary': '#d7ccc8',
    '--accent-primary': '#8d6e63',
    '--status-ok-bg': '#e8f5e9',
    '--status-ok-fg': '#2e7d32',
    '--status-warn-bg': '#fff3e0',
    '--status-warn-fg': '#ef6c00',
    '--status-error-bg': '#fbe9e7',
    '--status-error-fg': '#c62828',
    '--status-info-bg': '#e3f2fd',
    '--status-info-fg': '#1565c0',
  },
  'high-contrast': {
    '--bg-primary': '#000000',
    '--bg-secondary': '#000000',
    '--bg-tertiary': '#1a1a1a',
    '--bg-hover': '#333333',
    '--text-primary': '#ffffff',
    '--text-secondary': '#cccccc',
    '--border-primary': '#ffffff',
    '--accent-primary': '#ffff00',
    '--status-ok-bg': '#00ff00',
    '--status-ok-fg': '#000000',
    '--status-warn-bg': '#ffff00',
    '--status-warn-fg': '#000000',
    '--status-error-bg': '#ff0000',
    '--status-error-fg': '#ffffff',
    '--status-info-bg': '#00ffff',
    '--status-info-fg': '#000000',
  },
  'blue-light': {
    '--bg-primary': '#e8f0fe',
    '--bg-secondary': '#d1e3fa',
    '--bg-tertiary': '#b8d0f5',
    '--bg-hover': '#a0bdf0',
    '--text-primary': '#1a237e',
    '--text-secondary': '#3949ab',
    '--border-primary': '#90caf9',
    '--accent-primary': '#1565c0',
    '--status-ok-bg': '#c8e6c9',
    '--status-ok-fg': '#2e7d32',
    '--status-warn-bg': '#ffe0b2',
    '--status-warn-fg': '#ef6c00',
    '--status-error-bg': '#ffcdd2',
    '--status-error-fg': '#c62828',
    '--status-info-bg': '#bbdefb',
    '--status-info-fg': '#1565c0',
  },
  custom: {
    '--bg-primary': '#ffffff',
    '--bg-secondary': '#f5f5f5',
    '--bg-tertiary': '#eeeeee',
    '--bg-hover': '#e0e0e0',
    '--text-primary': '#1a1a1a',
    '--text-secondary': '#666666',
    '--border-primary': '#dddddd',
    '--accent-primary': '#0066cc',
    '--status-ok-bg': '#e8f5e9',
    '--status-ok-fg': '#2e7d32',
    '--status-warn-bg': '#fff3e0',
    '--status-warn-fg': '#ef6c00',
    '--status-error-bg': '#fbe9e7',
    '--status-error-fg': '#c62828',
    '--status-info-bg': '#e3f2fd',
    '--status-info-fg': '#1565c0',
  },
}

const current = ref<ThemeName>('light')

function applyTheme(name: ThemeName) {
  const vars = themes[name]
  const root = document.documentElement
  for (const [key, value] of Object.entries(vars)) {
    root.style.setProperty(key, value)
  }
  root.setAttribute('data-theme', name)
  current.value = name
}

async function saveThemeToSettings(name: ThemeName) {
  try {
    const settings = await getSettings()
    await setSettings({ ...settings, theme: name })
  } catch {
    // 忽略保存错误
  }
}

async function initTheme() {
  try {
    const settings = await getSettings()
    const saved = settings.theme as ThemeName
    if (saved && themes[saved]) {
      applyTheme(saved)
      return
    }
  } catch {
    // 忽略加载错误
  }
  const local = localStorage.getItem('akispace-theme') as ThemeName | null
  if (local && themes[local]) {
    applyTheme(local)
  } else if (window.matchMedia('(prefers-color-scheme: dark)').matches) {
    applyTheme('dark')
  } else {
    applyTheme('light')
  }
}

export function useTheme() {
  const provided = inject<ReturnType<typeof createThemeContext>>(themeKey)
  if (provided) return provided
  return createThemeContext()
}

function createThemeContext() {
  onMounted(initTheme)
  return {
    current: computed(() => current.value),
    set: async (name: ThemeName) => {
      applyTheme(name)
      await saveThemeToSettings(name)
    },
    themes: Object.keys(themes) as ThemeName[],
  }
}

export function provideTheme() {
  provide(themeKey, createThemeContext())
}